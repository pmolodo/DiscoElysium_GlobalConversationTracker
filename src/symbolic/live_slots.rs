// SPDX-License-Identifier: MIT
//! Which slots an entry's successors can still read, so the rest can be forgotten.
//!
//! ## What this is for
//!
//! A slot that no path onward from an entry reads before overwriting it cannot change any
//! answer from that entry on. The set held there does not need to distinguish its values,
//! so its variables can be existentially quantified away - and every pair of states
//! differing only in them collapses into one. That is the standard abstraction in symbolic
//! model checking, and [`crate::symbolic::reachability::Budget::forget_dead`] is what
//! applies it.
//!
//! `measurements/live_ranges.rs` measured the opportunity and found half to three quarters
//! of a layout dead at the average entry. This is the analysis that can be acted on, which
//! is a stricter thing than the one that can be counted - see below.
//!
//! ## The dataflow
//!
//! Ordinary backward liveness to a fixed point over the link graph:
//!
//! ```text
//!   live_out(n) = union of live_in(s) over successors s
//!   live_in(n)  = reads(n) union (live_out(n) minus kills(n))
//! ```
//!
//! [`Self::live_out`] is the half a caller wants, because the set the search stores at an
//! entry is what that entry HANDS ON: its own guard and actions have already been applied.
//!
//! LINKS ALONE, guards ignored, so the successor set is an over-approximation and a slot is
//! called live wherever it might be. Over-approximating LIVENESS is the safe direction: it
//! forgets less than it could.
//!
//! ## Why this cannot simply reuse the measurement's reads and kills
//!
//! `live_ranges.rs` counts what a DIAGRAM pays for. This decides what may be thrown away,
//! and the search reads more slots than a guard mentions:
//!
//! - A ROLLED CHECK reads its pass and fail flags to know the roll is still open. Those are
//!   already in [`DataLayout::read_by_nodes`], which is why that routine is used here.
//! - A CHECK THAT CLOSES ONCE SEEN reads its own seen slot -
//!   [`crate::symbolic::reachability::Reachability::unseen`]. Nothing in the guard says so.
//! - A ONE-TIME EFFECT reads its once slot to know whether it has fired, and a price paid
//!   once reads the same slot to know whether it has been paid.
//!
//! Miss any of those and the abstraction forgets something the search is about to consult,
//! which is not a slower answer but a wrong one.
//!
//! ## And it kills less
//!
//! A write is only a kill if it happens to EVERY state the entry hands on. Three do not:
//!
//! - A ROLL's pass flag goes up on the passing branch only, and the two branches are
//!   unioned. Same for the fail flag. Both are reads anyway, so nothing is lost by it.
//! - A ONCE SLOT is raised only on the states that had it clear, so the rest keep what they
//!   had. It too is a read wherever it exists.
//! - A PASSIVE check that can fail hands its incoming states straight through UNCHARGED, so
//!   its seen slot is not written on that branch.
//!
//! Counting one of those as a kill would drop a slot from `live_in` that a predecessor's set
//! still has to carry.

use std::collections::{HashMap, HashSet};

use crate::core::action::DialogueActionKind;
use crate::core::state::StateSymbols;
use crate::core::types::{DialogueCheckKind, DialogueNodeId};
use crate::graph::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::symbolic::data_layout::DataLayout;

/// Which slots are still worth carrying, per entry.
///
/// Over SLOTS rather than diagram variables, and so independent of any layout: a group's
/// links and actions decide this, and the same answer serves every world the group is
/// searched in. That is what lets one of these be built beside a
/// [`crate::symbolic::known::GroupShape`] and kept for as long as the group is.
pub struct LiveSlots {
    live_out: HashMap<DialogueNodeId, Vec<usize>>,
}

impl LiveSlots {
    /// Runs the analysis over one group.
    pub fn of(graph: &LookAheadGraph) -> Self {
        let symbols = graph.symbols();

        let mut reads: HashMap<DialogueNodeId, HashSet<usize>> = HashMap::new();
        let mut kills: HashMap<DialogueNodeId, HashSet<usize>> = HashMap::new();
        for node in graph.nodes() {
            reads.insert(node.id, reads_of(node, symbols));
            kills.insert(node.id, kills_of(node));
        }

        // Backward to a fixed point. The link graph is cyclic, so this iterates rather than
        // walking a topological order; sets only grow, so it terminates.
        let mut live_in: HashMap<DialogueNodeId, HashSet<usize>> =
            graph.nodes().map(|node| (node.id, HashSet::new())).collect();
        let mut changed = true;
        while changed {
            changed = false;
            for node in graph.nodes() {
                let mut mine = reads[&node.id].clone();
                let killed = &kills[&node.id];
                for to in &node.links {
                    let Some(theirs) = live_in.get(to) else { continue };
                    mine.extend(theirs.iter().copied().filter(|slot| !killed.contains(slot)));
                }

                // Sets only grow, so a size that did not move is a set that did not.
                if mine.len() != live_in[&node.id].len() {
                    live_in.insert(node.id, mine);
                    changed = true;
                }
            }
        }

        // SORTED, so a caller building a cube walks the slots in a fixed order and two runs
        // over one group build the same diagram.
        let live_out = graph
            .nodes()
            .map(|node| {
                let mut out: HashSet<usize> = HashSet::new();
                for to in &node.links {
                    if let Some(theirs) = live_in.get(to) {
                        out.extend(theirs.iter().copied());
                    }
                }
                let mut out: Vec<usize> = out.into_iter().collect();
                out.sort_unstable();
                (node.id, out)
            })
            .collect();

        Self { live_out }
    }

    /// The slots some path onward from `id` can read before overwriting them.
    ///
    /// EMPTY FOR AN ENTRY THE GROUP DOES NOT HOLD, which is the same answer as an entry with
    /// no successors and is the right one either way: nothing onward reads anything.
    pub fn live_out(&self, id: DialogueNodeId) -> &[usize] {
        self.live_out.get(&id).map(|slots| slots.as_slice()).unwrap_or(&[])
    }

    /// The slots `layout` carries that nothing onward from `id` reads.
    ///
    /// The complement [`Self::live_out`] leaves, narrowed to what is actually in the layout -
    /// a slot trimmed to no bits has no variables to quantify and must not be asked for.
    pub fn dead_out(&self, id: DialogueNodeId, layout: &DataLayout) -> Vec<usize> {
        let live = self.live_out(id);
        (0..layout.slot_count())
            .filter(|slot| layout.slot(*slot).map(|(_, bits)| bits > 0).unwrap_or(false))
            .filter(|slot| live.binary_search(slot).is_err())
            .collect()
    }
}

/// Every slot entering `node` can consult.
fn reads_of(node: &LookAheadNode, symbols: &StateSymbols) -> HashSet<usize> {
    let mut reads = HashSet::new();

    // The guard, and a rolled check's two flags.
    for name in DataLayout::read_by_nodes(std::iter::once(node), symbols) {
        if let Some(slot) = symbols.find(&name) {
            reads.insert(slot);
        }
    }

    // An increment READS ITS OWN VALUE to produce the new one.
    for action in &node.actions {
        if action.kind() == DialogueActionKind::Increment {
            if let Ok(slot) = usize::try_from(action.slot()) {
                reads.insert(slot);
            }
        }
    }

    // A check that closes once seen asks whether it has been.
    if node.closes_once_seen() {
        if let Ok(slot) = usize::try_from(node.seen_slot) {
            reads.insert(slot);
        }
    }

    // A one-time effect, or a price paid once, asks whether it has already fired.
    if node.needs_once_slot() {
        if let Ok(slot) = usize::try_from(node.once_slot) {
            reads.insert(slot);
        }
    }

    reads
}

/// Every slot entering `node` overwrites in EVERY state it hands on.
fn kills_of(node: &LookAheadNode) -> HashSet<usize> {
    let mut kills = HashSet::new();

    // ASSIGNMENTS ONLY. An increment reads its own value, and the other kinds write money
    // or the clock, neither of which is a slot. An assignment is never a one-time action -
    // only increments and money carry that flag - so there is no half-firing case to
    // exclude here, unlike the once slot itself.
    for action in &node.actions {
        if action.kind() != DialogueActionKind::Assign {
            continue;
        }
        if let Ok(slot) = usize::try_from(action.slot()) {
            kills.insert(slot);
        }
    }

    // The seen slot goes up wherever the entry is charged, which is everywhere except a
    // passive check that can fail - that one hands its incoming states through untouched.
    if node.kind != DialogueCheckKind::Passive {
        if let Ok(slot) = usize::try_from(node.seen_slot) {
            kills.insert(slot);
        }
    }

    kills
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::action::DialogueAction;
    use crate::core::guard::GuardExpression;

    /// A chain of entries, each linking to the next, over a shared symbol table.
    fn chain(built: Vec<LookAheadNode>) -> LookAheadGraph {
        let mut symbols = StateSymbols::new();
        for name in ["a", "b", "c"] {
            symbols.variable(name);
        }
        LookAheadGraph::new(built, symbols).unwrap()
    }

    fn node(entry: i32, guard: GuardExpression, actions: Vec<DialogueAction>, links: Vec<i32>)
        -> LookAheadNode
    {
        LookAheadNode::new(
            DialogueNodeId::new(1, entry),
            false,
            DialogueCheckKind::None,
            guard,
            actions,
            links.into_iter().map(|to| DialogueNodeId::new(1, to)).collect(),
            0,
            false,
            false,
            -1,
            -1,
            false,
            -1,
        )
    }

    fn slot(graph: &LookAheadGraph, name: &str) -> usize {
        graph.symbols().find(name).unwrap()
    }

    #[test]
    fn a_slot_nothing_onward_reads_is_dead() {
        let graph = chain(vec![
            node(0, GuardExpression::always_true(), vec![], vec![1]),
            node(1, GuardExpression::Variable("a".into()), vec![], vec![]),
        ]);
        let live = LiveSlots::of(&graph);

        // Entry 0 hands on to an entry that reads `a` and nothing else.
        assert_eq!(live.live_out(DialogueNodeId::new(1, 0)), &[slot(&graph, "a")]);
        // Entry 1 hands on to nothing at all.
        assert!(live.live_out(DialogueNodeId::new(1, 1)).is_empty());
    }

    #[test]
    fn a_read_travels_back_along_the_chain() {
        let graph = chain(vec![
            node(0, GuardExpression::always_true(), vec![], vec![1]),
            node(1, GuardExpression::always_true(), vec![], vec![2]),
            node(2, GuardExpression::Variable("b".into()), vec![], vec![]),
        ]);
        let live = LiveSlots::of(&graph);

        let b = slot(&graph, "b");
        assert_eq!(live.live_out(DialogueNodeId::new(1, 0)), &[b]);
        assert_eq!(live.live_out(DialogueNodeId::new(1, 1)), &[b]);
    }

    #[test]
    fn an_assignment_stops_the_read_travelling_further_back() {
        let graph = chain(vec![
            node(0, GuardExpression::always_true(), vec![], vec![1]),
            node(1, GuardExpression::always_true(), vec![assign("b", 1)], vec![2]),
            node(2, GuardExpression::Variable("b".into()), vec![], vec![]),
        ]);
        let live = LiveSlots::of(&graph);

        // Entry 1 overwrites `b` before entry 2 reads it, so what entry 0 hands on cannot
        // matter.
        assert!(live.live_out(DialogueNodeId::new(1, 0)).is_empty());
        assert_eq!(live.live_out(DialogueNodeId::new(1, 1)), &[slot(&graph, "b")]);
    }

    #[test]
    fn a_once_increment_keeps_its_slot_live_rather_than_killing_it() {
        let graph = chain(vec![
            node(0, GuardExpression::always_true(), vec![], vec![1]),
            node(1, GuardExpression::always_true(), vec![once_increment("b")], vec![]),
        ]);
        let live = LiveSlots::of(&graph);

        // The increment reads what the slot held, and its once slot says whether it has
        // fired - so both survive back to entry 0.
        let out = live.live_out(DialogueNodeId::new(1, 0));
        assert!(out.contains(&slot(&graph, "b")));
        assert!(out.contains(&usize::try_from(graph.get(DialogueNodeId::new(1, 1))
            .unwrap().once_slot).unwrap()));
    }

    #[test]
    fn an_increment_reads_rather_than_kills() {
        let graph = chain(vec![
            node(0, GuardExpression::always_true(), vec![], vec![1]),
            node(1, GuardExpression::always_true(), vec![increment("c")], vec![]),
        ]);
        let live = LiveSlots::of(&graph);

        assert_eq!(live.live_out(DialogueNodeId::new(1, 0)), &[slot(&graph, "c")]);
    }

    #[test]
    fn a_loop_settles_rather_than_iterating_for_ever() {
        let graph = chain(vec![
            node(0, GuardExpression::always_true(), vec![], vec![1]),
            node(1, GuardExpression::Variable("a".into()), vec![], vec![0]),
        ]);
        let live = LiveSlots::of(&graph);

        let a = slot(&graph, "a");
        assert_eq!(live.live_out(DialogueNodeId::new(1, 0)), &[a]);
        assert_eq!(live.live_out(DialogueNodeId::new(1, 1)), &[a]);
    }

    #[test]
    fn dead_out_names_what_the_layout_carries_and_nothing_onward_reads() {
        let graph = chain(vec![
            node(0, GuardExpression::always_true(), vec![assign("a", 1)], vec![1]),
            node(1, GuardExpression::Variable("a".into()), vec![assign("b", 1)], vec![]),
        ]);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let live = LiveSlots::of(&graph);

        // Entry 1 leads nowhere, so everything the layout carries is dead there.
        let dead = live.dead_out(DialogueNodeId::new(1, 1), &layout);
        assert!(dead.contains(&slot(&graph, "a")));
        assert!(dead.contains(&slot(&graph, "b")));

        // Entry 0 hands on to a reader of `a`, so that one is not.
        let dead = live.dead_out(DialogueNodeId::new(1, 0), &layout);
        assert!(!dead.contains(&slot(&graph, "a")));
    }

    fn assign(name: &str, value: i32) -> DialogueAction {
        DialogueAction::assign(slot_number(name), value, name.to_string())
    }

    fn once_increment(name: &str) -> DialogueAction {
        DialogueAction::increment(slot_number(name), 1, true, name.to_string())
    }

    fn increment(name: &str) -> DialogueAction {
        DialogueAction::increment(slot_number(name), 1, false, name.to_string())
    }

    /// The slot numbers `chain` interns, in the order it interns them.
    fn slot_number(name: &str) -> usize {
        match name {
            "a" => 0,
            "b" => 1,
            "c" => 2,
            other => panic!("no fixture slot named {other}"),
        }
    }
}
