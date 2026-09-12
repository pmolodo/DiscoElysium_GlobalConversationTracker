// SPDX-License-Identifier: MIT
//! Does the writer produce the expanded saves this repository has already committed?
//!
//! ## Which direction this is
//!
//! `committed_saves_pack` goes the other way - an expanded save into the archive the game
//! loads. This one is how an expanded save comes to exist at all: a save leaves the game as
//! an archive, and what gets committed is the directory, written as a CHANGE to a save that
//! is already there.
//!
//! ## What it is held to, and why not the bytes
//!
//! THE SAVE HAS TO SURVIVE, and the plan has to hold the same files the committed save
//! holds. Each committed save is rebuilt into the archive form it came from, expanded again
//! against the same base, and the result is resolved: the members and the tables that come
//! out have to be the ones the committed save resolves to, and the set of files planned has
//! to be exactly the set committed.
//!
//! BYTE-FOR-BYTE IS NOT THE BAR, and three separate things stop it being. The 19 member
//! diffs were written by .NET's default encoder and carry its 493 escapes and its own key
//! order. A unified diff is not unique, so the two text diffs are a different edit script
//! for the same change. And the sparse form does not record a grouped table's property
//! order, which `committed_sparse_tables` already names the three saves affected by. All
//! three are spelling, all three are rewritten by de-xz48.4, and none of them changes what
//! a reader gets - which is what this checks instead.
//!
//! THE MANIFEST IS THE EXCEPTION and is held to the bytes, because it is the one file whose
//! whole content this writer decides. The diffs carry text a diff library or another
//! encoder produced; the manifest is this writer's own sentence about what the save holds,
//! so a difference in it is a difference in meaning rather than in spelling.
//!
//! ## Why this skips without the id map
//!
//! A save's `Variable` table leaves out the variables that only repeat its conversations,
//! and both halves of this - reading the committed save and writing it again - need
//! `articy_ids_final_cut.json` to put them back and to leave them out. It is not committed,
//! so a fresh clone has nothing to check and says so, the same answer `committed_saves_pack`
//! gives.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use lookahead_engine::formats::expand::{self, ExpandFault};
use lookahead_engine::formats::expanded_save::{
    self, EXPANDED_SUFFIX, MANIFEST_NAME, OnDisk, Pending, Written,
};
use lookahead_engine::formats::lua_simx::{self, Orders};
use lookahead_engine::formats::packed_save::{Entry, LUA_SUFFIX, Unpacked};
use lookahead_engine::formats::{lua_blob, lua_parts};

mod common;

/// How many saves written as a change to another one the repository is known to carry.
///
/// A LOWER BOUND, asserted so that a test finding none - a moved directory, a glob that
/// stopped matching - fails rather than passing over an empty list.
const AT_LEAST: usize = 12;

/// The id map, which is not committed. Nothing to read means nothing here can be written.
fn orders() -> Option<Orders> {
    let path = common::repo_root().join(lua_simx::ORDERS_FILE_NAME);
    let text = fs::read_to_string(&path).ok()?;
    Some(Orders::read(&text).unwrap_or_else(|why| panic!("{}: {why}", path.display())))
}

/// Every committed save that is written as a change to another one.
fn changed_saves() -> Vec<PathBuf> {
    let scenarios = common::repo_root().join("testing").join("scenarios");
    let mut found: Vec<PathBuf> = fs::read_dir(&scenarios)
        .unwrap_or_else(|why| panic!("{}: {why}", scenarios.display()))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join(MANIFEST_NAME).is_file())
        .collect();

    found.sort();
    found
}

/// A committed save put back into the shape it left the game in.
///
/// Reading the save resolves every diff beneath it, so what comes back is the whole save
/// rather than the change - which is what an archive holds and therefore what the writer
/// has to be given.
fn as_the_game_wrote_it(save: &Path, orders: &Orders) -> Unpacked {
    let stem = save
        .file_name()
        .expect("a save has a name")
        .to_string_lossy()
        .strip_suffix(EXPANDED_SUFFIX)
        .expect("a save's directory ends in .ntwtf")
        .to_string();

    let blob = lua_parts::document(&OnDisk, save, Some(orders))
        .unwrap_or_else(|why| panic!("{}: {why}", save.display()));
    let members = expanded_save::members_of(&OnDisk, save)
        .unwrap_or_else(|why| panic!("{}: {why}", save.display()));

    Unpacked {
        lua_name: format!("{stem}{LUA_SUFFIX}"),
        lua: lua_blob::write(&blob),
        members: members
            .into_iter()
            .map(|(suffix, bytes)| Entry {
                name: format!("{stem}{suffix}"),
                bytes,
            })
            .collect(),
    }
}

/// What is committed under a save's own directory, as paths.
fn committed_files(save: &Path) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    let mut looking = vec![save.to_path_buf()];
    while let Some(directory) = looking.pop() {
        for entry in fs::read_dir(&directory)
            .unwrap_or_else(|why| panic!("{}: {why}", directory.display()))
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                looking.push(path);
            } else {
                found.insert(path);
            }
        }
    }

    found
}

/// Where a save says its base is, which is what it has to be written against again.
fn base_of(save: &Path) -> PathBuf {
    let manifest = save.join(MANIFEST_NAME);
    let text =
        fs::read_to_string(&manifest).unwrap_or_else(|why| panic!("{}: {why}", manifest.display()));
    let read = expanded_save::read_manifest(&text, &manifest.to_string_lossy())
        .unwrap_or_else(|why| panic!("{}: {why}", manifest.display()));

    save.join(read.base)
}

/// Writes one committed save again and checks what comes of it.
fn rewrites(save: &Path, orders: &Orders) -> Result<(), ExpandFault> {
    let named = save.display().to_string();
    let base = base_of(save);
    let packed = as_the_game_wrote_it(save, orders);
    let planned = expand::expansion(&OnDisk, &packed, save, Some(&base), Some(orders))?;

    let wrote: BTreeSet<PathBuf> = planned.iter().map(|file| file.path.clone()).collect();
    assert_eq!(
        wrote,
        committed_files(save),
        "{named}: the writer plans a different set of files from the one committed",
    );

    // THE ONE FILE HELD TO ITS BYTES, for the reason at the top of this file.
    let manifest = save.join(MANIFEST_NAME);
    let planned_manifest = planned
        .iter()
        .find(|file: &&Written| file.path == manifest)
        .unwrap_or_else(|| panic!("{named}: no manifest was planned"));
    assert_eq!(
        String::from_utf8_lossy(&planned_manifest.bytes),
        fs::read_to_string(&manifest).expect("the committed manifest reads"),
        "{named}: the manifest it writes is not the one committed",
    );

    // WHAT A READER GETS, which is the claim, rather than the bytes it reads to get there.
    let pending = Pending(&planned);
    assert_eq!(
        expanded_save::members_of(&pending, save)?,
        expanded_save::members_of(&OnDisk, save)?,
        "{named}: the members it writes resolve to something else",
    );
    // DECODED, not as the trees they are stored as. The sparse form does not record a
    // grouped table's property order, so a committed tree can spell a table one way and
    // this writer another - which `committed_sparse_tables` names the three saves affected
    // by. What both spellings have to agree on is the table, and they do.
    assert_eq!(
        lua_parts::document(&pending, save, Some(orders)).map_err(ExpandFault::from)?,
        lua_parts::document(&OnDisk, save, Some(orders)).map_err(ExpandFault::from)?,
        "{named}: the tables it writes resolve to something else",
    );

    Ok(())
}

#[test]
fn every_committed_save_is_written_again_as_the_save_it_already_is() {
    let Some(orders) = orders() else {
        println!(
            "no {} beside the repository, so nothing is written",
            lua_simx::ORDERS_FILE_NAME
        );
        return;
    };

    let saves = changed_saves();
    assert!(
        saves.len() >= AT_LEAST,
        "only {} saves written as a change were found; there are at least {AT_LEAST}",
        saves.len(),
    );

    for save in &saves {
        rewrites(save, &orders).unwrap_or_else(|why| panic!("{}: {why}", save.display()));
    }
}
