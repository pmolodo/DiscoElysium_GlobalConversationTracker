// SPDX-License-Identifier: MIT
//! The world the game answered a menu from, against the one the fixtures assemble.
//!
//! ## Why this exists at all
//!
//! THE TWO EXECUTORS DISAGREED ABOUT A MENU AND NOTHING COULD SAY WHY. Conversation 29 is
//! marked differently in game and offline - two options, and the Fail half of a check - and
//! every hypothesis about which part of the world was missing turned out to be wrong when it
//! was staged: the journal, the cabinet, the balance and the clock changed nothing. What was
//! never available is the comparison itself, because only one side's world was ever written
//! down.
//!
//! Both sides write one now. The mod's `KeepLookAheadRequests` setting writes what crossed to
//! the engine, per group, and the harness keeps a copy before the profile is restored;
//! `kim_case_offline` writes the request it built. This diffs them.
//!
//! ## What it is not
//!
//! NOT A CLAIM THAT THE TWO MUST MATCH. Some of what the game answers cannot be read out of a
//! save at all - what is worn, whether it is raining - so a difference here is a FINDING
//! rather than a failure, and this prints it. It fails only when a captured pair cannot be
//! read, which would mean the capture itself is broken.
//!
//! ```text
//! cargo test --release --test request_agreement -- --nocapture
//! ```

use std::path::PathBuf;

use lookahead_engine::formats::json_diff;

mod common;

use common::repo_root;

/// Where the harness keeps what the game sent.
fn from_the_game() -> PathBuf {
    repo_root()
        .join(".build")
        .join("automation")
        .join("requests")
}

/// Where the offline report writes what it built.
fn from_the_fixtures() -> PathBuf {
    repo_root().join(".build").join("offline-requests")
}

#[test]
fn the_world_the_game_answered_from_matches_the_one_the_fixtures_build() {
    let theirs = from_the_game();
    let mine = from_the_fixtures();

    if !theirs.is_dir() {
        eprintln!(
            "no captured requests at {}; run the kim-case suite in game first.",
            theirs.display()
        );
        return;
    }

    let mut compared = 0;
    for entry in std::fs::read_dir(&theirs).expect("the captured folder reads") {
        let path = entry.expect("a captured file").path();
        let Some(name) = path.file_name() else {
            continue;
        };

        let beside = mine.join(name);
        if !beside.exists() {
            eprintln!(
                "{} has no offline counterpart at {}",
                path.display(),
                beside.display()
            );
            continue;
        }

        compared += 1;
        report(&path, &beside);
    }

    assert!(compared > 0, "nothing to compare");
}

/// What differs between one captured pair, field by field.
fn report(theirs: &std::path::Path, mine: &std::path::Path) {
    let in_game: serde_json::Value = read(theirs);
    let offline: serde_json::Value = read(mine);

    println!("\n{}", theirs.file_name().unwrap().to_string_lossy());

    let Some(patch) = json_diff::create(&offline, &in_game) else {
        println!("  the two worlds are identical");
        return;
    };

    let changes = patch
        .get(json_diff::CHANGES_KEY)
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    for (field, value) in changes.as_object().into_iter().flatten() {
        println!("  {field}: {}", summarise(value, offline.get(field)));
    }

    for gone in patch
        .get(json_diff::REMOVE_KEY)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        println!(
            "  {}: the game sends nothing here",
            gone.as_str().unwrap_or("")
        );
    }
}

/// One field's difference, short enough to read.
fn summarise(in_game: &serde_json::Value, offline: Option<&serde_json::Value>) -> String {
    let shown = |value: &serde_json::Value| {
        let text = value.to_string();
        if text.len() > 120 {
            format!("{}... ({} bytes)", &text[..117], text.len())
        } else {
            text
        }
    };

    match offline {
        Some(was) => format!("offline {} -> in game {}", shown(was), shown(in_game)),
        None => format!("only in game: {}", shown(in_game)),
    }
}

/// One captured request.
fn read(path: &std::path::Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|why| panic!("{} does not read: {why}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|why| panic!("{} is not a request: {why}", path.display()))
}
