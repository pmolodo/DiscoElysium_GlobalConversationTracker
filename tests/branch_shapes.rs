// SPDX-License-Identifier: MIT
//! Every Pass / Fail line the in-game suites arrange, drawn again without a game.
//!
//! ## What this is, and what it is not
//!
//! NOT a second set of examples that happen to cover the same rules. The rows come from
//! `testing/scenarios/branch-shapes.json`, and each one names the SAME fixture the in-game
//! suite stages - the same global state file, the same save's read entries, the same
//! budget, the same conversation and entry - so this runs the scenario rather than
//! something like it. `BranchShapeTests` on the C# side holds the same rows against the
//! suites that stage them, so a row cannot describe a run that is not happening.
//!
//! ## Why it is worth having both
//!
//! The in-game run is the only thing that can say the Harmony patch is installed, that a
//! real response menu was composed, and that the marker reached the text the game drew. It
//! costs a launch and several minutes, takes over the display, and cannot run at all on a
//! locked machine.
//!
//! This costs the time to read the index, runs under `cargo test`, and says the other half:
//! that from this exact world the engine reaches this exact answer. When the two disagree,
//! the disagreement is the finding - the wiring is wrong, or the fixture no longer means
//! what it meant.
//!
//! ## What a failure here means
//!
//! Either the engine's answer changed, or the fixture did. Both are worth stopping for,
//! and the message names the row so the in-game suite that shares it can be run directly:
//! `look-ahead --suite <name>`.

use std::collections::HashSet;

use lookahead_engine::bridge::{answer, BranchAnswer, LookAheadRequest, NodeRef, WorldSnapshot};
use lookahead_engine::index::{build_group_graph, read_index};
use serde::Deserialize;

mod common;

use common::fixtures;

/// The table both sides read.
const TABLE: &str = "testing/scenarios/branch-shapes.json";

/// A novelty, spelled as the colour the mod paints it.
///
/// The mod's own vocabulary rather than the engine's, because that is what a row is
/// describing - what the player sees - and because a row is read by a person deciding
/// whether a fixture still says what they meant.
fn rung(colour: &str) -> i32 {
    match colour {
        "orange" => 2,
        "red" => 1,
        "darkRed" => 0,
        other => panic!("'{other}' is not a colour a half can be drawn in"),
    }
}

#[derive(Debug, Deserialize)]
struct Table {
    conversation: i32,
    entry: i32,
    rows: Vec<Row>,
}

#[derive(Debug, Deserialize)]
struct Row {
    suite: String,
    what: String,
    state: String,
    save: String,
    /// The state budget to run at, or 0 for no such limit.
    ///
    /// What a row starves a crawl with, so that a shape only an unfinished search can
    /// produce is reachable at all. TEST-ONLY since de-7z0f: no player setting writes it,
    /// and the memory budget cannot do the job - see `LookAheadRequest::state_budget`.
    #[serde(rename = "stateBudget")]
    state_budget: usize,
    pass: Half,
    fail: Half,
}

/// One half of the line a row expects: the word's colour, and its asterisk if it has one.
#[derive(Debug, Deserialize)]
struct Half {
    colour: String,
    #[serde(default)]
    marker: Option<String>,
}

impl Half {
    /// Whether an answer draws this half.
    ///
    /// The rules the mod's `BranchLine` follows, restated against the answer rather than
    /// against the markup: the word takes the destination's colour, and an asterisk is
    /// drawn in the best's colour when the best outranks the destination, or grey when the
    /// search did not finish.
    fn matches(&self, branch: &BranchAnswer) -> bool {
        if branch.destination != rung(&self.colour) {
            return false;
        }

        // AT MOST THE DESTINATION, not exactly it. A search cut short before it dequeues
        // anything reports the floor, which is BELOW where the branch lands - the branch
        // knows where it leads without searching, and the search is what would have found
        // more. So "no asterisk" and "gave up" are both best <= destination, told apart by
        // whether the search finished.
        match self.marker.as_deref() {
            None => branch.best <= branch.destination && branch.complete,
            Some("gaveUp") => branch.best <= branch.destination && !branch.complete,
            Some(colour) => branch.best == rung(colour) && branch.best > branch.destination,
        }
    }

    fn describe(branch: &BranchAnswer) -> String {
        let colour = match branch.destination {
            2 => "orange",
            1 => "red",
            _ => "darkRed",
        };

        // THE SAME ORDER BranchLine DRAWS IN. A witness beats a give-up: a search that ran
        // out of budget AFTER finding something has still found it, so the found marker
        // wins and only a search that found nothing reports as uncertain. Testing them the
        // other way round described every incomplete half as grey, including ones the mod
        // draws an asterisk on - which made a passing row and a failing row read alike.
        if branch.best > branch.destination {
            let marker = if branch.best == 2 { "orange" } else { "red" };
            return format!("{colour} with a {marker} asterisk");
        }

        if !branch.complete {
            return format!("{colour} with a grey '*?'");
        }

        colour.to_string()
    }
}

/// What a scenario's global state fixture records, and what its save has already read,
/// both come from `common::fixtures` - the same reader `scenario_suites.rs` uses.
///
/// SHARED RATHER THAN COPIED, and it was copied first. Assembling "what has this save
/// read" is the fiddly half of standing a scenario up offline, and two copies of it is
/// two things to keep agreeing - which is the drift the shared definition exists to
/// remove, reappearing one level down in the executors.

fn table() -> Table {
    let path = common::repo_root().join(TABLE);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} does not read: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not a shape table: {error}", path.display()))
}

#[test]
fn every_shape_the_suites_arrange_is_reached_offline() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let table = table();
    let start = NodeRef { conversation: table.conversation, entry: table.entry };
    let (graph, group) = build_group_graph(&index, table.conversation)
        .expect("the fan's group builds");
    let everything: Vec<NodeRef> =
        graph.nodes().map(|node| NodeRef::from(node.id)).collect();

    let mut failures: Vec<String> = Vec::new();

    for row in &table.rows {
        // OVER THE WHOLE GROUP, not the conversation the row names. The engine loads
        // everything reachable from it, so every entry it might walk to has to be
        // classified; matching entry ids against one conversation's records lets the rest
        // fall through to "never seen anywhere", which invents the top rung wherever a
        // group spans more than one conversation. The fan's does not, today - it is the
        // one conversation - so this changes no answer here, and it is what the reading
        // means rather than what this fixture happens to allow.
        let recorded: HashSet<(i32, i32)> =
            fixtures::recorded_elsewhere_in_group(&row.state, &group);
        let read_here = fixtures::read_in_save_group(&row.save, &group);
        let rung_of = |node: &NodeRef| {
            let key = (node.conversation, node.entry);
            if read_here.contains(&key) {
                0
            } else if recorded.contains(&key) {
                1
            } else {
                2
            }
        };

        // The three rungs, exactly as the plugin builds them: read in THIS save wins,
        // then recorded in some other save, then never seen anywhere.
        let request = LookAheadRequest {
            conversation: table.conversation,
            starts: vec![start],
            unseen_any_game: everything
                .iter()
                .copied()
                .filter(|node| rung_of(node) == 2)
                .collect(),
            unseen_this_game: everything
                .iter()
                .copied()
                .filter(|node| rung_of(node) == 1)
                .collect(),
            state_budget: row.state_budget,
            world: WorldSnapshot { day_minutes: 720, day_counter: 1, ..Default::default() },
            ..Default::default()
        };

        let response = answer(&index, None, &request);
        assert!(response.error.is_none(), "{}: {:?}", row.suite, response.error);

        let reply = response.answers.first().expect("one start, one answer");
        let branches = reply.branches.as_ref().unwrap_or_else(|| {
            panic!("{}: {}:{} came back with no branches", row.suite, table.conversation, table.entry)
        });

        for (name, want, got) in [
            ("Pass", &row.pass, &branches.pass),
            ("Fail", &row.fail, &branches.fail),
        ] {
            if !want.matches(got) {
                let wanted = match want.marker.as_deref() {
                    None => want.colour.clone(),
                    Some("gaveUp") => format!("{} with a grey '*?'", want.colour),
                    Some(marker) => format!("{} with a {marker} asterisk", want.colour),
                };
                failures.push(format!(
                    "{} ({}): {name} should be {wanted}, and the engine draws {} \
                     - run it in game with --suite {}",
                    row.suite,
                    row.what,
                    Half::describe(got),
                    row.suite,
                ));
            }
        }
    }

    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    eprintln!("{} shapes reached offline", table.rows.len() * 2);
}

/// The table covers every shape a half can take, and says so by construction.
///
/// EIGHT, AND THE NINTH THAT CANNOT EXIST. Without this the table could lose a row and
/// still pass, since the test above only checks the rows that are there - and losing a row
/// is exactly how coverage disappears without anyone deciding to drop it.
#[test]
fn the_table_covers_every_shape() {
    let table = table();

    let mut seen: HashSet<String> = HashSet::new();
    for row in &table.rows {
        for half in [&row.pass, &row.fail] {
            seen.insert(match half.marker.as_deref() {
                None => half.colour.clone(),
                Some(marker) => format!("{}+{marker}", half.colour),
            });
        }
    }

    let wanted = [
        "orange",
        "red",
        "red+orange",
        "red+gaveUp",
        "darkRed",
        "darkRed+red",
        "darkRed+orange",
        "darkRed+gaveUp",
    ];

    let missing: Vec<&str> = wanted.iter().copied().filter(|s| !seen.contains(*s)).collect();
    assert!(missing.is_empty(), "no fixture arranges: {}", missing.join(", "));

    // An orange word cannot carry anything: nothing outranks the top rung, so there is
    // never something beyond it to report and a search that gave up has not made that
    // doubtful. A row claiming otherwise would be describing a line the mod will not draw.
    for row in &table.rows {
        for (name, half) in [("Pass", &row.pass), ("Fail", &row.fail)] {
            assert!(
                half.colour != "orange" || half.marker.is_none(),
                "{}: {name} is orange and claims a marker, which cannot be drawn",
                row.suite,
            );
        }
    }
}
