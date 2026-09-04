// SPDX-License-Identifier: MIT
//! Every marker the in-game suites arrange, drawn again without a game.
//!
//! ## What this is, and what it is not
//!
//! NOT a second set of examples that happen to cover the same rules. The suites come from
//! `testing/scenarios/suites.json`, and each scenario names the SAME fixture the in-game
//! run stages - the same global state file, the same save's read entries, the same
//! balance, the same conversation - so this runs the scenario rather than something like
//! it. `tools/GameHarness` builds its runs from the same file, so a scenario cannot
//! describe a run that is not happening.
//!
//! `branch_shapes.rs` is the same argument for a check's two outcomes, over the table that
//! holds those. Between them the two cover every scenario the harness knows.
//!
//! ## The rule this applies
//!
//! The mod's own, restated against the answer rather than against the markup - see
//! `MarkerFor` in `ResponseLookAheadPatch.cs`, which this mirrors line for line:
//!
//! - An option that is itself UNSEEN ANYWHERE is never marked. Nothing outranks the top
//!   rung, so there is nothing a marker could report.
//! - A best AT OR BELOW the option's own novelty draws nothing if the search finished, and
//!   the uncertain marker if it did not. Not finding something is provisional; the search
//!   that ran out has not established that nothing is there.
//! - A best ABOVE it draws the colour of what was found. That is definite even under a
//!   budget, because a witness is a witness.
//!
//! ## What the in-game run still earns, and this cannot
//!
//! That the Harmony patch is installed, that a real response menu was composed, and that
//! the marker reached the text the game drew. It also earns the NEGATIVE half of a `named`
//! policy - that no OTHER option in the menu is marked - because only the game can say
//! what else the menu offered. This checks the options a scenario names and says so.

use std::collections::HashSet;

use lookahead_engine::bridge::{answer, LookAheadAnswer, LookAheadRequest, NodeRef, WorldSnapshot};
use lookahead_engine::index::{build_group_graph, read_index};
use serde::Deserialize;

mod common;

use common::fixtures;

/// The definition both sides read.
const TABLE: &str = "testing/scenarios/suites.json";

/// The clock every scenario that does not name one runs at: midday, day one.
///
/// The same default `branch_shapes.rs` uses, and it has to be A default rather than
/// nothing: a guard comparing the time answers differently at midnight, so leaving it at
/// zero would be choosing an hour rather than declining to. A scenario whose answer
/// depends on the clock names its own.
const NOON: i32 = 720;

#[derive(Debug, Deserialize)]
struct Table {
    suites: Vec<Suite>,
}

#[derive(Debug, Deserialize)]
struct Suite {
    suite: String,
    state: String,
    /// The state budget to run at, or 0 for no such limit. Suite-wide, as it is in game.
    #[serde(default, rename = "stateBudget")]
    state_budget: usize,
    scenarios: Vec<Scenario>,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    save: String,
    conversation: i32,
    what: String,
    #[serde(default)]
    money: Option<i32>,
    #[serde(default, rename = "dayMinutes")]
    day_minutes: Option<i32>,
    /// How much the scenario claims about the markers: named, noneAnywhere or ignored.
    #[serde(default = "named")]
    markers: String,
    #[serde(default)]
    options: Vec<Option_>,
}

fn named() -> String {
    "named".to_string()
}

/// What one option must carry. `Option_` because `Option` is taken and this is a row.
#[derive(Debug, Deserialize)]
struct Option_ {
    entry: i32,
    marker: String,
    why: String,
}

/// The three rungs as the engine numbers them.
const SEEN_THIS_GAME: i32 = 0;
const UNSEEN_THIS_GAME: i32 = 1;
const UNSEEN_ANY_GAME: i32 = 2;

/// What the mod would draw on an option, given its own novelty and what the search found.
///
/// Spelled as the definition spells it, so a failure reads in the same words the row does.
fn drawn(own: i32, answer: &LookAheadAnswer) -> &'static str {
    if own == UNSEEN_ANY_GAME {
        return "none";
    }

    if answer.best <= own {
        return if answer.complete { "none" } else { "gaveUp" };
    }

    if answer.best == UNSEEN_ANY_GAME {
        "orange"
    } else {
        "red"
    }
}

fn table() -> Table {
    let path = common::repo_root().join(TABLE);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} does not read: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not a scenario table: {error}", path.display()))
}

#[test]
fn every_marker_the_suites_arrange_is_reached_offline() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let table = table();
    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for suite in &table.suites {
        for scenario in &suite.scenarios {
            // NOTHING TO ASK OFFLINE. Both of the other policies are claims about the
            // options a scenario did NOT name - that nothing else in the menu is marked -
            // and only a composed menu knows what else there was.
            if scenario.markers != "named" {
                continue;
            }

            let conversation = scenario.conversation;
            let recorded = fixtures::recorded_elsewhere(&suite.state, conversation);
            let read_here = fixtures::read_in_save(&scenario.save, conversation);

            let Ok((graph, _)) = build_group_graph(&index, conversation) else {
                failures.push(format!(
                    "{}/{}: conversation {conversation}'s group does not build",
                    suite.suite, scenario.save,
                ));
                continue;
            };
            let everything: Vec<NodeRef> =
                graph.nodes().map(|node| NodeRef::from(node.id)).collect();

            // The three rungs, exactly as the plugin builds them: read in THIS save wins,
            // then recorded in some other save, then never seen anywhere.
            let request = LookAheadRequest {
                conversation,
                starts: scenario
                    .options
                    .iter()
                    .map(|option| NodeRef { conversation, entry: option.entry })
                    .collect(),
                unseen_any_game: everything
                    .iter()
                    .copied()
                    .filter(|n| !recorded.contains(&n.entry) && !read_here.contains(&n.entry))
                    .collect(),
                unseen_this_game: recorded
                    .iter()
                    .filter(|entry| !read_here.contains(entry))
                    .map(|entry| NodeRef { conversation, entry: *entry })
                    .collect(),
                state_budget: suite.state_budget,
                world: WorldSnapshot {
                    money: scenario.money.unwrap_or_default(),
                    day_minutes: scenario.day_minutes.unwrap_or(NOON),
                    day_counter: 1,
                    // FROM THE SAVE, and the difference between a run and no run. An
                    // ordinary option is often guarded on a dialogue variable - 451:86 is
                    // guarded on whether Siileng has the sneakers to sell - and a world
                    // that cannot answer stops the crawl before it builds a state, so the
                    // option draws nothing where the game draws a marker.
                    variables: fixtures::variables_in_save(&scenario.save),
                    ..Default::default()
                },
                ..Default::default()
            };

            let response = answer(&index, None, &request);
            assert!(
                response.error.is_none(),
                "{}/{}: {:?}",
                suite.suite,
                scenario.save,
                response.error,
            );

            let by_start: std::collections::HashMap<i32, &LookAheadAnswer> = response
                .answers
                .iter()
                .map(|reply| (reply.start.entry, reply))
                .collect();

            for option in &scenario.options {
                let Some(reply) = by_start.get(&option.entry) else {
                    failures.push(format!(
                        "{}/{}: nothing came back for {conversation}:{}",
                        suite.suite, scenario.save, option.entry,
                    ));
                    continue;
                };

                let own = novelty_of(option.entry, &recorded, &read_here);
                let got = drawn(own, reply);
                checked += 1;

                if got != option.marker {
                    failures.push(format!(
                        "{}/{} ({}): {conversation}:{} should be {} - {} - and the engine \
                         draws {got}: own {own}, best {}, {}, {} states over {} entries \
                         - run it in game with --suite {}",
                        suite.suite,
                        scenario.save,
                        scenario.what,
                        option.entry,
                        option.marker,
                        option.why,
                        reply.best,
                        if reply.complete { "finished" } else { "gave up" },
                        reply.states_explored,
                        reply.nodes_reached,
                        suite.suite,
                    ));
                }
            }
        }
    }

    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    assert!(checked > 0, "{TABLE} named no option for any scenario");
    eprintln!("{checked} markers reached offline");
}

/// Which rung an entry sits on, by the same order the plugin applies.
///
/// READ HERE WINS. An entry recorded in the global state AND read in this save is on the
/// bottom rung, not the middle one - the state records what some save has displayed, and
/// this save is one of them.
fn novelty_of(entry: i32, recorded: &HashSet<i32>, read_here: &HashSet<i32>) -> i32 {
    if read_here.contains(&entry) {
        SEEN_THIS_GAME
    } else if recorded.contains(&entry) {
        UNSEEN_THIS_GAME
    } else {
        UNSEEN_ANY_GAME
    }
}

/// The definition names a marker the mod can draw, and an option the conversation has.
///
/// Worth asking separately, and cheaply: the test above only checks the rows that are
/// there, so a row that named a marker nobody draws - or an entry that is not in the
/// conversation at all - would be a fixture describing a menu the game will not compose,
/// and it would fail with a message about the engine rather than about the row.
#[test]
fn the_definition_names_markers_that_can_be_drawn() {
    let table = table();
    let allowed = ["none", "orange", "red", "gaveUp"];
    let mut wrong: Vec<String> = Vec::new();

    for suite in &table.suites {
        assert!(
            !suite.scenarios.is_empty(),
            "{} names no scenario, so it would run and claim nothing",
            suite.suite,
        );

        for scenario in &suite.scenarios {
            if scenario.markers == "named" && scenario.options.is_empty() {
                wrong.push(format!(
                    "{}/{}: claims its markers are named and names none",
                    suite.suite, scenario.save,
                ));
            }

            for option in &scenario.options {
                if !allowed.contains(&option.marker.as_str()) {
                    wrong.push(format!(
                        "{}/{}: '{}' is not a marker an option can carry",
                        suite.suite, scenario.save, option.marker,
                    ));
                }
            }
        }
    }

    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}
