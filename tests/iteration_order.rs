// SPDX-License-Identifier: MIT
//! Is the iteration order right, checked against the whole corpus?
//!
//! de-3x76.3. Both symbolic searches are a dataflow fixed point over the group's links with
//! a decision diagram at each entry, and both used to take their worklist first-in-first-out.
//! FIFO ignores the graph's shape: at a join it pops the entry once per arm that happens to
//! deliver at a different time, where a topological order pops it once with every arm
//! already folded into what is pending. See `symbolic::order`.
//!
//! ## What is checked here, and what is checked elsewhere
//!
//! THIS FILE CHECKS THE DECOMPOSITION, against the real corpus rather than against
//! hand-built graphs - `symbolic::order`'s own unit tests do those, and they cover the
//! distance tie-break and the worklist. A strongly-connected-components decomposition that
//! is subtly wrong still produces a plausible-looking order, so the guarantee is checked
//! directly, over all fifteen hundred groups:
//!
//! - every entry is ranked, and the component numbers run 0..components with none skipped;
//! - every link that LEAVES a component climbs in rank;
//! - a component really is a maximal set of mutually reachable entries, cross-checked by
//!   two link walks from a representative rather than by trusting Tarjan.
//!
//! THAT THE SEARCHES STILL ANSWER CORRECTLY is checked where it always was, and those are
//! the tests that matter for soundness: `tests/symbolic_reachability.rs` runs the forward
//! search against the explicit search, and `tests/backward_oracle.rs` runs the backward
//! search against it on every target of every group it can check both ways.
//!
//! WHAT THE ORDER IS WORTH is a measurement and does not belong in tests/ - see de-18bo,
//! which carries both the numbers and the job of re-homing the grid that produced them.
//! The short version: against a plain queue it is the difference between answering and not
//! on conversations 631 and 368, and the arrangements it beat are in the history.

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{Index, build_group_graph, read_index};
use lookahead_engine::symbolic::order::IterationOrder;

mod common;

/// The largest group the mutual-reachability cross-check is run on.
///
/// It walks the graph twice per component, so it is quadratic in a way the other checks are
/// not. The groups above this are covered by the two linear checks, and the shapes that
/// could break Tarjan - nested cycles, a component entered at two places - are not rare
/// enough to need a big group to find one.
const CROSS_CHECK_CEILING: usize = 400;

/// Which entries `from` can reach, following `edges`.
fn walk(
    from: DialogueNodeId,
    edges: &HashMap<DialogueNodeId, Vec<DialogueNodeId>>,
) -> HashSet<DialogueNodeId> {
    let mut seen = HashSet::from([from]);
    let mut queue = VecDeque::from([from]);
    while let Some(id) = queue.pop_front() {
        for &next in edges.get(&id).into_iter().flatten() {
            if seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    seen
}

/// The group's links, and the same links reversed.
fn edges_of(
    graph: &LookAheadGraph,
) -> (
    HashMap<DialogueNodeId, Vec<DialogueNodeId>>,
    HashMap<DialogueNodeId, Vec<DialogueNodeId>>,
) {
    let mut forward: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
    let mut backward: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
    for node in graph.nodes() {
        for &child in &node.links {
            if graph.get(child).is_none() {
                continue;
            }
            forward.entry(node.id).or_default().push(child);
            backward.entry(child).or_default().push(node.id);
        }
    }
    (forward, backward)
}

/// Every group in the index, in a stable order so a failure names a repeatable one.
fn groups(index: &Index) -> Vec<i32> {
    let mut all: Vec<i32> = index.keys().copied().collect();
    all.sort_unstable();
    all
}

/// Every entry is ranked, and the component numbers run from zero without a gap.
#[test]
fn every_entry_is_ranked_and_the_components_are_numbered() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");

    let mut checked = 0;
    for conversation in groups(&index) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        if graph.count() == 0 {
            continue;
        }
        let order = IterationOrder::of(&graph);

        assert_eq!(
            order.len(),
            graph.count(),
            "conversation {conversation}: {} entries but {} ranked",
            graph.count(),
            order.len(),
        );

        // BY COMPONENT, NOT BY RANK. A rank packs the component together with a distance,
        // so it is the component number that should number the components.
        let mut sizes: HashMap<u32, usize> = HashMap::new();
        for node in graph.nodes() {
            let component = order.component_of(node.id).expect("every entry is in one");
            *sizes.entry(component).or_default() += 1;
        }

        let mut used: Vec<u32> = sizes.keys().copied().collect();
        used.sort_unstable();
        let expected: Vec<u32> = (0..order.components() as u32).collect();
        assert_eq!(
            used, expected,
            "conversation {conversation}: the numbers do not run over its components",
        );
        assert_eq!(
            sizes.values().copied().max().unwrap_or(0),
            order.largest_component(),
            "conversation {conversation}: largest_component disagrees with the numbering",
        );
        checked += 1;
    }

    assert!(
        checked > 100,
        "only {checked} groups were checked; the corpus did not load"
    );
}

/// THE GUARANTEE. A link that leaves a component always climbs in rank.
///
/// This is the whole of what the searches rely on: a component is finished before anything
/// downstream of it begins. Inside a component nothing is promised beyond the distance
/// tie-break, which `symbolic::order` pins on its own.
#[test]
fn a_link_between_components_always_climbs() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");

    let mut crossings = 0;
    for conversation in groups(&index) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let order = IterationOrder::of_from(&graph, DialogueNodeId::new(conversation, 0));

        for node in graph.nodes() {
            for &child in &node.links {
                if graph.get(child).is_none() {
                    continue;
                }
                if order.component_of(node.id) == order.component_of(child) {
                    continue;
                }
                assert!(
                    order.rank_of(node.id) < order.rank_of(child),
                    "conversation {conversation}: {} -> {child} leaves a component but does \
                     not climb ({} then {})",
                    node.id,
                    order.rank_of(node.id),
                    order.rank_of(child),
                );
                crossings += 1;
            }
        }
    }

    assert!(
        crossings > 1000,
        "only {crossings} boundaries were crossed; too few to trust"
    );
}

/// A component is exactly the entries mutually reachable with any one of its members.
///
/// CROSS-CHECKED BY WALKING, not by asking Tarjan again. An entry `e` is in the component of
/// `r` exactly when `r` reaches `e` and `e` reaches `r`, so the component is the intersection
/// of a forward walk and a backward walk from any member. A decomposition that merged two
/// components or split one would fail here and would pass every other check in this file.
#[test]
fn a_component_is_exactly_what_is_mutually_reachable() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");

    let mut checked = 0;
    let mut cyclic = 0;
    for conversation in groups(&index) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        if graph.count() == 0 || graph.count() > CROSS_CHECK_CEILING {
            continue;
        }

        let order = IterationOrder::of(&graph);
        let (forward, backward) = edges_of(&graph);

        let mut members: HashMap<u32, HashSet<DialogueNodeId>> = HashMap::new();
        for node in graph.nodes() {
            let component = order.component_of(node.id).expect("every entry is in one");
            members.entry(component).or_default().insert(node.id);
        }

        for claimed in members.values() {
            let representative = *claimed.iter().next().expect("a component is not empty");
            let reaches = walk(representative, &forward);
            let reached_by = walk(representative, &backward);
            let truly: HashSet<DialogueNodeId> =
                reaches.intersection(&reached_by).copied().collect();

            assert_eq!(
                *claimed, truly,
                "conversation {conversation}: the component holding {representative} is not \
                 what is mutually reachable with it",
            );
            if claimed.len() > 1 {
                cyclic += 1;
            }
        }
        checked += 1;
    }

    assert!(
        checked > 50,
        "only {checked} groups were cross-checked; the corpus did not load"
    );
    assert!(
        cyclic > 0,
        "no group had a cycle at all, so nothing interesting was checked"
    );
}
