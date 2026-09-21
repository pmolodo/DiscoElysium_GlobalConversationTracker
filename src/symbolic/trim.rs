// SPDX-License-Identifier: MIT
//! The group as one request can walk it: entries whose guard holds in no state closed, and
//! what the starts can then no longer reach cut off.
//!
//! ## What it is for
//!
//! The searches decide guards state by state, so an entry whose guard can never hold is
//! refused by every search already. What does not refuse it is everything that walks the
//! LINKS and ignores guards to stay cheap: the parent map and iteration order in
//! [`super::known::GroupShape`], the hub candidates and the "can get back" components in
//! [`super::hub`], the choice-distance bound in [`super::seen_state_search::choice_bounds`], the
//! link walk that settles options before a pass in [`crate::graph::LookAheadGraph::best_linked_class`],
//! and the dominator tree. Each is sound because ignoring guards only ever keeps routes, and
//! each is loose for the same reason: a route through a door the world keeps shut still counts.
//!
//! So this closes those doors once, for the request, and every walk that reads the links gets
//! the tighter graph without learning anything about guards.
//!
//! ## A copy with links taken away, not a different graph
//!
//! The nodes, their actions and the symbol table are the group's, unchanged, so the layout, the
//! manager and the compiled guards built from the whole group line up with it entry for entry.
//! Only links go: every link into a closed entry, and every link out of an entry the starts can
//! no longer reach - which also takes those entries out of every parent list, so a backward
//! walk does not step onto them either.
//!
//! ## What closes an entry
//!
//! An ordinary entry whose compiled guard may hold in NO state at all - not only the one the
//! search starts from, since a write on the way can open a guard the seed does not satisfy.
//!
//! NOT A CHECK OF ANY KIND. A passive check whose condition fails is stepped over onto its
//! links rather than refusing them, and a rolled check's guard is what offers it; neither is a
//! door in the sense this needs.
//!
//! NOT A START. The menu was composed from this world, so a start's guard held when it was
//! offered; a compiled guard that says otherwise is a gap in the compiler, and closing a start
//! on it would draw a menu the player is looking at as unreachable.
//!
//! ## One round
//!
//! Closing entries removes the writes behind them, and a slot left with no writer is a constant
//! of the world that could close more guards. Seeing that needs the guards recompiled over a
//! layout without those slots, which this does not do: one round, sound, and less tight than a
//! fixed point would be.

use std::collections::{HashSet, VecDeque};

use oxidd::BooleanFunction;

use super::guard_formula::GuardCompiler;
use crate::core::types::{DialogueCheckKind, DialogueNodeId, SeenState};
use crate::graph::LookAheadGraph;

/// A request's view of its group.
pub struct Trimmed {
    /// The group with the links this request cannot take removed.
    pub graph: LookAheadGraph,
    /// Every entry the starts can still reach, closed entries not among them.
    ///
    /// WHAT THERE IS TO FIND. An unread entry outside it is not a target: nothing the request
    /// can do arrives there, so a seen state that counted it would hand every link walk a class
    /// no search can meet. See [`Self::seen_state`].
    pub reachable: HashSet<DialogueNodeId>,
    /// How many entries were closed by their guard.
    pub closed: usize,
    /// How many entries the starts could reach before closing and cannot after, closed
    /// entries not counted.
    pub cut_off: usize,
}

impl Trimmed {
    /// `seen_state`, with every entry the request cannot reach read as already seen.
    pub fn seen_state<'a, F: Fn(DialogueNodeId) -> SeenState + 'a>(
        &'a self,
        seen_state: F,
    ) -> impl Fn(DialogueNodeId) -> SeenState + 'a {
        move |id| {
            if self.reachable.contains(&id) {
                seen_state(id)
            } else {
                SeenState::SeenThisGame
            }
        }
    }
}

/// The group as a request starting at `starts` can walk it. See the module doc.
pub fn trimmed(
    graph: &LookAheadGraph,
    compiler: &mut GuardCompiler<'_>,
    starts: &[DialogueNodeId],
) -> Trimmed {
    let before = reached(graph, starts);

    let mut closed = HashSet::new();
    let mut reachable = HashSet::new();
    let mut pending: VecDeque<DialogueNodeId> = starts.iter().copied().collect();
    while let Some(id) = pending.pop_front() {
        if !reachable.insert(id) {
            continue;
        }
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if reachable.contains(&child) || closed.contains(&child) {
                continue;
            }
            let Some(entry) = graph.get(child) else {
                continue;
            };
            if entry.kind == DialogueCheckKind::None
                && !starts.contains(&child)
                && holds_nowhere(compiler, child, &entry.guard)
            {
                closed.insert(child);
                continue;
            }
            pending.push_back(child);
        }
    }

    let mut walkable = graph.clone();
    let ids: Vec<DialogueNodeId> = graph.nodes().map(|node| node.id).collect();
    for id in ids {
        let node = walkable.get_mut(id).expect("the id came from this graph");
        if reachable.contains(&id) {
            node.links.retain(|child| !closed.contains(child));
        } else {
            node.links.clear();
        }
    }

    let cut_off = before
        .iter()
        .filter(|id| !reachable.contains(id) && !closed.contains(id))
        .count();
    Trimmed {
        graph: walkable,
        reachable,
        closed: closed.len(),
        cut_off,
    }
}

/// Whether a guard holds in no state.
///
/// A REPUTATION QUESTION IS COMPILED WITHOUT BEING KEPT. The compiler settles reputation ranges
/// on the trimmed graph, after this has run, and a guard kept from before would keep its
/// per-state answer after its range settled.
fn holds_nowhere(
    compiler: &mut GuardCompiler<'_>,
    id: DialogueNodeId,
    guard: &crate::core::guard::Guard,
) -> bool {
    let compiled = if GuardCompiler::asks_reputation(guard) {
        compiler.compile(guard)
    } else {
        compiler.compile_for(id, guard)
    };
    !compiled.may_be_true.satisfiable()
}

/// Every entry the links reach from `starts`.
fn reached(graph: &LookAheadGraph, starts: &[DialogueNodeId]) -> HashSet<DialogueNodeId> {
    let mut seen = HashSet::new();
    let mut pending: VecDeque<DialogueNodeId> = starts.iter().copied().collect();
    while let Some(id) = pending.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(node) = graph.get(id) {
            pending.extend(node.links.iter().copied());
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::guard_value::GuardValue;
    use crate::symbolic::budget::DiagramBudget;
    use crate::symbolic::data_layout::DataLayout;
    use crate::symbolic::vars::DataVars;
    use crate::test_graph::{Entry, GraphBuilder, node};
    use crate::world::GameWorld;

    const DOOR: &str = r#"Variable["door"] == true"#;

    /// The group trimmed from entry 0, in a world where the door variable is shut.
    fn trim(graph: &LookAheadGraph) -> Trimmed {
        let world = GameWorld::blank().set_variable("door", GuardValue::from_boolean(false));
        let layout = DataLayout::for_graph(graph, 16, None, false);
        let vars = DataVars::new(&layout, graph.symbols(), DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        trimmed(graph, &mut compiler, &[node(0)])
    }

    /// A door the world keeps shut and nothing opens closes, and what only it led to is cut
    /// off: its links go, and it is no longer something to find.
    #[test]
    fn a_door_nothing_opens_closes_and_cuts_off_what_is_behind_it() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1).guard(DOOR).links(&[3]))
            .add(Entry::new(2))
            .add(Entry::new(3).links(&[0]))
            .build();
        let trimmed = trim(&graph);

        assert_eq!(trimmed.closed, 1);
        assert_eq!(trimmed.cut_off, 1, "3 is reached only through the door");
        assert_eq!(trimmed.graph.get(node(0)).unwrap().links, vec![node(2)]);
        assert!(trimmed.graph.get(node(3)).unwrap().links.is_empty());
        let seen_state = trimmed.seen_state(|_| SeenState::UnseenAnyGame);
        assert_eq!(seen_state(node(3)), SeenState::SeenThisGame);
        assert_eq!(seen_state(node(2)), SeenState::UnseenAnyGame);
    }

    /// A door something on the way can open is not closed, though the world has it shut.
    #[test]
    fn a_door_a_write_can_open_stays_open() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1).guard(DOOR).links(&[3]))
            .add(
                Entry::new(2)
                    .script(r#"SetVariableValue("door", true)"#)
                    .links(&[1]),
            )
            .add(Entry::new(3))
            .build();
        let trimmed = trim(&graph);

        assert_eq!(trimmed.closed, 0);
        assert!(trimmed.reachable.contains(&node(3)));
    }

    /// A check whose guard cannot hold is stepped over rather than refused, so it closes
    /// nothing.
    #[test]
    fn a_check_never_closes() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(
                Entry::new(1)
                    .guard(DOOR)
                    .kind(DialogueCheckKind::Passive)
                    .links(&[2]),
            )
            .add(Entry::new(2))
            .build();
        let trimmed = trim(&graph);

        assert_eq!(trimmed.closed, 0);
        assert!(trimmed.reachable.contains(&node(2)));
    }

    /// A start is never closed: the menu was offered from this world.
    #[test]
    fn a_start_never_closes() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).guard(DOOR).links(&[1]))
            .add(Entry::new(1))
            .build();
        let trimmed = trim(&graph);

        assert_eq!(trimmed.closed, 0);
        assert!(trimmed.reachable.contains(&node(1)));
    }
}
