// SPDX-License-Identifier: MIT
//! Does Rust read every state file this repository has committed, and write it back?
//!
//! ## Why writing it back is the interesting half
//!
//! Every one of these was written by the C# writer that has kept this format so far. So
//! reading one and writing it out again, and comparing the BYTES, holds the Rust writer to
//! the C# one over real files - the status order, the conversation order, the run encoding,
//! the orb sort and the absence of whitespace, all at once and without either side having
//! to describe itself to the other.
//!
//! A state written as a CHANGE to another one cannot be compared that way, because what it
//! resolves to was never a file. Those are read, and what they resolve to is written and
//! read back, which is the same property one step removed.
//!
//! ## Its other half, which is not in this language
//!
//! `tools/GameAutomation.Tests/CommittedStateFixtureTests.cs` does the same thing with the
//! mod's own reader and writer. THIS FORMAT IS READ BY TWO PROGRAMS ON PURPOSE - see
//! [`lookahead_engine::formats::global_state`] for why - and the pair of tests is what
//! stops them drifting: neither can change what it writes without one of the two failing,
//! and a fixture cannot be updated to suit one of them without the other saying so.
//!
//! ## What a committed fixture is for
//!
//! These are what the offline runs stage as "what some other save has already read", which
//! is the input that decides whether a look-ahead has anything to look for. A reader that
//! quietly returned an empty state would not fail here; it would make every scenario look
//! like a fresh playthrough. So the entry counts are asserted too, not only that it parsed.

use std::fs;
use std::path::{Path, PathBuf};

use lookahead_engine::formats::global_state::{self, Status};
use lookahead_engine::formats::{header, resolve};

mod common;

/// How many state fixtures the repository is known to carry.
///
/// A LOWER BOUND, so a test finding none - a moved directory, a glob that stopped matching
/// - fails rather than passing over an empty list.
const AT_LEAST: usize = 15;

/// Every committed state fixture.
///
/// BY WHAT A FILE SAYS IT IS rather than by what it is called, which is the point of a
/// header: a state that was named something else would be missed by a glob and is not
/// missed by this.
fn states() -> Vec<PathBuf> {
    let scenarios = common::repo_root().join("testing").join("scenarios");
    let Ok(entries) = fs::read_dir(&scenarios) else {
        return Vec::new();
    };

    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|kind| kind == "json"))
        .filter(|path| is_a_state(path))
        .collect();

    found.sort();
    found
}

/// Whether a file is a state, once whatever it is a change to has been followed.
fn is_a_state(path: &Path) -> bool {
    let Ok(document) = resolve::document(path) else {
        return false;
    };

    document
        .get(header::FORMAT_KEY)
        .and_then(serde_json::Value::as_str)
        == Some(global_state::FORMAT.format)
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Whether a fixture is written as a change to another one rather than whole.
fn is_a_diff(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|document| {
            document
                .get(header::BASE_KEY)
                .map(serde_json::Value::is_string)
        })
        .unwrap_or(false)
}

#[test]
fn every_committed_state_reads_and_writes_back_as_the_bytes_it_came_from() {
    let states = states();
    assert!(
        states.len() >= AT_LEAST,
        "found {} state fixtures, expected at least {AT_LEAST}",
        states.len(),
    );

    let mut whole = 0;
    let mut resolved = 0;
    for path in &states {
        let name = name_of(path);
        let text = fs::read_to_string(path).unwrap_or_else(|why| panic!("{name}: {why}"));

        if is_a_diff(&text) {
            // What it resolves to was never a file, so what is held here is that the
            // resolved document reads and survives a round trip through this writer.
            let document = resolve::document(path).unwrap_or_else(|why| panic!("{name}: {why}"));
            let read = global_state::read_document(&document, &name)
                .unwrap_or_else(|why| panic!("{name}: {why}"));
            assert_eq!(read.skipped, 0, "{name}: {:?}", read.warnings);

            let back = global_state::read(&global_state::write(&read.state), &name)
                .unwrap_or_else(|why| panic!("{name}, written back: {why}"));
            assert_eq!(
                back.state, read.state,
                "{name} did not survive a round trip"
            );
            resolved += 1;
            continue;
        }

        let read = global_state::read(&text, &name).unwrap_or_else(|why| panic!("{name}: {why}"));
        assert_eq!(read.skipped, 0, "{name}: {:?}", read.warnings);

        // THE TRAILING NEWLINE IS THE REPOSITORY'S, not the writer's: every committed text
        // file ends in one and nothing this writes does.
        assert_eq!(
            global_state::write(&read.state),
            text.trim_end_matches('\n'),
            "{name} is not written back as the bytes it came from",
        );
        whole += 1;
    }

    assert!(whole > 0, "no whole state fixture was compared");
    assert!(
        resolved > 0,
        "no state written as a change to another was read"
    );
}

/// And the states really do record what the offline runs think they record.
///
/// A reader that returned an empty state would pass every assertion above - it would write
/// an empty state back out and match nothing, which is why this counts what came out.
#[test]
fn the_states_record_what_the_scenarios_are_built_around() {
    let scenarios = common::repo_root().join("testing").join("scenarios");
    let worst = scenarios.join("global-state-worst-case.json");
    let text = fs::read_to_string(&worst).expect("the worst case is committed");
    let read = global_state::read(&text, "the worst case").expect("it reads");

    // The fixture that records every entry of every conversation in the game. The exact
    // number is not the point; that it is the whole game rather than a handful is.
    assert!(
        read.state.len() > 100_000,
        "the worst case records {} entries",
        read.state.len(),
    );
    assert!(read.state.conversations().count() > 1_000);

    let one = scenarios.join("global-state-fan-pass-recorded.json");
    let text = fs::read_to_string(&one).expect("it is committed");
    let read = global_state::read(&text, "one scenario").expect("it reads");

    assert_eq!(read.state.entries_at(9, Status::WasDisplayed), vec![50, 82]);
}
