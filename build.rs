// SPDX-License-Identifier: MIT
//! Stamps every build of the look-ahead library with the source it was built from.
//!
//! ## Why a binary needs to say this about itself
//!
//! A native library is copied into the game by one build system and deployed by another,
//! and neither can tell whether the copy it is holding was built from the code in front of
//! it. On 2026-09-04 that cost a wrong conclusion: a session built the library in DEBUG,
//! the plugin's build prefers RELEASE where one exists, and every in-game run afterwards
//! used a release library from the previous day. The suite passed 66 of 66 checks against
//! an engine that did not contain the change under test, and nothing anywhere said so.
//!
//! ## What is written
//!
//! `lookahead_engine.built.json` beside the library, carrying the commit and a TREE HASH -
//! a sha over the working tree including uncommitted changes to tracked files, which is
//! what a commit alone cannot give. The check that matters is an equality between that hash
//! and the same hash computed when the game is about to run.
//!
//! ## How the hash is taken
//!
//! `git add -u` into a COPY of the index, then `git ls-files -s` over the paths this
//! library is built from, hashed. The copy is what lets uncommitted changes count: the real
//! index and the working tree are untouched, and `git status` is unchanged afterwards.
//!
//! ONLY THIS LIBRARY'S SOURCES, and that narrowness is the point rather than an
//! optimisation. A hash over the whole tree changes when a C# harness file is edited, which
//! happens constantly and has no bearing on what the library does - so the check would
//! refuse runs that are perfectly sound, and a guard that cries wolf is one people learn to
//! pass over. Narrow enough to be trusted is worth more than broad enough to be certain.
//!
//! Untracked files are deliberately not counted - `git add -u` rather than `-A`. A file
//! nothing references cannot change what the library does, and one that IS referenced
//! fails the build rather than producing a stale artefact quietly.
//!
//! ## When it cannot say
//!
//! No git, no repository, or git refusing for any reason writes `nogit` rather than failing
//! the build. A stamp that admits it does not know is honest; losing the build over a label
//! is not. The reader treats `nogit` as "cannot be checked" and says so.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What a stamp says when git cannot answer.
const NO_REVISION: &str = "nogit";

/// Everything this engine is built from, as git spells the paths.
///
/// RESTATED IN THE READER, in tools/GameAutomation/NativeEngineStamp.cs, because the two
/// have to ask git the same question and neither can call the other. A path added to one
/// and not the other makes the check silently narrower, which is why they name each other.
///
/// ## The exclusion is not a detail
///
/// `src/` in this repository holds the Rust crate AND eight C# projects, all of them named
/// `GlobalConversationTracker.*`. Without the last entry the stamp counts every C# file,
/// which is exactly the failure the note above warns about: measured 2026-09-05 (de-b0e7),
/// a commit touching one plugin file moved the hash and the in-game harness refused to run
/// against an engine that was byte-for-byte the right one. A guard that cries wolf is one
/// people learn to pass over.
///
/// EXCLUDED BY PATTERN RATHER THAN LISTED POSITIVELY, so that a Rust module added tomorrow
/// is covered without anyone remembering to add it - which is the direction the mistake
/// must not be able to go, since a source left out is a stale engine that passes.
///
/// [`watch_sources`] reads this list too, and reads the exclusions with it, so what Cargo
/// re-runs this for and what the hash is taken over are the same set.
const SOURCES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "build.rs",
    "src",
    ":(exclude)src/GlobalConversationTracker.*",
    // The wire's schema, which this build generates types from. A change to it changes
    // what the engine says and understands, so a stamp that left it out would call an
    // engine current while it spoke the previous shape.
    "proto",
];

fn main() {
    // RE-RUN FOR THE PATHS THE STAMP IS BUILT FROM, which is [`SOURCES`] and therefore
    // cannot fall behind it: a path named there is a path watched here, and a path left
    // out of there was already outside the hash. Saying nothing at all makes Cargo re-run
    // this whenever ANY file in the package changes, which recompiles the library for an
    // edit to a measurement or a test - work that cannot alter the answer, since the hash
    // is taken over these paths and no others.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let listed = source_listing(&root);
    let watched = watch_sources(&root);
    if let Some(listed) = &listed {
        refuse_unwatched(listed, &watched);
    }

    generate_wire();

    let stamp = format!(
        "{{\n  \"commit\": \"{}\",\n  \"tree\": \"{}\",\n  \"profile\": \"{}\"\n}}\n",
        git(&root, &["rev-parse", "HEAD"]).unwrap_or_else(|| NO_REVISION.to_string()),
        listed.as_deref().map_or_else(
            || NO_REVISION.to_string(),
            |listed| digest(listed.as_bytes())
        ),
        std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string()),
    );

    // OUT_DIR is target/<profile>/build/<crate>-<hash>/out, so the library sits three
    // levels up. Derived rather than assumed from PROFILE, which does not name the
    // directory for a custom profile.
    let Some(out) = std::env::var_os("OUT_DIR").map(PathBuf::from) else {
        return;
    };
    let Some(target) = out.ancestors().nth(3) else {
        return;
    };

    let _ = std::fs::write(target.join("lookahead_engine.built.json"), stamp);
}

/// How a git pathspec spells "and not this".
const EXCLUDE: &str = ":(exclude)";

/// Tells Cargo to re-run this for a change to any source the stamp is taken over.
///
/// ## Why this is not one line per [`SOURCES`] entry
///
/// `rerun-if-changed` takes a PATH, not a git pathspec, and Cargo walks a directory whole.
/// So naming `src` watches the eight C# projects that share it with the Rust crate, and the
/// `:(exclude)` that keeps them out of the hash does nothing about the trigger. Re-running a
/// build script marks its crate dirty, and this crate is the workspace root that every test,
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

/// Fails the build if any source the stamp is taken over is not one Cargo will re-run for.
///
/// ## Why this is checked rather than reasoned about
///
/// The two lists come from one declaration, so they cannot disagree by being edited apart -
/// but [`watch`] descends a directory to skip what is excluded, and a source that landed
/// somewhere that descent does not reach would be HASHED WITHOUT BEING WATCHED. That is the
/// silent failure the whole stamp exists to prevent, one level down: the library would go
/// stale and its stamp would still say it was current.
///
/// The listing is already in hand for the hash, so the check costs a string scan.
fn refuse_unwatched(listed: &str, watched: &[String]) {
    let unwatched: Vec<&str> = listed
        .lines()
        .filter_map(|line| line.split_once('\t').map(|(_, path)| path))
        .filter(|path| {
            !watched
                .iter()
                .any(|under| *path == under || path.starts_with(&format!("{under}/")))
        })
        .collect();

    assert!(
        unwatched.is_empty(),
        "the stamp is taken over {} source(s) Cargo will not re-run this for, so the library \
         could go stale while the stamp says it is current: {}",
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

/// One git command's output, or None if it could not be run or failed.
fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Every source the stamp covers, as `git ls-files -s` spells them.
///
/// A blob sha and a path per line, with uncommitted changes to tracked files counted - so
/// hashing the listing hashes the CONTENT of every source without reading one, git having
/// already hashed them all. It is also the list of what has to be watched, which is why it
/// is taken once and used for both.
///
/// NOT TRIMMED, and it matters: the reader in `tools/GameAutomation/NativeEngineStamp.cs`
/// hashes what git printed, trailing newline included, and a stamp that disagreed with its
/// reader would refuse every run.
fn source_listing(root: &Path) -> Option<String> {
    let index = root.join("target").join("built-stamp-index");
    std::fs::create_dir_all(index.parent()?).ok()?;
    std::fs::copy(root.join(".git").join("index"), &index).ok()?;

    let staged = Command::new("git")
        .args(["add", "-u"])
        .current_dir(root)
        .env("GIT_INDEX_FILE", &index)
        .output()
        .ok()?;
    if !staged.status.success() {
        let _ = std::fs::remove_file(&index);
        return None;
    }

    let mut arguments = vec!["ls-files", "-s", "--"];
    arguments.extend_from_slice(SOURCES);
    let listed = Command::new("git")
        .args(&arguments)
        .current_dir(root)
        .env("GIT_INDEX_FILE", &index)
        .output()
        .ok()?;

    let _ = std::fs::remove_file(&index);
    if !listed.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&listed.stdout).into_owned())
}

/// A hex digest of some bytes, in a form the reader can reproduce.
///
/// FNV-1a, and 64 bits of it. Not a cryptographic hash and not trying to be: it compares
/// one build's sources against one checkout's, where the only adversary is forgetfulness.
/// Written out by hand because the alternative is a dependency in the build script of a
/// library that has to compile on a machine that may have nothing cached.
fn digest(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    format!("{hash:016x}")
}
