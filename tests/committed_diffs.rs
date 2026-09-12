// SPDX-License-Identifier: MIT
//! Can Rust read every diff this repository has committed?
//!
//! ## The two things every one of them has to do
//!
//! READ, which is what matters on every offline run: 26 committed files are read to build
//! a scenario's world, and one that mangled would answer against a world nobody wrote.
//!
//! And SAY WHAT IT IS A DIFF OF. Each names its base itself - the JSON kinds in `_base`,
//! the unified diffs on their first line - so a diff can be followed from its own path
//! rather than only through whatever manifest happens to sit beside it. The file it names
//! has to be there, and that is checked rather than assumed.
//!
//! ## Byte-for-byte, which it now is
//!
//! Every committed diff was written by this build's writer, so re-spelling one has to give
//! back what is on disk. That was not so while the corpus carried .NET's output - the
//! member diffs had 493 escapes with mixed hex casing - and it is the bar de-xz48.4 set by
//! writing all of them again.

use std::fs;
use std::path::{Path, PathBuf};

use lookahead_engine::formats::expanded_save;
use lookahead_engine::formats::sparse;
use lookahead_engine::formats::sparse_diff;
use lookahead_engine::formats::text_diff;

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
fn every_committed_diff_is_spelled_the_way_this_writer_spells_one() {
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

    assert!(
        differing.is_empty(),
        "{} of {} committed sparse diffs are spelled some other way: {differing:?}",
        differing.len(),
        diffs.len(),
    );
}

/// And every one of them says what it is a diff of, which is what makes it readable alone.
#[test]
fn every_committed_diff_names_the_file_it_is_a_diff_of() {
    let diffs = committed_diffs();
    assert!(diffs.len() >= AT_LEAST, "only {} were found", diffs.len());

    for path in &diffs {
        let name = name_of(path);
        let text = fs::read_to_string(path).unwrap_or_else(|why| panic!("{name}: {why}"));
        let tree = sparse::read(&text, &name).unwrap_or_else(|why| panic!("{why}"));

        let base = sparse_diff::base_of(&tree)
            .unwrap_or_else(|| panic!("{name} names nothing to be a diff of"));
        let beneath = path
            .parent()
            .expect("a diff sits in a directory")
            .join(base);

        assert!(
            beneath.is_file(),
            "{name} is a diff of '{base}', which is not there",
        );
    }
}

/// Every `*.diff` under the scenarios, which is where the text members' diffs live.
fn committed_text_diffs() -> Vec<PathBuf> {
    let scenarios = common::repo_root().join("testing").join("scenarios");
    let mut found = Vec::new();
    collect_diffs(&scenarios, &mut found);
    found.sort();
    found
}

fn collect_diffs(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_diffs(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "diff") {
            found.push(path);
        }
    }
}

/// Both committed text diffs are in a shape this build can read.
///
/// NOT APPLIED TO THE REAL THING, because the text a diff was taken against is inside a
/// packed save and getting it means resolving the base chain - which is de-xz48.6.2's
/// subject. What this catches is the one failure that would otherwise wait until then: a
/// diff whose SHAPE the reader has no case for.
///
/// Applying each against an empty baseline is how that is told apart. A diff that parsed
/// and then found nothing where its context should be reports a mismatch; one that never
/// parsed reports a malformed document. The first is expected and the second is the
/// failure.
#[test]
fn every_committed_text_diff_is_in_a_shape_this_build_reads() {
    let diffs = committed_text_diffs();
    assert_eq!(
        diffs.len(),
        2,
        "this repository carries two text-member diffs; found {}",
        diffs.len(),
    );

    for path in &diffs {
        let name = name_of(path);
        let patch = fs::read_to_string(path).unwrap_or_else(|why| panic!("{name}: {why}"));

        match text_diff::apply("", &patch, &name) {
            Err(text_diff::TextDiffFault::Mismatch(_, _)) | Ok(_) => {}
            Err(malformed) => panic!("{malformed}"),
        }

        // AND IT SAYS WHAT IT IS A DIFF OF, on its first line, which is where a unified
        // diff's `_base` lives. See the note on that in the text-diff module.
        let base = text_diff::base_of(&patch, &name).unwrap_or_else(|why| panic!("{why}"));
        let beneath = path
            .parent()
            .expect("a diff sits in a directory")
            .join(&base);

        assert!(
            beneath.is_file(),
            "{name} is a diff of '{base}', which is not there",
        );
    }
}

/// Every expanded save that carries a manifest.
fn committed_saves() -> Vec<PathBuf> {
    let scenarios = common::repo_root().join("testing").join("scenarios");
    let Ok(entries) = fs::read_dir(&scenarios) else {
        return Vec::new();
    };

    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join(expanded_save::MANIFEST_NAME).is_file())
        .collect();
    found.sort();
    found
}

#[test]
fn every_committed_manifest_reads() {
    let saves = committed_saves();
    assert!(
        saves.len() >= 20,
        "found only {} saves with a manifest, which is fewer than this repository carries",
        saves.len(),
    );

    for save in &saves {
        let name = save.file_name().unwrap_or_default().to_string_lossy();
        let path = save.join(expanded_save::MANIFEST_NAME);
        let text = fs::read_to_string(&path).unwrap_or_else(|why| panic!("{name}: {why}"));

        let manifest =
            expanded_save::read_manifest(&text, &name).unwrap_or_else(|why| panic!("{why}"));

        assert!(
            manifest.stem().is_some(),
            "{name}: its members do not agree about the save's own name",
        );
    }
}

/// And every one of them resolves, through however many bases it takes, to real bytes.
///
/// THE PASS-THROUGH MEMBERS ONLY. The Lua blob is de-xz48.6.3, and this deliberately does
/// not reach for it - what is under test is the manifest, the member kinds and the chain,
/// which is everything about an expanded save that is not the game's binary format.
#[test]
fn every_committed_save_resolves_to_its_members() {
    for save in &committed_saves() {
        let name = save.file_name().unwrap_or_default().to_string_lossy();
        let members = expanded_save::members_of(&expanded_save::OnDisk, save)
            .unwrap_or_else(|why| panic!("{name}: {why}"));

        assert!(!members.is_empty(), "{name} resolved to no members at all");
        for (suffix, bytes) in &members {
            assert!(
                !bytes.is_empty(),
                "{name}: its '{suffix}' member resolved to nothing",
            );
        }
    }
}
