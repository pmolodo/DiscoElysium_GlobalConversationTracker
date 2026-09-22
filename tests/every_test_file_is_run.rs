// SPDX-License-Identifier: MIT
//! A FILE IN `tests/` THAT NOTHING BUILDS IS THE PRICE of one test binary, and this is what
//! refuses to pay it.
//!
//! `autotests` is off, so `tests/` is not a category Cargo reads: a test target exists because
//! `tests/suite.rs` names it or `Cargo.toml` declares it. Nothing about writing a new test file
//! says so, and the failure is the quiet kind - the file compiles nowhere, runs nowhere, and
//! reports nothing, which reads exactly like a test that passes.
//!
//! So the roll is checked against the directory rather than trusted. This is the same bargain
//! `environment_table` makes for the variables: a list that has to be kept in step is fine as
//! long as something fails when it is not.

use std::collections::HashSet;
use std::path::PathBuf;

/// Where the roll of gathered files lives, relative to the package root.
const SUITE: &str = "tests/suite.rs";

/// Where a target that cannot join the suite is declared instead.
const MANIFEST: &str = "Cargo.toml";

/// The package root, which is where both of those are.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `tests/*.rs` in the repository, by file stem.
fn files_on_disk() -> HashSet<String> {
    let mut found = HashSet::new();
    let entries = std::fs::read_dir(root().join("tests")).expect("the tests folder reads");
    for entry in entries {
        let path = entry.expect("the entry reads").path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            let stem = path.file_stem().expect("a name").to_string_lossy();
            found.insert(stem.into_owned());
        }
    }
    found
}

/// Every stem the suite gathers, read from its `mod` declarations.
fn gathered_by_the_suite() -> HashSet<String> {
    let text = std::fs::read_to_string(root().join(SUITE)).expect("the suite reads");
    text.lines()
        .filter_map(|line| line.strip_prefix("mod "))
        .filter_map(|rest| rest.strip_suffix(';'))
        .map(str::to_owned)
        .collect()
}

/// Every stem the manifest declares a target for, read from its `path = "tests/*.rs"` lines.
fn declared_in_the_manifest() -> HashSet<String> {
    let text = std::fs::read_to_string(root().join(MANIFEST)).expect("the manifest reads");
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("path = \"tests/"))
        .filter_map(|rest| rest.strip_suffix(".rs\""))
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_test_file_is_named_by_the_suite_or_by_the_manifest() {
    let suite_stem = PathBuf::from(SUITE)
        .file_stem()
        .expect("a name")
        .to_string_lossy()
        .into_owned();

    let gathered = gathered_by_the_suite();
    let declared = declared_in_the_manifest();

    let mut orphans: Vec<String> = files_on_disk()
        .into_iter()
        .filter(|stem| *stem != suite_stem)
        .filter(|stem| !gathered.contains(stem) && !declared.contains(stem))
        .collect();
    orphans.sort();

    assert!(
        orphans.is_empty(),
        "these test files are built by nothing and run by nobody: {}\n\
         add each to {SUITE}, or declare it in {MANIFEST} if it needs its own binary",
        orphans.join(", ")
    );
}

#[test]
fn the_suite_gathers_nothing_that_is_not_there() {
    let on_disk = files_on_disk();

    let mut missing: Vec<String> = gathered_by_the_suite()
        .into_iter()
        .filter(|stem| !on_disk.contains(stem))
        .collect();
    missing.sort();

    assert!(
        missing.is_empty(),
        "{SUITE} names files that do not exist: {}",
        missing.join(", ")
    );
}

#[test]
fn nothing_is_both_gathered_and_declared() {
    let mut both: Vec<String> = gathered_by_the_suite()
        .intersection(&declared_in_the_manifest())
        .cloned()
        .collect();
    both.sort();

    assert!(
        both.is_empty(),
        "{} would be compiled twice, once into the suite and once as its own target: {}",
        match both.len() {
            1 => "this file",
            _ => "these files",
        },
        both.join(", ")
    );
}
