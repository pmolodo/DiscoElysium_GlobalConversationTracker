// SPDX-License-Identifier: MIT
//! One suite scenario's world, staged offline from the same files the in-game run loads.
//!
//! Its own module because more than one caller stages a scenario: `tests/scenario_suites.rs`
//! checks the markers a stop names and the budget claim, and `examples/scenario_menus.rs` plays
//! a scenario's stops and reports what each option cost. Two copies of "the same fixture" would
//! be two worlds that only look alike.

use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use lookahead_engine::bridge::{
    GameWorld, LookAheadAnswer, LookAheadRequest, LookAheadResponse, NodeRef, Questions,
    WorldRawData,
};
use lookahead_engine::core::types::{DialogueNodeId, SeenState};
use lookahead_engine::index::{Index, build_group_graph};
use lookahead_engine::service::Service;
use lookahead_engine::walkthrough::{Walkthrough, walk_inputs};

use super::fixtures;
use super::suites::{BranchHalfRow, Scenario, Suite};
use crate::plugin_defaults::Budgets;

/// The offline claim that every menu a suite's scenarios reach is answered inside the budgets
/// the plugin ships - see [`play_stops`], which both the claim and the runner ask through.
pub const EVERY_MENU_FINISHES_IN_BUDGET: &str = "everyMenuFinishesInBudget";

/// One stop of a scenario, as the engine answered it.
pub struct PlayedStop<'a> {
    /// What the stop is for, as its row says.
    pub what: &'a str,
    /// The walk that reached its menu.
    pub walk: Walkthrough,
    /// Lines the walk displayed that have no text - see [`silent`].
    pub silent: Vec<DialogueNodeId>,
    /// What the engine was asked, as the plugin would have sent it.
    pub request: LookAheadRequest,
    /// What the engine answered.
    pub response: LookAheadResponse,
    /// How long the whole menu took, as the caller waited for it.
    pub took_ms: u128,
}

/// Every stop of `scenario`, answered in order under `budgets`, as the game would ask them.
///
/// ONE SERVICE FOR THE SCENARIO, asked stop by stop, which is what the engine host does as the
/// player walks from menu to menu: a later menu in the same group is answered by the workspace
/// the earlier one built, and pays no warm-up. A service of its own per scenario, so no scenario
/// is answered warmer than the game would answer it.
///
/// `shipped` is the index the service opens, and `texts` the full one, which alone carries the
/// dialogue text [`silent`] reads.
///
/// # Errors
///
/// The group does not build, the engine will not open, a walk does not reach its menu, or the
/// engine refuses a request. Each is a scenario that cannot be asked, rather than an answer.
pub fn play_stops<'a>(
    index: &Index,
    texts: &Index,
    shipped: &Path,
    suite: &Suite,
    scenario: &'a Scenario,
    budgets: Budgets,
) -> Result<(Staged, Vec<PlayedStop<'a>>), String> {
    let conversation = scenario.conversation;
    let staged = stage(index, suite, scenario).ok_or_else(|| {
        format!(
            "{}/{}: conversation {conversation}'s group does not build",
            suite.suite, scenario.save,
        )
    })?;
    let service = Service::open(shipped, &super::declared_path())
        .map_err(|status| format!("the engine will not open: {status:?}"))?;

    let mut played = Vec::new();
    for stop in scenario.stops() {
        let walk = staged
            .walk(conversation, stop.inputs.as_deref())
            .map_err(|misfit| {
                format!(
                    "{}/{} ({}): the walk to its menu fails: {misfit}",
                    suite.suite, scenario.save, stop.what,
                )
            })?;
        let silent = silent(texts, &walk);

        let request = budgets.apply(staged.asking(&walk));
        let began = Instant::now();
        let response = service.answer_request(request.clone());
        let took_ms = began.elapsed().as_millis();
        if let Some(error) = &response.error {
            return Err(format!(
                "{}/{} ({}): {error}",
                suite.suite, scenario.save, stop.what,
            ));
        }

        played.push(PlayedStop {
            what: stop.what,
            walk,
            silent,
            request,
            response,
            took_ms,
        });
    }
    Ok((staged, played))
}

/// The three rungs as the engine numbers them.
pub const SEEN_THIS_GAME: i32 = 0;
pub const UNSEEN_THIS_GAME: i32 = 1;
pub const UNSEEN_ANY_GAME: i32 = 2;

/// What the mod would draw on an option, given its own seen state and what the search found.
///
/// Spelled as the definition spells it, so a failure reads in the same words the row does.
pub fn drawn(own: i32, answer: &LookAheadAnswer) -> &'static str {
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

/// What the mod would draw for one outcome of a rolled check, as a suite's half spells it.
///
/// The rule `BranchLine.Half` follows in the plugin: the word is coloured by where the outcome
/// LANDS, and a marker follows where something beyond it outranks that. An outcome that lands
/// on the top rung has nothing to outrank it, so it carries no marker even from a search that
/// gave up.
pub fn drawn_half(answer: &LookAheadAnswer) -> BranchHalfRow {
    let colour = match answer.destination {
        UNSEEN_ANY_GAME => "orange",
        UNSEEN_THIS_GAME => "red",
        _ => "darkRed",
    };
    let marker = if answer.best > answer.destination {
        Some(if answer.best == UNSEEN_ANY_GAME {
            "orange"
        } else {
            "red"
        })
    } else if answer.destination >= UNSEEN_ANY_GAME || answer.complete {
        None
    } else {
        Some("gaveUp")
    };
    BranchHalfRow {
        colour: colour.to_string(),
        marker: marker.map(str::to_string),
    }
}

/// One scenario's world, staged from the same two files the in-game run loads.
pub struct Staged {
    /// The group's graph, and every conversation in it.
    pub graph: lookahead_engine::graph::LookAheadGraph,
    /// What some other save has read, per the suite's global state fixture.
    pub recorded: HashSet<(i32, i32)>,
    /// What this save has read, per the save itself.
    pub read_here: HashSet<(i32, i32)>,
    /// The request, with no starts on it yet.
    pub request: LookAheadRequest,
    /// What the group asks the world, which the walk needs its answers put back onto.
    pub questions: Questions,
}

impl Staged {
    /// Which rung an entry sits on, by the same order the plugin applies.
    ///
    /// READ HERE WINS. An entry recorded in the global state AND read in this save is on
    /// the bottom rung, not the middle one - the state records what some save has
    /// displayed, and this save is one of them.
    pub fn seen_state_of(&self, node: NodeRef) -> i32 {
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
    pub fn seen_state(&self, id: DialogueNodeId) -> SeenState {
        match self.seen_state_of(NodeRef::from(id)) {
            UNSEEN_ANY_GAME => SeenState::UnseenAnyGame,
            UNSEEN_THIS_GAME => SeenState::UnseenThisGame,
            _ => SeenState::SeenThisGame,
        }
    }

    /// The walk `inputs` make from a conversation's start, in this world.
    ///
    /// THE WORLD `answer` BUILDS, answers put back onto their names, so the walk and the
    /// search it feeds decide every guard the same way.
    ///
    /// FROM THE START EVERY TIME, including for a scenario's later stops: a stop's inputs
    /// carry every earlier stop's in front of them, so walking from the start reaches the same
    /// menu the game reaches by carrying on in place.
    pub fn walk(
        &self,
        conversation: i32,
        inputs: Option<&[lookahead_engine::walkthrough::Input]>,
    ) -> Result<Walkthrough, String> {
        let mut world = GameWorld::declaring(self.request.world.clone(), super::declared());
        world.resolve(&self.questions)?;
        // WITH THE DECLARED TABLE, as the game has it: a variable the save does not hold
        // answers with the initial the database declares, or false for a name nothing
        // declares. A walk taken against a table declaring nothing gets the second of those
        // for every variable, and goes where the game will not.
        walk_inputs(&self.graph, &world, conversation, inputs)
    }

    /// The request for the menu a walk ended at, with what the walk showed on the way.
    ///
    /// THE WHOLE MENU, as the plugin asks it, rather than the options a row names: which
    /// siblings a menu has is part of what the engine is told, and a request of the named
    /// options alone asks about a menu the game never draws.
    pub fn asking(&self, walk: &Walkthrough) -> LookAheadRequest {
        LookAheadRequest {
            starts: walk.menu.iter().copied().map(NodeRef::from).collect(),
            encountered: walk
                .encountered
                .iter()
                .copied()
                .map(NodeRef::from)
                .collect(),
            ..self.request.clone()
        }
    }
}

/// The entries a walk displayed that have no text, which the game may not put up as a line.
///
/// REFUSED RATHER THAN MODELLED. They are rare - 114 of some 46,000 NPC lines - and whether
/// the game waits on one is unmeasured, so a walk through one is a walk that may disagree with
/// the game about where the inputs land.
///
/// NOT THE START, which leads every walk: the game reports it, but it is where the conversation
/// begins rather than a line put up on the way, so nothing waits on it.
pub fn silent(index: &Index, walk: &Walkthrough) -> Vec<DialogueNodeId> {
    walk.displayed
        .iter()
        .skip(1)
        .copied()
        .filter(|id| {
            index
                .get(&id.conversation_id)
                .and_then(|record| record.entries.iter().find(|entry| entry.id == id.entry_id))
                .is_some_and(|entry| {
                    entry
                        .fields
                        .get("Dialogue Text")
                        .is_none_or(|text| text.trim().is_empty())
                })
        })
        .collect()
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
pub fn stage(
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
    let holdings = fixtures::holdings_in_save(&scenario.save)
        .with_hardcore_playthrough_completed(scenario.hardcore_playthrough_completed);
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
            world: WorldRawData {
                // THE ROW WINS OVER THE SAVE where it names one, because a row that names a
                // balance is staging a balance - the money suite's three scenarios are one
                // save at three of them, and the save can only hold one.
                money: scenario.money.unwrap_or(holdings.money),
                day_minutes: scenario.day_minutes.unwrap_or(holdings.day_minutes),
                day_counter: holdings.day_counter,
                // AS THE GAME WOULD HAVE IT AT THAT HOUR, derived the way the loader derives
                // it - see `fixtures::clock_locked_in_save`. From the hour the scenario is
                // STAGED at rather than the one the save holds, since the rule turns on the
                // hour and a scenario may move it.
                clock_locked: fixtures::clock_locked_in_save(
                    &scenario.save,
                    scenario.day_minutes.unwrap_or(holdings.day_minutes),
                    holdings.day_counter,
                ),
                // WHAT THE ENGINE ASKED TO HAVE READ, positionally, as the plugin sends it -
                // see `bridge::DataRequest`. No query keys: a save cannot answer a call, and
                // `every_question_the_suites_ask_is_answered_offline` fails if a group asks one.
                data_values: holdings.data_for(&asked),
                items: holdings.items_asked(&asked),
                thoughts: holdings.thoughts_asked(&asked),
                // FROM THE SAVE, and the difference between a run and no run. An ordinary
                // option is often guarded on a dialogue variable - 451:86 is guarded on
                // whether Siileng has the sneakers to sell - and a world that cannot answer
                // stops the search before it builds a state, so the option draws nothing
                // where the game draws a marker.
                variables: fixtures::variables_sent(&scenario.save, &asked),
                failed_white_checks: fixtures::failed_white_checks_in_save(&scenario.save),
                // WHAT THIS SAVE HAS ALREADY SHOWN, which the engine seeds its seen slots from
                // and which no offline run has ever sent. The same set the seen state rungs are
                // built out of, put where a guard on having been shown can read it: without it
                // every once-only entry starts unfired, and a route that is spent in the save
                // is open to the crawl. Found by diffing against what the game sends - de-v702.
                seen,
                checks_pass: checks.pass,
                checks_fail: checks.fail,
                check_margins: checks.margins,
                // WHAT THE SAVE'S THOUGHTS DO TO RED CHECKS, as the plugin reads it from the
                // game: a cooking precarious_world forces every red roll to fail.
                red_checks_fail: fixtures::passive_thoughts_in_save(&scenario.save).red_checks_fail,
                ..Default::default()
            },
            ..Default::default()
        },
        questions: asked,
    };

    // WHAT SOME PLAYTHROUGH SHOWED, exactly as the plugin sends it: the two lower rungs
    // together, since seen this game implies seen any game. The world carries which of them
    // THIS game showed, and `world::seen_state` takes the three rungs from the pair.
    let everything: Vec<NodeRef> = staged
        .graph
        .nodes()
        .map(|node| NodeRef::from(node.id))
        .collect();
    staged.request.seen_any_game = everything
        .iter()
        .copied()
        .filter(|node| staged.seen_state_of(*node) != UNSEEN_ANY_GAME)
        .collect();

    Some(staged)
}
