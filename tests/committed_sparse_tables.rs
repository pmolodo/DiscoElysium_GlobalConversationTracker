// SPDX-License-Identifier: MIT
//! Does Rust read every Lua table this repository has committed, and write it back?
//!
//! ## What is on disk, and how little of it is whole
//!
//! One save's tables are committed WHOLE - `testing/save_template.ntwtf` - and every
//! scenario is a chain of sparse diffs ending at it. So reading the committed tables means
//! walking each chain and applying what each link changes, which is what this does: the
//! result is the tree the offline runner actually reads for that scenario, rather than the
//! patch file beside it.
//!
//! ## The bar, and why it is not byte-for-byte
//!
//! THE TABLE HAS TO SURVIVE. A tree decodes to a Lua table, that table encodes back, and
//! the tree it produces decodes to the SAME table. That is the property a save depends on,
//! and it is checked on every table of every scenario.
//!
//! BYTE-FOR-BYTE IS NOT THE BAR HERE, because the sparse form does not record a grouped
//! table's property order: the writer lists one key range per distinct value in the order
//! the values were first MET, and the reader hands the keys back in ascending order, which
//! is a different order in general. So a tree that came out of a save in some other order
//! is written back in ascending order, once, and is a fixed point from then on. The count
//! of committed tables that would be rewritten is asserted rather than left to be
//! discovered, so the size of that change is known before de-xz48.4 makes it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use lookahead_engine::formats::expanded_save;
use lookahead_engine::formats::lua_blob::TABLE_NAMES;
use lookahead_engine::formats::lua_simx::{self, Derivation, Orders};
use lookahead_engine::formats::lua_sparse::{self, CONVERSATION_TABLE, LuaSparseFault};
use lookahead_engine::formats::sparse::{self, SparseMap, SparseValue};
use lookahead_engine::formats::sparse_diff;

mod common;

/// How many saves the repository is known to carry.
///
/// A LOWER BOUND, asserted so that a test finding none - a moved directory, a glob that
/// stopped matching - fails rather than passing over an empty list.
const AT_LEAST: usize = 12;

/// The saves whose tables this writer spells differently, and what differs.
///
/// NO DATA DIFFERS in any of them - each table decodes to exactly what it decoded to
/// before, which is what the survival check asserts - so what is left is spelling, of two
/// kinds. A grouped table's property ORDER, because the form does not record it and the
/// reader hands the keys back ascending. And a key range written out NUMBER BY NUMBER in a
/// fixture that never went through a writer, where this collapses the run.
///
/// Named rather than counted, so a fourth one fails here with the difference spelt out
/// rather than passing as a number that happened to stay the same.
const REWRITTEN: [&str; 3] = [
    "at-trashcan.ntwtf",
    "fan-read-all.ntwtf",
    "orb-read-all.ntwtf",
];

/// The table that leaves its derived variables out.
///
/// The template's carries a `_derived_simx` header and every scenario built on it inherits
/// one, so this is where the rebuild is held to the real thing: the variables come back,
/// the table is written out again, and the header it produces is compared with the one the
/// file carries. A checkout without the id map beside it cannot do that and REFUSES these
/// tables instead of reading them short.
const DERIVED: &str = "Variable";

/// Every expanded save under `testing`, the template included.
fn saves() -> Vec<PathBuf> {
    let testing = common::repo_root().join("testing");
    let mut found = vec![testing.join("save_template.ntwtf")];
    if let Ok(entries) = fs::read_dir(testing.join("scenarios")) {
        found.extend(
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir()),
        );
    }

    found.sort();
    found
}

/// The split directory holding a save's five table files.
fn parts_of(save: &Path) -> PathBuf {
    let name = save.file_name().unwrap_or_default().to_string_lossy();
    save.join(format!("{name}.lua.parts"))
}

/// What a save is called in a failure.
fn name_of(save: &Path) -> String {
    save.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// The chain a save sits at the end of, oldest first, ending with the save itself.
fn chain(save: &Path) -> Vec<PathBuf> {
    let manifest = save.join("_archive.json");
    let Ok(text) = fs::read_to_string(&manifest) else {
        return vec![save.to_path_buf()];
    };

    let read = expanded_save::read_manifest(&text, &manifest.to_string_lossy())
        .expect("a committed manifest reads");
    let mut links = chain(&save.join(read.base));
    links.push(save.to_path_buf());
    links
}

/// The five tables of a save, with every diff between it and a whole save applied.
fn tables_of(save: &Path) -> BTreeMap<&'static str, SparseMap> {
    let mut tables = BTreeMap::new();
    for (at, link) in chain(save).iter().enumerate() {
        let parts = parts_of(link);
        for name in TABLE_NAMES {
            let path = parts.join(format!("{name}.json"));
            let Ok(text) = fs::read_to_string(&path) else {
                assert!(at > 0, "{} has no {name} table", name_of(link));
                continue;
            };

            let document = sparse::read(&text, &path.to_string_lossy()).expect("it reads");
            let tree = match tables.remove(name) {
                // A LINK OF THE CHAIN, which changes what the one before it held.
                Some(was) => sparse_diff::apply(&was, &document)
                    .unwrap_or_else(|why| panic!("{}: {why}", path.display())),
                // The whole table the chain starts from.
                None => {
                    let named = document.find(FORMAT_KEY).and_then(as_text);
                    let stamped = document.find(VERSION_KEY).and_then(as_count);
                    lua_sparse::FORMAT
                        .check(named, stamped)
                        .unwrap_or_else(|why| panic!("{}: {why}", path.display()));
                    document
                }
            };
            tables.insert(name, tree);
        }
    }

    tables
}

const FORMAT_KEY: &str = "_format";
const VERSION_KEY: &str = "_formatVersion";

fn as_text(value: &SparseValue) -> Option<&str> {
    match value {
        SparseValue::Text(text) => Some(text),
        _ => None,
    }
}

fn as_count(value: &SparseValue) -> Option<u32> {
    match value {
        SparseValue::Int(whole) => u32::try_from(*whole).ok(),
        _ => None,
    }
}

/// Where two trees first differ, and whether it is only the order they are written in.
///
/// SAID PRECISELY rather than left as "these are not equal", because the two differences
/// mean opposite things: a property order that moved is the known cost of not recording a
/// grouped table's order, and a value that changed is data lost.
fn first_difference(written: &SparseMap, expected: &SparseMap, path: &str) -> Option<String> {
    for (name, value) in written.entries() {
        let Some(was) = expected.find(name) else {
            return Some(format!("{path}/{name} is written and was not there"));
        };

        match (value, was) {
            (SparseValue::Map(mine), SparseValue::Map(theirs)) => {
                if let Some(deeper) = first_difference(mine, theirs, &format!("{path}/{name}")) {
                    return Some(deeper);
                }
            }
            _ if value != was => {
                return Some(format!("{path}/{name} is {value:?} and was {was:?}"));
            }
            _ => {}
        }
    }

    for (name, _) in expected.entries() {
        if !written.has(name) {
            return Some(format!("{path}/{name} was there and is not written"));
        }
    }

    let order: Vec<&str> = written
        .entries()
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let was: Vec<&str> = expected
        .entries()
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    (order != was).then(|| format!("{path} holds the same entries in another order"))
}

/// A tree without the two fields that say what it is, which is what the writer produces.
fn without_header(tree: &SparseMap) -> SparseMap {
    let mut stripped = SparseMap::new();
    for (name, value) in tree.entries() {
        if name != FORMAT_KEY && name != VERSION_KEY {
            stripped.add(name.clone(), value.clone());
        }
    }

    stripped
}

/// The id map, which is not committed. Nothing to read means the Variable tables cannot be.
fn orders() -> Option<Orders> {
    let path = common::repo_root().join(lua_simx::ORDERS_FILE_NAME);
    let text = fs::read_to_string(&path).ok()?;
    Some(Orders::read(&text).unwrap_or_else(|why| panic!("{}: {why}", path.display())))
}

#[test]
fn every_committed_table_decodes_and_comes_back_as_the_same_table() {
    let saves = saves();
    assert!(
        saves.len() >= AT_LEAST,
        "found {} saves under testing, expected at least {AT_LEAST}",
        saves.len(),
    );

    let orders = orders();
    if orders.is_none() {
        println!(
            "no {} beside the repository, so the {DERIVED} tables are checked only for \
             being refused",
            lua_simx::ORDERS_FILE_NAME,
        );
    }

    let mut checked = 0;
    let mut refused = Vec::new();
    let mut rewritten = Vec::new();
    for save in &saves {
        let trees = tables_of(save);

        // THE CONVERSATION TABLE FIRST, because the Variable table leaves out the variables
        // that only repeat it and cannot be read without it.
        let conversations = lua_sparse::decode(
            &SparseValue::Map(trees[CONVERSATION_TABLE].clone()),
            CONVERSATION_TABLE,
            None,
        )
        .unwrap_or_else(|why| panic!("{}/{CONVERSATION_TABLE}: {why}", name_of(save)));
        let derivation = orders.as_ref().map(|orders| Derivation {
            conversations: &conversations,
            orders,
        });

        for (name, tree) in &trees {
            let table = match lua_sparse::decode(
                &SparseValue::Map(tree.clone()),
                name,
                derivation.as_ref(),
            ) {
                Ok(table) => table,
                Err(LuaSparseFault::NeedsDerivation { .. }) => {
                    assert_eq!(*name, DERIVED, "{} refused its {name} table", name_of(save));
                    refused.push(name_of(save));
                    continue;
                }
                Err(why) => panic!("{}/{name}: {why}", name_of(save)),
            };

            let written = lua_sparse::encode(&table, name, derivation.as_ref())
                .unwrap_or_else(|why| panic!("{}/{name}: {why}", name_of(save)));
            let back = lua_sparse::decode(
                &SparseValue::Map(written.clone()),
                name,
                derivation.as_ref(),
            )
            .unwrap_or_else(|why| panic!("{}/{name}, written back: {why}", name_of(save)));

            assert_eq!(back, table, "{}/{name} did not survive", name_of(save));
            if let Some(how) = first_difference(&written, &without_header(tree), name) {
                rewritten.push((name_of(save), how));
            }
            checked += 1;
        }
    }

    assert!(checked > 0, "no tables were read");
    assert_eq!(
        refused.len(),
        if orders.is_some() { 0 } else { saves.len() },
        "the {DERIVED} tables that could not be read: {refused:?}",
    );
    let rewritten_saves: Vec<&str> = rewritten.iter().map(|(save, _)| save.as_str()).collect();
    assert_eq!(
        rewritten_saves,
        REWRITTEN,
        "the committed tables this writer spells differently have changed:\n  {}",
        rewritten
            .iter()
            .map(|(save, how)| format!("{save}: {how}"))
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}
