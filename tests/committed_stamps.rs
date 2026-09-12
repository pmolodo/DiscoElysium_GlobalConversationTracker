// SPDX-License-Identifier: MIT
//! Does every committed document say it is something this build writes, at this version?
//!
//! ## The standing rule this enforces
//!
//! ONLY THE LATEST VERSION OF EVERY FORMAT IS COMMITTED. A version bump regenerates every
//! file of that format in the repository, so no reader here has to understand an older one
//! and the code that would have read one goes with the bump. A committed file at an older
//! version would quietly make that false, and every reader refuses one - so the failure
//! would arrive as a run that cannot read its own fixtures.
//!
//! `gct-engine-host rewrite` is what regenerates a save after a bump, and
//! `gct-engine-host convert` is what brings a file from outside the repository forward.
//!
//! ## Why the list of formats is not here
//!
//! Because it was in two places before, in two languages, and a format present in one and
//! missing from the other fails silently: a file whose name is unknown reads as "not one of
//! ours" rather than as an error. Both this and the converter read
//! `lookahead_engine::formats::registry` now.
//!
//! ## What is passed over, and why that is not a hole
//!
//! A file that is not JSON, and a JSON file that names no format at all. The second is the
//! one worth explaining: `testing/` holds documents this repository never wrote - the
//! game's own settings, a scenario's expectations before it was stamped - and demanding a
//! header of those would be demanding this build own them. What the rule is about is files
//! this build DID write, and those all name themselves.

use std::fs;
use std::path::{Path, PathBuf};

use lookahead_engine::formats::header;
use lookahead_engine::formats::registry;

mod common;

/// How many stamped documents the repository is known to carry.
///
/// A LOWER BOUND, asserted so that a test finding none - a moved directory, a walk that
/// stopped descending - fails rather than passing over an empty list.
const AT_LEAST: usize = 50;

/// Every `.json` file committed under `testing`, wherever it sits.
fn committed_documents() -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(&common::repo_root().join("testing"), &mut found);
    found.sort();
    found
}

fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "json") {
            found.push(path);
        }
    }
}

#[test]
fn every_committed_document_is_at_the_version_this_build_writes() {
    let documents = committed_documents();
    assert!(
        documents.len() >= AT_LEAST,
        "only {} committed documents were found; there are at least {AT_LEAST}",
        documents.len(),
    );

    let mut stamped = 0;
    let mut wrong = Vec::new();

    for path in &documents {
        let text =
            fs::read_to_string(path).unwrap_or_else(|why| panic!("{}: {why}", path.display()));
        let Ok(document) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(named) = document
            .get(header::FORMAT_KEY)
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };

        stamped += 1;
        let shown = path
            .strip_prefix(common::repo_root())
            .unwrap_or(path)
            .display();

        match registry::current_version(named) {
            None => wrong.push(format!(
                "{shown}: '{named}' is not a format this build writes"
            )),
            Some(current) => {
                let version = document
                    .get(header::VERSION_KEY)
                    .and_then(serde_json::Value::as_u64);
                match version {
                    None => wrong.push(format!("{shown}: a {named} carrying no version")),
                    Some(found) if found != u64::from(current) => wrong.push(format!(
                        "{shown}: {named} version {found}, and this build writes {current}",
                    )),
                    Some(_) => {}
                }
            }
        }
    }

    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert!(stamped > 0, "nothing under testing names a format");
    println!(
        "{stamped} of {} committed documents name a format",
        documents.len()
    );
}

/// And the registry is not a list somebody remembered to add to.
///
/// Every format the repository has a committed FILE of has to be in it, which is the half
/// this can check from outside: the modules' own constants are checked against the registry
/// by its own tests, and this catches a fixture format nobody registered.
#[test]
fn every_format_a_committed_document_names_is_registered() {
    let mut named: Vec<String> = Vec::new();

    for path in committed_documents() {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(document) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if let Some(format) = document
            .get(header::FORMAT_KEY)
            .and_then(serde_json::Value::as_str)
            && !named.iter().any(|seen| seen == format)
        {
            named.push(format.to_string());
        }
    }

    named.sort();
    let unknown: Vec<&String> = named
        .iter()
        .filter(|format| registry::current_version(format).is_none())
        .collect();

    assert!(
        unknown.is_empty(),
        "committed documents name formats the registry does not: {unknown:?}",
    );
    println!("committed documents name: {named:?}");
}
