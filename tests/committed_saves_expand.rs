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
//! ## What it is held to, which is the bytes
//!
//! Each committed save is rebuilt into the archive form it came from, expanded again
//! against the same base, and what comes out has to be WHAT IS COMMITTED - the same files,
//! each holding the same bytes. Every one of them was written by this writer, so anything
//! else means the writer has changed and the corpus has not.
//!
//! That bar was not available while the corpus carried .NET's encoder output: 493 escapes,
//! its own key order, and its own edit script for the two unified diffs. de-xz48.4 wrote
//! all of it again, and being able to demand the bytes is what that bought.
//!
//! THE SAVE STILL HAS TO SURVIVE, and that is checked too, because equal bytes are the
//! easier half. The plan is read back through a view that falls through to disk for the
//! base, so what a READER makes of the writer's output is compared with what it makes of
//! the committed save - which is the property that would still matter if the spelling ever
//! did drift.
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
    self, EXPANDED_SUFFIX, MANIFEST_NAME, OnDisk, Pending,
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

    // EVERY FILE HELD TO ITS BYTES. Shown as text where it is text, so a difference reads
    // as the line it is on rather than as two lists of numbers.
    for file in &planned {
        let committed =
            fs::read(&file.path).unwrap_or_else(|why| panic!("{}: {why}", file.path.display()));
        assert_eq!(
            String::from_utf8_lossy(&file.bytes),
            String::from_utf8_lossy(&committed),
            "{}: what this writer produces is not what is committed",
            file.path.display(),
        );
    }

    // WHAT A READER GETS, which is the claim, rather than the bytes it reads to get there.
    let pending = Pending(&planned);
    assert_eq!(
        expanded_save::members_of(&pending, save)?,
        expanded_save::members_of(&OnDisk, save)?,
        "{named}: the members it writes resolve to something else",
    );
    // DECODED, not as the trees they are stored as. Equal bytes already say the trees
    // agree; what this adds is that they agree about the TABLE, which is what a save is.
    assert_eq!(
        lua_parts::document(&pending, save, Some(orders)).map_err(ExpandFault::from)?,
        lua_parts::document(&OnDisk, save, Some(orders)).map_err(ExpandFault::from)?,
        "{named}: the tables it writes resolve to something else",
    );

    Ok(())
}

/// The id map, or nothing with a line saying why nothing here is written.
fn orders_or_say_why() -> Option<Orders> {
    let found = orders();
    if found.is_none() {
        println!(
            "no {} beside the repository, so nothing is written",
            lua_simx::ORDERS_FILE_NAME
        );
    }

    found
}

/// The same save, called `stem` on every entry instead of its own name.
fn named(packed: &Unpacked, stem: &str) -> Unpacked {
    let own = packed.stem();
    let rename = |name: &str| {
        let suffix = name
            .strip_prefix(own)
            .unwrap_or_else(|| panic!("{name} is not prefixed with {own}"));
        format!("{stem}{suffix}")
    };

    Unpacked {
        lua_name: rename(&packed.lua_name),
        lua: packed.lua.clone(),
        members: packed
            .members
            .iter()
            .map(|entry| Entry {
                name: rename(&entry.name),
                bytes: entry.bytes.clone(),
            })
            .collect(),
    }
}

/// A save named the way the game names one is expanded under its directory's name.
///
/// A save leaves the game with a timestamp in its name and is expanded into a directory named
/// without one, and what reads the directory back finds its files by the directory's name. So
/// the archive's own name has to make no difference to what is written.
#[test]
fn a_save_named_by_the_game_is_expanded_under_its_directorys_name() {
    let Some(orders) = orders_or_say_why() else {
        return;
    };

    let save = changed_saves()
        .into_iter()
        .next()
        .expect("the repository carries a save written as a change");
    let base = base_of(&save);
    let packed = as_the_game_wrote_it(&save, &orders);
    let from_the_game = named(&packed, "MARTINAISE, DAY 3, 13-22(9_15_2026 10-24-16 AM)");

    let plan = |unpacked: &Unpacked| -> Vec<(PathBuf, Vec<u8>)> {
        expand::expansion(&OnDisk, unpacked, &save, Some(&base), Some(&orders))
            .unwrap_or_else(|why| panic!("{}: {why}", save.display()))
            .into_iter()
            .map(|file| (file.path, file.bytes))
            .collect()
    };

    assert_eq!(
        plan(&from_the_game),
        plan(&packed),
        "{}: an archive named by the game expands into different files",
        save.display(),
    );
}

#[test]
fn every_committed_save_is_written_again_as_the_save_it_already_is() {
    let Some(orders) = orders_or_say_why() else {
        return;
    };

    let saves = changed_saves();
    assert!(
        saves.len() >= AT_LEAST,
        "only {} saves written as a change were found; there are at least {AT_LEAST}",
        saves.len(),
    );

    // ONLY THE CHANGED ONES by default - see `common::committed_saves_to_check`.
    for save in &common::committed_saves_to_check(saves) {
        rewrites(save, &orders).unwrap_or_else(|why| panic!("{}: {why}", save.display()));
    }
}
