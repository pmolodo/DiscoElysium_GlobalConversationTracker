// SPDX-License-Identifier: MIT
//! What is true of a group before any search over it runs.
//!
//! A backward pass starts from nothing today: it rebuilds the parent map, re-derives the
//! order to take entries in, and recompiles every guard a previous pass over the same group
//! already compiled. This is the part of that which can be handed on, and the part that
//! cannot is worth naming just as precisely - see de-cnjw.
//!
//! ## Two kinds of thing live here
//!
//! THE GRAPH'S SHAPE - the parent map, and the order to take entries in - is true of the
//! group and of nothing else. It depends on no target, no start and no world, so sharing it
//! can change no answer at all; what it saves is a driver working the same thing out once
//! per candidate, forty times over for a menu that asks about forty of them.
//!
//! WHERE A SEARCH BEGINS, and what it holds arriving there, is the other kind. It is true
//! of one start rather than of the group, and it is what makes the meet below possible.
//!
//! ## The meet, which is what a start is carried here for
//!
//! A backward search from a target records, per entry, the states from which that target is
//! still reachable. Put that against what a search actually holds where it begins and a
//! path is complete: if the beginning's states and the backward set at that same entry have
//! anything in common, the target is reachable and the pass never had to settle.
//!
//! This PROVES reachable and can never refuse anything. A backward pass that has not
//! settled holds a subset of the sets it would eventually hold, so an empty intersection
//! means only "not yet", and only a settled pass can answer no.
//!
//! ## Which set is compared against which
//!
//! `Backward::states_at(c)` holds what a search must have ON ARRIVAL at `c`, before c's own
//! guard is tested. So what is stored here for a beginning entry is likewise what the
//! search holds ARRIVING there, before that entry's guard - the seed for an ordinary start,
//! and for one outcome of a rolled check what entering by that outcome left. Comparing a
//! post-entry set against a pre-entry one would answer a question nobody asked.
//!
//! ## Several beginnings, not one
//!
//! One outcome of a rolled check begins at every destination that outcome leads to, all of
//! them holding the same states - never at the check itself, whose pre-entry states are
//! reachable by either roll and would let a meet there prove the wrong thing. So the
//! beginnings are a map rather than a single entry, and a meet at any of them proves the
//! same thing as a meet at the one. See `novelty_search::Where`.

use std::collections::HashMap;
use std::sync::Arc;

use oxidd::BooleanFunction;
use oxidd::bdd::BDDFunction;

use crate::core::types::DialogueNodeId;
use crate::graph::graph::LookAheadGraph;
use crate::symbolic::order::IterationOrder;

/// The group's shape, and where a search over it begins.
pub struct Known {
    /// Every entry's incoming links, which is the edge direction a backward pass walks.
    ///
    /// TARGET-INDEPENDENT, and rebuilt from scratch by every backward pass that is not
    /// handed one. A driver that asks about forty candidates walks the whole graph forty
    /// times to build the same map.
    ///
    /// AND START-INDEPENDENT TOO, which is what the handle is for: see [`GroupShape`], and
    /// the menu of twenty-four options that would otherwise build twenty-four of these.
    parents: Arc<HashMap<DialogueNodeId, Vec<DialogueNodeId>>>,
    /// The order to take entries in, so that a loop is finished before what follows it.
    ///
    /// TARGET-INDEPENDENT AND DIRECTION-INDEPENDENT, for the same reason the parent map is
    /// and with the same saving: it is a fact about the graph's shape, and a driver asking
    /// about forty candidates would otherwise work it out forty times. See
    /// [`IterationOrder`] for why reversing the edges does not change it.
    order: IterationOrder,
    /// The entries a search begins at, and what it holds arriving at each.
    ///
    /// Empty until a caller says, and a pass told nothing has nothing to meet.
    beginnings: HashMap<DialogueNodeId, BDDFunction>,
}

/// The half of [`Known`] that is true of the GROUP rather than of one search.
///
/// ## Why this is a type of its own
///
/// A response menu asks about a dozen options at once - twenty-four when they are rolled
/// checks, since de-fes makes each outcome its own start - and `bridge::answer_within` runs
/// every one of them against one manager and one compiler. What it did NOT share was this:
/// each start built its own parent map and its own Tarjan decomposition, which depend on
/// the links and on nothing else, and so were twenty-four copies of one answer.
///
/// Measured before it existed, `measurements/per_start_setup.rs`, per menu of 24 starts:
///
/// ```text
///   conv  entries  order ms  of_from ms  known ms  per menu ms
///     28     2186      1.14        1.52      2.95         98.0
///    368     4724      5.80        3.40      6.94        305.9
///     14     3594      2.38        3.21      5.44        187.7
///    631     4514      3.24        4.14      7.02        246.1
///    362     1860      1.04        1.31      2.56         86.4
/// ```
///
/// For scale: `menu_residue` answers twenty-four starts over conversation 28 in 0.8
/// seconds, and de-2wtl's whole per-REQUEST setup - the group graph and the diagram side
/// together - is eighteen to twenty-seven milliseconds. This was the larger waste by an
/// order of magnitude, and unlike that one it needs nothing that outlives the query.
///
/// ## What it does not hold
///
/// Where a search begins. That is per start, per world and per seed, and it is what
/// [`Known`] adds on top - see [`Self::known_from`].
pub struct GroupShape {
    parents: Arc<HashMap<DialogueNodeId, Vec<DialogueNodeId>>>,
    order: IterationOrder,
}

impl GroupShape {
    /// Works the group's shape out, once, for every search that will run over it.
    pub fn of(graph: &LookAheadGraph) -> Self {
        let mut parents: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
        for node in graph.nodes() {
            for &child in &node.links {
                parents.entry(child).or_default().push(node.id);
            }
        }

        Self {
            parents: Arc::new(parents),
            order: IterationOrder::of(graph),
        }
    }

    /// The order a search over this group should take its entries in.
    ///
    /// No start distances, so this is what a caller passes to
    /// [`crate::symbolic::reachability::Reachability::explore_branch_knowing`] rather than
    /// letting it build one per search.
    pub fn order(&self) -> &IterationOrder {
        &self.order
    }

    /// This shape as a [`Known`] for one start, sharing everything that can be shared.
    ///
    /// The parent map is handed over by handle and the Tarjan decomposition with it; only
    /// the start distances are worked out, which is one BFS over the links - 0.9 ms of
    /// conversation 631's 4.1 rather than all of it.
    pub fn known_from(&self, graph: &LookAheadGraph, start: DialogueNodeId) -> Known {
        Known {
            parents: Arc::clone(&self.parents),
            order: self.order.distanced_from(graph, start),
            beginnings: HashMap::new(),
        }
    }
}

impl Known {
    /// The group's shape alone, with no beginning named yet.
    ///
    /// Works the shape out for this one search. A caller running SEVERAL searches over one
    /// group - every option of a response menu is one - should build a [`GroupShape`] once
    /// and ask it for each of these instead.
    pub fn of(graph: &LookAheadGraph) -> Self {
        let shape = GroupShape::of(graph);
        Self {
            parents: shape.parents,
            order: shape.order,
            beginnings: HashMap::new(),
        }
    }

    /// The same, told where a search begins, so the start-distances can be worked out.
    ///
    /// Without it the members of a component simply tie and the push order decides, which
    /// is a coarser order rather than a wrong one - but it is a measurement reporting
    /// less than it could, so a caller that knows the start should say so.
    pub fn of_from(graph: &LookAheadGraph, start: DialogueNodeId) -> Self {
        GroupShape::of(graph).known_from(graph, start)
    }

    /// The same, plus one entry a search begins at and what it holds arriving there.
    ///
    /// ADDS RATHER THAN REPLACES, because one outcome of a rolled check begins at several
    /// destinations at once and a meet at any of them proves the same thing. Calling this
    /// once per destination is how a caller says so.
    pub fn from(mut self, start: DialogueNodeId, arriving: &BDDFunction) -> Self {
        self.beginnings.insert(start, arriving.clone());
        self
    }

    /// The order to take entries in.
    ///
    /// An order changes no settled answer - only how many pops reaching it takes - so a
    /// search handed this uses it rather than deciding whether to. See [`IterationOrder`].
    pub fn order(&self) -> &IterationOrder {
        &self.order
    }

    pub fn parents_of(&self, id: DialogueNodeId) -> &[DialogueNodeId] {
        self.parents.get(&id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn parents(&self) -> &HashMap<DialogueNodeId, Vec<DialogueNodeId>> {
        &self.parents
    }

    /// Whether anything is known that a backward pass could meet.
    pub fn can_meet(&self) -> bool {
        !self.beginnings.is_empty()
    }

    /// Whether a search could arrive at `id` holding one of `wanted`.
    ///
    /// The meet. `wanted` is a backward set - what a search must hold arriving at `id` for
    /// the target to still be reachable - so this asks whether what a search holds on
    /// arrival at one of its beginnings is in it.
    ///
    /// An `Err` from the manager is out of room, and answering "no meet" on it is the safe
    /// direction: the pass carries on and ends on its own budget rather than reporting a
    /// proof it does not have.
    pub fn meets(&self, id: DialogueNodeId, wanted: &BDDFunction) -> bool {
        self.beginnings
            .get(&id)
            .and_then(|arriving| arriving.and(wanted).ok())
            .map(|both| both.satisfiable())
            .unwrap_or(false)
    }
}
