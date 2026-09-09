// SPDX-License-Identifier: MIT
//! What earlier searches over this group already worked out.
//!
//! Every symbolic search starts from nothing today: it rebuilds the parent map, recompiles
//! every guard and re-derives sets a previous run over the same group already had. This is
//! the part of that which can be handed on, and the part that cannot is worth naming just
//! as precisely - see de-cnjw.
//!
//! ## Two kinds of thing live here, and only one of them is a proof
//!
//! THE GRAPH'S SHAPE - the parent map, and the order to take entries in - is true of the
//! group and of nothing else. It depends on no target, no start and no world, so sharing it
//! can change no answer at all; what it saves is a driver working the same thing out once
//! per candidate. A FORWARD RUN'S SETS are the other kind, and everything below is about
//! what may and may not be read out of them.
//!
//! ## The meet, which is the interesting one
//!
//! A forward search records, per entry, the data states a search can hold there. A backward
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
//! `Reachability::states_at(e)` holds what a search has AFTER entering `e` - its guard
//! tested, its cost paid, its actions applied - because that is what gets handed to e's
//! children. `Backward::states_at(c)` holds what a search must have ON ARRIVAL at `c`,
//! before c's own guard.
//!
//! So the two do not meet at an entry, they meet across an EDGE: for a link `e -> c`, what
//! the forward run can hand from `e` is exactly what the backward run wants to see at `c`.
//! Meeting them at the same entry instead would compare a post-state against a pre-state
//! and answer a question nobody asked.
//!
//! The start is the one entry with no incoming edge to meet on, and the seed stands in for
//! it: the seed is what the search holds arriving at the start, which is the same shape as
//! everything else this compares.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use oxidd::BooleanFunction;
use oxidd::bdd::BDDFunction;

use crate::core::types::DialogueNodeId;
use crate::graph::graph::LookAheadGraph;
use crate::symbolic::order::IterationOrder;
use crate::symbolic::reachability::Reachability;

/// What a settled forward run says can arrive at one entry.
#[derive(Clone)]
enum Arriving {
    /// Exactly these states, and no others.
    Bounded(BDDFunction),
    /// None at all: the search provably never gets here.
    Nothing,
    /// No settled run, or the manager ran out of room working it out. Bounds nothing.
    Unknown,
}

/// The group's shape and whatever a forward run has already established about it.
pub struct Known {
    /// Every entry's incoming links, which is the edge direction a backward pass walks.
    ///
    /// TARGET-INDEPENDENT, and rebuilt from scratch by every backward pass today. A driver
    /// that asks about forty candidates walks the whole graph forty times to build the
    /// same map.
    ///
    /// AND START-INDEPENDENT TOO, which is what the handle is for: see [`GroupShape`], and
    /// the menu of twenty-four options that used to build twenty-four of these.
    parents: Arc<HashMap<DialogueNodeId, Vec<DialogueNodeId>>>,
    /// The order to take entries in, so that a loop is finished before what follows it.
    ///
    /// TARGET-INDEPENDENT AND DIRECTION-INDEPENDENT, for the same reason the parent map is
    /// and with the same saving: it is a fact about the graph's shape, and a driver asking
    /// about forty candidates would otherwise work it out forty times. One rank serves both
    /// searches - see [`IterationOrder`] for why reversing the edges does not change it.
    order: IterationOrder,
    /// What a forward run left at each entry, AFTER that entry - so, what it can hand on.
    forward: HashMap<DialogueNodeId, BDDFunction>,
    /// The entry a search begins at, and what it holds arriving there.
    start: Option<(DialogueNodeId, BDDFunction)>,
    /// What can arrive at an entry, worked out on demand from [`Self::forward`].
    ///
    /// A cache rather than state: every value in it is derivable from the fields above, and
    /// it exists because a backward pass asks about the same entry once per widening.
    arriving: RefCell<HashMap<DialogueNodeId, Arriving>>,
    /// Whether a settled forward run may narrow a backward pass at all.
    ///
    /// Off unless a caller asks. `portfolio::Budget::default` asks; see
    /// [`Self::restricted`].
    narrow: bool,
    /// Whether the forward run settled. False for a partial one, and for no run at all.
    ///
    /// Only a settled run bounds anything, which is what [`Self::arriving_at`] refuses to
    /// answer without. The meet does not need it - a partial run proves just as well - and
    /// keeping the two apart is what stops a proof and a bound being confused.
    forward_settled: bool,
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
/// Anything a forward run established. That is per start, per world and per budget, and it
/// is what [`Known`] adds on top - see [`Self::known_from`].
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
            forward: HashMap::new(),
            start: None,
            arriving: RefCell::new(HashMap::new()),
            narrow: false,
            forward_settled: false,
        }
    }
}

impl Known {
    /// The group's shape alone, with nothing established about it yet.
    ///
    /// Works the shape out for this one search. A caller running SEVERAL searches over one
    /// group - every option of a response menu is one - should build a [`GroupShape`] once
    /// and ask it for each of these instead.
    pub fn of(graph: &LookAheadGraph) -> Self {
        let shape = GroupShape::of(graph);
        Self {
            parents: shape.parents,
            order: shape.order,
            forward: HashMap::new(),
            start: None,
            arriving: RefCell::new(HashMap::new()),
            narrow: false,
            forward_settled: false,
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

    /// The same, plus where a search begins and what it holds when it does.
    pub fn from(mut self, start: DialogueNodeId, seed: &BDDFunction) -> Self {
        self.start = Some((start, seed.clone()));
        self
    }

    /// Lets a settled forward run narrow the backward passes told about it.
    pub fn pruning(mut self, on: bool) -> Self {
        self.narrow = on;
        self
    }

    /// The order to take entries in.
    ///
    /// An order changes no settled answer - only how many pops reaching it takes - so a
    /// search handed this uses it rather than deciding whether to. See [`IterationOrder`].
    pub fn order(&self) -> &IterationOrder {
        &self.order
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

    /// Everything a search could be holding when it arrives at `id`.
    ///
    /// ONLY FROM A SETTLED FORWARD RUN, and that restriction is the whole of the soundness
    /// argument. A settled run's sets are complete, so a state outside this can never be
    /// held here and dropping it changes no answer. A partial run's sets are a subset of
    /// that, and a state missing from one may simply not have been reached yet - pruning
    /// against it would refuse states that are genuinely reachable, which is how a marker
    /// gets lost.
    ///
    /// Across the incoming edges, for the reason the module explains: what the forward run
    /// holds at a parent is what that parent hands on, and it is what its child receives.
    /// The start receives the seed instead, having no incoming edge to receive anything on.
    ///
    /// CACHED, because a backward pass asks about the same entry once per widening and the
    /// union costs one disjunction per incoming edge.
    fn arriving_at(&self, id: DialogueNodeId) -> Arriving {
        if !self.forward_settled {
            return Arriving::Unknown;
        }
        if let Some(known) = self.arriving.borrow().get(&id) {
            return known.clone();
        }

        let mut union = match &self.start {
            Some((start, seed)) if *start == id => Some(seed.clone()),
            _ => None,
        };

        let mut no_room = false;
        for parent in self.parents_of(id) {
            let Some(held) = self.forward.get(parent) else {
                continue;
            };
            union = match union.take() {
                None => Some(held.clone()),
                // An `Err` is the manager out of room. Give up on bounding this entry
                // rather than bounding it by a union missing a disjunct, which would refuse
                // states that can be here.
                Some(sofar) => match sofar.or(held) {
                    Ok(both) => Some(both),
                    Err(_) => {
                        no_room = true;
                        break;
                    }
                },
            };
        }

        let answer = match union {
            _ if no_room => Arriving::Unknown,
            Some(bound) => Arriving::Bounded(bound),
            // A settled run reached no parent of this entry and it is not the start, so
            // NOTHING can arrive here at all. Stronger than a bound, and the case worth
            // having: a backward pass otherwise spends a fixed point on entries the search
            // provably never visits.
            None => Arriving::Nothing,
        };

        self.arriving.borrow_mut().insert(id, answer.clone());
        answer
    }

    /// Whether a settled forward run says NOTHING can arrive at this entry.
    ///
    /// The cheap half of pruning, and it costs no diagram work at all: an entry no reached
    /// parent hands anything to, and which is not the start, is one the search provably
    /// never visits.
    fn nothing_arrives(&self, id: DialogueNodeId) -> bool {
        if !self.forward_settled {
            return false;
        }
        if matches!(&self.start, Some((start, _)) if *start == id) {
            return false;
        }
        !self
            .parents_of(id)
            .iter()
            .any(|parent| self.forward.contains_key(parent))
    }

    /// `states` narrowed to what could actually be held on arrival at `id`.
    ///
    /// The identity where nothing is known, or where the forward run did not settle.
    ///
    /// OFF UNLESS ASKED FOR, and the shipped portfolio asks: `portfolio::Budget::default`
    /// carries `pruning: true`. A caller that builds its own budget and leaves this alone
    /// gets the identity, which is why the field is a request rather than a policy.
    ///
    /// SOUND, and checked rather than argued: tests/backward_oracle.rs asks every target of
    /// every group it can check both ways twice, plain and pruned, with the explicit search
    /// as referee, and the two have never differed.
    ///
    /// AND SELF-GUARDING, which is what makes asking for it cheap. Nothing is narrowed
    /// without [`Self::forward_settled`], so a group whose forward run halted or ran out of
    /// budget behaves exactly as it would with this off - no bound, no intersection, no
    /// cost. `measurements/settles_within.rs` is why that is worth having: at the fifty
    /// milliseconds the slice gets, 119 of 120 ordinary groups settle and 25 of the 50 that
    /// span conversations do.
    pub fn restricted(&self, id: DialogueNodeId, states: BDDFunction) -> BDDFunction {
        if !self.narrow {
            return states;
        }
        if self.nothing_arrives(id) {
            return self.empty().unwrap_or(states);
        }

        match self.arriving_at(id) {
            // An `Err` is the manager out of room; keep the wider set, which is the safe
            // direction - it can only make the pass do more work, never less.
            Arriving::Bounded(bound) => states.and(&bound).unwrap_or(states),
            Arriving::Nothing => self.empty().unwrap_or(states),
            Arriving::Unknown => states,
        }
    }

    /// The empty set, built from a formula this already holds.
    ///
    /// Built rather than stored because `Known` has no variables of its own - it holds
    /// formulas somebody else made, and any of them can be turned into the empty set.
    fn empty(&self) -> Option<BDDFunction> {
        let any = self
            .start
            .as_ref()
            .map(|(_, seed)| seed)
            .or_else(|| self.forward.values().next())?;
        any.and(&any.not().ok()?).ok()
    }

    /// Whether a search could arrive at `id` holding one of `wanted`.
    ///
    /// The meet. `wanted` is a backward set - what a search must hold arriving at `id` for
    /// the target to still be reachable - so this asks whether anything already known to
    /// arrive there is in it. Across the incoming edges, because what the forward run holds
    /// at a parent is what that parent hands on; and at the start, against the seed.
    ///
    /// An `Err` from the manager is out of room, and answering "no meet" on it is the safe
    /// direction: the pass carries on and ends on its own budget rather than reporting a
    /// proof it does not have.
    pub fn meets(&self, id: DialogueNodeId, wanted: &BDDFunction) -> bool {
        if let Some((start, seed)) = &self.start {
            if *start == id
                && seed
                    .and(wanted)
                    .map(|both| both.satisfiable())
                    .unwrap_or(false)
            {
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
