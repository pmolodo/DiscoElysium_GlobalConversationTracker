// SPDX-License-Identifier: MIT
//! Every Pass / Fail line the in-game suites arrange, drawn again without a game.
//!
//! ## What this is, and what it is not
//!
//! NOT a second set of examples that happen to cover the same rules. The rows come from
//! `testing/scenarios/branch-shapes.json`, and each one names the SAME fixture the in-game
//! suite stages - the same global state file, the same save's read entries, the same
//! budget, the same conversation and entry - so this runs the scenario rather than
//! something like it. `LookAheadSuites.BranchShapes` on the C# side builds its suites out
//! of that same file, so a row cannot describe a run that is not happening.
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

use lookahead_engine::bridge::{LookAheadAnswer, LookAheadRequest, NodeRef, WorldSnapshot, answer};
use lookahead_engine::index::{build_group_graph, read_index};
use serde::Deserialize;

mod common;

use common::fixtures;

/// The table both sides read.
const TABLE: &str = "testing/scenarios/branch-shapes.json";

/// A seen state, spelled as the colour the mod paints it.
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
    checks: Vec<Check>,
}

/// One rolled check, and every shape its Pass / Fail line is asked to take.
///
/// GROUPED BY CHECK, because a shape is a property of the line the mod draws and the same
/// code draws it onto a white check's band and a red one's. A shape that had only ever
/// been arranged on the fan's white check was a shape nobody had seen the mod put on a red
/// one - which is the half of de-8hh2.9 its fixture did not finish.
#[derive(Debug, Deserialize)]
struct Check {
    /// Which check it is, in one line, for the message a failing row prints.
    what: String,
    conversation: i32,
    entry: i32,
    /// What each half of this check's line can take.
    coverage: Coverage,
    rows: Vec<Row>,
}

/// What each half of one check's line is expected to cover.
///
/// PER HALF, WHICH IS THE WHOLE POINT. Counting a check's shapes with Pass and Fail thrown
/// into one set let all eight be present while the FAIL half only ever took the three bare
/// colours - and that is exactly what had happened: every markered shape in the table sat
/// on a Pass half, so the mod's Fail half had never been seen drawing an asterisk at all.
#[derive(Debug, Deserialize)]
struct Coverage {
    pass: String,
    fail: String,
}

#[derive(Debug, Deserialize)]
struct Row {
    suite: String,
    what: String,
    state: String,
    save: String,
    /// The state budget to run at, or 0 for no such limit.
    ///
    /// What a row starves a search with, so that a shape only an unfinished search can
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
    fn matches(&self, branch: &LookAheadAnswer) -> bool {
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

    fn describe(branch: &LookAheadAnswer) -> String {
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

// What a scenario's global state fixture records, and what its save has already read,
// both come from `common::fixtures` - the same reader `scenario_suites.rs` uses.
//
// SHARED RATHER THAN COPIED. Assembling "what has this save read" is the fiddly half of
// standing a scenario up offline, and two copies of it is two things to keep agreeing -
// which is the drift the shared definition exists to remove, reappearing one level down
// in the executors.

/// The header this reader is written for.
///
/// CHECKED RATHER THAN ASSUMED, and the cheap mistake it catches is real: the two tables
/// are the same shape of document, so one read as the other parses, deserialises into
/// something, and quietly describes nothing the reader wanted.
const FORMAT: lookahead_engine::formats::header::Expected =
    lookahead_engine::formats::header::Expected {
        format: "branch-shapes",
        version: 1,
    };
fn table() -> Table {
    let path = common::repo_root().join(TABLE);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} does not read: {error}", path.display()));

    let document: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not JSON: {error}", path.display()));
    FORMAT
        .check_document(&document)
        .unwrap_or_else(|fault| panic!("{}: {fault}", path.display()));

    serde_json::from_value(document)
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
    let mut failures: Vec<String> = Vec::new();
    let mut shapes = 0;

    for check in &table.checks {
        shapes += check.rows.len() * 2;
        failures.extend(disagreements(&index, check));
    }

    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    eprintln!("{shapes} shapes reached offline");
}

/// Every way one check's rows and the engine disagree, which is nothing when they agree.
///
/// ONE CHECK AT A TIME because the graph is the expensive part and it is per check, not
/// per row: the group is built once here and every row of the check is answered over it.
fn disagreements(index: &lookahead_engine::index::Index, check: &Check) -> Vec<String> {
    let start = NodeRef {
        conversation: check.conversation,
        entry: check.entry,
    };
    let (graph, group) = build_group_graph(index, check.conversation)
        .unwrap_or_else(|error| panic!("{}'s group builds: {error}", check.what));
    let everything: Vec<NodeRef> = graph.nodes().map(|node| NodeRef::from(node.id)).collect();

    let mut failures: Vec<String> = Vec::new();

    for row in &check.rows {
        // OVER THE WHOLE GROUP, not the conversation the check names. The engine loads
        // everything reachable from it, so every entry it might walk to has to be
        // classified; matching entry ids against one conversation's records lets the rest
        // fall through to "never seen anywhere", which invents the top rung wherever a
        // group spans more than one conversation. Neither check's group does, today - each
        // is the one conversation - so this changes no answer here, and it is what the
        // reading means rather than what these fixtures happen to allow.
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

        // The three rungs, exactly as the plugin builds them, from TWO SENT SETS. What this save
        // has read goes to `world.seen`, because that is the game's own per-save record.
        // `seen_any_game` names what SOME playthrough has read, which is the bottom two rungs
        // together - seen this game implies seen any game. NOTHING SENDS THE MIDDLE RUNG: an
        // entry this world has not seen and `seen_any_game` DOES hold is that rung. See
        // `world::seen_state`, which is the one place the three rungs are decided.
        let request = LookAheadRequest {
            conversation: check.conversation,
            starts: vec![start],
            seen_any_game: everything
                .iter()
                .copied()
                .filter(|node| rung_of(node) < 2)
                .collect(),
            state_budget: row.state_budget,
            world: WorldSnapshot {
                day_minutes: 720,
                day_counter: 1,
                seen: everything
                    .iter()
                    .copied()
                    .filter(|node| rung_of(node) == 0)
                    .collect(),
                ..Default::default()
            },
            ..Default::default()
        };

        let response = answer(index, None, &request);
        assert!(
            response.error.is_none(),
            "{}: {:?}",
            row.suite,
            response.error
        );

        // TWO ANSWERS, ONE PER OUTCOME. A check is two options wearing one line of
        // text, and since de-8hh2.6 the engine says so directly rather than nesting a
        // pair inside one answer for the option.
        let (pass, fail) = response.outcomes(start).unwrap_or_else(|| {
            panic!(
                "{}: {}:{} did not come back as two outcomes",
                row.suite, check.conversation, check.entry,
            )
        });

        for (name, want, got) in [("Pass", &row.pass, pass), ("Fail", &row.fail, fail)] {
            if !want.matches(got) {
                let wanted = match want.marker.as_deref() {
                    None => want.colour.clone(),
                    Some("gaveUp") => format!("{} with a grey '*?'", want.colour),
                    Some(marker) => format!("{} with a {marker} asterisk", want.colour),
                };
                failures.push(format!(
                    "{} (on {}, {}): {name} should be {wanted}, and the engine draws {} \
                     - run it in game with --suite {}",
                    row.suite,
                    check.what,
                    row.what,
                    Half::describe(got),
                    row.suite,
                ));
            }
        }
    }

    failures
}

/// The eight shapes, in the order the table's `_eight` lists them.
const EIGHT: [&str; 8] = [
    "orange",
    "red",
    "red+orange",
    "red+gaveUp",
    "darkRed",
    "darkRed+red",
    "darkRed+orange",
    "darkRed+gaveUp",
];

/// The three a half can take when nothing can ever lie beyond where it lands.
const COLOURS: [&str; 3] = ["orange", "red", "darkRed"];

/// One half's shape, spelled the way `_eight` spells it.
fn shape(half: &Half) -> String {
    match half.marker.as_deref() {
        None => half.colour.clone(),
        Some(marker) => format!("{}+{marker}", half.colour),
    }
}

/// Every half of every check covers what that check says it can, and nothing more.
///
/// EIGHT, AND THE NINTH THAT CANNOT EXIST. Without this the table could lose a row and
/// still pass, since the test above only checks the rows that are there - and losing a row
/// is exactly how coverage disappears without anyone deciding to drop it.
///
/// PER CHECK, because the same code draws the line onto a white check's band and a red
/// one's, so a shape arranged only on the fan is a shape nobody has seen the mod put on a
/// red check - the gap de-8hh2.9 left behind.
///
/// AND PER HALF, which is the stronger claim and the one this used to miss. Pass and Fail
/// were counted into ONE set per check, so all eight could be present while the Fail half
/// only ever took the three bare colours - and that is what had happened. Every markered
/// shape in the table sat on a Pass half; the mod's Fail half had never been seen drawing
/// an asterisk of any kind, so a fault in that path would have been invisible here.
///
/// BOTH DIRECTIONS. A half claiming `eight` that produces seven fails, and a half claiming
/// `colours` that produces a marker fails too - because that would mean the graph changed
/// underneath the claim, and the claim is the interesting half of the fixture. Klaasje's
/// flower says `colours` for its Fail because failing lands on 656:24, whose `to` is empty:
/// nothing can ever lie beyond it to outrank it.
#[test]
fn every_half_covers_what_its_check_claims() {
    let table = table();

    for check in &table.checks {
        // COLLECTED PER HALF FIRST, rather than iterated in the loop below: the two
        // closures that would pick `pass` and `fail` out of a row are different types, so
        // an array holding both is not a thing that compiles.
        let passes: HashSet<String> = check.rows.iter().map(|row| shape(&row.pass)).collect();
        let fails: HashSet<String> = check.rows.iter().map(|row| shape(&row.fail)).collect();

        for (name, claim, seen) in [
            ("Pass", &check.coverage.pass, &passes),
            ("Fail", &check.coverage.fail, &fails),
        ] {
            let wanted: &[&str] = match claim.as_str() {
                "eight" => &EIGHT,
                "colours" => &COLOURS,
                other => panic!(
                    "{}: {name} claims coverage '{other}', which is not one this reads",
                    check.what,
                ),
            };

            let missing: Vec<&str> = wanted
                .iter()
                .copied()
                .filter(|shape| !seen.contains(*shape))
                .collect();
            assert!(
                missing.is_empty(),
                "on {}, no fixture puts these on the {name} half: {}",
                check.what,
                missing.join(", "),
            );

            // A half that cannot reach past where it lands must never be given a marker,
            // and a row claiming one would be describing a line the mod will not draw.
            if claim == "colours" {
                let impossible: Vec<&String> =
                    seen.iter().filter(|shape| shape.contains('+')).collect();
                assert!(
                    impossible.is_empty(),
                    "on {}, the {name} half claims only bare colours but a row arranges: \
                     {impossible:?} - either the row is wrong or the check's shape changed",
                    check.what,
                );
            }
        }

        // An orange word cannot carry anything: nothing outranks the top rung, so there is
        // never something beyond it to report and a search that gave up has not made that
        // doubtful. A row claiming otherwise would be describing a line the mod will not
        // draw.
        for row in &check.rows {
            for (name, half) in [("Pass", &row.pass), ("Fail", &row.fail)] {
                assert!(
                    half.colour != "orange" || half.marker.is_none(),
                    "{}: {name} is orange and claims a marker, which cannot be drawn",
                    row.suite,
                );
            }
        }
    }
}

/// No two checks answer to the same suite name.
///
/// A suite name is how a row is run in game - `look-ahead --suite <name>` - and how a
/// failure here names the run that would show the same thing. Two rows sharing one would
/// make that ambiguous, and the harness builds one suite per row regardless.
#[test]
fn every_row_has_its_own_suite_name() {
    let table = table();

    let mut seen: HashSet<&str> = HashSet::new();
    for check in &table.checks {
        for row in &check.rows {
            assert!(
                seen.insert(&row.suite),
                "'{}' names more than one row, and a suite name has to pick one",
                row.suite,
            );
        }
    }
}
