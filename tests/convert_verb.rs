// SPDX-License-Identifier: MIT
//! The engine host's `convert` verb, run as a person runs it: the binary, on files on disk.
//!
//! What matters about it is what is on disk afterwards - which file holds the conversion,
//! where the original went, and that nothing already there was replaced - and that is a
//! question about the process rather than about the conversion, which `convert.rs` tests.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use lookahead_engine::formats::global_state;

/// A state file as version 3 wrote it: statuses grouping lists of entry ids.
const OLDER_STATE: &str = r#"{"version": 3, "conversations": {"WasDisplayed": {"3": [17, 19]}}}"#;

/// The name the original is kept under, beside a file called `state.json`.
const KEPT_NAME: &str = "state.v3.json";

/// A folder of its own for one test, emptied first.
fn scratch(test: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("gct-convert-verb-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("a scratch folder");
    folder
}

/// Runs `convert` with the given arguments.
fn convert(arguments: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gct-engine-host"))
        .arg("convert")
        .args(arguments)
        .output()
        .expect("the engine host runs")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|fault| panic!("{}: {fault}", path.display()))
}

#[test]
fn with_no_output_the_file_is_converted_in_place_and_the_original_kept() {
    let folder = scratch("in-place");
    let state = folder.join("state.json");
    std::fs::write(&state, OLDER_STATE).expect("the state writes");

    let ran = convert(&[&state]);

    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );

    // The name a reader looks for now holds the current version...
    let converted = read(&state);
    assert!(
        converted.contains(global_state::FORMAT.format),
        "{converted}"
    );
    assert!(
        global_state::read(&converted, "the converted state").is_ok(),
        "{converted}"
    );

    // ...and the original is kept, byte for byte, under the version it was.
    assert_eq!(read(&folder.join(KEPT_NAME)), OLDER_STATE);
    assert!(!folder.join("state.json.converting").exists());
}

#[test]
fn in_place_is_refused_where_the_kept_name_is_taken_and_nothing_moves() {
    let folder = scratch("kept-taken");
    let state = folder.join("state.json");
    let kept = folder.join(KEPT_NAME);
    std::fs::write(&state, OLDER_STATE).expect("the state writes");
    std::fs::write(&kept, "somebody's own file").expect("the other file writes");

    let ran = convert(&[&state]);

    assert!(!ran.status.success());
    assert_eq!(read(&state), OLDER_STATE);
    assert_eq!(read(&kept), "somebody's own file");
    assert!(!folder.join("state.json.converting").exists());
}

#[test]
fn a_named_output_leaves_the_input_where_it_is() {
    let folder = scratch("named");
    let state = folder.join("state.json");
    let out = folder.join("converted.json");
    std::fs::write(&state, OLDER_STATE).expect("the state writes");

    let ran = convert(&[&state, &out]);

    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(read(&state), OLDER_STATE);
    assert!(global_state::read(&read(&out), "the converted state").is_ok());
    assert!(!folder.join(KEPT_NAME).exists());
}

#[test]
fn a_file_that_will_not_convert_is_left_exactly_as_it_was() {
    let folder = scratch("refused");
    let state = folder.join("state.json");
    let damaged = r#"{"version": 3, "conversations": {"WasEaten": {"3": [17]}}}"#;
    std::fs::write(&state, damaged).expect("the state writes");

    let ran = convert(&[&state]);

    assert!(!ran.status.success());
    assert_eq!(read(&state), damaged);
    assert!(!folder.join(KEPT_NAME).exists());
    assert!(!folder.join("state.json.converting").exists());
}
