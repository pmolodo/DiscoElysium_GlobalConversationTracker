// SPDX-License-Identifier: MIT
//! Reading the scenario fixtures the in-game harness stages, without staging them.
//!
//! ## What a fixture is made of
//!
//! Two halves, and they are stored in different places because the game stores them in
//! different places. What OTHER saves have read is the mod's own global state file, staged
//! beside the profile and named by a suite. What THIS save has read is the game's per-save
//! SimStatus, which lives inside the save and nothing staged beside it can set - which is
//! why a scenario that needs an entry already read needs a save of its own.
//!
//! An offline executor has to assemble both to mean what the in-game run means, so it
//! reads them from the same two files the run loads rather than from a copy written down
//! beside the expectation. The copy is the thing that drifts.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use lookahead_engine::bridge::WireValue;
use serde::Deserialize;

use super::repo_root;

/// Where the committed scenario fixtures live.
pub fn scenarios() -> PathBuf {
    repo_root().join("testing").join("scenarios")
}

/// Where the base every scenario save eventually rests on lives.
///
/// Outside the scenarios folder, which is why a chain can walk out of it.
fn testing() -> PathBuf {
    repo_root().join("testing")
}

/// A save's folder, wherever it is.
///
/// A scenario names its save without a path - `afford-both` - and the folder is that plus
/// the game's extension. The chain then names its bases by relative path, so those are
/// resolved against the folder they were named in rather than looked up here.
fn folder_of(save: &str) -> PathBuf {
    let named = scenarios().join(format!("{save}.ntwtf"));
    if named.is_dir() {
        return named;
    }

    testing().join(format!("{save}.ntwtf"))
}

/// What a save folder's archive says it was built on top of.
#[derive(Debug, Deserialize)]
struct Archive {
    /// The next save down, by path relative to this folder. Absent at the bottom.
    #[serde(default)]
    base: Option<String>,
}

/// The save folders one save is made of, BASE-MOST FIRST.
///
/// A scenario save in this repository is a diff over a base, which is a diff over another,
/// down to `save_template` - so what a save holds is only knowable by walking the chain.
/// Reading the leaf alone was enough while the only question asked of a save was which
/// entries it had displayed, because no base records any; it is not enough for the
/// variables, which is exactly where the base does the work.
///
/// # Panics
///
/// If a folder is missing, its archive will not read, or the chain loops. A loop would
/// otherwise be an out-of-memory rather than a message.
fn chain(save: &str) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    let mut seen = HashSet::new();
    let mut current = folder_of(save);

    loop {
        assert!(
            current.is_dir(),
            "{save}'s chain reaches {}, which is not a save folder",
            current.display(),
        );
        assert!(
            seen.insert(current.clone()),
            "{save}'s chain of bases loops back to {}",
            current.display(),
        );

        folders.push(current.clone());

        let archive = current.join("_archive.json");
        let Some(base) = read_json::<Archive>(&archive).and_then(|a| a.base) else {
            break;
        };

        // Relative to the folder that named it, and normalised, because a chain that walks
        // out of the scenarios folder - as every one of them does, to save_template -
        // produces a path with `..` in it that no `is_dir` would answer for on its own.
        current = normalise(&current.join(base));
    }

    folders.reverse();
    folders
}

/// A path with its `.` and `..` steps applied, without touching the filesystem.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }

    out
}

/// One of a save folder's Lua parts, or None where the save does not change it.
///
/// The part files sit under `<save>.ntwtf.lua.parts`, named for the Lua table they carry.
fn part(folder: &Path, name: &str) -> Option<serde_json::Value> {
    let stem = folder.file_name()?.to_str()?;
    let path = folder.join(format!("{stem}.lua.parts")).join(name);
    read_json(&path)
}

/// A JSON document, or None where it is not there.
///
/// # Panics
///
/// If it is there and will not parse. A fixture that has become unreadable is a thing to
/// stop for, not to treat as absent.
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    if !path.exists() {
        return None;
    }

    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    Some(
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} will not parse: {e}", path.display())),
    )
}

/// The changes a part carries, whether it is a full table or a diff over one.
///
/// A base writes its table out whole and a diff writes only what it changes under
/// `_changes`. Both are read the same way here, since the merge that follows does not care
/// which it was - only the order does, and that is what [`chain`] fixes.
fn changes(document: &serde_json::Value) -> Option<&serde_json::Map<String, serde_json::Value>> {
    document
        .get("_changes")
        .unwrap_or(document)
        .as_object()
}

/// Keys a part carries that are about the FILE rather than about the world.
///
/// `_derived_simx` is the packer's own index into the entry tables; `_format` says which
/// of the two shapes above the document is in. Neither is a dialogue variable, and a world
/// that answered one as though it were would be answering a question no guard asks.
const NOT_A_VARIABLE: [&str; 2] = ["_format", "_derived_simx"];

/// Every dialogue variable a save holds, with the bases it rests on merged in.
///
/// ## Why this is read and not declared
///
/// An ordinary option's guard asks about the world in a way a check's rarely does. 451:86
/// - buy the Faln sneakers - is guarded on `jam.siileng_faln_sneakers == true`, and a
/// world that cannot answer it stops the crawl before it builds a single state: the engine
/// reports the floor over zero states, and the option draws nothing where the game draws
/// orange. The answer is in the save the in-game run loads, so it is read from there
/// rather than copied into the definition beside the expectation, for the same reason
/// [`read_in_save`] reads what a save has displayed.
///
/// ## What it cannot answer
///
/// The INVENTORY. `CheckItem("x")` is answered from the world's items and a save's Lua
/// parts carry the item definition table rather than what is held, so an offline world
/// says "not held" for everything. That is the right answer for every fixture here - the
/// Siileng scenarios exist to buy the sneakers, so the save does not hold them - and it is
/// more permissive than the game in general. A scenario that needs an item HELD will have
/// to say so in its row.
///
/// # Panics
///
/// If the chain will not resolve, or a value is one no guard could hold.
pub fn variables_in_save(save: &str) -> HashMap<String, WireValue> {
    let mut variables: HashMap<String, WireValue> = HashMap::new();

    for folder in chain(save) {
        let Some(document) = part(&folder, "Variable.json") else { continue };
        let Some(entries) = changes(&document) else {
            panic!("{}'s variables are not a table", folder.display());
        };

        for (name, value) in entries {
            if NOT_A_VARIABLE.contains(&name.as_str()) {
                continue;
            }

            match wire(value) {
                Some(answer) => {
                    variables.insert(name.clone(), answer);
                }
                // A TABLE OR A NULL, which no guard can compare against. Left unanswered
                // rather than guessed at, which reads as Unknown and is the permissive
                // answer - the same thing the plugin sends for a variable it could not
                // read.
                None => {
                    variables.remove(name);
                }
            }
        }
    }

    variables
}

/// One JSON value as the engine's wire vocabulary, or None where it is not one.
fn wire(value: &serde_json::Value) -> Option<WireValue> {
    match value {
        serde_json::Value::Bool(value) => Some(WireValue::Bool { value: *value }),
        serde_json::Value::Number(number) => {
            number.as_f64().map(|value| WireValue::Number { value })
        }
        serde_json::Value::String(value) => Some(WireValue::Text { value: value.clone() }),
        _ => None,
    }
}

/// What a scenario's global state fixture records, by entry, for one conversation.
///
/// The mod's own state file, read as the mod reads it. Format 3 keys the entries by
/// conversation under `conversations.WasDisplayed`.
#[derive(Debug, Deserialize)]
struct GlobalState {
    #[serde(default)]
    conversations: Recorded,
}

#[derive(Debug, Default, Deserialize)]
struct Recorded {
    #[serde(rename = "WasDisplayed", default)]
    was_displayed: std::collections::HashMap<String, Vec<i32>>,
}

/// What some other save has read of one conversation, per a staged global state file.
///
/// # Panics
///
/// If the file is missing or is not a global state. Both mean the fixture the in-game run
/// would stage is not there, which is not something to pass over quietly.
pub fn recorded_elsewhere(state_file: &str, conversation: i32) -> HashSet<i32> {
    let path = scenarios().join(state_file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} does not read: {e}", path.display()));
    let state: GlobalState = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} is not a global state: {e}", path.display()));

    state
        .conversations
        .was_displayed
        .get(&conversation.to_string())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// What a scenario save has already displayed, of one conversation.
///
/// READ FROM THE SAVE THE IN-GAME RUN LOADS, rather than copied into the definition beside
/// it. A row names its save and nothing more, so the two executors cannot come to disagree
/// about what that save holds - which they would the first time a save was edited and the
/// copy was not.
///
/// A scenario save is a diff over a base, and only the entries it CHANGES are written
/// down. THE WHOLE CHAIN IS WALKED, base-most first, and the last folder to say anything
/// about this conversation wins - which is what a diff means.
///
/// It used to read the leaf alone, on the documented grounds that no base in this
/// repository records a displayed entry. That was true, and it was an assumption where
/// walking the chain is a fact; the chain has to be resolved for the variables anyway
/// (see [`variables_in_save`]), so there is nothing left to buy by assuming it.
///
/// # Panics
///
/// If the save is not there, or a Conversation part will not parse.
pub fn read_in_save(save: &str, conversation: i32) -> HashSet<i32> {
    let mut displayed = HashSet::new();

    for folder in chain(save) {
        let Some(document) = part(&folder, "Conversation.json") else { continue };
        let Some(runs) = changes(&document)
            .and_then(|entries| entries.get(&conversation.to_string()))
            .and_then(|entry| entry.get("Dialog"))
            .and_then(|dialog| dialog.get("WasDisplayed"))
            .and_then(|runs| runs.as_str())
        else {
            continue;
        };

        displayed = parse_runs(runs);
    }

    displayed
}

/// An entry list as a save writes it: ids and `a-b` ranges, comma separated.
pub fn parse_runs(text: &str) -> HashSet<i32> {
    let mut entries = HashSet::new();
    for piece in text.split(',').map(str::trim).filter(|piece| !piece.is_empty()) {
        match piece.split_once('-') {
            Some((first, last)) => {
                let first: i32 = first.trim().parse().expect("a range starts at a number");
                let last: i32 = last.trim().parse().expect("and ends at one");
                entries.extend(first..=last);
            }
            None => {
                entries.insert(piece.parse().expect("an entry list holds numbers"));
            }
        }
    }

    entries
}
