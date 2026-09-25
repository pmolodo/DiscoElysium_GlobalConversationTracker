// SPDX-License-Identifier: MIT
//! The question the look-ahead actually asks, answered one target at a time.
//!
//! What the mod asks for is the best seen state among the entries a start can reach, and
//! nothing beyond the best there is. So the answer is a MAXIMUM over an ordered enum, and
//! the way to compute a maximum is not to compute the set it is a maximum of.
//!
//! This asks [`Backward`] about one candidate at a time and stops at the first one that
//! can be reached.
//!
//! ## Class before distance
//!
//! The obvious order is nearest first, and it is wrong. Proving a near `UnseenThisGame`
//! entry reachable says nothing about whether a far `UnseenAnyGame` one is, and the far
//! one is the answer if it is - `best` is a maximum, not a first sighting.
//!
//! So candidates are grouped by seen state class, best class first, and only WITHIN a class
//! sorted by how far away they are. The first candidate proved reachable in a class ends
//! the search, because every better class has already been refused entirely. A class every
//! candidate of which is refused drops to the next one down.
//!
//! Distance is the link distance, guards ignored. It is a heuristic about which question
//! is cheap to answer, not a claim about reachability, so it can be as rough as it likes:
//! a near entry has a shorter chain of guards in front of it and a smaller backward
//! fixed point, and that is the whole of the reasoning.
//!
//! ## What it costs when the answer is no
//!
//! One fixed point per candidate. That is what [`Budget`] exists for: a group with a long
//! candidate list, every one of them unreachable, has to pay for every refusal separately,
//! and it is the shape where this costs most.

use std::collections::{HashMap, HashSet, VecDeque};

use oxidd::BooleanFunction;
use oxidd::bdd::BDDFunction;

use crate::core::types::{DialogueNodeId, StartBranch};
use crate::graph::LookAheadGraph;
use crate::symbolic::guard_formula::GuardCompiler;
use crate::symbolic::reachability::{Reachability, never_displays};
use crate::world::ILookAheadWorld;

/// Why a search stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoppedBy {
    /// Every candidate was asked about, so the answer is final.
    Nothing,
    /// A census had all the findings it came for, so the answer is a lower bound.
    ///
    /// The cap counts candidates PROVED UNREACHABLE rather than candidates asked about -
    /// see [`Classify::verdict`]. It is the caller's appetite running out rather than the
    /// search failing, and it leaves a caveat behind: an unasked candidate might have
    /// carried a better class.
    Targets,
    /// The time budget ran out.
    Time,
    /// A backward pass could not finish - out of nodes, or out of its own budget.
    Incomplete,
}

/// Where a search begins: one or more entries, and the states it holds arriving at them.
///
/// TWO SHAPES, AND THEY ARE THE SAME QUESTION ASKED FROM DIFFERENT PLACES.
///
/// An ordinary search begins at its start, holding the world's seed - what it holds
/// ARRIVING there, before that entry's own guard, cost or actions. One entry, one set.
///
/// A search about one outcome of a rolled start begins at the start's CHILDREN, holding
/// what entering the start by that outcome left. It cannot begin at the check itself: a
/// backward set there answers about either roll, since the pre-image unions both ways in,
/// and this is exactly the question that needs them apart.
pub struct Where {
    at: Vec<DialogueNodeId>,
    holding: BDDFunction,
    /// Whether the manager filled while working out where this search begins.
    ///
    /// A caller that sees it MUST NOT read the rest: `holding` is then the empty set for
    /// want of nodes rather than because the outcome opens nothing, and the two look
    /// alike from here. Asking candidates from an empty position refuses every one of
    /// them on no evidence and settles, which is a wrong answer where the honest one is
    /// "the representation did not fit".
    out_of_nodes: bool,
}

/// A lower bound on choices between this option and each target. Guards can only
/// remove these routes; cut options cannot be entered, even after a loop.
pub fn choice_bounds(
    graph: &LookAheadGraph,
    position: &super::backward::Position,
    cut: &HashSet<DialogueNodeId>,
) -> HashMap<DialogueNodeId, usize> {
    let mut distances = HashMap::new();
    let mut pending = VecDeque::new();
    for &id in &position.entries {
        if !cut.contains(&id) {
            distances.insert(id, 0usize);
            pending.push_back((id, 0usize));
        }
    }
    while let Some((id, distance)) = pending.pop_front() {
        if distances.get(&id) != Some(&distance) {
            continue;
        }
        let Some(node) = graph.get(id) else { continue };
        let cost = usize::from(node.choice && id != position.option);
        for &child in &node.links {
            if cut.contains(&child) || graph.get(child).is_none() {
                continue;
            }
            let candidate = distance + cost;
            if distances
                .get(&child)
                .is_none_or(|previous| candidate < *previous)
            {
                distances.insert(child, candidate);
                if cost == 0 {
                    pending.push_front((child, candidate));
                } else {
                    pending.push_back((child, candidate));
                }
            }
        }
    }
    distances
}

impl Where {
    /// The states and entries from which this outcome is searched.
    pub fn position(&self, option: DialogueNodeId) -> super::backward::Position {
        super::backward::Position {
            option,
            entries: self.at.clone(),
            holding: self.holding.clone(),
        }
    }
    /// The starting position for this outcome of this start.
    pub fn of<'a>(
        graph: &LookAheadGraph,
        start: DialogueNodeId,
        branch: StartBranch,
        seed: &BDDFunction,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
    ) -> Self {
        if branch == StartBranch::Either {
            return Self {
                at: vec![start],
                holding: seed.clone(),
                out_of_nodes: false,
            };
        }

        let Some(holding) =
            Reachability::entry_states(graph, start, branch, seed, compiler, world, counter_cap)
        else {
            // NO ENTRIES, so nothing can be asked from here even by a caller that ignores
            // the flag. The set is the seed only because the field needs one; it is not an
            // answer, and `out_of_nodes` is what says so.
            return Self {
                at: Vec::new(),
                holding: seed.clone(),
                out_of_nodes: true,
            };
        };
        let at = graph
            .get(start)
            .map(|node| node.links.clone())
            .unwrap_or_default();

        Self {
            at,
            holding,
            out_of_nodes: false,
        }
    }

    /// Whether working out where this search begins ran the manager out of nodes.
    ///
    /// Nothing else here is worth reading once this is set - the position is empty for
    /// want of nodes, not because the outcome opens nothing.
    pub fn out_of_nodes(&self) -> bool {
        self.out_of_nodes
    }

    /// The entries this outcome actually OPENS, guards and costs considered.
    ///
    /// WHAT THE BASELINE IS MADE OF. A branch's answer is "does this outcome lead anywhere
    /// better than where it LANDS", and where it lands is this: the first non-group entries
    /// that can be entered holding what the outcome left. A group is walked through rather
    /// than to, as everywhere else - it is expanded in place and never scored.
    ///
    /// GUARDS ARE HONOURED HERE, unlike in `LookAheadGraph::best_linked_class`. A cheap
    /// over-approximation is right when the question is whether to spend a search; it is
    /// wrong for a baseline, where naming a destination nothing can reach would raise the
    /// bar a real search has to clear and cost a marker.
    ///
    /// RUNNING OUT OF NODES EMPTIES THE ANSWER AND SETS [`Self::out_of_nodes`], because a
    /// short list here is not a smaller baseline - it is no baseline. A destination the
    /// walk never reached for want of room would lower the bar a real search has to clear,
    /// which is the same cost as naming one nothing can reach, in the other direction.
    pub fn destinations<'a>(
        &mut self,
        graph: &LookAheadGraph,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
    ) -> Vec<DialogueNodeId> {
        let mut found = Vec::new();
        let mut seen: Vec<DialogueNodeId> = Vec::new();
        let mut pending: VecDeque<(DialogueNodeId, BDDFunction)> = self
            .at
            .iter()
            .map(|id| (*id, self.holding.clone()))
            .collect();

        while let Some((id, arriving)) = pending.pop_front() {
            let Some(node) = graph.get(id) else { continue };
            let Some(entered) = Reachability::entry_states(
                graph,
                id,
                StartBranch::Either,
                &arriving,
                compiler,
                world,
                counter_cap,
            ) else {
                self.out_of_nodes = true;
                return Vec::new();
            };
            if !entered.satisfiable() {
                continue;
            }

            // A CHECK THIS SHEET FAILS IS WALKED THROUGH, exactly as a group is. The
            // outcome does not LAND on a line the player will never read, and naming one
            // here would raise the bar a real search has to clear - the same cost as
            // naming a destination nothing can reach.
            if !node.is_group && !never_displays(node, world) {
                if !found.contains(&id) {
                    found.push(id);
                }
                continue;
            }

            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            for child in &node.links {
                pending.push_back((*child, entered.clone()));
            }
        }

        found
    }

    /// What an earlier search may treat as already known, for the meet.
    ///
    /// The pairs are (entry, states arriving there), which is what [`Known::from`] takes.
    /// For an outcome that is its destinations, NOT the check: telling the backward driver
    /// that the check's pre-entry states are known would let a meet there prove a target
    /// reachable by the other roll.
    pub fn known_pairs(&self) -> Vec<(DialogueNodeId, &BDDFunction)> {
        self.at.iter().map(|id| (*id, &self.holding)).collect()
    }
}
