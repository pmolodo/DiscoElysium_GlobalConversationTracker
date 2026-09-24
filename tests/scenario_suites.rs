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
//! - A best AT OR BELOW the option's own seen state draws nothing if the search finished, and
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

use lookahead_engine::bridge::{DataKind, LookAheadAnswer, LookAheadRequest, NodeRef, answer};
use lookahead_engine::core::types::{DialogueNodeId, SeenState};
use lookahead_engine::index::read_index;

use gct_measure::common;

use common::fixtures;
use common::staging::{
    EVERY_MENU_FINISHES_IN_BUDGET, UNSEEN_ANY_GAME, drawn, play_stops, silent, stage,
};
use common::suites::{self, TABLE};
use gct_measure::plugin_defaults::Budgets;

/// Whether a hardcore game was finished is answered offline though no save records it: false
/// unless the scenario row fixes it.
#[test]
fn a_hardcore_completion_no_save_records_is_answered_from_the_row() {
    let asked = [lookahead_engine::bridge::DataRequest::set(
        DataKind::HardcorePlaythroughCompleted,
    )];
    let answered = |row: Option<bool>| {
        let answers = fixtures::holdings_in_save(COMPLETION_TEST_SAVE)
            .with_hardcore_playthrough_completed(row)
            .data_for(&asked);
        assert!(
            answers[0].read,
            "the question is answered, not left Unknown"
        );
        match answers[0].value {
            lookahead_engine::bridge::WireValue::Bool { value } => value,
            ref other => panic!("answered as {other:?} rather than a boolean"),
        }
    };

    assert!(!answered(None));
    assert!(answered(Some(true)));
}

/// The save the hardcore-completion test stages its world from. Which one does not matter:
/// whether a hardcore game was ever finished is profile state no save records, so that one
/// answer comes from the row. Whether hardcore mode is ACTIVE is still read from each save's
/// `gameModeState.gameMode`, and staging a different mode means a new save diff, not a row.
const COMPLETION_TEST_SAVE: &str = "at-trashcan";

/// Every question a suite's groups ask is answered by the world the offline run stages.
///
/// WHY THIS EXISTS: an unanswered question reads Unknown, and Unknown is permissive, so a
/// suite over a group that asks something the fixtures cannot answer passes by NOT SEEING
/// rather than by being right - the all-seen suite once guarded on five such functions
/// (de-eb2d). Crossing the questions against the staged answers catches that per group.
#[test]
fn every_question_the_suites_ask_is_answered_offline() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    if common::actors().is_none() {
        eprintln!("no actor table; skipping.");
        return;
    }
    let index = read_index(&path).expect("the shipped index reads");

    let mut blind: Vec<String> = Vec::new();
    for suite in &suites::table().suites {
        if suite.disabled.is_some() {
            continue;
        }

        for scenario in &suite.scenarios {
            let Some(staged) = stage(&index, suite, scenario) else {
                continue;
            };
            let world = &staged.request.world;
            let place = format!(
                "{}/{} (group of {})",
                suite.suite, scenario.save, scenario.conversation
            );

            for key in &staged.questions.queries {
                if !world.queries.contains_key(key) {
                    blind.push(format!("{place}: query {key}"));
                }
            }

            for (request, answer) in staged.questions.data.iter().zip(&world.data_values) {
                if !answer.read {
                    blind.push(format!("{place}: data {request:?}"));
                }
            }
        }
    }

    assert!(
        blind.is_empty(),
        "offline runs cannot see these, so a suite over them passes by not seeing:\n{}",
        blind.join("\n"),
    );
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
    // THE FULL INDEX AS WELL, for one question the shipped one cannot answer: it carries no
    // dialogue text, so whether a walked line has any can only be read here.
    let texts =
        read_index(&common::conversation_index().expect("the shipped index is built from it"))
            .expect("the full index reads");

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

            // ONE WALK PER STOP, each from the conversation's START by every input pressed up
            // to it - see `Scenario::stops`. In game the conversation carries on in place from
            // one stop to the next; walking the whole thing again reaches the same menu, and
            // costs milliseconds.
            for stop in scenario.stops() {
                // WALKED BY THE INPUTS THE IN-GAME RUN PRESSES, so the menu asked about is the
                // one the game draws and the request carries what the game stepped through on
                // the way, which is what the hub cut reads.
                let walk = match staged.walk(conversation, stop.inputs.as_deref()) {
                    Ok(walk) => walk,
                    Err(misfit) => {
                        failures.push(format!(
                            "{}/{} ({}): the walk to its menu fails: {misfit}",
                            suite.suite, scenario.save, stop.what,
                        ));
                        continue;
                    }
                };
                eprintln!(
                    "{}/{} ({}): walked {:?} to the menu {:?}",
                    suite.suite, scenario.save, stop.what, walk.encountered, walk.menu,
                );

                let silent = silent(&texts, &walk);
                if !silent.is_empty() {
                    failures.push(format!(
                        "{}/{}: the walk displays {silent:?}, which have no text, and whether \
                         the game waits on such a line is unmeasured",
                        suite.suite, scenario.save,
                    ));
                    continue;
                }

                // BY ENTRY ALONE, as the in-game run matches them: a walk can cross into another
                // conversation of the group, so the menu's own ids say where each option lives.
                let offered = |entry: i32| -> Vec<NodeRef> {
                    walk.menu
                        .iter()
                        .filter(|id| id.entry_id == entry)
                        .map(|id| NodeRef::from(*id))
                        .collect()
                };
                let ambiguous: Vec<i32> = stop
                    .options
                    .iter()
                    .map(|option| option.entry)
                    .filter(|entry| offered(*entry).len() > 1)
                    .collect();
                assert!(
                    ambiguous.is_empty(),
                    "{}/{} ({}): the menu {:?} offers {ambiguous:?} from more than one \
                     conversation, so a row naming entries alone cannot say which",
                    suite.suite,
                    scenario.save,
                    stop.what,
                    walk.menu,
                );
                let unoffered: Vec<i32> = stop
                    .options
                    .iter()
                    .map(|option| option.entry)
                    .filter(|entry| offered(*entry).is_empty())
                    .collect();
                if !unoffered.is_empty() {
                    failures.push(format!(
                        "{}/{} ({}): the row names {unoffered:?}, which the menu its inputs \
                         reach does not offer: {:?}",
                        suite.suite, scenario.save, stop.what, walk.menu,
                    ));
                    continue;
                }

                let request = staged.asking(&walk);
                let response = answer(&index, common::declared(), None, &request);
                assert!(
                    response.error.is_none(),
                    "{}/{}: {:?}",
                    suite.suite,
                    scenario.save,
                    response.error,
                );

                let by_start: std::collections::HashMap<NodeRef, &LookAheadAnswer> = response
                    .answers
                    .iter()
                    .map(|reply| (reply.start, reply))
                    .collect();

                for option in stop.options {
                    let start = offered(option.entry)[0];
                    let Some(reply) = by_start.get(&start) else {
                        failures.push(format!(
                            "{}/{}: nothing came back for {}:{}",
                            suite.suite, scenario.save, start.conversation, start.entry,
                        ));
                        continue;
                    };

                    let own = staged.seen_state_of(start);
                    let got = drawn(own, reply);
                    checked += 1;

                    if got != option.marker {
                        // THE WITNESS BY NAME, because it is the engine's own reason for
                        // `best` and the row's why is a sentence about that line. A
                        // disagreement is then readable without a second run: either the
                        // line it found is one the row overlooked, or the row is right and
                        // the route to that line is one the search should not have had.
                        let witness = match reply.witness {
                            Some(node) => format!("{}:{}", node.conversation, node.entry),
                            None => "nothing".to_string(),
                        };
                        failures.push(format!(
                            "{}/{} ({}): {}:{} should be {} - {} - and the engine \
                             draws {got}: own {own}, best {}, witness {witness}, {}, \
                             {} pass(es) - run it in game with --suite {}",
                            suite.suite,
                            scenario.save,
                            stop.what,
                            start.conversation,
                            start.entry,
                            option.marker,
                            option.why,
                            reply.best,
                            if reply.complete {
                                "finished"
                            } else {
                                "gave up"
                            },
                            reply.nodes_reached,
                            suite.suite,
                        ));
                    }
                }
            }
        }
    }

    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    assert!(checked > 0, "{TABLE} named no option for any scenario");
    eprintln!("{checked} markers reached offline");
}

/// Evrart's folder never has to carry the copotype amounts: it raises each one only behind the
/// guard that it is winning, so from either of the `reputation-branch` menus the whole split is
/// answered from the world.
#[test]
fn evarts_copotype_split_is_answered_from_the_world() {
    const FOLDER: i32 = 785;
    /// Every guard on the split under "What kind of a cop does it say I am?".
    const SPLIT: [i32; 8] = [31, 32, 124, 125, 149, 150, 3, 4];

    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    if common::actors().is_none() {
        eprintln!("no actor table; skipping.");
        return;
    }
    let index = read_index(&path).expect("the shipped index reads");
    let table = suites::table();
    let suite = table.suite("reputation-branch");
    let scenario = &suite.scenarios[0];
    let staged = stage(&index, suite, scenario).expect("Evrart's group builds");
    let split = SPLIT.map(|entry| DialogueNodeId::new(FOLDER, entry));

    for stop in scenario.stops() {
        let walk = staged
            .walk(scenario.conversation, stop.inputs.as_deref())
            .unwrap_or_else(|misfit| panic!("{}: {misfit}", stop.what));
        let compiled = common::compiled_guards(&index, &staged.asking(&walk), &split);
        assert_eq!(
            compiled.reputation_from_world,
            SPLIT.len(),
            "{}: every question on the split is answered from the world",
            stop.what
        );
        assert_eq!(compiled.fallbacks, 0, "{}", stop.what);
    }
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
        // ASKED OF MENUS RATHER THAN OF THE GROUP, so it has a test of its own:
        // `every_budget_claim_holds_at_its_menus`.
        if claim.claim == EVERY_MENU_FINISHES_IN_BUDGET {
            continue;
        }

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
                        || staged.seen_state_of(*node) == UNSEEN_ANY_GAME
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
                // own seen state here would be restating the rule rather than testing it.
                // Affordable at this size: an empty global state puts nearly every entry
                // on the top rung, so the searches that would be expensive are the ones
                // being refused.
                "unseenAnywhereIsNeverCrawled" => {
                    let request = LookAheadRequest {
                        starts: about.clone(),
                        ..staged.request.clone()
                    };
                    let response = answer(&index, common::declared(), None, &request);
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
                        // OR WALKED SOMETHING, so an option that was searched and found
                        // nothing is reported too - that is the interesting half of a menu.
                        // The passes are what says so: a refused search takes none, and the
                        // other cost figure is a menu's, carried on its first answer, so it
                        // cannot say which OPTION was walked.
                        .filter(|reply| {
                            reply.nodes_reached > 0
                                || drawn(staged.seen_state_of(reply.start), reply) != "none"
                        })
                        .map(|reply| {
                            format!(
                                "{}:{} drew {} over {} pass(es)",
                                reply.start.conversation,
                                reply.start.entry,
                                drawn(staged.seen_state_of(reply.start), reply),
                                reply.nodes_reached,
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
                        let own = staged.seen_state(id);
                        own < SeenState::UnseenAnyGame
                            && staged
                                .graph
                                .best_linked_class(id, |id| staged.seen_state(id))
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

/// Every menu a budget-claiming suite reaches is answered inside the budgets the plugin ships.
///
/// ASKED AS THE GAME ASKS IT, through `staging::play_stops`: each scenario's stops in order,
/// through one engine service, under `Budgets::SHIPPED`. The scenario runner,
/// `crates/gct-measure/examples/scenario_menus.rs`, prints what this asserts, so a failure here
/// can be read option by option with the same numbers.
///
/// A WALL-CLOCK CLAIM, which the marker tests deliberately are not, so a suite should only make
/// it of a menu that settles far inside its wall. One that finishes near its budget will pass or
/// fail with the load on the machine, and says more as a row of the runner than as a test.
#[test]
fn every_budget_claim_holds_at_its_menus() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    // FOR THE SAME REASON AS THE MARKER TEST: without the actor table every passive check is
    // undecided, and the world staged is a more permissive one than the run it stands for.
    if common::actors().is_none() {
        eprintln!("no actor table; skipping.");
        return;
    }
    let index = read_index(&path).expect("the shipped index reads");
    let texts =
        read_index(&common::conversation_index().expect("the shipped index is built from it"))
            .expect("the full index reads");

    let table = suites::table();
    let mut failures: Vec<String> = Vec::new();
    let mut menus = 0usize;

    for suite in &table.suites {
        if let Some(why) = &suite.disabled {
            println!("SKIPPING suite '{}': {why}", suite.suite);
            continue;
        }
        if suite
            .offline
            .as_ref()
            .is_none_or(|claim| claim.claim != EVERY_MENU_FINISHES_IN_BUDGET)
        {
            continue;
        }

        for scenario in &suite.scenarios {
            let played = play_stops(&index, &texts, &path, suite, scenario, Budgets::SHIPPED);
            let played = match played {
                Ok((_, played)) => played,
                Err(fault) => {
                    failures.push(fault);
                    continue;
                }
            };

            for stop in played {
                menus += 1;
                if !stop.silent.is_empty() {
                    failures.push(format!(
                        "{}/{} ({}): the walk displays {:?}, which have no text, and whether \
                         the game waits on such a line is unmeasured",
                        suite.suite, scenario.save, stop.what, stop.silent,
                    ));
                    continue;
                }
                for reply in stop.response.answers.iter().filter(|reply| !reply.complete) {
                    failures.push(format!(
                        "{}/{} ({}): {}:{} did not finish - stopped by {}, {} ms, {} diagram \
                         nodes, {} reached; the whole menu took {} ms",
                        suite.suite,
                        scenario.save,
                        stop.what,
                        reply.start.conversation,
                        reply.start.entry,
                        reply.stopped_by,
                        reply.elapsed_ms,
                        reply.diagram_nodes,
                        reply.nodes_reached,
                        stop.took_ms,
                    ));
                }
            }
        }
    }

    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    assert!(
        menus > 0,
        "{TABLE} makes no budget claim, so this checked nothing"
    );
    eprintln!("{menus} menus finished inside the shipped budgets, as claimed");
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
            if scenario.stops.is_empty() {
                wrong.push(format!(
                    "{}/{}: names no stop, so it walks to no menu and claims nothing",
                    suite.suite, scenario.save,
                ));
            }

            for stop in &scenario.stops {
                if scenario.markers == "named" && stop.options.is_empty() {
                    wrong.push(format!(
                        "{}/{} ({}): claims its markers are named and names none",
                        suite.suite, scenario.save, stop.what,
                    ));
                }

                for option in &stop.options {
                    if !allowed.contains(&option.marker.as_str()) {
                        wrong.push(format!(
                            "{}/{}: '{}' is not a marker an option can carry",
                            suite.suite, scenario.save, option.marker,
                        ));
                    }
                }
            }
        }
    }

    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}
