// SPDX-License-Identifier: MIT
//! The world a committed save puts a group in, for the measurements that need one that answers.
//!
//! ## Why a measurement would want this
//!
//! A `WorldSnapshot::default()` answers nothing: no skill check, no item, no variable, no
//! equipment query. That is fine for a measurement whose search only has to be given SOMETHING
//! to chew, and fatal for one that walks, because `walkthrough` refuses rather than guesses
//! wherever it cannot decide what the game would show. Measured on 537, a default world walks
//! 0 -> 171 -> 574 -> 482 and then stops.
//!
//! Shared rather than copied because it is easy to build almost right, and almost right here
//! is silent: every field left out is a question the world answers Unknown, and the only
//! symptom is a walk that stops early while reporting itself finished.
//!
//! ## The two that were built almost right, and cost an afternoon
//!
//! `checks_in_save` RETURNS AN OPTION AND THE NONE IS NOT "the save cannot decide". It computes
//! outcomes from the character sheet; a `None` says the actor table or the full index is
//! missing. Defaulting it to empty answers Unknown for every check, and a walk then refuses at
//! the first one.
//!
//! A SNAPSHOT'S DATA ANSWERS ARRIVE POSITIONALLY and a request resolves them against the
//! questions that asked for them. A world built directly has no request to do that, so without
//! [`WorldSnapshot::resolve`] every query stays Unknown - which stopped 761 one step past its
//! start, at `CheckEquipped("jacket_carabineer")` on 761:87.

#![allow(dead_code)]

use lookahead_engine::bridge::{NodeRef, WorldSnapshot};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{Index, discover_group};

#[path = "../tests/common/mod.rs"]
mod common;

/// The save the walked measurements take their world from.
///
/// THE FAIR COMMON DENOMINATOR: the blank slate every committed scenario save is eventually a
/// diff over, so every group is walked from the same place and none is favoured by a save that
/// happens to suit it. Which saves are legitimate for which conversations is a real question
/// and a much larger one - a conversation can assume variables that only hold at a point in the
/// game - and this does not answer it. What it costs is visible rather than hidden: a group
/// whose content assumes a later game is not walked far, and its row says so.
pub const TEMPLATE: &str = "save_template";

/// The world `save` puts `conversation`'s group in, built the way `tests/scenario_suites.rs`
/// builds one.
pub fn of_save(
    graph: &LookAheadGraph,
    conversation: i32,
    index: &Index,
    save: &str,
) -> WorldSnapshot {
    let group: Vec<i32> = discover_group(index, conversation).into_iter().collect();
    let asked = lookahead_engine::bridge::questions_of(graph, group.clone());
    let holdings = common::fixtures::holdings_in_save(save);
    let checks = common::fixtures::checks_in_save(save, &group)
        .expect("the actor table and the full index are both present");

    let mut snapshot = WorldSnapshot {
        money: holdings.money,
        day_minutes: holdings.day_minutes,
        day_counter: holdings.day_counter,
        // LOCKED, as the plugin sends it, for the reason the scenario suites give: nothing the
        // game exposes to Lua says whether its clock is locked, so a walk that let time pass
        // would be walking a world no run of the game is in.
        clock_locked: true,
        data_values: holdings.data_for(&asked.data),
        items: holdings.items.clone(),
        thoughts: holdings.thoughts.clone(),
        variables: common::fixtures::variables_sent(save, &asked),
        // WHAT THIS SAVE HAS ALREADY SHOWN, which the engine seeds its `once` and `seen` slots
        // from: without it every one-time effect starts unfired and a route the save has
        // already spent is open to the walk.
        seen: common::fixtures::read_in_save_group(save, &group)
            .into_iter()
            .map(|(conversation, entry)| NodeRef {
                conversation,
                entry,
            })
            .collect(),
        checks_pass: checks.pass,
        checks_fail: checks.fail,
        check_margins: checks.margins,
        red_checks_fail: common::fixtures::passive_thoughts_in_save(save).red_checks_fail,
        ..Default::default()
    };
    snapshot
        .resolve(&asked)
        .expect("the answers were built from these very questions");
    snapshot
}
