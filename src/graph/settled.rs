// SPDX-License-Identifier: MIT
//! Slots whose value a search cannot change before it reads them - de-gigq.
//!
//! ## What is being looked for
//!
//! A slot every write of which lies on every route to every read of it. Arriving at a read, the
//! write has happened, so the slot holds what it wrote - whatever route was taken. Where the
//! writes also agree on a literal, the guard reading it has one answer before the search starts,
//! and [`crate::core::guard::Guard::substituting`] can put that answer in its place. The slot
//! then has no reader and the layout drops it by the rule it already has.
//!
//! ## The two halves, and why only one of them is kept
//!
//! WHERE THE WRITES ARE, and whether they dominate the reads, is a fact about the links: the
//! same answer for every world and every request, and the expensive half - a dominator tree
//! over four thousand entries. That is what [`Candidates`] holds and what
//! `index::facts` keeps on disk.
//!
//! WHETHER A WRITE ACTUALLY FIRES is a fact about the WORLD, and cheap. An action can be turned
//! off for a world - see `DialogueAction::is_enabled` - and a check's success actions fire only
//! when the check succeeds, which `world::passive_outcome` answers for a settled check and
//! refuses for one the group can still move. So the value is worked out where the graph is
//! fitted, against candidates that were worked out once.
//!
//! ## Where a search can start, which decides everything
//!
//! A REQUEST'S STARTS ARE THE OPTIONS A MENU OFFERS - the plugin sends
//! `response.destinationEntry` for each response the game drew - so a start is a player entry
//! and never a group. Every such entry in the group is treated as a possible start here, which
//! is wider than any one menu, so an answer holds for every request.
//!
//! IT MATTERS MORE THAN ANYTHING ELSE HERE. Allowing EVERY entry to start a search - which is
//! what a layout narrowed per conversation has to assume - leaves 1 slot of 249 on conversation
//! 761. Allowing only what a menu can offer leaves 53.
//!
//! A READER THAT CAN ITSELF BE OFFERED IS A START, and then nothing has written the slot when
//! its guard is tested, so no write dominates it and it is not a candidate. That falls out of
//! the start set rather than needing a rule.

use std::collections::{HashMap, HashSet};

use petgraph::algo::dominators;
use petgraph::graph::{Graph, NodeIndex};
use serde::{Deserialize, Serialize};

use crate::core::action::DialogueActionKind;
use crate::core::types::DialogueNodeId;
use crate::graph::LookAheadGraph;

/// A slot whose writes lie on every route to its reads, and where those writes are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub slot: usize,
    /// Every entry whose action writes it, each of which a caller resolves against a world.
    pub written_at: Vec<DialogueNodeId>,
}

/// What a group offers, worked out from its links alone.
pub type Candidates = Vec<Candidate>;

/// Every slot whose writes dominate its reads.
///
/// THE VALUE IS NOT DECIDED HERE, because it cannot be: whether a write fires depends on the
/// world. See the module note.
pub fn candidates(graph: &LookAheadGraph) -> Candidates {
    let symbols = graph.symbols();
    let mut writers: HashMap<usize, Vec<DialogueNodeId>> = HashMap::new();
    let mut readers: HashMap<usize, Vec<DialogueNodeId>> = HashMap::new();
    let mut refused: HashSet<usize> = HashSet::new();
    let mut names = HashSet::new();

    for node in graph.nodes() {
        for action in &node.actions {
            let Ok(slot) = usize::try_from(action.slot()) else {
                continue;
            };
            if !action.writes_slot() {
                continue;
            }
            // ONLY AN ASSIGN OF A LITERAL can be settled: an increment's value depends on how
            // often it fired, and an `AssignUnless` on a slot this says nothing about.
            match action.kind() == DialogueActionKind::Assign && action.unless().is_none() {
                true => writers.entry(slot).or_default().push(node.id),
                false => {
                    refused.insert(slot);
                }
            }
        }
        // A FAILING BRANCH'S WRITE IS REFUSED OUTRIGHT. It fires on the outcome the success
        // actions do not, so a slot written on both sides holds one value on one route and
        // another on the other, and neither dominates.
        for action in &node.failure_actions {
            if let Ok(slot) = usize::try_from(action.slot())
                && action.writes_slot()
            {
                refused.insert(slot);
            }
        }
        // THE ENGINE'S OWN SLOTS are written where no action is - a once, a seen, a check's
        // flags - so nothing here can see those writes and none of them may be substituted.
        for slot in [
            node.once_slot,
            node.seen_slot,
            node.flag_slot,
            node.failed_flag_slot,
        ] {
            if let Ok(slot) = usize::try_from(slot) {
                refused.insert(slot);
            }
        }

        names.clear();
        crate::symbolic::data_layout::DataLayout::read_by_node_into(node, symbols, &mut names);
        for name in &names {
            if let Some(slot) = symbols.find(name) {
                readers.entry(slot).or_default().push(node.id);
            }
        }
    }

    let (built, at) = as_petgraph(graph);
    let tree = dominators::simple_fast(&built, at.root);

    let mut found: Candidates = writers
        .into_iter()
        .filter(|(slot, _)| !refused.contains(slot))
        .filter_map(|(slot, mut written_at)| {
            let read_at = readers.get(&slot)?;
            written_at.sort_unstable_by_key(|id| (id.conversation_id, id.entry_id));
            let dominated = read_at.iter().all(|reader| {
                at.of.get(reader).is_some_and(|&over| {
                    written_at.iter().any(|writer| {
                        at.of.get(writer).is_some_and(|&up| {
                            up == over
                                || tree
                                    .dominators(over)
                                    .is_some_and(|mut chain| chain.any(|above| above == up))
                        })
                    })
                })
            });
            dominated.then_some(Candidate { slot, written_at })
        })
        .collect();

    // IN SLOT ORDER, so what is kept on disk is the same bytes in every process.
    found.sort_unstable_by_key(|candidate| candidate.slot);
    found
}

/// The graph as petgraph sees it, with one added root linking to every entry a menu can offer.
///
/// A SET OF STARTS BECOMES THE ONE ROOT the algorithm takes, which is the usual way of asking
/// about several at once.
struct Indexed {
    root: NodeIndex,
    of: HashMap<DialogueNodeId, NodeIndex>,
}

fn as_petgraph(graph: &LookAheadGraph) -> (Graph<(), ()>, Indexed) {
    let mut built: Graph<(), ()> = Graph::new();
    let root = built.add_node(());
    let of: HashMap<DialogueNodeId, NodeIndex> = graph
        .nodes()
        .map(|node| (node.id, built.add_node(())))
        .collect();

    for node in graph.nodes() {
        for link in &node.links {
            if let Some(&onward) = of.get(link) {
                built.add_edge(of[&node.id], onward, ());
            }
        }
        if !node.is_group && (node.player || node.choice) {
            built.add_edge(root, of[&node.id], ());
        }
    }
    (built, Indexed { root, of })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::action::DialogueAction;
    use crate::core::guard::Guard;
    use crate::core::guard_value::GuardValue;
    use crate::core::state::StateSymbols;
    use crate::core::types::{DialogueCheckKind, Ternary};
    use crate::graph::Fitting;
    use crate::graph::node::LookAheadNode;
    use crate::symbolic::data_layout::DataLayout;
    use crate::world::test_world::TestWorld;

    fn node(entry: i32) -> DialogueNodeId {
        DialogueNodeId::new(1, entry)
    }

    /// An option, a passive check that sets `x`, and an entry whose guard reads it.
    ///
    /// THE READER IS NOT A PLAYER LINE, deliberately: a reader a menu could offer is a start
    /// in its own right, and then nothing has written the slot when its guard is tested.
    fn line_writing_then_reading() -> (LookAheadGraph, usize) {
        let mut symbols = StateSymbols::new();
        let slot = symbols.variable("x");

        let offered = LookAheadNode {
            player: true,
            choice: true,
            links: vec![node(1)],
            ..LookAheadNode::new(node(0))
        };
        let writes = LookAheadNode {
            kind: DialogueCheckKind::Passive,
            actions: vec![DialogueAction::assign(slot, 1, "sets x".to_string())],
            links: vec![node(2)],
            ..LookAheadNode::new(node(1))
        };
        let reads = LookAheadNode {
            guard: Guard::comparison(
                "==",
                Guard::variable("x"),
                Guard::literal(GuardValue::from_number(1.0)),
            ),
            ..LookAheadNode::new(node(2))
        };

        (
            LookAheadGraph::new(vec![offered, writes, reads], symbols).unwrap(),
            slot,
        )
    }

    /// A world that says which way the check went.
    fn world_where_the_check(outcome: Option<Ternary>) -> TestWorld {
        let mut world = TestWorld::declaring_nothing();
        if let Some(outcome) = outcome {
            world.check_results.insert(node(1), outcome);
        }
        world
    }

    /// The write dominates the read, so the slot is a candidate whatever any world says.
    #[test]
    fn a_write_on_every_route_to_a_read_is_a_candidate() {
        let (graph, slot) = line_writing_then_reading();
        let found = graph.settled_candidates();

        assert_eq!(found.len(), 1, "expected one candidate, got {found:?}");
        assert_eq!(found[0].slot, slot);
        assert_eq!(found[0].written_at, vec![node(1)]);
    }

    /// A check the world says PASSES settles the slot, and the guard reading it stops naming it
    /// - which is what leaves the layout free to drop it.
    #[test]
    fn a_passing_check_settles_the_slot_and_the_guard_stops_reading_it() {
        let (mut graph, _) = line_writing_then_reading();
        let world = world_where_the_check(Some(Ternary::True));

        let fitting = Fitting::read(&graph, &world);
        assert_eq!(fitting.settled.get(&0).copied(), Some(1));

        graph.fit(&fitting);
        assert!(
            !DataLayout::read_by(&graph).contains("x"),
            "the guard still reads x, so the slot cannot be dropped"
        );
    }

    /// A check the world cannot call settles nothing, which is the common case: a snapshot
    /// carries an outcome only for the checks the plugin evaluated.
    #[test]
    fn a_check_the_world_cannot_call_settles_nothing() {
        let (mut graph, _) = line_writing_then_reading();
        let world = world_where_the_check(None);

        let fitting = Fitting::read(&graph, &world);
        assert!(fitting.settled.is_empty());

        graph.fit(&fitting);
        assert!(
            DataLayout::read_by(&graph).contains("x"),
            "an unsettled slot lost its reader anyway"
        );
    }

    /// A check the world says FAILS never fires its write, so the slot is not settled to the
    /// value it would have written.
    #[test]
    fn a_failing_check_settles_nothing() {
        let (graph, _) = line_writing_then_reading();
        let fitting = Fitting::read(&graph, &world_where_the_check(Some(Ternary::False)));
        assert!(fitting.settled.is_empty());
    }

    /// A GRAPH FITTED TO ONE WORLD AND THEN ANOTHER answers about the second, which is what
    /// keeping the source guard is for: the substitution must not be one-way.
    #[test]
    fn fitting_again_puts_the_guard_back() {
        let (mut graph, _) = line_writing_then_reading();

        graph.fit(&Fitting::read(
            &graph,
            &world_where_the_check(Some(Ternary::True)),
        ));
        assert!(!DataLayout::read_by(&graph).contains("x"));

        let unknown = world_where_the_check(None);
        let fitting = Fitting::read(&graph, &unknown);
        graph.fit(&fitting);
        assert!(
            DataLayout::read_by(&graph).contains("x"),
            "a guard settled for one world stayed settled for another"
        );
    }
}
