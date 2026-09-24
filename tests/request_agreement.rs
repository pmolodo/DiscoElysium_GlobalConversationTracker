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
//! THE GAME'S SIDE IS READ FROM ITS BYTES, the `.pb` the mod writes beside its text rendering,
//! through the decoder the engine host uses. The text is protobuf's spelling and the offline
//! side is the engine's serde form; decoding the bytes puts both in the engine's, so a
//! difference here is one in the world rather than in how a field is written down.
//!
//! ## What it is not
//!
//! NOT A CLAIM THAT THE TWO MUST MATCH. Some of what the game answers cannot be read out of a
//! save at all - what is worn, whether it is raining - so a difference here is a FINDING
//! rather than a failure, and this prints it. It fails only when a captured pair cannot be
//! read, which would mean the capture itself is broken.
//!
//! ```text
//! cargo test --profile release-incremental --test suite request_agreement:: -- --nocapture
//! ```

use std::path::PathBuf;

use lookahead_engine::formats::json_diff;

use gct_measure::common;

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
        if path.extension().is_none_or(|extension| extension != "pb") {
            continue;
        }
        let Some(stem) = path.file_stem() else {
            continue;
        };

        let beside = mine.join(format!("{}.json", stem.to_string_lossy()));
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
    let in_game = decoded(theirs);
    let offline = read(mine);

    println!("\n{}", theirs.file_name().unwrap().to_string_lossy());

    let conversation = in_game["conversation"].as_i64().unwrap_or_default() as i32;
    let names = asked_of(conversation);
    let in_game = canonical(&in_game, names.as_ref());
    let offline = canonical(&offline, names.as_ref());

    let Some(patch) = json_diff::create(&offline, &in_game) else {
        println!("  the two worlds are identical");
        return;
    };

    let changes = patch
        .get(json_diff::CHANGES_KEY)
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    for (field, value) in changes.as_object().into_iter().flatten() {
        describe(field, value, offline.get(field));
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

/// One field's difference, as a line or as the members that differ under it.
///
/// A WHOLE MAP PRINTED AS ONE LINE SAYS NOTHING, which is what the first version of this
/// did: 223 variables and 36 queries came out as one 700 KB value against one 9 KB value.
/// Where both sides hold an object, the members are walked and the ones that differ are
/// named.
fn describe(field: &str, in_game: &serde_json::Value, offline: Option<&serde_json::Value>) {
    let (Some(theirs), Some(mine)) = (
        in_game.as_object(),
        offline.and_then(serde_json::Value::as_object),
    ) else {
        println!("  {field}: {}", summarise(in_game, offline));
        return;
    };

    let mut differed = 0;
    for (member, value) in theirs {
        let was = mine.get(member);
        if was == Some(value) {
            continue;
        }

        differed += 1;
        if differed <= MEMBERS_SHOWN {
            println!("  {field}/{member}: {}", summarise(value, was));
        }
    }

    if differed > MEMBERS_SHOWN {
        println!(
            "  {field}: and {} more member(s) differ",
            differed - MEMBERS_SHOWN
        );
    }

    if differed == 0 {
        println!("  {field}: the same members, written differently");
    }
}

/// How many differing members of one field are named before the rest are counted.
const MEMBERS_SHOWN: usize = 12;

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

/// The names a group's questions come in, in the order the answers are sent.
///
/// THE POSITIONAL ANSWERS ARE UNREADABLE WITHOUT THEM, and unreadable is what they were:
/// the plugin sends 223 variable values and 36 query values as bare lists, because the
/// names are constant for a group and sending them per menu was 14 KB of a 22.5 KB request.
/// Both sides are put back into named form here so a difference reads as the variable it is
/// about rather than as position 147.
fn asked_of(conversation: i32) -> Option<lookahead_engine::bridge::Questions> {
    let path = common::shipped_index()?;
    let index = lookahead_engine::index::read_index(&path).ok()?;
    lookahead_engine::bridge::questions_for(&index, conversation).ok()
}

/// One world in a form the two sides can be compared in.
///
/// The request around it is not the subject: the budgets are the harness's own, and the
/// starts are the menu the game composed, which the offline report is handed rather than
/// composing. What is left is the world, with its positional answers named.
fn canonical(
    request: &serde_json::Value,
    names: Option<&lookahead_engine::bridge::Questions>,
) -> serde_json::Value {
    let mut world = request["world"].clone();
    let Some(object) = world.as_object_mut() else {
        return world;
    };

    for (positional, named, keys) in [
        (
            "variable_values",
            "variables",
            names.map(|asked| &asked.variables),
        ),
        ("query_values", "queries", names.map(|asked| &asked.queries)),
    ] {
        let Some(answers) = object.remove(positional) else {
            continue;
        };
        let (Some(answers), Some(keys)) = (answers.as_array(), keys) else {
            continue;
        };

        let mut by_name = object
            .remove(named)
            .and_then(|held| held.as_object().cloned())
            .unwrap_or_default();
        // A NAMED ANSWER WINS, as it does in the engine - see `WorldRawData::resolve`. The
        // failed-check locks arrive named, and the same flag asked positionally carries the Lua
        // read, which knows nothing of the lock.
        for (key, answer) in keys.iter().zip(answers) {
            by_name.entry(key.clone()).or_insert_with(|| answer.clone());
        }

        object.insert(named.to_string(), serde_json::Value::Object(by_name));
    }

    // SETS AS SETS. The engine holds these unordered, so the order either side wrote them in
    // says nothing, and left alone it reads as a difference in the world.
    for set in ["items", "thoughts"] {
        if let Some(serde_json::Value::Array(members)) = object.get_mut(set) {
            members.sort_by_key(ToString::to_string);
        }
    }

    world
}

/// The request the game sent, from its bytes, in the engine's serde form.
fn decoded(path: &std::path::Path) -> serde_json::Value {
    use prost::Message;

    let bytes =
        std::fs::read(path).unwrap_or_else(|why| panic!("{} does not read: {why}", path.display()));
    let wire = lookahead_engine::wire::LookAheadRequest::decode(bytes.as_slice())
        .unwrap_or_else(|why| panic!("{} is not a request: {why}", path.display()));
    let request = lookahead_engine::wire_convert::read_look_ahead(wire)
        .unwrap_or_else(|why| panic!("{} will not read: {why:?}", path.display()));
    serde_json::to_value(&request).expect("a request serialises")
}

/// The request the offline report wrote, in the engine's serde form.
fn read(path: &std::path::Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|why| panic!("{} does not read: {why}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|why| panic!("{} is not a request: {why}", path.display()))
}
