// SPDX-License-Identifier: MIT
//! Can Rust read every sparse diff this repository has committed?
//!
//! ## Why this is the bar, and byte-for-byte with the C# writer is not
//!
//! Two things have to be true before the C# copy of this format can go. READING is the
//! one that matters today: 26 committed files are read on every offline run, and a reader
//! that mangled one would answer a scenario against a world that is not the one on disk.
//! That is checked here against the files themselves.
//!
//! WRITING has to be STABLE rather than identical. The committed files were written by
//! .NET, and the sparse ones happen to come out the same way this writer does - same
//! two-space indent, same separators, same absence of escapes - but that is a coincidence
//! worth not depending on, because the MEMBER diffs beside them do not: those carry 493
//! escapes from .NET's default encoder, mixed hex casing and all. So what is held here is
//! that this writer's own output reads back as the same tree, and the byte-for-byte bar
//! begins when de-xz48.4 rewrites the committed files with it.
//!
//! The test says out loud how many of them this writer would rewrite, so the size of that
//! change is known in advance rather than discovered.

use std::fs;
use std::path::{Path, PathBuf};

use lookahead_engine::formats::sparse;
use lookahead_engine::formats::sparse_diff;

mod common;

/// How many sparse diffs the repository is known to carry.
///
/// A LOWER BOUND, asserted so that a test finding none - a moved directory, a glob that
/// stopped matching - fails rather than passing over an empty list.
const AT_LEAST: usize = 20;

/// Every `*.lua.parts/*.json` under the scenarios, which is where the sparse diffs live.
fn committed_diffs() -> Vec<PathBuf> {
    let scenarios = common::repo_root().join("testing").join("scenarios");
    let mut found = Vec::new();
    collect(&scenarios, &mut found);
    found.sort();
    found
}

fn collect(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "json")
            && path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|parent| parent.to_string_lossy().ends_with(".lua.parts"))
        {
            found.push(path);
        }
    }
}

/// What a file is called in a failure, which is the part a reader can act on.
fn name_of(path: &Path) -> String {
    let parent = path
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file = path.file_name().unwrap_or_default().to_string_lossy();
    format!("{parent}/{file}")
}

#[test]
fn every_committed_sparse_diff_reads() {
    let diffs = committed_diffs();
    assert!(
        diffs.len() >= AT_LEAST,
        "found only {} sparse diffs, which is fewer than the {AT_LEAST} this repository \
         carries - the test is looking in the wrong place",
        diffs.len(),
    );

    for path in &diffs {
        let name = name_of(path);
        let text = fs::read_to_string(path).unwrap_or_else(|why| panic!("{name}: {why}"));
        let tree = sparse::read(&text, &name).unwrap_or_else(|why| panic!("{why}"));

        // AND IS A DIFF, not merely valid JSON. Applying it to an empty tree exercises the
        // header check and the shape of both halves, which is everything about the
        // document that this build has an opinion on.
        sparse_diff::apply(&sparse::SparseMap::new(), &tree)
            .unwrap_or_else(|why| panic!("{name}: {why}"));
    }
}

/// What this writer produces reads back as what it was given.
///
/// The property that lets de-xz48.4 rewrite the committed files safely: whatever the
/// encoding, a tree written and read again is the same tree.
#[test]
fn what_this_writer_produces_reads_back_as_the_same_tree() {
    for path in &committed_diffs() {
        let name = name_of(path);
        let text = fs::read_to_string(path).unwrap_or_else(|why| panic!("{name}: {why}"));
        let tree = sparse::read(&text, &name).unwrap_or_else(|why| panic!("{why}"));

        let written = sparse::write(&tree);
        let again = sparse::read(&written, &name).unwrap_or_else(|why| panic!("{why}"));

        assert_eq!(again, tree, "{name} did not survive being written");
        assert_eq!(
            sparse::write(&again),
            written,
            "{name} did not settle after one round trip",
        );
    }
}

/// How many committed files this writer would rewrite, said out loud.
///
/// Not an assertion about the number: it is a fact about files this build did not write,
/// and pinning it would turn every future commit of a fixture into a failure here. What it
/// is for is that de-xz48.4 knows the size of its rewrite before it starts.
#[test]
fn how_many_committed_diffs_this_writer_would_respell() {
    let diffs = committed_diffs();
    let mut differing = Vec::new();

    for path in &diffs {
        let name = name_of(path);
        let text = fs::read_to_string(path).unwrap_or_else(|why| panic!("{name}: {why}"));
        let tree = sparse::read(&text, &name).unwrap_or_else(|why| panic!("{why}"));

        if sparse::write(&tree) != text {
            differing.push(name);
        }
    }

    println!(
        "{} of {} committed sparse diffs would be respelled by this writer",
        differing.len(),
        diffs.len(),
    );
    for name in &differing {
        println!("  {name}");
    }
}
