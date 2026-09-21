// SPDX-License-Identifier: MIT
//! Does Rust read every Lua table this repository has committed, and write it back?
//!
//! ## What is on disk, and how little of it is whole
//!
//! One save's tables are committed WHOLE - `testing/save_template.ntwtf` - and every
//! scenario is a chain of sparse diffs ending at it. So reading the committed tables means
//! walking each chain and applying what each link changes, which
//! [`lookahead_engine::formats::lua_parts`] does: what is checked here is the tree the
//! offline runner actually reads for that scenario, rather than the patch file beside it.
//!
//! ## The two bars, and both of them are met
//!
//! THE TABLE HAS TO SURVIVE. A tree decodes to a Lua table, that table encodes back, and
//! the tree it produces decodes to the SAME table. That is the property a save depends on,
//! and it is checked on every table of every scenario.
//!
//! AND THE SPELLING HAS TO BE THIS WRITER'S, exactly. Every committed table was written by
//! it, so re-encoding one has to produce what is on disk and not merely something equal to
//! it. That is a fixed point rather than a coincidence: the sparse form does not record a
//! grouped table's property order, so a tree that arrived in some other order is written
//! back in ascending order once and stays there.
//!
//! A save spelled some other way is therefore a failure now, where it used to be a named
//! exception. Three of them were, until the corpus was written again by this writer.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use lookahead_engine::formats::lua_blob::TABLE_NAMES;
use lookahead_engine::formats::lua_simx::{self, Derivation, Orders};
use lookahead_engine::formats::lua_sparse::{self, CONVERSATION_TABLE, LuaSparseFault};
use lookahead_engine::formats::sparse::{SparseMap, SparseValue};
use lookahead_engine::formats::{expanded_save, header, lua_parts};

use gct_measure::common;

/// How many saves the repository is known to carry.
///
/// A LOWER BOUND, asserted so that a test finding none - a moved directory, a glob that
/// stopped matching - fails rather than passing over an empty list.
const AT_LEAST: usize = 12;

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

/// What a save is called in a failure.
fn name_of(save: &Path) -> String {
    save.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// The five tables of a save, with every diff between it and a whole save applied.
///
/// Through the library rather than by walking the chain here, which is the point of there
/// being a library: a reader in the test would be a second one to keep in step, and the
/// thing it would drift from is the one the game's saves are rebuilt by.
fn tables_of(save: &Path) -> BTreeMap<&'static str, SparseMap> {
    let parts = lua_parts::read(&expanded_save::OnDisk, save)
        .unwrap_or_else(|why| panic!("{}: {why}", save.display()));

    TABLE_NAMES.into_iter().zip(parts.tables).collect()
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
        if name != header::FORMAT_KEY && name != header::VERSION_KEY {
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

    // ONLY THE CHANGED ONES by default - see `common::committed_saves_to_check`. The count
    // above is of every save, so a glob that stopped matching still fails.
    let saves = common::committed_saves_to_check(saves);

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

    assert!(saves.is_empty() || checked > 0, "no tables were read");
    assert_eq!(
        refused.len(),
        if orders.is_some() { 0 } else { saves.len() },
        "the {DERIVED} tables that could not be read: {refused:?}",
    );
    let rewritten_saves: Vec<&str> = rewritten.iter().map(|(save, _)| save.as_str()).collect();
    assert!(
        rewritten_saves.is_empty(),
        "the committed tables are this writer's own output, so it must respell none:\n  {}",
        rewritten
            .iter()
            .map(|(save, how)| format!("{save}: {how}"))
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}
