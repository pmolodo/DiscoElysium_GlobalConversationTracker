// SPDX-License-Identifier: MIT
//! Every marker the in-game suites arrange, drawn again without a game.
//!
//! ## What this is, and what it is not
//!
//! NOT a second set of examples that happen to cover the same rules. The suites come from
//! `testing/scenarios/suites.json`, and each scenario names the SAME fixture the in-game
//! run stages - the same global state file, the same save's read entries and variables, the
//! same balance, the same conversation - so this runs the scenario rather than something
//! like it. `tools/GameHarness` builds its runs from the same file, so a scenario cannot
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
//! ## The two things it checks, and why they are different
//!
//! MARKERS, for the scenarios that name options. Those are claims about a menu, and this
//! checks the options a row names.
//!
//! CLAIMS, for the suites whose subject is a rule rather than a menu. Those are asked of
//! every entry in the conversation's group, which no in-game run can do - it only ever sees
//! what a menu composed - and which is the stronger form of what the suite is for.
//!
//! ## What the in-game run still earns, and this cannot
//!
//! That the Harmony patch is installed, that a real response menu was composed, and that
//! the marker reached the text the game drew. It also earns the NEGATIVE half of a `named`
//! policy - that no OTHER option in the menu is marked - because only the game can say what
//! else the menu offered. This checks the options a scenario names and says so.

use std::collections::HashSet;

use lookahead_engine::bridge::{LookAheadAnswer, LookAheadRequest, NodeRef, WorldSnapshot, answer};
use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::index::{build_group_graph, read_index};

mod common;

use common::fixtures;
use common::suites::{self, Scenario, Suite, TABLE};

/// The clock every scenario that does not name one runs at: midday, day one.
///
/// The same default `branch_shapes.rs` uses, and it has to be A default rather than
/// nothing: a guard comparing the time answers differently at midnight, so leaving it at
/// zero would be choosing an hour rather than declining to. A scenario whose answer depends
/// on the clock names its own.
const NOON: i32 = 720;

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

/// One scenario's world, staged from the same two files the in-game run loads.
struct Staged {
    /// The group's graph, and every conversation in it.
    graph: lookahead_engine::graph::graph::LookAheadGraph,
    /// What some other save has read, per the suite's global state fixture.
    recorded: HashSet<(i32, i32)>,
    /// What this save has read, per the save itself.
    read_here: HashSet<(i32, i32)>,
    /// The request, with no starts on it yet.
    request: LookAheadRequest,
}

impl Staged {
    /// Which rung an entry sits on, by the same order the plugin applies.
    ///
    /// READ HERE WINS. An entry recorded in the global state AND read in this save is on
    /// the bottom rung, not the middle one - the state records what some save has
    /// displayed, and this save is one of them.
    fn novelty_of(&self, node: NodeRef) -> i32 {
        let key = (node.conversation, node.entry);
        if self.read_here.contains(&key) {
            SEEN_THIS_GAME
        } else if self.recorded.contains(&key) {
            UNSEEN_THIS_GAME
        } else {
            UNSEEN_ANY_GAME
        }
    }

    /// The same, as the engine spells it.
    fn novelty(&self, id: DialogueNodeId) -> Novelty {
        match self.novelty_of(NodeRef::from(id)) {
            UNSEEN_ANY_GAME => Novelty::UnseenAnyGame,
            UNSEEN_THIS_GAME => Novelty::UnseenThisGame,
            _ => Novelty::SeenThisGame,
        }
    }

    /// The request for one scenario's options, asked as the menu they are.
    ///
    /// THIS ASKED ONE OPTION AT A TIME UNTIL 2026-09-11, down a second path a flag on the
    /// request used to select. The plugin never selected it, so every scenario here was
    /// green against code the game does not run, and two defects in the path it DOES run
    /// survived until an in-game run found them. The flag is gone and the path with it - see
    /// de-0jsf.21 - and this is now the only way to ask.
    fn asking(&self, starts: Vec<NodeRef>) -> LookAheadRequest {
        LookAheadRequest {
            starts,
            ..self.request.clone()
        }
    }
}

/// Stages one scenario.
///
/// The whole of what "the same fixture" means, in one place: the recorded entries from the
/// staged global state, the displayed entries and the dialogue variables from the save, and
/// the balance and clock the row names.
///
/// OVER THE WHOLE GROUP, not the one conversation the scenario opens. The engine loads
/// everything reachable from it, so every entry it might walk to has to be classified - see
/// `fixtures::recorded_elsewhere_in_group`, which is where the mistake this cost is written
/// down.
fn stage(
    index: &lookahead_engine::index::Index,
    suite: &Suite,
    scenario: &Scenario,
) -> Option<Staged> {
    let conversation = scenario.conversation;
    let (graph, group) = build_group_graph(index, conversation).ok()?;

    let recorded = fixtures::recorded_elsewhere_in_group(&suite.state, &group);
    let read_here = fixtures::read_in_save_group(&scenario.save, &group);

    // THE CHECKS, FROM THE SAVE'S OWN SHEET. The plugin decides each passive check against
    // the live character sheet and sends the outcomes; a world that answered nothing about
    // them would carry both branches of every one, which draws markers the game does not.
    // The caller has already established that the tables this reads are there, so nothing
    // here is a reason to answer "the group does not build", which is what a None from this
    // function means to it.
    let checks = fixtures::checks_in_save(&scenario.save, &group)
        .expect("the actor table and the full index are both present");

    // WHAT THE PLUGIN ANSWERS FROM THE RUNNING GAME, answered from the save instead: the
    // inventory, the journal, the thought cabinet, the balance and the clock. A world
    // missing them is not a stricter one, it is a different one - an empty item set says
    // "not held" rather than "unknown" - and a guard on either side of that opens or closes
    // a route the game does not.
    let holdings = fixtures::holdings_in_save(&scenario.save);
    let asked = lookahead_engine::bridge::questions_of(&graph, group.clone());

    // BUILT BEFORE THE FIELD THAT MOVES IT, since the rungs and the seen set are the same
    // reading of the same save.
    let seen = read_here
        .iter()
        .map(|&(conversation, entry)| NodeRef {
            conversation,
            entry,
        })
        .collect();

    let mut staged = Staged {
        graph,
        recorded,
        read_here,
        request: LookAheadRequest {
            conversation,
            state_budget: suite.state_budget,
            world: WorldSnapshot {
                // THE ROW WINS OVER THE SAVE where it names one, because a row that names a
                // balance is staging a balance - the money suite's three scenarios are one
                // save at three of them, and the save can only hold one.
                money: scenario.money.unwrap_or(holdings.money),
                day_minutes: scenario.day_minutes.unwrap_or(holdings.day_minutes),
                day_counter: holdings.day_counter,
                // LOCKED, as the plugin sends it: nothing the game exposes to Lua says whether
                // its clock is locked, so the mod reports locked and a crawl leaves the hour
                // where it found it. A fixture that let time pass would be staging a world no
                // run of the game is in. See de-3jec.
                clock_locked: true,
                queries: holdings.answers_to(&asked.queries),
                items: holdings.items,
                tasks: holdings.tasks,
                thoughts: holdings.thoughts,
                // FROM THE SAVE, and the difference between a run and no run. An ordinary
                // option is often guarded on a dialogue variable - 451:86 is guarded on
                // whether Siileng has the sneakers to sell - and a world that cannot answer
                // stops the search before it builds a state, so the option draws nothing
                // where the game draws a marker.
                variables: fixtures::variables_in_save(&scenario.save),
                // WHAT THIS SAVE HAS ALREADY SHOWN, which the engine seeds its seen slots from
                // and which no offline run has ever sent. The same set the novelty rungs are
                // built out of, put where a guard on having been shown can read it: without it
                // every once-only entry starts unfired, and a route that is spent in the save
                // is open to the crawl. Found by diffing against what the game sends - de-v702.
                seen,
                checks_pass: checks.pass,
                checks_fail: checks.fail,
                ..Default::default()
            },
            ..Default::default()
        },
    };

    // The three rungs, exactly as the plugin builds them.
    let everything: Vec<NodeRef> = staged
        .graph
        .nodes()
        .map(|node| NodeRef::from(node.id))
        .collect();
    staged.request.unseen_any_game = everything
        .iter()
        .copied()
        .filter(|node| staged.novelty_of(*node) == UNSEEN_ANY_GAME)
        .collect();
    staged.request.unseen_this_game = everything
        .iter()
        .copied()
        .filter(|node| staged.novelty_of(*node) == UNSEEN_THIS_GAME)
        .collect();

    Some(staged)
}

#[test]
fn every_marker_the_suites_arrange_is_reached_offline() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    // ASKED FOR HERE rather than where it is read. Staging a scenario decides its passive
    // checks against the save's character sheet, and which skill each one tests is a
    // question only the actor table answers. Without it every check would be undecided and
    // every scenario would be staged into a more permissive world than the run it stands
    // for - a green suite that tested something easier.
    if common::actors().is_none() {
        eprintln!("no actor table; skipping.");
        return;
    }
    let index = read_index(&path).expect("the shipped index reads");

    let table = suites::table();
    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for suite in &table.suites {
        // A SUITE THAT SAYS WHY IT IS OFF IS SKIPPED, LOUDLY. See `Suite::disabled`: the
        // sentence is printed rather than swallowed, so a run that is missing a claim says
        // which claim and why, and a green suite is never quietly a smaller one.
        if let Some(why) = &suite.disabled {
            println!("SKIPPING suite '{}': {why}", suite.suite);
            continue;
        }

        for scenario in &suite.scenarios {
            // NOTHING TO ASK OFFLINE. Both of the other policies are claims about the
            // options a scenario did NOT name - that nothing else in the menu is marked -
            // and only a composed menu knows what else there was. What those suites are
            // really for is checked by their offline claim instead.
            if scenario.markers != "named" {
                continue;
            }

            let conversation = scenario.conversation;
            let Some(staged) = stage(&index, suite, scenario) else {
                failures.push(format!(
                    "{}/{}: conversation {conversation}'s group does not build",
                    suite.suite, scenario.save,
                ));
                continue;
            };

            let request = staged.asking(
                scenario
                    .options
                    .iter()
                    .map(|option| NodeRef {
                        conversation,
                        entry: option.entry,
                    })
                    .collect(),
            );
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

                let own = staged.novelty_of(NodeRef {
                    conversation,
                    entry: option.entry,
                });
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
                        if reply.complete {
                            "finished"
                        } else {
                            "gave up"
                        },
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

/// The claims a suite makes about every entry in a group, rather than about a menu.
///
/// ## Why these are not marker rows
///
/// A row can only name options somebody knows are in the menu, and pristine and all-seen
/// are not about a menu at all - they are about a rule holding everywhere. The in-game run
/// states them as "nothing anywhere in this menu is marked", which is as much as it can
/// see; here the whole conversation is in hand, so the same claim can be put to every entry
/// of it. That is the stronger reading, and it is the one the suites were written for.
///
/// ## Why this asks the prefilter and not the bridge
///
/// Both claims are that NO SEARCH IS WORTH RUNNING, and that is one question, asked in one
/// place: `reaches_potential_improvement`, which the bridge puts to every option and to
/// each outcome of every check. Asking it directly is the claim.
///
/// Putting all 1,770 entries of Joyce's group through `answer` instead was tried and
/// abandoned - it did not finish in twenty minutes. THE REASON IS WORTH KEEPING: a rolled
/// check reports its two outcomes even when the option itself is refused, and an outcome
/// that lands on an entry THIS SAVE HAS READ has the bottom rung as its baseline, from
/// where an unseen-this-game entry does outrank it. So the branch searches run, in their
/// hundreds, over the largest conversations in the game. Those searches are correct and are
/// not what these suites are about.
#[test]
fn every_offline_claim_holds_over_the_whole_group() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    // ASKED FOR HERE rather than where it is read. Staging a scenario decides its passive
    // checks against the save's character sheet, and which skill each one tests is a
    // question only the actor table answers. Without it every check would be undecided and
    // every scenario would be staged into a more permissive world than the run it stands
    // for - a green suite that tested something easier.
    if common::actors().is_none() {
        eprintln!("no actor table; skipping.");
        return;
    }
    let index = read_index(&path).expect("the shipped index reads");

    let table = suites::table();
    let mut failures: Vec<String> = Vec::new();
    let mut asked = 0usize;

    for suite in &table.suites {
        if let Some(why) = &suite.disabled {
            println!("SKIPPING suite '{}': {why}", suite.suite);
            continue;
        }

        let Some(claim) = &suite.offline else {
            continue;
        };

        let known = ["nothingIsWorthCrawling", "unseenAnywhereIsNeverCrawled"];
        if !known.contains(&claim.claim.as_str()) {
            failures.push(format!(
                "{}: '{}' is not a claim; the ones there are {}",
                suite.suite,
                claim.claim,
                known.join(" and "),
            ));
            continue;
        }

        for scenario in &suite.scenarios {
            let conversation = scenario.conversation;
            let Some(staged) = stage(&index, suite, scenario) else {
                failures.push(format!(
                    "{}/{}: conversation {conversation}'s group does not build",
                    suite.suite, scenario.save,
                ));
                continue;
            };

            // Which entries the claim is about, and it is not the same set.
            //
            // all-seen records every entry, so nothing outranks anything ANYWHERE and the
            // claim covers the group. pristine records nothing, so it holds only of the
            // entries that are themselves unseen anywhere - these saves are real
            // playthroughs, and an entry read in the SAVE sits on the bottom rung, from
            // where something can legitimately outrank it.
            let about: Vec<NodeRef> = staged
                .graph
                .nodes()
                // A group is expanded in place and never scored, so it is not an option.
                .filter(|node| !node.is_group)
                .map(|node| NodeRef::from(node.id))
                .filter(|node| {
                    claim.claim == "nothingIsWorthCrawling"
                        || staged.novelty_of(*node) == UNSEEN_ANY_GAME
                })
                .collect();

            let examined = about.len();
            asked += examined;

            if examined == 0 {
                failures.push(format!(
                    "{}/{} ({}): the claim covers no entry of conversation {conversation}, \
                     so it says nothing",
                    suite.suite, scenario.save, claim.claim,
                ));
                continue;
            }

            let refused = match claim.claim.as_str() {
                // THROUGH THE BRIDGE, because the claim is about what the mod DRAWS as
                // well as about what it spends, and because deciding it from the option's
                // own novelty here would be restating the rule rather than testing it.
                // Affordable at this size: an empty global state puts nearly every entry
                // on the top rung, so the searches that would be expensive are the ones
                // being refused.
                "unseenAnywhereIsNeverCrawled" => {
                    let response = answer(&index, None, &staged.asking(about.clone()));
                    assert!(
                        response.error.is_none(),
                        "{}/{}: {:?}",
                        suite.suite,
                        scenario.save,
                        response.error,
                    );

                    response
                        .answers
                        .iter()
                        .filter(|reply| {
                            reply.states_explored > 0
                                || drawn(staged.novelty_of(reply.start), reply) != "none"
                        })
                        .map(|reply| {
                            format!(
                                "{}:{} drew {} over {} states",
                                reply.start.conversation,
                                reply.start.entry,
                                drawn(staged.novelty_of(reply.start), reply),
                                reply.states_explored,
                            )
                        })
                        .collect::<Vec<_>>()
                }

                // THROUGH THE PREFILTER, which is the one place a search is refused, for
                // an option and for each outcome of a check alike. The bridge was tried
                // here and did not finish in twenty minutes, and the reason is worth
                // keeping: a rolled check reports its two outcomes even when the option is
                // refused, and an outcome landing on an entry THIS SAVE HAS READ has the
                // bottom rung as its baseline, from where an unseen-this-game entry does
                // outrank it - so the branch searches run, in their hundreds, over the
                // largest conversations in the game. Those searches are correct and are not
                // what this suite is about.
                _ => about
                    .iter()
                    .filter(|node| {
                        let id = DialogueNodeId::from(**node);
                        let own = staged.novelty(id);
                        own < Novelty::UnseenAnyGame
                            && staged
                                .graph
                                .best_linked_class(id, |id| staged.novelty(id))
                                .is_some_and(|best| best > own)
                    })
                    .map(|node| {
                        format!(
                            "{}:{} is still worth a search",
                            node.conversation, node.entry
                        )
                    })
                    .collect::<Vec<_>>(),
            };

            if !refused.is_empty() {
                failures.push(format!(
                    "{}/{} ({}): {} of {examined} entries should have been refused and \
                     were not: {}",
                    suite.suite,
                    scenario.save,
                    claim.claim,
                    refused.len(),
                    refused
                        .iter()
                        .take(10)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", "),
                ));
            }
        }
    }

    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    assert!(
        asked > 0,
        "{TABLE} makes no offline claim, so this checked nothing"
    );
    eprintln!("{asked} entries refused a search, as claimed");
}

/// The definition names a marker the mod can draw, and says something in every suite.
///
/// Worth asking separately, and cheaply: the tests above only check the rows that are
/// there, so a row naming a marker nobody draws - or a suite that claims its markers are
/// named and names none - would be a fixture describing a menu the game will not compose,
/// and it would fail with a message about the engine rather than about the row.
#[test]
fn the_definition_names_markers_that_can_be_drawn() {
    let table = suites::table();
    let allowed = ["none", "orange", "red", "gaveUp"];
    let mut wrong: Vec<String> = Vec::new();

    for suite in &table.suites {
        assert!(
            !suite.scenarios.is_empty(),
            "{} names no scenario, so it would run and claim nothing",
            suite.suite,
        );

        // A SUITE HAS TO CLAIM SOMETHING SOMEWHERE. One whose every scenario waives its
        // markers and which names no offline claim would run, cost a launch, and assert
        // nothing an offline reader can see - which is how a suite quietly stops testing.
        let names_options = suite
            .scenarios
            .iter()
            .any(|scenario| scenario.markers == "named");
        if !names_options && suite.offline.is_none() {
            wrong.push(format!(
                "{}: no scenario names an option and there is no offline claim, so nothing \
                 here is checked without a game",
                suite.suite,
            ));
        }

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
