// SPDX-License-Identifier: MIT
//! How much does asking about ONE entry prune, and does the answer depend on the shape?
//!
//! The measurement behind de-sze.14, redone through the engine's own graph. The first
//! version was a regex pass over the conversation index, written to size the idea in an
//! afternoon, and a design should not rest on one - this repository has a rule about that,
//! and the modelling-gaps report carries the scar of ignoring it.
//!
//! ## What is measured
//!
//! For a spread of targets in each group:
//!
//! - THE SLICE: the entries that can reach the target by links alone. Guards can refuse an
//!   edge but never create one, so this is an upper bound on what any search for that
//!   target could touch.
//! - THE CONE: the slots the guards on those entries read. Reported against what
//!   `DataLayout::keeping_only_read` already prunes to, because the marginal win over
//!   today is the only number that decides anything.
//! - THE LARGEST STRONGLY CONNECTED COMPONENT, which the rough pass suggested predicts
//!   both of the above.
//!
//! ## Why it still matters now the backward pass exists
//!
//! The backward pass prunes its own variables - a slot enters a formula only if a guard on
//! a path to the target names it - so it does not need the cone. What this is for is the
//! other half: deciding, before either search runs, whether a group looks like conversation
//! 368, where a static slice would remove three quarters of it, or like 631, where it
//! removes nothing.

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};

mod common;

const COUNTER_CAP: i32 = 16;

/// The groups that drive the cost, the same five every other measurement uses.
const EXPENSIVE: [i32; 5] = [368, 631, 14, 28, 1030];

/// How many targets to sample per group.
const TARGETS: usize = 300;

fn conversations(default: &[i32]) -> Vec<i32> {
    match lookahead_engine::core::env::var("CONVERSATION") {
        Ok(named) => named
            .split(',')
            .filter_map(|id| id.trim().parse().ok())
            .collect(),
        Err(_) => default.to_vec(),
    }
}

/// The size of the largest strongly connected component, through petgraph.
///
/// A hub loop returns to its hub, so the loop and the hub land in the SAME component -
/// which is the whole reason this number predicts how badly slicing does.
fn largest_scc(graph: &LookAheadGraph) -> usize {
    let mut petg: DiGraph<DialogueNodeId, ()> = DiGraph::new();
    let mut index: HashMap<DialogueNodeId, NodeIndex> = HashMap::new();

    for node in graph.nodes() {
        let at = petg.add_node(node.id);
        index.insert(node.id, at);
    }
    for node in graph.nodes() {
        for &child in &node.links {
            if let (Some(from), Some(to)) = (index.get(&node.id), index.get(&child)) {
                petg.add_edge(*from, *to, ());
            }
        }
    }

    tarjan_scc(&petg)
        .into_iter()
        .map(|component| component.len())
        .max()
        .unwrap_or(0)
}

/// Every entry's incoming links.
fn parents_of(graph: &LookAheadGraph) -> HashMap<DialogueNodeId, Vec<DialogueNodeId>> {
    let mut parents: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
    for node in graph.nodes() {
        for &child in &node.links {
            parents.entry(child).or_default().push(node.id);
        }
    }
    parents
}

/// The entries that can reach `target` by links alone.
fn slice(
    parents: &HashMap<DialogueNodeId, Vec<DialogueNodeId>>,
    target: DialogueNodeId,
) -> HashSet<DialogueNodeId> {
    let mut seen = HashSet::from([target]);
    let mut queue = VecDeque::from([target]);
    while let Some(id) = queue.pop_front() {
        for &parent in parents.get(&id).into_iter().flatten() {
            if seen.insert(parent) {
                queue.push_back(parent);
            }
        }
    }
    seen
}

/// The middle value, and the one nine tenths of the way up.
fn spread(values: &mut [usize]) -> (usize, usize) {
    values.sort_unstable();
    let median = values[values.len() / 2];
    let p90 = values[(values.len() * 9 / 10).min(values.len() - 1)];
    (median, p90)
}

#[test]
fn how_much_does_one_target_prune() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");

    println!(
        "{:>6} {:>8} {:>6} {:>7} {:>7} {:>8} {:>7} {:>6} {:>7}",
        "conv", "entries", "SCC%", "slice%", "p90%", "slots", "kept", "cone", "vs kept"
    );

    let mut measured = 0;

    for conversation in conversations(&EXPENSIVE) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let symbols = graph.symbols().clone();
        let entries = graph.count();
        if entries == 0 {
            continue;
        }

        // What the layout keeps today: the slots some guard anywhere in the group reads,
        // plus the bookkeeping the engine reads for itself.
        let whole = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
        let kept_today = DataLayout::for_graph(&graph, COUNTER_CAP, None, false)
            .keeping_only_read(&symbols, &DataLayout::read_by(&graph));
        let kept = (0..whole.slot_count())
            .filter(|slot| kept_today.slot(*slot).is_some_and(|(_, bits)| bits > 0))
            .count();

        let parents = parents_of(&graph);
        let mut ids: Vec<DialogueNodeId> = graph.nodes().map(|node| node.id).collect();
        ids.sort_by_key(|id| (id.conversation_id, id.entry_id));
        let step = (ids.len() / TARGETS).max(1);

        let mut slices = Vec::new();
        let mut cones = Vec::new();
        for target in ids.iter().step_by(step) {
            let reaching = slice(&parents, *target);
            let read = DataLayout::read_by_some(&graph, reaching.iter().copied());
            // Only the names that are actually slots: a guard naming something no action
            // in this group writes is answered from the world and costs no variable.
            let cone = read
                .iter()
                .filter(|name| symbols.find(name).is_some())
                .count();

            slices.push(reaching.len());
            cones.push(cone);
        }

        let (slice_median, slice_p90) = spread(&mut slices);
        let (cone_median, _) = spread(&mut cones);
        let scc = largest_scc(&graph);

        println!(
            "{conversation:>6} {entries:>8} {:>5.0}% {:>6.0}% {:>6.0}% {:>8} {:>7} {:>6} {:>6.0}%",
            100.0 * scc as f64 / entries as f64,
            100.0 * slice_median as f64 / entries as f64,
            100.0 * slice_p90 as f64 / entries as f64,
            whole.slot_count(),
            kept,
            cone_median,
            100.0 * cone_median as f64 / kept.max(1) as f64,
        );

        measured += 1;
    }

    assert!(measured > 0, "no group could be measured");
}
