// SPDX-License-Identifier: MIT
#![allow(dead_code)] // Each test binary uses a different part of this.

//! Finding the extracted game data these tests run on, and REGENERATING it when it is
//! missing.
//!
//! ## Why not just skip
//!
//! The corpus is extracted game content and is not committed, so a test that needs it has
//! to cope with its absence. The obvious answer - pass silently - is the wrong one: a
//! corpus that quietly stops being generated turns every test that depends on it into a
//! test that always passes, which is worse than having no test, because it still reads as
//! green. That is how a whole class of coverage disappears without anyone deciding to
//! drop it.
//!
//! So the order here is: use it, else build it, else say loudly why it cannot be built.
//!
//! ## The one case that is still allowed to skip
//!
//! Building the corpus needs the exported dialogue database, which is many gigabytes of
//! AssetRipper output from a real game install. A checkout on a machine without the game
//! cannot produce it by any means, and failing there would mean the suite could only ever
//! be run by someone with the game.
//!
//! That case skips - and says so in terms nobody will mistake for success. Every other
//! failure, including the extractor running and not producing the file, is a hard error.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// Serialises regeneration across the tests in one binary.
///
/// Cargo runs the tests within a binary IN PARALLEL, so without this every test that
/// finds the corpus missing launches its own `dotnet run` at the same moment, and they
/// collide over the build output - `Cannot open DialogueAsset.dll for writing, being used
/// by another process`. Observed, not anticipated: four corpus tests raced on the first
/// run after the file was deleted.
///
/// Holding the lock is not enough on its own; the file is re-checked after acquiring it,
/// because by then another test has usually just built it.
static REGENERATION: Mutex<()> = Mutex::new(());

/// Where the extractor writes, relative to the repo root.
const DERIVED: &str = ".game_reference_copies/derived";

/// The exported database the extractor reads, relative to the repo root.
const SOURCE_ASSET: &str = ".game_reference_copies/AssetRipperExport/ExportedProject/Assets/\
Dialogue Databases/Disco Elysium.asset";

/// What to run to rebuild the corpus files.
const CORPUS_COMMAND: [&str; 5] =
    ["run", "--project", "tools/DialogueExtract", "--", "corpus"];

/// What to run to rebuild the conversation index.
const INDEX_COMMAND: [&str; 5] =
    ["run", "--project", "tools/DialogueExtract", "--", "conversation-index"];

/// The repo root, found by walking up from this crate.
pub fn repo_root() -> PathBuf {
    let mut dir: Option<&Path> = Some(Path::new(env!("CARGO_MANIFEST_DIR")));
    while let Some(d) = dir {
        if d.join(".game_reference_copies").exists() || d.join(".git").exists() {
            return d.to_path_buf();
        }
        dir = d.parent();
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Whether the exported database the extractor reads is present.
///
/// The one thing no amount of running things can conjure up.
fn source_asset_present(root: &Path) -> bool {
    root.join(SOURCE_ASSET).exists()
}

/// Runs the extractor, and fails loudly if it does not succeed.
fn extract(root: &Path, args: &[&str], what: &str) {
    println!("{what} is missing; regenerating with: dotnet {}", args.join(" "));
    let output = Command::new("dotnet")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| panic!("could not run dotnet to regenerate {what}: {e}"));

    if !output.status.success() {
        panic!(
            "regenerating {what} failed ({}).\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

/// A derived file, regenerated if absent; `None` only when the game data cannot be had.
///
/// `args` is the extractor invocation that would produce it.
fn derived(file_name: &str, args: &[&str], what: &str) -> Option<PathBuf> {
    let root = repo_root();
    let path = root.join(DERIVED).join(file_name);
    if path.exists() {
        return Some(path);
    }

    // One regeneration at a time. A poisoned lock is not a reason to give up - it only
    // means some other test panicked while holding it, which says nothing about whether
    // the file can be built.
    let _guard = REGENERATION.lock().unwrap_or_else(|e| e.into_inner());

    // Another test may have built it while this one waited.
    if path.exists() {
        return Some(path);
    }

    if !source_asset_present(&root) {
        // Deliberately shouty. This is the only path that lets a test pass without
        // having tested anything, and it should never be mistaken for a clean run.
        println!(
            "\n!! SKIPPING: {what} is missing and cannot be regenerated.\n\
             !! The exported dialogue database is not present at:\n\
             !!   {}\n\
             !! Nothing was tested. This needs a game install and an AssetRipper export.\n",
            root.join(SOURCE_ASSET).display(),
        );
        return None;
    }

    extract(&root, args, what);

    if !path.exists() {
        panic!(
            "regenerating {what} reported success but did not produce {}",
            path.display()
        );
    }

    Some(path)
}

/// One of the two corpus files, regenerated if absent.
pub fn corpus_file(file_name: &str) -> Option<PathBuf> {
    derived(file_name, &CORPUS_COMMAND, file_name)
}

/// The conversation index, regenerated if absent.
pub fn conversation_index() -> Option<PathBuf> {
    derived(
        "conversation_index.jsonl",
        &INDEX_COMMAND,
        "conversation_index.jsonl",
    )
}
