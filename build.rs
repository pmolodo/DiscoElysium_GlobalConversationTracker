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

/// Everything this library is built from, as git spells the paths.
///
/// RESTATED IN THE READER, in tools/GameAutomation/NativeEngineStamp.cs, because the two
/// have to ask git the same question and neither can call the other. A path added to one
/// and not the other makes the check silently narrower, which is why they name each other.
const SOURCES: &[&str] = &["Cargo.toml", "Cargo.lock", "build.rs", "src"];

fn main() {
    // No rerun-if-changed: the default is to rerun when anything in the package changes,
    // which is exactly when the stamp could go out of date. Naming files here would mean
    // remembering to add each new one, and a forgotten one is a stale stamp - the failure
    // this whole file exists to prevent.

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let stamp = format!(
        "{{\n  \"commit\": \"{}\",\n  \"tree\": \"{}\",\n  \"profile\": \"{}\"\n}}\n",
        git(&root, &["rev-parse", "HEAD"]).unwrap_or_else(|| NO_REVISION.to_string()),
        tree_hash(&root).unwrap_or_else(|| NO_REVISION.to_string()),
        std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string()),
    );

    // OUT_DIR is target/<profile>/build/<crate>-<hash>/out, so the library sits three
    // levels up. Derived rather than assumed from PROFILE, which does not name the
    // directory for a custom profile.
    let Some(out) = std::env::var_os("OUT_DIR").map(PathBuf::from) else { return };
    let Some(target) = out.ancestors().nth(3) else { return };

    let _ = std::fs::write(target.join("lookahead_engine.built.json"), stamp);
}

/// One git command's output, or None if it could not be run or failed.
fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").args(arguments).current_dir(root).output().ok()?;
    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// A hash over this library's sources, uncommitted changes to tracked files included.
fn tree_hash(root: &Path) -> Option<String> {
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

    // Each line is a blob sha and a path, so hashing the listing hashes the CONTENT of
    // every source without reading one - git has already hashed them all.
    Some(digest(&listed.stdout))
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
