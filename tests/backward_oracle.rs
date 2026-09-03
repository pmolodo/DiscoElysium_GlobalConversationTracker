// SPDX-License-Identifier: MIT
//! Does the backward search find what the explicit crawl walks to?
//!
//! The third corner of the triangle. `symbolic_reachability` checks the FORWARD symbolic
//! search against the explicit crawl; this checks the backward one, and against the same
//! oracle rather than against the forward search, so an error the two symbolic searches
//! share cannot hide behind their agreement.
//!
//! ## The direction a disagreement is allowed to run
//!
//! The backward search may say REACHABLE where the crawl did not get there. Its guards
//! let an undecided answer through, money is not in its layout so no cost can be refused,
//! and a once action fires every time round a loop while de-sze.15 is open - three
//! deliberate over-approximations, and a set that is too big loses precision while one
//! that is too small loses markers.
//!
//! What it may never do is say UNREACHABLE about an entry the crawl walked to. That is
//! not an approximation, it is a lost state in the pre-image, and it would make the engine
//! report an option as dead that has something behind it.
//!
//! ## Why every target, rather than one
//!
//! The forward search answers about every entry in one run, so one comparison covers the
//! group. The backward search answers about ONE entry per run, so covering the group means
//! one run per entry - which is also the honest way to find out what the approach costs
//! when the answer is no.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::backward::Backward;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::novelty_search::{best_novelty, Budget as SearchBudget};
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::world::world::ILookAheadWorld;

mod common;

const COUNTER_CAP: i32 = 16;
const NODE_CAPACITY: usize = 1 << 22;
const CACHE_CAPACITY: usize = 1 << 20;

/// Small enough that the explicit crawl can exhaust them, which is what makes them an
/// oracle at all. The same six `symbolic_reachability` uses, deliberately: the two tests
/// should be comparable.
const CHECKABLE: [i32; 6] = [1123, 484, 1066, 1147, 949, 511];

/// How many targets to ask about per group.
///
/// One backward pass each, so this is the whole cost of the test. Spread across the depth
/// range rather than taken from the front, because the first entries of a conversation are
/// its opening lines and they are all trivially reachable.
const TARGETS: usize = 40;

/// Which conversations this process should check, one per process being the intended way.
fn conversations(default: &[i32]) -> Vec<i32> {
    match std::env::var("CONVERSATION") {
        Ok(named) => named.split(',').filter_map(|id| id.trim().parse().ok()).collect(),
        Err(_) => default.to_vec(),
    }
}

/// Every entry the explicit crawl reaches from `start`, and whether it ran out of budget.
fn explicit(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    budget: usize,
) -> (HashSet<DialogueNodeId>, bool) {
    let reached: Arc<Mutex<HashSet<DialogueNodeId>>> = Arc::default();
    let sink = Arc::clone(&reached);

    let engine = LookAheadEngine::new(LookAheadOptions {
        state_budget: budget,
        time_budget: std::time::Duration::from_secs(60),
        counter_cap: COUNTER_CAP,
        // Every state, not a sample: an entry only one state reaches would be dropped.
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

/// Entries reachable from `start` by following links alone, with how far away they are.
fn structural_depths(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
) -> HashMap<DialogueNodeId, usize> {
    let mut depth = HashMap::new();
    let mut queue = std::collections::VecDeque::new();
    depth.insert(start, 0usize);
    queue.push_back(start);

    while let Some(id) = queue.pop_front() {
        let here = depth[&id];
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if graph.get(child).is_some() && !depth.contains_key(&child) {
                depth.insert(child, here + 1);
                queue.push_back(child);
            }
        }
    }

    depth
}

/// A spread of targets across the depth range, in a fixed order.
///
/// Sorted before sampling so the same targets are asked about on every run: a test that
/// picks different questions each time reports a different answer each time, and a rare
/// disagreement would look like a flake rather than a bug.
fn targets(depths: &HashMap<DialogueNodeId, usize>) -> Vec<DialogueNodeId> {
    let mut ordered: Vec<(usize, DialogueNodeId)> =
        depths.iter().map(|(id, d)| (*d, *id)).collect();
    ordered.sort_by_key(|(depth, id)| (*depth, id.conversation_id, id.entry_id));

    let step = (ordered.len() / TARGETS).max(1);
    ordered.iter().step_by(step).map(|(_, id)| *id).collect()
}

#[test]
fn the_backward_search_finds_what_the_explicit_crawl_reaches() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9} {:>8}",
        "conv", "entries", "targets", "walked", "agreed", "surplus", "bddnodes", "ms"
    );

    let mut compared = 0;

    for conversation in conversations(&CHECKABLE) {
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
        let seed = seed_of(&graph, &world, &vars);

        let depths = structural_depths(&graph, start);
        let asked = targets(&depths);

        let began = std::time::Instant::now();
        let mut missed: Vec<DialogueNodeId> = Vec::new();
        let mut agreed = 0;
        let mut surplus = 0;
        let mut diagram_nodes = 0;

        for target in &asked {
            let backward =
                Backward::reaching(&graph, *target, &mut compiler, &world, COUNTER_CAP as u32);
            let stats = backward.stats();
            assert!(
                !stats.out_of_memory,
                "conversation {conversation}: the manager ran out of room on {target}",
            );
            diagram_nodes += stats.diagram_nodes;

            let says_reachable = backward.reachable_from(start, &seed);
            match (walked.contains(target), says_reachable) {
                (true, true) => agreed += 1,
                (true, false) => missed.push(*target),
                (false, true) => surplus += 1,
                (false, false) => agreed += 1,
            }
        }

        println!(
            "{conversation:>6} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9} {:>8}",
            graph.count(),
            asked.len(),
            asked.iter().filter(|t| walked.contains(t)).count(),
            agreed,
            surplus,
            diagram_nodes,
            began.elapsed().as_millis(),
        );

        assert!(
            missed.is_empty(),
            "conversation {conversation}: the backward search called {} entries \
             unreachable that the explicit crawl walked to, which is the direction it is \
             never allowed to be wrong in: {:?}",
            missed.len(),
            missed.iter().take(10).collect::<Vec<_>>(),
        );

        compared += 1;
    }

    assert!(compared > 0, "no conversation could be checked both ways");
}

/// The whole driver against the whole engine, on real conversations.
///
/// The test above checks one target at a time, which is the part that can be wrong
/// quietly. This checks the thing the plugin would actually call, and against the thing it
/// calls today.
///
/// ## The question is asked the hard way round
///
/// Against a fresh save everything is unseen, the first entry reached answers it, and both
/// engines return instantly having proved nothing. So the novelty function here says
/// almost everything is SEEN, leaving a handful of entries deep in the group unseen -
/// which is the shape that costs, and the one de-sze.14 is about.
#[test]
fn the_driver_answers_what_the_engine_answers() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>10} {:>10} {:>7} {:>9} {:>8} {:>7}",
        "conv", "entries", "engine", "driver", "asked", "of", "witness", "ms"
    );

    let mut compared = 0;

    for conversation in conversations(&CHECKABLE) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        // The deepest few entries are the unseen ones: far from the start, so the answer
        // cannot be had by glancing at the first link.
        let depths = structural_depths(&graph, start);
        let mut by_depth: Vec<(usize, DialogueNodeId)> =
            depths.iter().map(|(id, d)| (*d, *id)).collect();
        by_depth.sort_by_key(|(depth, id)| {
            (std::cmp::Reverse(*depth), id.conversation_id, id.entry_id)
        });
        let unseen: HashSet<DialogueNodeId> =
            by_depth.iter().take(3).map(|(_, id)| *id).collect();

        let novelty = |id: DialogueNodeId| {
            if unseen.contains(&id) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        };

        let engine = LookAheadEngine::new(LookAheadOptions {
            counter_cap: COUNTER_CAP,
            state_budget: 400_000,
            time_budget: std::time::Duration::from_secs(60),
            ..Default::default()
        });
        let expected = engine.evaluate(&graph, start, &world, &novelty);
        if expected.budget_exhausted() {
            println!("{conversation:>6}  the engine ran out of budget; skipped");
            continue;
        }

        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
        let symbols = graph.symbols().clone();
        let vars = DataVars::new(&layout, &symbols, NODE_CAPACITY, CACHE_CAPACITY);
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(&graph));
        let seed = seed_of(&graph, &world, &vars);

        let answer = best_novelty(
            &graph,
            start,
            &seed,
            &mut compiler,
            &world,
            COUNTER_CAP as u32,
            &novelty,
            &SearchBudget::default(),
        );

        println!(
            "{conversation:>6} {:>8} {:>10} {:>10} {:>7} {:>9} {:>8} {:>7}",
            graph.count(),
            format!("{:?}", expected.best),
            format!("{:?}", answer.best),
            answer.targets_asked,
            answer.candidates,
            answer
                .witness
                .map(|w| format!("{}", w.entry_id))
                .unwrap_or_else(|| "-".to_string()),
            answer.elapsed.as_millis(),
        );

        assert!(
            answer.best >= expected.best,
            "conversation {conversation}: the driver said {:?} where the engine said \
             {:?}, which is a marker lost",
            answer.best,
            expected.best,
        );

        compared += 1;
    }

    assert!(compared > 0, "no conversation could be compared");
}
