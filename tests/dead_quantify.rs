// SPDX-License-Identifier: MIT
//! Forgetting a group's dead slots reaches the same entries, and never fewer states.
//!
//! `reachability::Budget::forget_dead` quantifies away, at each entry, the slots no path
//! onward reads before overwriting them. The argument that this is safe is a paragraph of
//! reasoning about a dataflow, and a paragraph is exactly the kind of thing that is right
//! about the analysis it was written for and wrong about the one that shipped - the search
//! reads slots no guard mentions, and a liveness analysis that misses one forgets something
//! the search is about to consult.
//!
//! So the claim is checked against real groups rather than argued:
//!
//! - THE SAME ENTRIES ARE REACHED. This is what every answer above this rests on, and it is
//!   the one that would fail if the analysis called a slot dead that a guard onward reads.
//! - NO STATE IS LOST. Abstraction may only ADD states, so the exact set at an entry must be
//!   contained in the abstracted one. A containment failure is a missed marker waiting to
//!   happen, and it is a sharper instrument than the entry count: a set can lose states
//!   while still reaching the same entries.
//!
//! ## Both arms have to SETTLE, which is why the groups are small
//!
//! A run stopped early is not comparable with another run stopped early. Abstraction changes
//! how big each delta is and therefore which entries are still queued when the budget runs
//! out, so two partial runs may honestly disagree. Only fixed points can be held to each
//! other, and a group is skipped rather than compared if either arm fails to reach one.

use std::collections::BTreeSet;
use std::sync::Arc;

use oxidd::BooleanFunction;

use lookahead_engine::bridge::{SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::live_slots::LiveSlots;
use lookahead_engine::symbolic::reachability::{Budget, Reachability, seed_of};

mod common;

const COUNTER_CAP: i32 = 16;

/// The band of group sizes to ask. Big enough to have structure, small enough that both
/// arms settle in a test rather than in a measurement.
const SMALLEST: usize = 20;
const LARGEST: usize = 400;

/// How many groups to check. The claim is about the analysis, not about any one group, so
/// what matters is that several different shapes agree rather than that all of them do.
const GROUPS: usize = 12;

#[test]
fn forgetting_dead_slots_reaches_the_same_entries_and_loses_no_state() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");

    let mut sizes: Vec<(i32, usize)> = index
        .iter()
        .map(|(id, conversation)| (*id, conversation.entries.len()))
        .filter(|(_, entries)| (SMALLEST..=LARGEST).contains(entries))
        .collect();
    // Deterministic, so a failure names the same group twice running.
    sizes.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));

    let mut compared = 0;
    for (conversation, _) in sizes {
        if compared == GROUPS {
            break;
        }
        if compare(&index, conversation) {
            compared += 1;
        }
    }

    // HOW MANY, not merely more than none. A check that quietly compared one group would
    // pass exactly as loudly as one that compared twelve, and the difference is whether
    // anything was established.
    assert!(
        compared >= GROUPS / 2,
        "only {compared} of {GROUPS} groups settled both ways, so little was compared"
    );
    eprintln!("compared {compared} groups");
}

/// Runs one group both ways, or `false` where it could not be compared.
fn compare(index: &lookahead_engine::index::Index, conversation: i32) -> bool {
    let Ok((graph, _)) = build_group_graph(index, conversation) else { return false };
    let start = DialogueNodeId::new(conversation, 0);
    if graph.get(start).is_none() {
        return false;
    }

    let symbols = graph.symbols().clone();
    let world = SnapshotWorld::declaring(
        WorldSnapshot { day_minutes: 720, day_counter: 1, ..Default::default() },
        None,
    );
    let layout = DataLayout::for_group(&graph, &world, COUNTER_CAP);
    let live = Arc::new(LiveSlots::of(&graph));

    // ONE MANAGER FOR BOTH ARMS, so the two sets can be intersected. Diagrams from two
    // managers cannot be combined at all, and the containment check is the whole reason
    // this is worth doing.
    let vars = lookahead_engine::symbolic::vars::DataVars::new(
        &layout,
        &symbols,
        DiagramBudget::modest(),
    );
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(&world)
        .with_constant_clock(DataLayout::group_passes_time(&graph));
    let seed = seed_of(&graph, &world, &vars).expect("room for a seed");

    let exact = Reachability::explore_within(
        &graph, start, &seed, &mut compiler, &world, COUNTER_CAP as u32, &Budget::default(),
    );
    if !exact.stats().reached_fixed_point {
        return false;
    }

    let forgetful = Reachability::explore_within(
        &graph,
        start,
        &seed,
        &mut compiler,
        &world,
        COUNTER_CAP as u32,
        &Budget { forget_dead: Some(live), ..Default::default() },
    );
    if !forgetful.stats().reached_fixed_point {
        return false;
    }

    let reached = |run: &Reachability| -> BTreeSet<(i32, i32)> {
        run.entries().map(|id| (id.conversation_id, id.entry_id)).collect()
    };
    assert_eq!(
        reached(&exact),
        reached(&forgetful),
        "conversation {conversation}: forgetting dead slots changed which entries are reachable",
    );

    for id in exact.entries() {
        let (Some(was), Some(now)) = (exact.states_at(id), forgetful.states_at(id)) else {
            continue;
        };
        let lost = was.and(&now.not().expect("not")).expect("and");
        assert!(
            !lost.satisfiable(),
            "conversation {conversation}, entry {}: forgetting dead slots dropped a state the \
             exact search reached",
            id.entry_id,
        );
    }

    true
}
