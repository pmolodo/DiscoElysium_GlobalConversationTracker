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
/// `_derived_simx` is the packer's own index into the entry tables; `_format` says which of
/// the two shapes above the document is in, and `_formatVersion` which version of it. None
/// is a dialogue variable, and a world that answered one as though it were would be
/// answering a question no guard asks.
///
/// `_formatVersion` is here BEFORE ANY COMMITTED SAVE CARRIES ONE. The stamp was added
/// while the shapes were unchanged, so the saves in this repository are unstamped and read
/// as version 1; the first one regenerated will carry it, and a reader that learned about
/// it only then would have been wrong in between with nothing to say so.
const NOT_A_VARIABLE: [&str; 3] = ["_format", "_formatVersion", "_derived_simx"];

/// Every dialogue variable a save holds, with the bases it rests on merged in.
///
/// ## Why this is read and not declared
///
/// An ordinary option's guard asks about the world in a way a check's rarely does. 451:86
/// - buy the Faln sneakers - is guarded on `jam.siileng_faln_sneakers == true`, and a
/// world that cannot answer it stops the search before it builds a single state: the engine
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

/// Format 4 writes each conversation's entries as a RUN-ENCODED STRING where format 3 wrote
/// an array - see `GlobalStateJson`, and `parse_runs` for the spelling. On the worst-case
/// fixture, which records every entry in the game, that took 423 KB to 22.5 KB.
#[derive(Debug, Default, Deserialize)]
struct Recorded {
    #[serde(rename = "WasDisplayed", default)]
    was_displayed: std::collections::HashMap<String, String>,
}

/// What some other save has read, per a staged global state file, over a whole group.
///
/// ## Why a group and not a conversation
///
/// A LOOK-AHEAD IS ASKED OF A GROUP, and a group is several conversations: the engine
/// loads everything reachable from the one that is open, and Joyce's runs to 2,857 entries
/// across many. Classifying only the open conversation's entries and letting the rest fall
/// through to "never seen anywhere" is not a small inaccuracy - it invents the top rung
/// almost everywhere, so a search that should be refused finds something to walk towards.
/// Measured: it made 2,629 of Joyce's 2,857 entries look worth searching under a fixture
/// that records every entry in the game.
///
/// A conversation the state says nothing about contributes nothing, which is the same
/// answer the mod's own state gives for it.
pub fn recorded_elsewhere_in_group(
    state_file: &str,
    conversations: &[i32],
) -> HashSet<(i32, i32)> {
    let state = read_state(state_file);
    let mut recorded = HashSet::new();

    for conversation in conversations {
        let Some(runs) = state.conversations.was_displayed.get(&conversation.to_string())
        else {
            continue;
        };

        recorded.extend(parse_runs(runs).into_iter().map(|entry| (*conversation, entry)));
    }

    recorded
}

/// A staged global state file, read as the mod reads it.
///
/// # Panics
///
/// If it is missing or is not a global state.
fn read_state(state_file: &str) -> GlobalState {
    let path = scenarios().join(state_file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} does not read: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} is not a global state: {e}", path.display()))
}

/// What a scenario save has already displayed, over a whole group.
///
/// READ FROM THE SAVE THE IN-GAME RUN LOADS, rather than copied into the definition beside
/// it. A row names its save and nothing more, so the two executors cannot come to disagree
/// about what that save holds - which they would the first time a save was edited and the
/// copy was not.
///
/// A scenario save is a diff over a base, and only the entries it CHANGES are written
/// down. THE WHOLE CHAIN IS WALKED, base-most first, and the last folder to say anything
/// about a conversation wins - which is what a diff means. It used to read the leaf alone,
/// on the documented grounds that no base in this repository records a displayed entry.
/// That was true, and it was an assumption where walking the chain is a fact; the chain has
/// to be resolved for the variables anyway (see [`variables_in_save`]), so there is nothing
/// left to buy by assuming it.
///
/// ONE WALK FOR EVERY CONVERSATION OF THE GROUP: the base writes out the whole Conversation
/// table, and re-reading it once per conversation would be re-parsing a megabyte per
/// question. See [`recorded_elsewhere_in_group`] for why the group rather than the open
/// conversation is what a look-ahead is asked about.
///
/// # Panics
///
/// If the save is not there, or a Conversation part will not parse.
pub fn read_in_save_group(save: &str, conversations: &[i32]) -> HashSet<(i32, i32)> {
    let mut displayed: HashSet<(i32, i32)> = HashSet::new();

    for folder in chain(save) {
        let Some(document) = part(&folder, "Conversation.json") else { continue };
        let Some(entries) = changes(&document) else { continue };

        for conversation in conversations {
            let Some(runs) = entries
                .get(&conversation.to_string())
                .and_then(|entry| entry.get("Dialog"))
                .and_then(|dialog| dialog.get("WasDisplayed"))
                .and_then(|runs| runs.as_str())
            else {
                continue;
            };

            // THE LAST FOLDER TO SAY ANYTHING WINS, per conversation, which is what a
            // diff means: a later save replacing the list replaces it, and says nothing
            // about the conversations it left alone.
            displayed.retain(|(had, _)| had != conversation);
            displayed.extend(parse_runs(runs).into_iter().map(|entry| (*conversation, entry)));
        }
    }

    displayed
}

/// A run-encoded list of numbers, as every file in this repository writes one.
///
/// ## The spelling, which is shared and not invented here
///
/// `3,5,7-25`: comma-separated pieces, each a number or a `first-last` range. The one
/// implementation that writes it is `SparseOrder` in `GlobalConversationTracker.Core`, and
/// this reads exactly what that writes - the sparse saves, and since format 4 the global
/// state file's entry sets too.
///
/// TWO THINGS IT HAS THAT A NAIVE SPLIT ON `-` DOES NOT, both of which the writer produces:
///
/// - A NEGATIVE BOUND. A leading `-` is a sign, not a separator, so the separator is looked
///   for past the first character. This is the whole of what the wire's `..` was chosen to
///   avoid, and it is one condition.
/// - A DESCENDING RANGE. `25-7` counts down. The saves write their dialogue variables
///   newest first, so a backwards run is as common as a forwards one and says the same
///   thing in the same space.
///
/// # Panics
///
/// If a piece is not a number or a range of them. A fixture that has stopped being readable
/// is a thing to stop for.
pub fn parse_runs(text: &str) -> HashSet<i32> {
    let mut entries = HashSet::new();

    for piece in text.split(',').map(str::trim).filter(|piece| !piece.is_empty()) {
        // Past the first character, so a leading minus reads as a sign.
        let split = piece
            .char_indices()
            .skip(1)
            .find(|(_, c)| *c == '-')
            .map(|(at, _)| at);

        let (first, last) = match split {
            Some(at) => (&piece[..at], &piece[at + 1..]),
            None => (piece, piece),
        };

        let first: i32 = first
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("'{piece}' is not a run: '{first}' is not a number"));
        let last: i32 = last
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("'{piece}' is not a run: '{last}' is not a number"));

        if first <= last {
            entries.extend(first..=last);
        } else {
            entries.extend(last..=first);
        }
    }

    entries
}
