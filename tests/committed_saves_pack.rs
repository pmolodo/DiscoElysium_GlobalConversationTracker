// SPDX-License-Identifier: MIT
//! Does a committed expanded save pack into an archive that holds what it started as?
//!
//! ## The two shapes a save comes in, one of each
//!
//! `testing/save_template.ntwtf` is WHOLE - its tables and its members are all there - and
//! every scenario beside it is a CHANGE to something, resolved along a chain of them. They
//! fail differently: a whole save exercises the naming and the zip, and a chain exercises
//! everything the diffs do before either of those gets a look. One of each is enough, and
//! is a great deal quicker than all thirteen.
//!
//! ## What is checked, and what only a running game could check
//!
//! That the archive holds the entries a save holds, that every one is prefixed with the
//! archive's own name - the rule that decides whether the game lists the save at all - and
//! that the Lua blob inside reads back as the tables the expanded save described.
//!
//! WHETHER THE GAME LOADS IT is not checkable here and is what the harness is for. What is
//! checkable here is that nothing was dropped or misnamed on the way.
//!
//! ## Why this skips rather than fails without the id map
//!
//! A save's `Variable` table leaves out the variables that only repeat its conversations,
//! and rebuilding them needs `articy_ids_final_cut.json`, which is not committed. Packing
//! is REFUSED without it rather than done short, so a fresh clone has nothing to check and
//! says so - the same answer the tests that need a conversation index give.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use lookahead_engine::formats::expanded_save::OnDisk;
use lookahead_engine::formats::lua_simx::{self, Orders};
use lookahead_engine::formats::packed_save::{LUA_SUFFIX, Stamp, ZIP_SUFFIX};
use lookahead_engine::formats::{lua_blob, lua_parts, packed_save};

mod common;

/// The save every scenario is a change to, which is the one committed whole.
const WHOLE: &str = "save_template.ntwtf";

/// One scenario, which is a change to the template and therefore a chain of one link.
const CHANGED: &str = "at-trashcan.ntwtf";

/// A moment with every field a different width, so a misplaced one shows in the name.
const STAMP: Stamp = Stamp {
    year: 2026,
    month: 8,
    day: 31,
    hour: 20,
    minute: 13,
    second: 30,
};

/// The id map, which is not committed. Nothing to read means nothing here can be packed.
fn orders() -> Option<Orders> {
    let path = common::repo_root().join(lua_simx::ORDERS_FILE_NAME);
    let text = fs::read_to_string(&path).ok()?;
    Some(Orders::read(&text).unwrap_or_else(|why| panic!("{}: {why}", path.display())))
}

/// Where a committed save is.
fn save(name: &str) -> PathBuf {
    let testing = common::repo_root().join("testing");
    if name == WHOLE {
        testing.join(name)
    } else {
        testing.join("scenarios").join(name)
    }
}

/// What an archive holds, by entry name.
fn entries_in(archive: &Path) -> Vec<(String, Vec<u8>)> {
    let file = fs::File::open(archive).expect("the archive opens");
    let mut zip = zip::ZipArchive::new(file).expect("it is a zip");

    let mut held = Vec::new();
    for at in 0..zip.len() {
        let mut entry = zip.by_index(at).expect("an entry reads");
        let name = entry.name().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("its bytes read");
        held.push((name, bytes));
    }

    held
}

/// Packs one committed save and checks the archive against what the save described.
fn packs(name: &str, orders: &Orders) {
    let source = save(name);
    let into = common::repo_root().join(".build").join("packed-saves");
    let asked = into.join(format!("{name}.zip"));

    let written = packed_save::pack(&OnDisk, &source, &asked, Some(orders), STAMP)
        .unwrap_or_else(|why| panic!("{name}: {why}"));

    // The name it was asked for carries no timestamp, so it is given one and the archive
    // goes to the name that makes rather than the one that was asked for.
    let stem = name.strip_suffix(".ntwtf").expect("a save is named .ntwtf");
    let archive_name = format!("{stem}{}", STAMP.as_name());
    assert_eq!(written, into.join(format!("{archive_name}{ZIP_SUFFIX}")));

    let entries = entries_in(&written);
    assert!(
        entries.len() > 1,
        "{name} packed to {} entries",
        entries.len()
    );
    for (held, _) in &entries {
        assert!(
            held.starts_with(&archive_name),
            "{name}: entry '{held}' is not prefixed with '{archive_name}'",
        );
    }

    // The blob first, and reading it back gives the tables the expanded save described.
    let (first, bytes) = &entries[0];
    assert_eq!(*first, format!("{archive_name}{LUA_SUFFIX}"));
    let read = lua_blob::read(bytes).unwrap_or_else(|why| panic!("{name}: {why}"));
    let expected = lua_parts::document(&OnDisk, &source, Some(orders))
        .unwrap_or_else(|why| panic!("{name}: {why}"));
    assert_eq!(read, expected, "{name}: the blob is not what was packed");
}

#[test]
fn a_save_committed_whole_packs_into_an_archive_that_reads_back() {
    let Some(orders) = orders() else {
        println!(
            "no {} beside the repository, so nothing is packed",
            lua_simx::ORDERS_FILE_NAME
        );
        return;
    };

    packs(WHOLE, &orders);
}

#[test]
fn a_save_committed_as_a_change_to_another_packs_the_same_way() {
    let Some(orders) = orders() else {
        println!(
            "no {} beside the repository, so nothing is packed",
            lua_simx::ORDERS_FILE_NAME
        );
        return;
    };

    packs(CHANGED, &orders);
}
