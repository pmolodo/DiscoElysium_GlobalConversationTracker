// SPDX-License-Identifier: MIT
//! What a state costs the forward crawl, so a change to the hot loop can be priced.
//!
//! ## Why this exists
//!
//! The crawl's inner loop runs once per state, and states run to hundreds of thousands, so
//! anything added there is multiplied by a number nobody has in their head. de-8hh2.14 added
//! a `try_reserve` per insert - the price of the frontier growing FALLIBLY rather than
//! aborting the game - and "probably cheap" is not a thing to take on trust about a loop
//! that size.
//!
//! ## Why a synthetic tree rather than a conversation
//!
//! Because this is measuring the FRONTIER and not the content. A binary tree with no guards
//! and no actions spends almost all its time in the two collections, which is exactly where
//! the check was added, so it is the worst case for the overhead and the least noisy place
//! to see it. A real group would spend most of its time in guard evaluation and hide the
//! thing being measured.
//!
//! ## How to read it
//!
//! `cargo test --release --test crawl_speed -- --ignored --nocapture`. Ignored because it is
//! a measurement rather than a claim: it prints, and the only thing it asserts is that the
//! crawl did the work it was asked to, so it cannot fail on a busy machine and waste
//! somebody's afternoon.

use std::time::Instant;

use lookahead_engine::core::action::DialogueAction;
use lookahead_engine::core::guard::GuardExpression;
use lookahead_engine::core::state::StateSymbols;
use lookahead_engine::core::types::{DialogueCheckKind, DialogueNodeId, Novelty};
use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::graph::node::LookAheadNode;
use lookahead_engine::world::test_world::TestWorld;

/// How deep the tree is: 2^18 - 1 nodes, a quarter of a million states.
///
/// Chosen so the run is seconds rather than milliseconds - a measurement of a loop needs
/// enough iterations that the setup does not dominate - and so it stays inside any budget
/// a caller here would set.
const DEPTH: u32 = 18;

/// How many times the crawl is run, with the fastest reported.
///
/// THE FASTEST, not the mean. The question is what the loop costs, and every source of
/// noise on a desktop - another process, a scheduler decision, a thermal step - can only
/// make a run slower. The best run is the closest to the cost being asked about.
const RUNS: usize = 5;

/// A binary tree: every node links to two children, nothing is seen, nothing prunes.
fn branching(depth: u32) -> LookAheadGraph {
    let symbols = StateSymbols::new();
    let mut nodes = Vec::new();

    let count = (1u32 << depth) - 1;
    for id in 0..count {
        let links: Vec<DialogueNodeId> = [2 * id + 1, 2 * id + 2]
            .into_iter()
            .filter(|child| *child < count)
            .map(|child| DialogueNodeId::new(1, child as i32))
            .collect();

        nodes.push(LookAheadNode::new(
            DialogueNodeId::new(1, id as i32),
            false,
            DialogueCheckKind::None,
            GuardExpression::always_true(),
            Vec::<DialogueAction>::new(),
            links,
            0,
            false,
            false,
            -1,
            -1,
            false,
            -1,
        ));
    }

    LookAheadGraph::new(nodes, symbols).expect("a binary tree is a graph")
}

#[test]
#[ignore = "a measurement; run it deliberately"]
fn what_a_state_costs_the_forward_crawl() {
    let graph = branching(DEPTH);
    let world = TestWorld::new();

    let engine = LookAheadEngine::new(LookAheadOptions {
        // NOTHING MAY STOP IT EARLY, or the measurement is of the budget.
        memory_budget: usize::MAX,
        state_budget: usize::MAX,
        time_budget: std::time::Duration::ZERO,
        ..Default::default()
    });

    let mut best = f64::MAX;
    let mut states = 0;

    for run in 1..=RUNS {
        let began = Instant::now();
        let result = engine.evaluate(
            &graph,
            DialogueNodeId::new(1, 0),
            &world,
            // Everything unseen THIS game and nothing unseen anywhere, so the crawl can
            // never stop early on a find and has to walk the whole tree.
            |_| Novelty::UnseenThisGame,
        );
        let seconds = began.elapsed().as_secs_f64();

        states = result.states_explored;
        let per_state = seconds * 1e9 / states as f64;
        println!("  run {run}: {states} states in {seconds:.3}s, {per_state:.0} ns/state");
        best = best.min(seconds);
    }

    let per_state = best * 1e9 / states as f64;
    println!("\nBEST: {states} states in {best:.3}s, {per_state:.0} ns per state");

    assert!(states > 100_000, "only {states} states; the tree is not the size it should be");
}
