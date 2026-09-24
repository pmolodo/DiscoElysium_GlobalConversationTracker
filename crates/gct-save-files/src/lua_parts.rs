// SPDX-License-Identifier: MIT
//! The directory a save's Lua tables are split into, read back as the blob they came from.
//!
//! ## What is in one
//!
//! One JSON file per table - `Actor.json` through `Conversation.json`, each a
//! [`super::lua_sparse`] tree - and `trailing.bin`, the bytes the blob held after the five.
//! The directory sits inside the expanded save and is named after it, so
//! `at-trashcan.ntwtf` keeps its tables in `at-trashcan.ntwtf/at-trashcan.ntwtf.lua.parts`.
//!
//! ## A save that is a change to another carries only what it changes
//!
//! Then each file present is a [`super::sparse_diff`] rather than a whole table, and the
//! ones that did not change are absent - as is the whole directory, for a save that changes
//! no table at all.
//!
//! ## The chain says where to look; the file says what it is a diff of
//!
//! Those are two questions and they have two answers. WHERE a table is written down can be
//! several links up, because a save that changes no table has no directory to hold one, so
//! finding it means walking the chain `_archive.json` names. WHAT it is a diff of is written
//! in the file, in `_base`, relative to itself - so once found, a table resolves from its
//! own path with no manifest in sight.
//!
//! A `_base` therefore SKIPS every save that did not touch that table, and is meant to.
//! `fan-read-all` changes the conversations; its base changes no table at all; so its
//! conversations are a diff of the template's, two links up.
//!
//! ## The order the tables are turned back into Lua in is not free
//!
//! `Conversation` FIRST, always. The `Variable` table leaves out the variables that are a
//! second copy of the conversations and names what it left out in a header; putting them
//! back needs the conversations and the id map, which is what [`super::lua_simx`] does with
//! them. A `Variable` table with that header and nothing to rebuild from is REFUSED rather
//! than read short.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::expanded_save::{self, Files, SaveFault, Written, shown};
use super::header::{self, HeaderFault};
use super::lua_blob::{Blob, LuaTable, LuaValue, TABLE_NAMES};
use super::lua_simx::Orders;
use super::lua_sparse::{self, CONVERSATION_TABLE, Derivation, LuaSparseFault};
use super::sparse::{self, SparseFault, SparseMap, SparseValue};
use super::sparse_diff::{self, SparseDiffFault};

/// What the bytes after the five tables are called.
pub const TRAILING_NAME: &str = "trailing.bin";

/// What a split directory's name ends in, after the Lua blob it stands for.
pub const PARTS_SUFFIX: &str = ".lua.parts";

/// Why a save's tables could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PartsFault {
    /// The chain the tables are spread along could not be walked.
    #[error("{0}")]
    Chain(#[from] SaveFault),
    /// Nothing in the chain holds a table, or the bytes after them.
    #[error("{0} has no {1}")]
    Missing(String, String),
    /// A file is there and its bytes are not text.
    #[error("{0} will not read: {1}")]
    Unreadable(String, String),
    /// A file is text and is not a sparse tree.
    #[error("{0}")]
    Malformed(#[from] SparseFault),
    /// A whole table does not say it is one.
    #[error("{0}: {1}")]
    Header(String, #[source] HeaderFault),
    /// A diff would not apply to the table it is a diff of.
    #[error("{0}: {1}")]
    Unapplicable(String, #[source] SparseDiffFault),
    /// A tree is not the table it claims to be.
    #[error("{0}: {1}")]
    Table(String, #[source] LuaSparseFault),
    /// A blob holds something other than a table where one of the five should be.
    #[error("a blob holds no {0} table")]
    NotATable(String),
}

/// A save's five tables and its trailing bytes, before any of it is Lua again.
#[derive(Debug, Clone, PartialEq)]
pub struct Parts {
    /// The five trees, in [`TABLE_NAMES`] order.
    pub tables: Vec<SparseMap>,
    /// What the blob held after them, carried through untouched.
    pub trailing: Vec<u8>,
}

/// Where a save keeps its split tables.
#[must_use]
pub fn directory_in(save: &Path) -> PathBuf {
    let name = save
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    save.join(name + PARTS_SUFFIX)
}

/// A save's tables, with every diff between it and a whole save applied.
///
/// # Errors
///
/// Where the chain will not walk, where a file will not read or is not a sparse tree, where
/// a whole table does not say it is one, where a diff will not apply, or where nothing in
/// the chain holds one of the five tables.
pub fn read(files: &impl Files, save: &Path) -> Result<Parts, PartsFault> {
    let tables = TABLE_NAMES
        .iter()
        .map(|name| read_table(files, save, name))
        .collect::<Result<Vec<_>, _>>()?;

    let trailing = expanded_save::table_beneath(files, save, TRAILING_NAME, directory_in)?
        .and_then(|path| files.read(&path))
        .ok_or_else(|| PartsFault::Missing(shown(save), TRAILING_NAME.to_string()))?;

    Ok(Parts { tables, trailing })
}

/// One of a save's tables, by name, with every diff between it and a whole table applied.
///
/// For a reader that wants one table and not the blob: the Conversation table alone is most
/// of a save's size, and a caller asking about the variables should not pay for it.
///
/// # Errors
///
/// As [`read`], for this table alone.
pub fn read_table(files: &impl Files, save: &Path, name: &str) -> Result<SparseMap, PartsFault> {
    // THE CHAIN SAYS WHERE TO LOOK, and the file says what it is a diff of. A save that
    // changes no table has no split directory at all, so the nearest one holding this table
    // can be several links up - but once found, it is followed from itself.
    let file = format!("{name}.json");
    let path = expanded_save::table_beneath(files, save, &file, directory_in)?
        .ok_or_else(|| PartsFault::Missing(shown(save), name.to_string()))?;
    tree(files, &path, &mut HashSet::new(), &mut Vec::new())
}

/// One table's tree, with every diff between it and a whole tree applied.
///
/// The same walk [`expanded_save::resolved`] does for a member, over the sparse form: a file
/// that names itself a `sparse-diff` names the tree it changes in `_base`, relative to
/// itself, and anything else has to be a whole table of this build's version.
fn tree(
    files: &impl Files,
    path: &Path,
    walked: &mut HashSet<PathBuf>,
    chain: &mut Vec<String>,
) -> Result<SparseMap, PartsFault> {
    let at = expanded_save::flatten(path);
    let context = shown(&at);
    chain.push(context.clone());
    if !walked.insert(at.clone()) {
        return Err(PartsFault::Chain(SaveFault::Circular(chain.join(" -> "))));
    }

    let raw = files
        .read(&at)
        .ok_or_else(|| PartsFault::Chain(SaveFault::Missing(context.clone())))?;
    let text = String::from_utf8(raw)
        .map_err(|why| PartsFault::Unreadable(context.clone(), why.to_string()))?;
    let document = sparse::read(&text, &context)?;

    let names = match document.find(header::FORMAT_KEY) {
        Some(SparseValue::Text(text)) => Some(text.as_str()),
        _ => None,
    };
    if names != Some(sparse_diff::FORMAT.format) {
        whole(&document, &context)?;
        return Ok(document);
    }

    let base = sparse_diff::base_of(&document)
        .ok_or_else(|| PartsFault::Chain(SaveFault::Baseless(context.clone())))?;
    let beneath = at.parent().unwrap_or_else(|| Path::new(".")).join(base);
    let was = tree(files, &beneath, walked, chain)?;

    sparse_diff::apply(&was, &document).map_err(|why| PartsFault::Unapplicable(context, why))
}

/// The blob a save's split directory holds.
///
/// `orders` is what the `Variable` table's derived variables are rebuilt from. Without one,
/// a table that left any out is REFUSED rather than read short - see [`super::lua_simx`].
///
/// # Errors
///
/// As [`read`], and where a tree is not the table it claims to be.
pub fn document(
    files: &impl Files,
    save: &Path,
    orders: Option<&Orders>,
) -> Result<Blob, PartsFault> {
    decode(&read(files, save)?, orders)
}

/// The blob a save's tables are, once they are Lua again.
///
/// # Errors
///
/// Where a tree is not the table it claims to be, or where one leaves its derived variables
/// out and there is nothing to rebuild them from.
pub fn decode(parts: &Parts, orders: Option<&Orders>) -> Result<Blob, PartsFault> {
    let at_conversations = at_conversations();
    let conversations = table(&parts.tables[at_conversations], CONVERSATION_TABLE, None)?;

    // THE CONVERSATIONS ARE BORROWED for as long as anything might rebuild variables from
    // them, so they are put in their own slot only once every other table is read.
    let mut decoded: Vec<Option<LuaTable>> = TABLE_NAMES.iter().map(|_| None).collect();
    {
        let derivation = orders.map(|orders| Derivation {
            conversations: &conversations,
            orders,
        });

        for (at, name) in TABLE_NAMES.iter().enumerate() {
            if at != at_conversations {
                decoded[at] = Some(table(&parts.tables[at], name, derivation.as_ref())?);
            }
        }
    }
    decoded[at_conversations] = Some(conversations);

    Ok(Blob {
        tables: decoded
            .into_iter()
            .map(|table| LuaValue::Table(table.expect("every slot was filled")))
            .collect(),
        trailing: parts.trailing.clone(),
    })
}

/// The trees a blob's tables are stored as, each stamped as a whole table.
///
/// The inverse of [`decode`]. `orders` is what the `Variable` table's derived variables are
/// left out against; without one every variable is written in full, which is a correct save
/// and a larger one.
///
/// # Errors
///
/// Where a blob holds something other than a table where one of the five should be, or
/// where a table holds a key that has no spelling as a JSON property name.
pub fn encode(blob: &Blob, orders: Option<&Orders>) -> Result<Parts, PartsFault> {
    let conversations = named_table(blob, at_conversations())?;
    let derivation = orders.map(|orders| Derivation {
        conversations,
        orders,
    });

    let mut tables = Vec::with_capacity(TABLE_NAMES.len());
    for (at, name) in TABLE_NAMES.iter().enumerate() {
        let mut tree = lua_sparse::encode(named_table(blob, at)?, name, derivation.as_ref())
            .map_err(|why| PartsFault::Table((*name).to_string(), why))?;
        stamp(&mut tree);
        tables.push(tree);
    }

    Ok(Parts {
        tables,
        trailing: blob.trailing.clone(),
    })
}

/// What a save's split directory is written against: the tables beneath it, and where each
/// of their files is, spelled from the directory being written.
///
/// THE TWO ARE NOT THE SAME QUESTION. What a table is a diff OF is the resolved tree, which
/// the base save answers for; WHERE that tree is written down can be several links further
/// up, because a save that changes no table has no split directory at all.
pub struct Beneath<'a> {
    /// The base's five trees, resolved and encoded the way this writer encodes.
    pub tables: &'a Parts,
    /// Where each one's file is, in [`TABLE_NAMES`] order.
    pub named: &'a [String],
}

/// The files a save's split directory holds, ready to be written.
///
/// With a `base`, only what DIFFERS from it: each changed table as a sparse diff carrying
/// the `_base` that says which file it is a diff of, the unchanged ones left out entirely,
/// and the trailing bytes only where they changed. So a save that changes no table at all
/// produces no files, and therefore no directory - which is what the reader already allows
/// for, and what stops a save carrying an empty folder to say it changed nothing.
#[must_use]
pub fn files(directory: &Path, parts: &Parts, base: Option<&Beneath<'_>>) -> Vec<Written> {
    let mut written = Vec::new();

    for (at, name) in TABLE_NAMES.iter().enumerate() {
        let tree = match base {
            None => parts.tables[at].clone(),
            Some(base) => {
                match sparse_diff::create(
                    &base.tables.tables[at],
                    &parts.tables[at],
                    &base.named[at],
                ) {
                    Some(patch) => patch,
                    None => continue,
                }
            }
        };

        written.push(Written {
            path: directory.join(format!("{name}.json")),
            bytes: sparse::write(&tree).into_bytes(),
        });
    }

    if base.is_none_or(|base| base.tables.trailing != parts.trailing) {
        written.push(Written {
            path: directory.join(TRAILING_NAME),
            bytes: parts.trailing.clone(),
        });
    }

    written
}

/// Where the conversations are among the five, which the other tables may be rebuilt from.
fn at_conversations() -> usize {
    TABLE_NAMES
        .iter()
        .position(|name| *name == CONVERSATION_TABLE)
        .expect("the conversations are one of the five")
}

/// One of a blob's five top-level values, where it is the table it must be.
fn named_table(blob: &Blob, at: usize) -> Result<&LuaTable, PartsFault> {
    match &blob.tables[at] {
        LuaValue::Table(table) => Ok(table),
        _ => Err(PartsFault::NotATable(TABLE_NAMES[at].to_string())),
    }
}

/// Puts the header on a whole table's tree, ahead of the table itself.
fn stamp(tree: &mut SparseMap) {
    #[allow(clippy::cast_possible_wrap)]
    tree.lead(
        header::VERSION_KEY,
        SparseValue::Int(lua_sparse::FORMAT.version as i32),
    );
    tree.lead(
        header::FORMAT_KEY,
        SparseValue::Text(lua_sparse::FORMAT.format.to_string()),
    );
}

/// One tree as the table it stands for.
fn table(
    tree: &SparseMap,
    name: &str,
    derivation: Option<&Derivation<'_>>,
) -> Result<LuaTable, PartsFault> {
    lua_sparse::decode_map(tree, name, derivation)
        .map_err(|why| PartsFault::Table(name.to_string(), why))
}

/// Whether a tree says it is a whole table of the version this build reads.
///
/// BEFORE ANYTHING IS READ OFF IT, and for the same reason every other format here is
/// checked first: a file from a newer build may hold a shape this one would half-read, and
/// half-reading a save is how a save gets corrupted by the write that follows.
fn whole(document: &SparseMap, context: &str) -> Result<(), PartsFault> {
    let named = match document.find(header::FORMAT_KEY) {
        Some(SparseValue::Text(text)) => Some(text.as_str()),
        _ => None,
    };
    let stamped = match document.find(header::VERSION_KEY) {
        Some(SparseValue::Int(whole)) => u32::try_from(*whole).ok(),
        _ => None,
    };

    lua_sparse::FORMAT
        .check(named, stamped)
        .map_err(|why| PartsFault::Header(context.to_string(), why))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expanded_save::Held;

    /// A whole save's five tables and the bytes after them.
    ///
    /// Every table holds one entry naming itself, so a test can say which table a value
    /// came out of rather than only that five of something arrived.
    fn whole_save(files: Held, save: &str) -> Held {
        let mut held = files;
        for name in TABLE_NAMES {
            held = held.with(
                &format!("{save}/{save}{PARTS_SUFFIX}/{name}.json"),
                &format!(r#"{{"_format": "sparse", "_formatVersion": 1, "who": "{name}"}}"#),
            );
        }

        held.with(
            &format!("{save}/{save}{PARTS_SUFFIX}/{TRAILING_NAME}"),
            "end",
        )
    }

    /// A manifest naming a base and one inherited member, which is the smallest valid one.
    fn manifest(base: &str) -> String {
        format!(
            r#"{{"_format": "expanded-save-diff", "_formatVersion": 1, "base": "{base}",
                "members": [{{"diff": null, "kind": "inherit", "name": "s.states.lua",
                "suffix": ".states.lua"}}]}}"#
        )
    }

    /// A sparse diff of the tree at `base`, changing the one entry every test table holds.
    fn diff_of(base: &str, becomes: &str) -> String {
        format!(
            r#"{{"_format": "sparse-diff", "_formatVersion": 2, "_base": "{base}",
                "_changes": {{"who": "{becomes}"}}}}"#
        )
    }

    fn text_of(tree: &SparseMap, name: &str) -> String {
        match tree.find(name) {
            Some(SparseValue::Text(text)) => text.clone(),
            other => panic!("{name} is {other:?}"),
        }
    }

    #[test]
    fn a_whole_save_holds_its_five_tables_and_the_bytes_after_them() {
        let files = whole_save(Held::default(), "whole.ntwtf");

        let parts = read(&files, Path::new("whole.ntwtf")).expect("it reads");

        assert_eq!(parts.tables.len(), TABLE_NAMES.len());
        for (tree, name) in parts.tables.iter().zip(TABLE_NAMES) {
            assert_eq!(text_of(tree, "who"), name);
        }
        assert_eq!(parts.trailing, b"end".to_vec());
    }

    /// The tables a save changes are its base's with its own patches applied, and the rest
    /// are the base's untouched.
    #[test]
    fn a_diff_changes_the_tables_it_carries_and_inherits_the_others() {
        let files = whole_save(Held::default(), "base.ntwtf")
            .with("save.ntwtf/_archive.json", &manifest("../base.ntwtf"))
            .with(
                &format!("save.ntwtf/save.ntwtf{PARTS_SUFFIX}/Actor.json"),
                &diff_of(
                    "../../base.ntwtf/base.ntwtf.lua.parts/Actor.json",
                    "changed",
                ),
            );

        let parts = read(&files, Path::new("save.ntwtf")).expect("it reads");

        assert_eq!(text_of(&parts.tables[0], "who"), "changed");
        assert_eq!(text_of(&parts.tables[1], "who"), TABLE_NAMES[1]);
        // A save that changes no byte after the tables carries none, and keeps the base's.
        assert_eq!(parts.trailing, b"end".to_vec());
    }

    /// And the bytes after the tables are the last ones any link of the chain states.
    #[test]
    fn the_bytes_after_the_tables_are_the_last_ones_stated() {
        let files = whole_save(Held::default(), "base.ntwtf")
            .with("save.ntwtf/_archive.json", &manifest("../base.ntwtf"))
            .with(
                &format!("save.ntwtf/save.ntwtf{PARTS_SUFFIX}/{TRAILING_NAME}"),
                "later",
            );

        let parts = read(&files, Path::new("save.ntwtf")).expect("it reads");

        assert_eq!(parts.trailing, b"later".to_vec());
    }

    /// A chain that never states one of the five is a save that cannot be rebuilt.
    #[test]
    fn a_table_no_link_of_the_chain_holds_is_refused() {
        let mut files = Held::default();
        for name in TABLE_NAMES {
            if name != "Item" {
                files = files.with(
                    &format!("whole.ntwtf/whole.ntwtf{PARTS_SUFFIX}/{name}.json"),
                    r#"{"_format": "sparse", "_formatVersion": 1}"#,
                );
            }
        }
        let files = files.with(
            &format!("whole.ntwtf/whole.ntwtf{PARTS_SUFFIX}/{TRAILING_NAME}"),
            "",
        );

        let why = read(&files, Path::new("whole.ntwtf")).expect_err("it is refused");

        assert!(
            matches!(&why, PartsFault::Missing(_, what) if what == "Item"),
            "{why}"
        );
    }

    /// As is one that states every table and not what followed them.
    #[test]
    fn bytes_after_the_tables_that_no_link_holds_are_refused() {
        let mut files = Held::default();
        for name in TABLE_NAMES {
            files = files.with(
                &format!("whole.ntwtf/whole.ntwtf{PARTS_SUFFIX}/{name}.json"),
                r#"{"_format": "sparse", "_formatVersion": 1}"#,
            );
        }

        let why = read(&files, Path::new("whole.ntwtf")).expect_err("it is refused");

        assert!(
            matches!(&why, PartsFault::Missing(_, what) if what == TRAILING_NAME),
            "{why}",
        );
    }

    /// A table that does not say what it is could be anything, and is not read.
    #[test]
    fn a_whole_table_that_says_nothing_about_itself_is_refused() {
        let files = whole_save(Held::default(), "whole.ntwtf").with(
            &format!("whole.ntwtf/whole.ntwtf{PARTS_SUFFIX}/Actor.json"),
            r#"{"who": "Actor"}"#,
        );

        let why = read(&files, Path::new("whole.ntwtf")).expect_err("it is refused");

        assert!(matches!(why, PartsFault::Header(_, _)), "{why}");
    }

    /// A save may state a table WHOLE rather than as a change, and then its base is not
    /// consulted about that table at all.
    ///
    /// Nothing this repository writes does that - every save beyond the template is a
    /// change - but it follows from a file saying what it is: a tree that names itself a
    /// whole table is one, wherever along a chain it stands. There is nothing left for a
    /// reader to be confused by, which is what the base written in each file bought.
    #[test]
    fn a_save_may_state_a_table_whole_instead_of_changing_it() {
        let files = whole_save(Held::default(), "base.ntwtf")
            .with("save.ntwtf/_archive.json", &manifest("../base.ntwtf"))
            .with(
                &format!("save.ntwtf/save.ntwtf{PARTS_SUFFIX}/Actor.json"),
                r#"{"_format": "sparse", "_formatVersion": 1, "who": "again"}"#,
            );

        let parts = read(&files, Path::new("save.ntwtf")).expect("it reads");

        assert_eq!(text_of(&parts.tables[0], "who"), "again");
    }

    /// A diff naming a base that is not there says which file it went looking for.
    #[test]
    fn a_diff_whose_base_is_not_there_is_refused_by_name() {
        let files = whole_save(Held::default(), "base.ntwtf")
            .with("save.ntwtf/_archive.json", &manifest("../base.ntwtf"))
            .with(
                &format!("save.ntwtf/save.ntwtf{PARTS_SUFFIX}/Actor.json"),
                &diff_of("nowhere/Actor.json", "changed"),
            );

        let why = read(&files, Path::new("save.ntwtf")).expect_err("it is refused");

        assert!(why.to_string().contains("Actor.json"), "{why}");
    }

    /// A diff that says nothing about what it changes is refused rather than guessed at.
    #[test]
    fn a_diff_that_names_no_base_is_refused() {
        let files = whole_save(Held::default(), "base.ntwtf")
            .with("save.ntwtf/_archive.json", &manifest("../base.ntwtf"))
            .with(
                &format!("save.ntwtf/save.ntwtf{PARTS_SUFFIX}/Actor.json"),
                r#"{"_format": "sparse-diff", "_formatVersion": 2,
                    "_changes": {"who": "changed"}}"#,
            );

        let why = read(&files, Path::new("save.ntwtf")).expect_err("it is refused");

        assert!(
            matches!(why, PartsFault::Chain(SaveFault::Baseless(_))),
            "{why}",
        );
    }

    /// The five trees come back as the five tables of a blob, in the order it holds them.
    #[test]
    fn the_tables_decode_into_the_blob_they_came_from() {
        let files = whole_save(Held::default(), "whole.ntwtf");

        let blob = document(&files, Path::new("whole.ntwtf"), None).expect("it reads");

        assert_eq!(blob.tables.len(), TABLE_NAMES.len());
        for (value, name) in blob.tables.iter().zip(TABLE_NAMES) {
            let LuaValue::Table(table) = value else {
                panic!("{name} is not a table");
            };
            assert_eq!(
                table.get(&LuaValue::Text("who".to_string())),
                Some(&LuaValue::Text(name.to_string())),
            );
        }
        assert_eq!(blob.trailing, b"end".to_vec());
    }
}
