// SPDX-License-Identifier: MIT
//! What a measurement keeps between processes, so that preparation is paid once rather than
//! once per group.
//!
//! ## Why anything is kept at all
//!
//! The matrix driver runs ONE PROCESS PER GROUP - a group that takes its process down costs
//! that group and nothing else - so nothing a process derives outlives the group it was
//! derived for, and a whole-game pass derives the same things 521 times over. A
//! before-and-after comparison is four passes of that. Every one of those derivations is a
//! pure function of files that did not change in between, which is the definition of
//! cacheable. See de-9z1u.
//!
//! ## What every key carries, and why none of it is optional
//!
//! THE EXECUTABLE, not only the inputs. A kept value is valid for the code that derived it and
//! nothing else, and that code changes. When it does, every value kept before it is wrong
//! SILENTLY, which is the worst thing a measurement cache can be: a run would report numbers
//! for a graph or a world the current code would not build, and nothing about the run would
//! look unusual.
//!
//! THE FILES IT WAS DERIVED FROM, named by the caller through [`stamp`] - which for everything
//! here means an index, rebuilt from the game's own files rather than committed, so it can
//! change under a cache that did not ask about it. The caller names it rather than this module
//! assuming one, because there are two indexes and a value derived from one is not a value
//! derived from the other.
//!
//! BY LENGTH AND MODIFICATION TIME rather than by content, which is what `cargo` itself does
//! to decide whether to rebuild. Hashing a sixteen-megabyte index in every process would cost
//! a good part of what the cache saves.
//!
//! ANYTHING THAT CANNOT BE ESTABLISHED IS A REASON NOT TO KEEP. [`at`] answers `None` where it
//! cannot stat what the key needs, and a caller that cannot key its value simply builds it:
//! the cache is an optimisation, and never the reason an answer is missing or wrong.
//!
//! ## What it does when a file is bad
//!
//! Builds the value. A file that does not read back may be half-written by a process still
//! running or left by a build that no longer exists; the cost of ignoring it is deriving the
//! value again, and the cost of trusting it is a wrong measurement.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Whether `DEGCT_NO_CACHE` says to derive everything rather than trusting what was kept.
///
/// A CACHE UNDERNEATH A MEASUREMENT HAS TO HAVE A WAY OFF. Every performance number this
/// repository produces is taken against values derived here, and a cache that cannot be
/// disabled is one whose correctness can only be argued about rather than checked.
pub fn no_cache() -> bool {
    lookahead_engine::core::env::var("NO_CACHE").as_deref() == Ok("1")
}

/// Whether `DEGCT_CACHE_VERIFY` says to derive everything AND check it against what was kept.
///
/// The answer a run reports is the derived one either way, so a verifying run measures the
/// uncached cost and cannot be quietly wrong about what it compared. See `tests/kept_cache.rs`,
/// which is what runs this over a handful of groups.
pub fn verifying() -> bool {
    lookahead_engine::core::env::var("CACHE_VERIFY").as_deref() == Ok("1")
}

/// Where a value of this `kind` described by `about` is kept, or `None` where it cannot safely
/// be kept at all.
///
/// `kind` names the folder - "worlds", "graphs" - and `about` says which value within it: the
/// group, the save, the [`stamp`] of every file it was read from. The executable is added here,
/// because it is the one input no caller could be trusted to remember and the one whose absence
/// from a key is silent.
pub fn at(kind: &str, about: &str) -> Option<PathBuf> {
    let key = format!("{about}\u{1}{}", stamp(&std::env::current_exe().ok()?)?);
    Some(folder(kind)?.join(format!("{}.{kind}", fingerprint(&key))))
}

/// What says whether a file is the same file: its length and when it was last written.
///
/// `None` where it cannot be asked, which a caller passes straight on to [`at`] as a reason not
/// to keep the value at all.
pub fn stamp(path: &Path) -> Option<String> {
    let about = std::fs::metadata(path).ok()?;
    let when = about
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some(format!("{}:{}", about.len(), when.as_nanos()))
}

/// FNV-1a, so the name of a kept value does not depend on a hasher whose output is allowed to
/// change between Rust releases - and a name that changed is a value nothing will ever read
/// again, which is a cache that silently stops working.
fn fingerprint(of: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in of.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The folder values of one kind live in, made if it is not there.
///
/// UNDER THE BUILD OUTPUT, never in the repository: it is derived, it is large, and it is
/// invalidated by the very thing `target/` is invalidated by.
fn folder(kind: &str) -> Option<PathBuf> {
    let root = match lookahead_engine::core::env::foreign("CARGO_TARGET_DIR") {
        Ok(named) if !named.is_empty() => PathBuf::from(named),
        _ => PathBuf::from("target"),
    };
    let folder = root.join("degct-cache").join(kind);
    std::fs::create_dir_all(&folder).ok()?;
    Some(folder)
}

/// A kept value written as JSON, or `None` for anything at all going wrong.
///
/// FOR WHAT BINCODE CANNOT CARRY. A world holds a `WireValue`, whose deserializer asks what
/// the next value IS, and a format that is not self-describing cannot answer that.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) {
    if let Ok(bytes) = serde_json::to_vec(value) {
        write_whole(path, &bytes);
    }
}

/// A kept value written as bincode, or `None` for anything at all going wrong.
///
/// FOR WHAT IS LARGE. A group's graph is megabytes of nodes, guards and actions, and JSON would
/// spend more on parsing it back than building it from the index costs.
pub fn read_packed<T: DeserializeOwned>(path: &Path) -> Option<T> {
    bincode::deserialize(&std::fs::read(path).ok()?).ok()
}

pub fn write_packed<T: Serialize>(path: &Path, value: &T) {
    if let Ok(bytes) = bincode::serialize(value) {
        write_whole(path, &bytes);
    }
}

/// Writes through a temporary file, so that no reader ever sees a partial value.
///
/// THE DRIVER RUNS GROUPS IN PARALLEL, so two processes can want the same value at the same
/// moment. Writing in place would let one read what the other is still writing; a rename is
/// atomic enough that a reader sees the whole file or no file. Two writers racing produce the
/// same bytes, so whichever lands last is right.
///
/// THE TEMPORARY NAME CARRIES THE PROCESS ID, or the two writers would race over one temporary
/// file instead - and a rename of a file another process is still filling is exactly what this
/// exists to prevent.
fn write_whole(path: &Path, bytes: &[u8]) {
    let scratch = path.with_extension(format!("{}.part", std::process::id()));
    if std::fs::write(&scratch, bytes).is_ok() && std::fs::rename(&scratch, path).is_err() {
        // A RENAME THAT FAILED LEAVES ITS OWN MESS BEHIND, and the next process would find a
        // stray file with a dead process's id on it and no idea whether to trust it.
        let _ = std::fs::remove_file(&scratch);
    }
}
