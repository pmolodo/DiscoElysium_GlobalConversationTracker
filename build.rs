// SPDX-License-Identifier: MIT
//! Generates the wire's Rust types, and tells Cargo which sources this library has.
//!
//! ## Why the second half is not Cargo's default
//!
//! A build script with no `rerun-if-changed` line is re-run whenever ANY file in the
//! package changes, and re-running one marks its crate dirty. This crate is the workspace
//! root that every test, example and binary depends on, so an edit to a test relinked all
//! of them. [`SOURCES`] names what the library is actually built from, and nothing else
//! triggers a rebuild.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Everything this engine is built from, as git spells the paths.
///
/// ## The exclusion is not a detail
///
/// `src/` in this repository holds the Rust crate AND eight C# projects, all of them named
/// `GlobalConversationTracker.*`. Without the last entry an edit to any one of them makes
/// this crate dirty, and every test, example and binary in the workspace relinks behind it.
/// Measured 2026-09-12: 0.4 seconds with nothing changed, 2 minutes 47 with one C# file
/// touched and nothing else.
///
/// EXCLUDED BY PATTERN RATHER THAN LISTED POSITIVELY, so that a Rust module added tomorrow
/// is covered without anyone remembering to add it - which is the direction the mistake
/// must not be able to go, since a source left out is a library that does not rebuild when
/// it changes.
///
/// [`watch_sources`] reads this list and the exclusions in it, and [`refuse_unwatched`]
/// fails the build where what git says the library is built from is not what Cargo was told
/// to watch.
const SOURCES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "build.rs",
    "src",
    ":(exclude)src/GlobalConversationTracker.*",
    // The wire's schema, which this build generates types from. A change to it changes what
    // the engine says and understands, so leaving it out would keep an engine that speaks
    // the previous shape.
    "proto",
];

fn main() {
    // RE-RUN FOR THE PATHS THE LIBRARY IS BUILT FROM, which is [`SOURCES`] and therefore
    // cannot fall behind it: a path named there is a path watched here. Saying nothing at
    // all makes Cargo re-run this whenever ANY file in the package changes, which
    // recompiles the library for an edit to a measurement or a test.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let watched = watch_sources(&root);
    if let Some(listed) = source_listing(&root) {
        refuse_unwatched(&listed, &watched);
    }

    generate_wire();
}

/// How a git pathspec spells "and not this".
const EXCLUDE: &str = ":(exclude)";

/// Tells Cargo to re-run this for a change to any source the library is built from.
///
/// ## Why this is not one line per [`SOURCES`] entry
///
/// `rerun-if-changed` takes a PATH, not a git pathspec, and Cargo walks a directory whole.
/// So naming `src` watches the eight C# projects that share it with the Rust crate, and the
/// `:(exclude)` in [`SOURCES`] does nothing about the trigger. Re-running a build
/// script marks its crate dirty, and this crate is the workspace root that every test,
/// example and binary depends on - so editing one C# file cost a relink of all of them.
/// Measured 2026-09-12: 0.4 seconds with nothing changed, 2 minutes 47 with one C# file
/// touched and nothing else.
///
/// A directory holding an excluded path is therefore listed a level at a time, and every
/// other path stays one line for Cargo to walk.
///
/// ## What still covers a module added tomorrow
///
/// `src` itself is not watched, so a new entry appearing directly under it is not noticed by
/// being created. It is noticed anyway, because a Rust file nothing declares is not part of
/// the crate: a new module means a `mod` line in a file that IS watched, and the same goes
/// for anything reached by `include!`. That is the direction this must not be able to go
/// wrong in, and it holds for the same reason the exclusion is a pattern rather than a list.
fn watch_sources(root: &Path) -> Vec<String> {
    let excluded: Vec<String> = SOURCES
        .iter()
        .filter_map(|source| source.strip_prefix(EXCLUDE))
        .map(|pattern| pattern.trim_end_matches('*').to_string())
        .collect();

    let mut watched = Vec::new();
    for source in SOURCES {
        if source.starts_with(':') {
            continue;
        }

        watch(root, source, &excluded, &mut watched);
    }

    watched
}

/// Watches one path, or its children where some of them are excluded.
fn watch(root: &Path, source: &str, excluded: &[String], watched: &mut Vec<String>) {
    let within = format!("{source}/");
    let holds_excluded = excluded.iter().any(|prefix| prefix.starts_with(&within));

    // UNREADABLE MEANS WATCH THE WHOLE THING. Listing nothing would leave the sources
    // unwatched and the library stale without a word, where watching too much only costs
    // the rebuild this exists to avoid.
    let entries = holds_excluded.then(|| std::fs::read_dir(root.join(source)).ok());
    let Some(Some(entries)) = entries else {
        println!("cargo::rerun-if-changed={source}");
        watched.push(source.to_string());
        return;
    };

    for entry in entries.flatten() {
        let path = format!("{source}/{}", entry.file_name().to_string_lossy());
        if excluded.iter().any(|prefix| path.starts_with(prefix)) {
            continue;
        }

        watch(root, &path, excluded, watched);
    }
}

/// Fails the build if any source the library is built from is not one Cargo will re-run for.
///
/// ## Why this is checked rather than reasoned about
///
/// The two lists come from one declaration, so they cannot disagree by being edited apart -
/// but [`watch`] descends a directory to skip what is excluded, and a source that landed
/// somewhere that descent does not reach would be BUILT FROM WITHOUT BEING WATCHED. That is
/// a library that keeps answering the way it did before the edit, with nothing to say so.
fn refuse_unwatched(listed: &str, watched: &[String]) {
    let unwatched: Vec<&str> = listed
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .filter(|path| {
            !watched
                .iter()
                .any(|under| *path == under || path.starts_with(&format!("{under}/")))
        })
        .collect();

    assert!(
        unwatched.is_empty(),
        "the library is built from {} source(s) Cargo will not re-run this for, so an edit to \
         one of them would leave the library as it was: {}",
        unwatched.len(),
        unwatched.join(", "),
    );
}

/// The path of the wire schema, relative to the package root.
const SCHEMA: &str = "proto/engine.proto";

/// Generates the wire's Rust types from [`SCHEMA`].
///
/// ## Why this fails the build rather than warning
///
/// The generated module IS the wire. A build that carried on without it would either not
/// compile, which says nothing useful, or compile against a stale copy from a previous run,
/// which is the failure this whole change exists to make impossible: two descriptions of
/// one shape, agreeing only because nobody touched them.
///
/// ## Why the file descriptor set is not written out
///
/// prost can emit one, and reflection is what it is for. Nothing here reflects: both ends
/// are generated from this schema at build time and know every message by name at compile
/// time. A descriptor set would be a second artefact to keep in step for no reader.
fn generate_wire() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let schema = root.join(SCHEMA);
    let includes = [root.join("proto")];

    let files = protox::compile([&schema], includes)
        .unwrap_or_else(|error| panic!("{SCHEMA} does not compile: {error}"));

    prost_build::Config::new()
        // THE REQUEST'S ONEOF IS SIZED BY ITS LOOK-AHEAD ARM, and that is fine: one request
        // is decoded, answered and dropped, never held in bulk. Boxing the arm would change
        // every match on the generated type to save bytes nothing is short of, and the code
        // is generated, so the lint is answered where it is generated.
        .type_attribute(
            ".gct.engine.v1.Request.kind",
            "#[allow(clippy::large_enum_variant)]",
        )
        .compile_fds(files)
        .unwrap_or_else(|error| panic!("{SCHEMA} produced no Rust types: {error}"));
}

/// Every source [`SOURCES`] names, one path per line, as git spells them.
///
/// ASKED OF GIT rather than walked, so that build output and a downloaded game's files can
/// never be in it, and so that the pathspec exclusion is applied by the thing that defines
/// it. `None` where there is no git or no repository, which is a machine that cannot answer
/// rather than an answer of none - [`refuse_unwatched`] checks nothing in that case.
fn source_listing(root: &Path) -> Option<String> {
    let mut arguments = vec!["ls-files", "--"];
    arguments.extend_from_slice(SOURCES);
    let listed = Command::new("git")
        .args(&arguments)
        .current_dir(root)
        .output()
        .ok()?;

    listed
        .status
        .success()
        .then(|| String::from_utf8_lossy(&listed.stdout).into_owned())
}
