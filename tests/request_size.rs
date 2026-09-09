// SPDX-License-Identifier: MIT
//! How big is one look-ahead request, and what shape should the entry sets travel in?
//!
//! The bridge sends the world as data, and three of its fields name ENTRIES: which the
//! player has been shown, which are unseen this game, which are unseen in any game. A
//! group is not small - conversation 631's is 4,514 entries across six conversations - and
//! the plugin builds one of these inside the frame that draws a response menu. So the
//! shape those sets travel in is not a detail, and de-i5xj.7 says to measure before
//! choosing it.
//!
//! ## The four shapes
//!
//! - `objects`   `[{"conversation":631,"entry":12},...]` - what the first crossing wrote,
//!               and the obvious thing.
//! - `ids`       `{"631":[0,1,2,...]}` - grouped by conversation. NOT invented here: it is
//!               the shape the mod's own global state has been written in on disk since
//!               format version 3, for this same data.
//! - `runs`      `{"631":"0..40,42,50..99"}` - the same, with consecutive ids collapsed.
//! - `bits`      a base64 bitmap, one bit per entry, in the order `questions_for` returned
//!               them.
//!
//! `runs` is chosen, and `bits` - the smallest - is not. At 146 bytes for a whole group
//! there is nothing left to buy, and what a bitmap would cost is self-description: it only
//! means anything against a matching entries list, so a plugin holding a cached one against
//! a rebuilt index would send bits that decode cleanly and mean something else.
//!
//! ## And the other half of the request
//!
//! Measuring the whole thing rather than only the entry sets is what showed where the bytes
//! actually were. With the sets collapsed, most of a request was the 306 VARIABLE NAMES -
//! which the engine itself handed out, which cannot change while the game is running, and
//! which the plugin has already cached. Answering by position instead took the request from
//! 22,622 bytes to 12,315.
//!
//! What is left is mostly the answers' own `{"kind":...,"value":...}` wrapper, at 29 bytes
//! for a boolean. Untagging it would be the next thing available; nothing needs it yet.

use std::collections::HashSet;

use lookahead_engine::bridge::{
    questions_for, LookAheadRequest, NodeRef, NodeSet, WireValue, WorldSnapshot,
};
use lookahead_engine::index::read_index;

mod common;

/// The group every measurement in this repo is taken on: six conversations, 4,514 entries.
const MEASURED: i32 = 631;

/// The real record of what another save has already shown, as the in-game suite stages it.
///
/// Measured against a real one rather than a synthetic density because the shapes differ
/// most on how CLUSTERED a set is, and a save's history is clustered - a player walks
/// through a conversation, not through every seventh entry of one.
const WORST_CASE_STATE: &str = "testing/scenarios/global-state-worst-case.json";

#[test]
fn a_request_is_measured_and_the_entry_sets_have_a_shape() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let questions = questions_for(&index, MEASURED).expect("the group builds");

    let entries: Vec<NodeRef> = questions.entries.clone();
    println!(
        "conversation {MEASURED}: {} conversations, {} entries, {} checks, {} variables, \
         {} queries",
        questions.conversations.len(),
        entries.len(),
        questions.checks.len(),
        questions.variables.len(),
        questions.queries.len(),
    );

    let seen_elsewhere = seen_elsewhere(&entries);
    println!(
        "the staged global state has shown {} of them in some other save",
        seen_elsewhere.len(),
    );

    // The three sets a request carries, at the ends of the range and in the middle.
    let cases: [(&str, Vec<NodeRef>); 5] = [
        ("nothing seen (a fresh save)", Vec::new()),
        ("everything seen (a completionist save)", entries.clone()),
        ("seen elsewhere, as the suite stages it", seen_elsewhere.iter().copied().collect()),
        ("one entry in ten, clustered", clustered(&entries, 10)),
        ("one entry in ten, scattered", scattered(&entries, 10)),
    ];

    println!(
        "\n{:<40} {:>7} {:>9} {:>8} {:>8} {:>7}",
        "one entry set", "members", "objects", "ids", "runs", "bits",
    );
    for (what, members) in &cases {
        let set: HashSet<NodeRef> = members.iter().copied().collect();
        println!(
            "{what:<40} {:>7} {:>9} {:>8} {:>8} {:>7}",
            set.len(),
            as_objects(&set).len(),
            as_ids(&set).len(),
            as_runs(&set).len(),
            as_bits(&set, &entries).len(),
        );
    }

    // And the whole request, as it actually travels: every question answered, and the
    // novelty of every entry decided.
    let mut world = WorldSnapshot {
        money: 250,
        day_minutes: 12 * 60,
        day_counter: 1,
        ..Default::default()
    };

    // Positionally, which is how the plugin answers: it cached the questions when it first
    // met the group, so sending the names back would be sending back what the engine said.
    world.variable_values =
        vec![WireValue::Bool { value: false }; questions.variables.len()];
    world.query_values = vec![WireValue::Bool { value: true }; questions.queries.len()];
    world.checks_pass = questions.checks.iter().copied().collect();
    world.seen = entries.iter().copied().collect();

    let request = LookAheadRequest {
        conversation: MEASURED,
        starts: entries.iter().copied().take(12).collect(),
        unseen_any_game: seen_elsewhere.iter().copied().collect(),
        unseen_this_game: NodeSet::default(),
        state_budget: 0,
        time_budget_ms: 0,
        menu_time_budget_ms: 0,
        memory_budget_mb: 0,
        world,
    };

    let text = serde_json::to_string(&request).expect("it serialises");
    println!(
        "\na whole request for conversation {MEASURED}, everything answered: {} bytes",
        text.len(),
    );

    // The same request with the names sent back too, which is what answering by name costs
    // per response menu for a list that cannot change while the game is running.
    let mut named = request.world.clone();
    named.variables = questions
        .variables
        .iter()
        .cloned()
        .zip(named.variable_values.drain(..))
        .collect();
    named.queries =
        questions.queries.iter().cloned().zip(named.query_values.drain(..)).collect();
    let by_name = LookAheadRequest { world: named, ..request };
    println!(
        "the same request answering by name instead of by position: {} bytes",
        serde_json::to_string(&by_name).expect("it serialises").len(),
    );

    // Not a threshold anybody tuned. It is well clear of what a request costs now and far
    // below the 341,357 bytes the same request cost when every entry set was a list of
    // JSON objects, so it fails if the entry sets ever go back to that.
    assert!(
        text.len() < 50_000,
        "a request grew to {} bytes; the entry sets have gone back to a verbose shape",
        text.len(),
    );
}

/// The entries some other save has already shown, out of the staged global state.
fn seen_elsewhere(entries: &[NodeRef]) -> Vec<NodeRef> {
    let path = common::repo_root().join(WORST_CASE_STATE);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the staged global state at {} would not read: {e}", path.display()));
    let state: serde_json::Value =
        serde_json::from_str(&text).expect("the staged global state is JSON");

    let shown = &state["conversations"]["WasDisplayed"];
    let group: HashSet<NodeRef> = entries.iter().copied().collect();

    let mut found = Vec::new();
    for (conversation, runs) in shown.as_object().expect("WasDisplayed is an object") {
        let conversation: i32 = conversation.parse().expect("a conversation id");

        // RUN-ENCODED since format 4, by the same encoder the wire's NodeSet is measured
        // against below. The file is now a twentieth of what it was - 423 KB to 22.5 KB on
        // this fixture - which does not change what this measures: the wire has always
        // carried runs, and this is the input to it.
        for entry in common::fixtures::parse_runs(
            runs.as_str().expect("entry ids are a run-encoded string"),
        ) {
            let node = NodeRef { conversation, entry };
            if group.contains(&node) {
                found.push(node);
            }
        }
    }

    found.sort_by_key(|node| (node.conversation, node.entry));
    found
}

/// Every `nth` entry, which is the worst case for anything that collapses runs.
fn scattered(entries: &[NodeRef], nth: usize) -> Vec<NodeRef> {
    entries.iter().copied().step_by(nth).collect()
}

/// A tenth of the entries in one stretch, which is what walking a conversation looks like.
fn clustered(entries: &[NodeRef], nth: usize) -> Vec<NodeRef> {
    entries.iter().copied().take(entries.len() / nth).collect()
}

fn as_objects(set: &HashSet<NodeRef>) -> String {
    let mut sorted: Vec<NodeRef> = set.iter().copied().collect();
    sorted.sort_by_key(|node| (node.conversation, node.entry));
    serde_json::to_string(&sorted).expect("it serialises")
}

/// Grouped by conversation, as the mod's own state file has been written since version 3.
fn as_ids(set: &HashSet<NodeRef>) -> String {
    serde_json::to_string(&by_conversation(set)).expect("it serialises")
}

/// The same, with consecutive ids collapsed - the shape the bridge chose, measured as the
/// bridge itself writes it rather than as a copy of it kept in step by hope.
fn as_runs(set: &HashSet<NodeRef>) -> String {
    let set: NodeSet = set.iter().copied().collect();
    serde_json::to_string(&set).expect("it serialises")
}

/// One bit per entry, in the order the engine returned them, base64 with no padding.
fn as_bits(set: &HashSet<NodeRef>, entries: &[NodeRef]) -> String {
    let mut bytes = vec![0u8; entries.len().div_ceil(8)];
    for (position, node) in entries.iter().enumerate() {
        if set.contains(node) {
            bytes[position / 8] |= 1 << (position % 8);
        }
    }

    base64(&bytes)
}

fn by_conversation(set: &HashSet<NodeRef>) -> std::collections::BTreeMap<String, Vec<i32>> {
    let mut grouped: std::collections::BTreeMap<String, Vec<i32>> = Default::default();
    for node in set {
        grouped.entry(node.conversation.to_string()).or_default().push(node.entry);
    }
    for ids in grouped.values_mut() {
        ids.sort_unstable();
    }
    grouped
}

/// Base64, written out because measuring a shape needs its real length and this crate has
/// no encoder. Unpadded, which is the shorter and the one a measurement should charge.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut block = 0u32;
        for (position, byte) in chunk.iter().enumerate() {
            block |= u32::from(*byte) << (16 - 8 * position);
        }
        for position in 0..chunk.len() + 1 {
            out.push(ALPHABET[((block >> (18 - 6 * position)) & 0x3F) as usize] as char);
        }
    }

    out
}
