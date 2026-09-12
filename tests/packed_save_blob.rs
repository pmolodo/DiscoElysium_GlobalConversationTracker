// SPDX-License-Identifier: MIT
//! Does the blob reader survive a save the game actually wrote?
//!
//! ## Why this reaches outside the repository
//!
//! No blob is committed. The saves under `testing/scenarios` are EXPANDED - their Lua
//! tables are split into a directory of JSON - and the binary form only exists inside a
//! packed archive, which is a build artefact rather than a fixture.
//!
//! The harness leaves those artefacts behind: every in-game run packs the saves it stages
//! into `.build/automation`. So this reads whatever is there, and SKIPS when there is
//! nothing, the same way the tests that need a conversation index do. A machine that has
//! run the suite checks the real thing; a fresh clone checks nothing and says so rather
//! than failing over an absence.
//!
//! ## Why the check is byte-for-byte
//!
//! A blob written slightly wrong is not a save that loads slightly wrong. The game reads
//! the whole archive or ignores it, and an archive it ignores shows up as a main menu with
//! no Continue and Load Game greyed out, with nothing anywhere saying why. Reading it back
//! exactly is the only bar worth having.
//!
//! ## And what the sparse form promises about the same tables
//!
//! It does NOT promise the bytes. It drops the layout - where a table's list part ended and
//! what order its entries came in - and keeps every key and every value. So the second test
//! here holds it to exactly that over the same real saves: a table written out and read
//! back holds the same entries, whatever order they end up in.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

use lookahead_engine::formats::lua_blob::{self, LuaTable, LuaValue};
use lookahead_engine::formats::lua_sparse;
use lookahead_engine::formats::sparse::SparseValue;

mod common;

/// The packed saves an in-game run leaves behind, newest first.
fn packed_saves() -> Vec<PathBuf> {
    let staged = common::repo_root().join(".build").join("automation");
    let Ok(entries) = fs::read_dir(&staged) else {
        return Vec::new();
    };

    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|kind| kind == "zip"))
        .collect();
    found.sort();

    // ONE PER SAVE, not the newest few. A run packs the same save several times, so the
    // newest handful are copies of each other - and what is worth reading is several
    // DIFFERENT saves, since they differ in how much of the game has been played.
    let mut seen = std::collections::HashSet::new();
    found.retain(|path| {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        seen.insert(name.split('(').next().unwrap_or_default().to_string())
    });
    found
}

/// The Lua blob inside a packed save, with the entry's own name.
fn blob_in(path: &PathBuf) -> Option<(String, Vec<u8>)> {
    let file = fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    let named = (0..archive.len()).find_map(|at| {
        let entry = archive.by_index(at).ok()?;
        entry
            .name()
            .ends_with(".ntwtf.lua")
            .then(|| entry.name().to_string())
    })?;

    let mut entry = archive.by_name(&named).ok()?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).ok()?;
    Some((named, bytes))
}

/// How many packed saves to read. They are large and they are all the same shape.
const ENOUGH: usize = 5;

#[test]
fn a_blob_the_game_wrote_reads_and_writes_back_byte_for_byte() {
    let packed = packed_saves();
    if packed.is_empty() {
        println!(
            "no packed saves under .build/automation, so there is no blob to read; \
             run the in-game suite to make some"
        );
        return;
    }

    let mut checked = 0;
    for path in packed.iter().take(ENOUGH) {
        let Some((name, bytes)) = blob_in(path) else {
            continue;
        };

        let blob = lua_blob::read(&bytes).unwrap_or_else(|why| panic!("{name}: {why}"));
        assert_eq!(
            lua_blob::write(&blob),
            bytes,
            "{name} did not come back the way it went in",
        );

        // AND IT WAS ACTUALLY READ, rather than passed through as trailing bytes. A reader
        // that gave up at the first marker would round-trip perfectly and mean nothing.
        assert_eq!(blob.tables.len(), lua_blob::TABLE_NAMES.len());
        assert!(
            blob.tables.iter().any(|table| matches!(
                table,
                lua_blob::LuaValue::Table(held) if !held.dict.is_empty()
            )),
            "{name}: every one of its five tables came back empty",
        );

        println!(
            "{name}: {} bytes, {} of them after the five tables",
            bytes.len(),
            blob.trailing.len(),
        );
        checked += 1;
    }

    assert!(
        checked > 0,
        "found {} packed save(s) and no Lua blob in any of them",
        packed.len(),
    );
}

/// Every leaf of a table, by the path it sits at.
///
/// A LIST ENTRY IS NAMED BY ITS OWN 1-BASED INDEX, which is the key Lua gives it, so an
/// entry is named the same way whichever half of the table it sits in. That is the point:
/// the sparse form is free to move the boundary, and this is what has to survive it.
fn leaves(table: &LuaTable, path: &str, out: &mut BTreeMap<String, String>) {
    if table.list.is_empty() && table.dict.is_empty() {
        out.insert(path.to_string(), "an empty table".to_string());
        return;
    }

    for (at, value) in table.list.iter().enumerate() {
        leaf(&format!("{path}/{}", at + 1), value, out);
    }

    for (key, value) in &table.dict {
        leaf(&format!("{path}/{key}"), value, out);
    }
}

fn leaf(path: &str, value: &LuaValue, out: &mut BTreeMap<String, String>) {
    match value {
        LuaValue::Table(child) => leaves(child, path, out),
        held => {
            out.insert(path.to_string(), format!("{held:?}"));
        }
    }
}

/// Where two sets of leaves first disagree, said in one line rather than by dumping both.
fn first_difference(
    was: &BTreeMap<String, String>,
    now: &BTreeMap<String, String>,
) -> Option<String> {
    for (path, value) in was {
        match now.get(path) {
            Some(held) if held == value => {}
            Some(held) => return Some(format!("{path} was {value} and came back {held}")),
            None => return Some(format!("{path} was {value} and is gone")),
        }
    }

    now.keys()
        .find(|path| !was.contains_key(*path))
        .map(|path| format!("{path} was not there and appeared"))
}

/// Does a real save's table keep every entry through the sparse form?
///
/// The bar is not the bytes and not the order - the form drops both on purpose. It is that
/// nothing is LOST: every key that went in comes back, under the same path, holding the
/// same value, wherever the list boundary ends up falling.
#[test]
fn a_blob_the_game_wrote_keeps_every_entry_through_the_sparse_form() {
    let packed = packed_saves();
    if packed.is_empty() {
        println!(
            "no packed saves under .build/automation, so there is no blob to read; \
             run the in-game suite to make some"
        );
        return;
    }

    let mut checked = 0;
    for path in packed.iter().take(ENOUGH) {
        let Some((name, bytes)) = blob_in(path) else {
            continue;
        };

        let blob = lua_blob::read(&bytes).unwrap_or_else(|why| panic!("{name}: {why}"));
        let mut entries = 0;
        for (table_name, value) in lua_blob::TABLE_NAMES.iter().zip(&blob.tables) {
            let LuaValue::Table(table) = value else {
                panic!("{name}: {table_name} is {value}, not a table");
            };

            let written = lua_sparse::encode(table, table_name)
                .unwrap_or_else(|why| panic!("{name}/{table_name}: {why}"));
            let back = lua_sparse::decode(&SparseValue::Map(written), table_name)
                .unwrap_or_else(|why| panic!("{name}/{table_name}, read back: {why}"));

            let mut was = BTreeMap::new();
            leaves(table, "", &mut was);
            let mut now = BTreeMap::new();
            leaves(&back, "", &mut now);

            if let Some(how) = first_difference(&was, &now) {
                panic!("{name}/{table_name}: {how}");
            }
            entries += was.len();
        }

        println!("{name}: {entries} entries through the sparse form and back");
        checked += 1;
    }

    assert!(
        checked > 0,
        "found {} packed save(s) and no Lua blob in any of them",
        packed.len(),
    );
}
