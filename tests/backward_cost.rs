// SPDX-License-Identifier: MIT
//! What does asking backwards cost, against the crawl that asks forwards?
//!
//! The verdict de-sze.14 exists for. Backwards is not free: it costs one fixed point per
//! candidate where the forward crawl costs one walk for all of them, so a long candidate
//! list none of which is reachable is where forward should win. This looks for the
//! crossover rather than assuming it.
//!
//! ## The question is asked the hard way round
//!
//! Against a fresh save everything is unseen, the first entry reached answers it, and both
//! engines return instantly having proved nothing. So one entry deep in the group is the
//! only unseen one - the shape that actually costs, and the one the explicit crawl cannot
//! answer on the big groups: it burns its budget in about half a second and returns
//! nothing useful, because with nothing novel to find it has no reason to stop early.
//!
//! A second question is asked as well, and it is the one the crossover lives in: the same
//! group with MANY unseen entries, all of them out of reach. There the driver has to ask
//! about every candidate and be refused by each, which is its worst case and the crawl's
//! best.
//!
//! ## One conversation per process
//!
//! These die in ways that take the process with them - a manager out of nodes, a stack
//! overflow inside a recursive diagram operation - so `tools/measure-symbolic.sh` runs one
//! per process and a crash costs one row rather than the run. Both tests here are
//! `#[ignore]`d: they are measurements, not tests, and nothing about them should fail a
//! normal build.

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::novelty_search::{best_novelty, candidates, Budget};
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;

mod common;

const COUNTER_CAP: i32 = 16;
const NODE_CAPACITY: usize = 1 << 24;
const CACHE_CAPACITY: usize = 1 << 22;

/// The groups that drive the cost.
const EXPENSIVE: [i32; 5] = [368, 631, 14, 28, 1030];

fn conversations(default: &[i32]) -> Vec<i32> {
    match std::env::var("CONVERSATION") {
        Ok(named) => named.split(',').filter_map(|id| id.trim().parse().ok()).collect(),
        Err(_) => default.to_vec(),
    }
}

/// How far each entry is from `start` by links alone.
fn depths(graph: &LookAheadGraph, start: DialogueNodeId) -> HashMap<DialogueNodeId, usize> {
    let mut depth = HashMap::from([(start, 0usize)]);
    let mut queue = VecDeque::from([start]);
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

/// The entries furthest from the start, which are the hardest fair questions to ask.
///
/// Structurally reachable, because an entry no path leads to is unreachable whatever the
/// guards say and both engines answering "not there" would measure nothing.
fn deepest(graph: &LookAheadGraph, start: DialogueNodeId, count: usize) -> Vec<DialogueNodeId> {
    let mut ordered: Vec<(usize, DialogueNodeId)> =
        depths(graph, start).into_iter().map(|(id, d)| (d, id)).collect();
    ordered.sort_by_key(|(depth, id)| {
        (std::cmp::Reverse(*depth), id.conversation_id, id.entry_id)
    });
    ordered
        .into_iter()
        .filter(|(_, id)| graph.get(*id).is_some_and(|node| !node.is_group))
        .take(count)
        .map(|(_, id)| id)
        .collect()
}

/// One row of the comparison, for one question over one group.
#[allow(clippy::too_many_arguments)]
fn compare(
    label: &str,
    conversation: i32,
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn lookahead_engine::world::world::ILookAheadWorld,
    unseen: &HashSet<DialogueNodeId>,
) {
    let novelty = |id: DialogueNodeId| {
        if unseen.contains(&id) {
            Novelty::UnseenAnyGame
        } else {
            Novelty::SeenThisGame
        }
    };

    // The crawl the plugin runs today, under its shipped budget.
    let engine = LookAheadEngine::new(LookAheadOptions {
        counter_cap: COUNTER_CAP,
        ..Default::default()
    });
    let began = std::time::Instant::now();
    let crawled = engine.evaluate(graph, start, world, &novelty);
    let crawl_ms = began.elapsed().as_millis();

    let layout = DataLayout::for_graph(graph, COUNTER_CAP, None, false);
    let symbols = graph.symbols().clone();
    let vars = DataVars::new(&layout, &symbols, NODE_CAPACITY, CACHE_CAPACITY);
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, world, &vars);

    let waiting = candidates(graph, start, &novelty).len();
    let began = std::time::Instant::now();
    let answer = best_novelty(
        graph,
        start,
        &seed,
        &mut compiler,
        world,
        COUNTER_CAP as u32,
        &novelty,
        &Budget { targets: 256, time: std::time::Duration::from_secs(120), ..Default::default() },
    );
    let backward_ms = began.elapsed().as_millis();

    println!(
        "{conversation:>6} {label:>10} {:>7} {:>14} {:>8} {:>14} {:>8} {:>7} {:>8}",
        waiting,
        format!("{:?}", crawled.best),
        crawl_ms,
        format!("{:?}", answer.best),
        backward_ms,
        answer.targets_asked,
        format!("{:?}", answer.stopped_by),
    );

    if crawled.budget_exhausted() {
        println!("         the crawl gave up: {:?}", crawled.stopped_by);
    }
}

/// One unseen entry, deep in the group. The shape the look-ahead struggles with.
#[test]
#[ignore = "a measurement, not a test: tools/measure-symbolic.sh runs it one per process"]
fn what_one_deep_target_costs_each_way() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>10} {:>7} {:>14} {:>8} {:>14} {:>8} {:>7} {:>8}",
        "conv", "question", "cands", "crawl", "ms", "backward", "ms", "asked", "stopped"
    );

    for conversation in conversations(&EXPENSIVE) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let one: HashSet<DialogueNodeId> = deepest(&graph, start, 1).into_iter().collect();
        compare("one deep", conversation, &graph, start, &world, &one);
    }
}

/// What ONE fixed point costs, which is what the crossover is made of.
///
/// The driver stops at its first witness, so the measurement above says what a lucky
/// question costs and nothing about an unlucky one. The crossover is arithmetic on two
/// numbers: what one crawl costs, and what one backward pass costs. Divide and you have
/// how many candidates the driver can afford before the crawl would have been cheaper.
///
/// Every pass here runs to a fixed point whatever the answer, because a pass that stops
/// early on a yes is not the case that decides anything.
#[test]
#[ignore = "a measurement, not a test: tools/measure-symbolic.sh runs it one per process"]
fn what_one_backward_pass_costs() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>7} {:>7} {:>7} {:>7} {:>9} {:>9} {:>8}",
        "conv", "entries", "crawlms", "asked", "medms", "maxms", "mednodes", "maxnodes", "afford"
    );

    for conversation in conversations(&EXPENSIVE) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        // One crawl, with nothing to find, which is what it costs to refuse everything.
        let engine = LookAheadEngine::new(LookAheadOptions {
            counter_cap: COUNTER_CAP,
            ..Default::default()
        });
        let began = std::time::Instant::now();
        let crawled = engine.evaluate(&graph, start, &world, |_| Novelty::SeenThisGame);
        let crawl_ms = began.elapsed().as_millis().max(1);
        let _ = crawled;

        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
        let symbols = graph.symbols().clone();
        let vars = DataVars::new(&layout, &symbols, NODE_CAPACITY, CACHE_CAPACITY);
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(&graph));

        // A spread of depths rather than the deepest, so the figure is what an ordinary
        // question costs rather than what the hardest one does.
        let mut ordered: Vec<(usize, DialogueNodeId)> =
            depths(&graph, start).into_iter().map(|(id, d)| (d, id)).collect();
        ordered.sort_by_key(|(depth, id)| (*depth, id.conversation_id, id.entry_id));
        let step = (ordered.len() / TARGETS_SAMPLED).max(1);

        let mut times: Vec<u128> = Vec::new();
        let mut sizes: Vec<usize> = Vec::new();
        let mut unsettled = 0;

        for (_, target) in ordered.iter().step_by(step) {
            let began = std::time::Instant::now();
            let backward = lookahead_engine::symbolic::backward::Backward::reaching(
                &graph,
                *target,
                &mut compiler,
                &world,
                COUNTER_CAP as u32,
            );
            times.push(began.elapsed().as_millis());
            sizes.push(backward.stats().diagram_nodes);
            if !backward.stats().reached_fixed_point {
                unsettled += 1;
            }
        }

        times.sort_unstable();
        sizes.sort_unstable();
        let median_ms = times[times.len() / 2].max(1);

        println!(
            "{conversation:>6} {:>8} {:>7} {:>7} {:>7} {:>7} {:>9} {:>9} {:>8}",
            graph.count(),
            crawl_ms,
            times.len(),
            median_ms,
            times[times.len() - 1],
            sizes[sizes.len() / 2],
            sizes[sizes.len() - 1],
            crawl_ms / median_ms,
        );

        if unsettled > 0 {
            println!("         {unsettled} of {} passes did not settle", times.len());
        }
    }
}

/// How many targets to time per group.
const TARGETS_SAMPLED: usize = 40;
