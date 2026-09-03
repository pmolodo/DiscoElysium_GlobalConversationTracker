// SPDX-License-Identifier: MIT
//! Does the symbolic search reach the same entries as the explicit crawl?
//!
//! The question de-sze turns on, asked the only way that means anything: run both over
//! the same graph and the same world and compare. A symbolic engine that agrees with
//! nothing has established nothing, and one that disagrees with the explicit crawl is
//! answering a different question.
//!
//! ## What is compared, and what is not
//!
//! The set of ENTRIES reached, not the states at them. The two searches do not carry the
//! same thing - one holds `(entry, state)` pairs and the other one data-state set per
//! entry - so their state counts are not comparable by construction. The reachable entry
//! set is the thing both compute and the thing the look-ahead actually uses: a marker
//! depends on whether an unseen entry can be reached, not on how many ways.
//!
//! ## The direction a disagreement is allowed to run
//!
//! The symbolic side may reach MORE. Its guards let an undecided answer through, money is
//! not in its layout so no cost can be refused, and both are deliberate
//! over-approximations - a set that is too big loses precision, while one that is too
//! small loses markers. So the assertion is containment, not equality, and the surplus is
//! reported rather than tolerated silently.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions};
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::reachability::{Budget, Reachability};
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::world::world::ILookAheadWorld;

mod common;

const COUNTER_CAP: i32 = 16;
const NODE_CAPACITY: usize = 1 << 22;
const CACHE_CAPACITY: usize = 1 << 20;

/// Small enough that the explicit crawl can exhaust them, which is what makes them usable
/// as an oracle. A conversation the explicit crawl gives up on proves nothing when the
/// symbolic side reaches more.
const CHECKABLE: [i32; 6] = [1123, 484, 1066, 1147, 949, 511];

/// Every entry the explicit crawl reaches from `start`, and whether it ran out of budget.
fn explicit(
    graph: &lookahead_engine::graph::graph::LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    budget: usize,
) -> (HashSet<DialogueNodeId>, bool) {
    // Shared with the callback, which the engine requires to be 'static, so a borrow of a
    // local will not do.
    let reached: Arc<Mutex<HashSet<DialogueNodeId>>> = Arc::default();
    let sink = Arc::clone(&reached);

    let engine = LookAheadEngine::new(LookAheadOptions {
        state_budget: budget,
        time_budget: std::time::Duration::from_secs(60),
        counter_cap: COUNTER_CAP,
        // EVERY state, not a sample: the entry set is what is being collected, and
        // sampling would drop an entry that only one state reaches.
        state_sample_interval: 1,
        on_state_reached: Some(Box::new(move |node, _state, _count| {
            sink.lock().expect("the sink").insert(node);
        })),
        ..Default::default()
    });

    let result = engine.evaluate(graph, start, world, |_| Novelty::SeenThisGame);
    let entries = reached.lock().expect("the sink").clone();
    (entries, result.budget_exhausted())
}

#[test]
fn the_symbolic_search_reaches_what_the_explicit_crawl_reaches() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>9} {:>9} {:>8} {:>9} {:>7}",
        "conv", "entries", "explicit", "symbolic", "surplus", "bddnodes", "steps"
    );

    let mut compared = 0;

    for conversation in CHECKABLE {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let (walked, exhausted) = explicit(&graph, start, &world, 400_000);
        if exhausted {
            println!("{conversation:>6}  the explicit crawl ran out of budget; skipped");
            continue;
        }

        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
        let symbols = graph.symbols().clone();
        let vars = DataVars::new(&layout, &symbols, NODE_CAPACITY, CACHE_CAPACITY);
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(&graph));

        // The same state the explicit crawl starts in, encoded - not every data state.
        // Starting from all of them would walk paths needing an item the player has not
        // got, and report entries the crawl cannot reach - a surplus that says nothing
        // about the encoding.
        let seed = lookahead_engine::symbolic::reachability::seed_of(&graph, &world, &vars);
        let found =
            Reachability::explore(&graph, start, &seed, &mut compiler, &world, COUNTER_CAP as u32);
        let symbolic: HashSet<DialogueNodeId> = found.entries().collect();

        let missed: Vec<&DialogueNodeId> = walked.difference(&symbolic).collect();
        let surplus = symbolic.difference(&walked).count();
        let stats = found.stats();

        println!(
            "{conversation:>6} {:>8} {:>9} {:>9} {:>8} {:>9} {:>7}",
            graph.count(),
            walked.len(),
            symbolic.len(),
            surplus,
            stats.diagram_nodes,
            stats.steps,
        );

        // A surplus is expected, but it should be explainable rather than mysterious.
        // The two known sources are an undecided guard let through and a cost check that
        // cannot be refused because money is not in the layout - and with a save holding
        // no money at all, the second is the one that bites: the crawl declines every
        // priced option and the symbolic search takes them all.
        if surplus > 0 {
            println!(
                "         {surplus} extra: {} guard fallbacks, {} cost checks undecidable",
                compiler.fallbacks(),
                stats.unaffordable_unknown,
            );
        }

        assert!(
            missed.is_empty(),
            "conversation {conversation}: the symbolic search MISSED {} entries the crawl \
             reached, which is the direction it is never allowed to be wrong in: {:?}",
            missed.len(),
            missed.iter().take(10).collect::<Vec<_>>(),
        );

        compared += 1;
    }

    assert!(compared > 0, "no conversation could be compared both ways");
}

/// What does the COMPLETE reachable set cost, for the conversations that drive the cost?
///
/// The question de-sze was opened to answer and the one that could not be asked before.
/// 631 and 14 are precisely the groups the explicit crawl cannot exhaust - it burns its
/// budget and returns nothing useful - so their complete sets had never been measured,
/// and could not be by enumerating states, because enumerating them is the thing that is
/// too expensive.
///
/// Symbolic reachability obtains one without enumerating it, so the number here is the
/// first honest answer to "what would holding all of it cost".
/// Run it deliberately: `cargo test --release --test symbolic_reachability -- --ignored
/// --nocapture --test-threads=1`.
///
/// Ignored by default because it is a MEASUREMENT rather than a test - it asserts nothing
/// and answers a question - and because it does not finish. The budget bounds the step
/// count and the wall clock between steps, but a single step late in a run can take
/// minutes on its own, so the cap is not a real ceiling. In a debug build it is worse
/// again; release is not optional here.
#[test]
#[ignore = "a long measurement, not a test: run it with --ignored --release"]
fn what_the_expensive_conversations_cost() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>7} {:>9} {:>9} {:>9} {:>8} {:>7}",
        "conv", "entries", "vars", "reached", "bddnodes", "largest", "steps", "ms"
    );

    for conversation in [368, 631, 14, 28, 1030] {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
        let symbols = graph.symbols().clone();
        let vars = DataVars::new(&layout, &symbols, NODE_CAPACITY, CACHE_CAPACITY);
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(&graph));

        let seed = lookahead_engine::symbolic::reachability::seed_of(&graph, &world, &vars);
        let budget = Budget {
            steps: 500_000,
            time: std::time::Duration::from_secs(120),
            report_every: 5_000,
            on_progress: Some(Box::new(move |steps, reached, held| {
                println!(
                    "         ... {conversation}: {steps} steps, {reached} entries, \
                     {held} diagram nodes"
                );
            })),
        };

        let found = Reachability::explore_within(
            &graph, start, &seed, &mut compiler, &world, COUNTER_CAP as u32, &budget,
        );
        let stats = found.stats();

        println!(
            "{conversation:>6} {:>8} {:>7} {:>9} {:>9} {:>9} {:>8} {:>7}  {}",
            graph.count(),
            layout.total_vars(),
            stats.entries_reached,
            stats.diagram_nodes,
            stats.largest_set,
            stats.steps,
            stats.elapsed.as_millis(),
            // The field that decides whether the rest of the row is an answer or a lower
            // bound on one.
            if stats.reached_fixed_point { "complete" } else { "OUT OF BUDGET" },
        );
        println!(
            "         guards {} compiled / {} fell back; {} cost checks undecidable, \
             {} actions ignored",
            compiler.compiled(),
            compiler.fallbacks(),
            stats.unaffordable_unknown,
            stats.actions_ignored,
        );
    }
}
