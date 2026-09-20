// SPDX-License-Identifier: MIT
//! Does `docs/environment.md` still list the variables the code actually asks for?
//!
//! ## Why this is a test and not a generator
//!
//! The table names every `DEGCT_` variable and the files that name it. It was pasted in by hand
//! from a script, and the doc said what would happen: "Nothing enforces the table, and a stale
//! one is worse than none: a reader who trusts it will look for a variable that has been
//! renamed." It went five variables stale, and the count above it said 45 against an actual 50,
//! for weeks, with nothing to notice. See de-yqjr.
//!
//! WHAT IS WANTED IS NOT GENERATION BUT ENFORCEMENT. A build script that rewrote a tracked file
//! would dirty the working tree on every build - which shows up in every measurement's log name
//! as `-dirty` - and cargo's own guidance is that a build script writes to `OUT_DIR` and nowhere
//! else. A human pasting rows is fine as long as something fails when they forget, so this is
//! that something: it regenerates the rows, compares, and prints the exact block to paste.
//!
//! ## Why in Rust, and why the Python is gone
//!
//! Shelling out to `tools/env-table.py` would make a Python interpreter a build dependency of
//! the crate for everyone forever, and it would have to find the right one - `python` is not
//! `python3` everywhere. Running the tests already needs cargo and nothing else.
//!
//! TWO IMPLEMENTATIONS OF "WHICH FILES NAME THIS VARIABLE" WOULD BE THE SAME DRIFT ONE LEVEL UP,
//! so the script was deleted rather than kept beside this.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The doc this checks, which would otherwise count every variable it lists.
const TABLE: &str = "docs/environment.md";

/// The files ABOUT the variables, whose own patterns and prose name them as examples.
///
/// A file whose subject is the list cannot also be evidence for it: every name it uses to explain
/// itself would become a row, and a name it stopped explaining would silently leave one.
///
/// FOUR OF THEM. This file and `tools/survey-env.py` look for variables and name them as patterns
/// to look for. The two shell HELPERS are the doors themselves - they take a name as a parameter
/// and read nothing of their own - so every name in them is showing how the door works. Three
/// rows survived in the table on the strength of an example in one of those headers, after
/// nothing read the variables any more.
const ABOUT: [&str; 4] = [
    "tests/environment_table.rs",
    "tools/survey-env.py",
    "tools/degct-env.sh",
    "tools/DegctEnv.psm1",
];

/// What a file has to end in to be worth reading: the languages that have a helper, plus the
/// docs, which name variables in prose.
const SUFFIXES: [&str; 6] = ["rs", "py", "sh", "psm1", "cs", "md"];

/// The prefix on every variable this project defines.
const PREFIX: &str = "DEGCT_";

/// Calls that take a BARE name as a quoted first argument.
///
/// Not `std::env::var`, which reads somebody else's name under its own spelling - the `env::`
/// entries below are matched only where `std::` does not precede them.
///
/// NO C# SHAPE HERE, and `.cs` is still read. No C# file asks for one of ours through a helper,
/// so there is no call shape to look for; a raw `GetEnvironmentVariable("DEGCT_X")` would still
/// be caught, by the spelled-out name. A helper and the shape that finds it arrive together.
const QUOTED: [&str; 14] = [
    "env::var(",
    "env::is_set(",
    "env::number(",
    "env::pass(",
    "env::qualified(",
    "from_env(",
    "from_env_i32(",
    "numbers(",
    "env(",
    "env_is_set(",
    "env_int(",
    "env_list(",
    "qualified(",
    "env_for_child(",
];

/// Calls that take a BARE name as an unquoted argument: bash and PowerShell.
const BARE: [&str; 6] = [
    "degct_env_is_set ",
    "degct_env_set ",
    "degct_env ",
    "Get-DegctEnv ",
    "Test-DegctEnv ",
    "Set-DegctEnv ",
];

#[test]
fn the_environment_doc_lists_what_the_code_asks_for() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let Some(found) = rows(&root) else {
        // NO GIT, NO LIST OF FILES, NO TEST. This reads what git tracks rather than walking the
        // tree, so that build output and a downloaded game's files are never in it.
        return;
    };

    let doc = std::fs::read_to_string(root.join(TABLE)).expect("the environment doc reads");
    let listed: Vec<String> = doc
        .lines()
        .filter(|line| line.starts_with("| `DEGCT_"))
        .map(str::to_string)
        .collect();

    assert_eq!(
        listed,
        found,
        "\n\ndocs/environment.md no longer lists what the code asks for.\n\
         Missing from the doc:\n{}\nIn the doc and not in the code:\n{}\n\
         Paste this in place of the table's rows:\n\n{}\n",
        only_in(&found, &listed),
        only_in(&listed, &found),
        found.join("\n")
    );

    // AND THE COUNT ABOVE IT, which drifted too and is the part a reader believes without
    // checking. It is written as a sentence rather than a field, so it is found by its shape.
    let said = doc
        .lines()
        .find_map(|line| {
            let (before, _) = line.split_once(" variables.")?;
            before.rsplit(' ').next()?.parse::<usize>().ok()
        })
        .expect("the doc says how many variables there are");
    assert_eq!(
        said,
        found.len(),
        "docs/environment.md says {said} variables and lists {}",
        found.len()
    );
}

/// Every name the tracked files ask for, as the table's rows, or `None` where git cannot say.
fn rows(root: &Path) -> Option<Vec<String>> {
    let listed = Command::new("git")
        .arg("ls-files")
        .current_dir(root)
        .output()
        .ok()?;
    if !listed.status.success() {
        return None;
    }

    let mut readers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for path in String::from_utf8_lossy(&listed.stdout).lines() {
        if path == TABLE || ABOUT.contains(&path) {
            continue;
        }
        if !SUFFIXES
            .iter()
            .any(|suffix| path.ends_with(&format!(".{suffix}")))
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(path)) else {
            continue;
        };
        for name in names_in(&text) {
            readers.entry(name).or_default().insert(path.to_string());
        }
    }

    Some(
        readers
            .into_iter()
            .map(|(name, files)| {
                let files: Vec<String> = files.iter().map(|path| format!("`{path}`")).collect();
                format!("| `{PREFIX}{name}` | {} |", files.join(", "))
            })
            .collect(),
    )
}

/// Every bare name `text` asks for, by any of the ways there are to ask.
fn names_in(text: &str) -> BTreeSet<String> {
    let bytes = text.as_bytes();
    let mut found = BTreeSet::new();

    // SPELLED OUT IN FULL, anywhere - code, a comment, a doc. Not where it is part of a longer
    // word, which is how the scratch prefix `DEGCTT_` and the doubled `DEGCT_DEGCT_` stay out.
    for at in matches(text, PREFIX) {
        if at > 0 && is_word(bytes[at - 1]) {
            continue;
        }
        if let Some(name) = name_at(bytes, at + PREFIX.len()) {
            found.insert(name);
        }
    }

    for call in QUOTED {
        for at in matches(text, call) {
            if call.starts_with("env::") && at >= 5 && &text[at - 5..at] == "std::" {
                continue;
            }
            if at > 0 && is_word(bytes[at - 1]) {
                continue;
            }
            // A METHOD CALL IS NOT OUR DOOR. `command.env("GIT_INDEX_FILE", ..)` sets a variable
            // GIT owns, on a child this project starts, and reading it as `env(` invented a
            // DEGCT_GIT_INDEX_FILE that nothing has ever defined. Ours is `env::pass`, which
            // applies the prefix - see `src/core/env.rs`, whose whole purpose is that the two
            // cannot be confused at a call site.
            if at > 0 && bytes[at - 1] == b'.' {
                continue;
            }
            if call == "env_for_child(" {
                found.extend(keywords(&text[at + call.len()..]));
                continue;
            }
            let mut i = skip_spaces(bytes, at + call.len());
            if bytes.get(i) != Some(&b'"') {
                continue;
            }
            i += 1;
            let Some(name) = name_at(bytes, i) else {
                continue;
            };
            if bytes.get(i + name.len()) != Some(&b'"') {
                continue;
            }
            // PYTHON'S FOREIGN DOOR reads a name somebody else owns under its own spelling, so
            // what it names is not one of ours.
            let rest = &text[i + name.len()..];
            let until = rest.find(')').unwrap_or(rest.len());
            if rest[..until].contains("foreign=True") {
                continue;
            }
            found.insert(name);
        }
    }

    for call in BARE {
        for at in matches(text, call) {
            if at > 0 && is_word(bytes[at - 1]) {
                continue;
            }
            if let Some(name) = name_at(bytes, at + call.len()) {
                found.insert(name);
            }
        }
    }

    // A NAME THAT ALREADY CARRIES THE PREFIX is the doubled spelling the helpers exist to
    // prevent, named in their docs as the mistake - not a variable.
    found.retain(|name| !name.starts_with("DEGCT"));
    found
}

/// The names `env_for_child(X=.., Y=..)` sets, from the text after its opening bracket.
fn keywords(rest: &str) -> BTreeSet<String> {
    let until = rest.find(')').unwrap_or(rest.len());
    let inside = &rest[..until];
    let bytes = inside.as_bytes();
    let mut found = BTreeSet::new();
    let mut at = 0;
    while at < bytes.len() {
        let Some(name) = name_at(bytes, at) else {
            at += 1;
            continue;
        };
        let after = skip_spaces(bytes, at + name.len());
        if bytes.get(after) == Some(&b'=') && bytes.get(after + 1) != Some(&b'=') {
            found.insert(name.clone());
        }
        at += name.len().max(1);
    }
    found
}

/// Every index at which `needle` appears in `text`.
fn matches(text: &str, needle: &str) -> Vec<usize> {
    text.match_indices(needle).map(|(at, _)| at).collect()
}

/// The name starting at `from`: a letter, then letters, digits and underscores, ending in a
/// letter or a digit. `None` where what is there is not one.
fn name_at(bytes: &[u8], from: usize) -> Option<String> {
    if !bytes.get(from)?.is_ascii_uppercase() {
        return None;
    }
    let mut end = from;
    while end < bytes.len()
        && (bytes[end].is_ascii_uppercase() || bytes[end].is_ascii_digit() || bytes[end] == b'_')
    {
        end += 1;
    }
    while end > from && bytes[end - 1] == b'_' {
        end -= 1;
    }
    (end - from >= 2).then(|| String::from_utf8_lossy(&bytes[from..end]).into_owned())
}

fn skip_spaces(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && (bytes[at] == b' ' || bytes[at] == b'\n' || bytes[at] == b'\t') {
        at += 1;
    }
    at
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The rows in `these` and not in `those`, for a failure that says what to change.
fn only_in(these: &[String], those: &[String]) -> String {
    let those: BTreeSet<&String> = those.iter().collect();
    let missing: Vec<&str> = these
        .iter()
        .filter(|row| !those.contains(row))
        .map(String::as_str)
        .collect();
    if missing.is_empty() {
        "  (none)".to_string()
    } else {
        missing.join("\n")
    }
}
