// SPDX-License-Identifier: MIT
//! What earlier searches over this group already worked out.
//!
//! Every symbolic search starts from nothing today: it rebuilds the parent map, recompiles
//! every guard and re-derives sets a previous run over the same group already had. This is
//! the part of that which can be handed on, and the part that cannot is worth naming just
//! as precisely - see de-cnjw.
//!
//! ## The meet, which is the interesting one
//!
//! A forward search records, per entry, the data states a crawl can hold there. A backward
//! search from a target records, per entry, the states from which that target is still
//! reachable. Put them together and a path is complete: if some state can be held at `e`
//! going forwards AND reaches the target from `e` going backwards, the target is reachable
//! and neither search had to finish.
//!
//! That is the standard bidirectional trick, and it is sound here in the direction that
//! matters. Every state a forward run has put at an entry is genuinely reachable there
//! under the engine's own over-approximation - the sets only ever grow, and an aborted run
//! holds a SUBSET of what it would have held. So a meet PROVES reachable, and it proves it
//! from a forward run that was stopped after five minutes exactly as well as from one that
//! settled.
//!
//! The converse is not true and nothing here should be built as though it were. A state
//! ABSENT from a partial forward set may simply not have been got to yet, so a partial run
//! can never be used to refuse anything. Only a settled forward run bounds an entry, and
//! only a settled backward pass can answer no.
//!
//! ## The two searches disagree about what a set at an entry means, and it works out
//!
//! `Reachability::states_at(e)` holds what a crawl has AFTER entering `e` - its guard
//! tested, its cost paid, its actions applied - because that is what gets handed to e's
//! children. `Backward::states_at(c)` holds what a crawl must have ON ARRIVAL at `c`,
//! before c's own guard.
//!
//! So the two do not meet at an entry, they meet across an EDGE: for a link `e -> c`, what
//! the forward run can hand from `e` is exactly what the backward run wants to see at `c`.
//! Meeting them at the same entry instead would compare a post-state against a pre-state
//! and answer a question nobody asked.
//!
//! The start is the one entry with no incoming edge to meet on, and the seed stands in for
//! it: the seed is what the crawl holds arriving at the start, which is the same shape as
//! everything else this compares.

use std::collections::HashMap;

use oxidd::bdd::BDDFunction;
use oxidd::BooleanFunction;

use crate::core::types::DialogueNodeId;
use crate::graph::graph::LookAheadGraph;
use crate::symbolic::reachability::Reachability;

/// The group's shape and whatever a forward run has already established about it.
pub struct Known {
    /// Every entry's incoming links, which is the edge direction a backward pass walks.
    ///
    /// TARGET-INDEPENDENT, and rebuilt from scratch by every backward pass today. A driver
    /// that asks about forty candidates walks the whole graph forty times to build the
    /// same map.
    parents: HashMap<DialogueNodeId, Vec<DialogueNodeId>>,
    /// What a forward run left at each entry, AFTER that entry - so, what it can hand on.
    forward: HashMap<DialogueNodeId, BDDFunction>,
    /// The entry a crawl begins at, and what it holds arriving there.
    start: Option<(DialogueNodeId, BDDFunction)>,
    /// Whether the forward run settled. False for a partial one, and for no run at all.
    ///
    /// Only a settled run bounds anything. Nothing here uses it yet - the meet does not
    /// need it - and it is recorded because the pruning that WOULD need it is the obvious
    /// next thing to want, and getting this wrong is the way to lose a marker.
    forward_settled: bool,
}

impl Known {
    /// The group's shape alone, with nothing established about it yet.
    pub fn of(graph: &LookAheadGraph) -> Self {
        let mut parents: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
        for node in graph.nodes() {
            for &child in &node.links {
                parents.entry(child).or_default().push(node.id);
            }
        }

        Self { parents, forward: HashMap::new(), start: None, forward_settled: false }
    }

    /// The same, plus where a crawl begins and what it holds when it does.
    pub fn from(mut self, start: DialogueNodeId, seed: &BDDFunction) -> Self {
        self.start = Some((start, seed.clone()));
        self
    }

    /// Adds what a forward run found, settled or not.
    pub fn with_forward(mut self, found: &Reachability<'_>) -> Self {
        for entry in found.entries() {
            if let Some(states) = found.states_at(entry) {
                self.forward.insert(entry, states.clone());
            }
        }
        self.forward_settled = found.stats().reached_fixed_point;
        self
    }

    pub fn parents_of(&self, id: DialogueNodeId) -> &[DialogueNodeId] {
        self.parents.get(&id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn parents(&self) -> &HashMap<DialogueNodeId, Vec<DialogueNodeId>> {
        &self.parents
    }

    /// Whether anything is known that a backward pass could meet.
    pub fn can_meet(&self) -> bool {
        !self.forward.is_empty() || self.start.is_some()
    }

    pub fn forward_settled(&self) -> bool {
        self.forward_settled
    }

    /// Whether a crawl could arrive at `id` holding one of `wanted`.
    ///
    /// The meet. `wanted` is a backward set - what a crawl must hold arriving at `id` for
    /// the target to still be reachable - so this asks whether anything already known to
    /// arrive there is in it. Across the incoming edges, because what the forward run holds
    /// at a parent is what that parent hands on; and at the start, against the seed.
    ///
    /// An `Err` from the manager is out of room, and answering "no meet" on it is the safe
    /// direction: the pass carries on and ends on its own budget rather than reporting a
    /// proof it does not have.
    pub fn meets(&self, id: DialogueNodeId, wanted: &BDDFunction) -> bool {
        if let Some((start, seed)) = &self.start {
            if *start == id && seed.and(wanted).map(|both| both.satisfiable()).unwrap_or(false) {
                return true;
            }
        }

        self.parents_of(id).iter().any(|parent| {
            self.forward
                .get(parent)
                .and_then(|held| held.and(wanted).ok())
                .map(|both| both.satisfiable())
                .unwrap_or(false)
        })
    }
}
