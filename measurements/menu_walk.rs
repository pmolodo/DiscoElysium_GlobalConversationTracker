// SPDX-License-Identifier: MIT
//! Successive menus along a walk through one group, and what each of them would ASK.
//!
//! ## Why this is a module rather than a function in one measurement
//!
//! Two measurements ask about the same population and their answers are multiplied
//! together. `candidate_recurrence`'s walk arm counts how many of a menu's asks an earlier
//! menu of the same walk already made - the ceiling on what a memo between requests could
//! ever be worth. `cacheable_asks` runs those same asks and counts how many of them a memo
//! could actually hold. The second number is a fraction OF the first, so the two have to be
//! about the same asks or the product means nothing.
//!
//! Kept in one place, that is true by construction. Kept in two, it is true until somebody
//! changes one walk.
//!
//! ## What a walk is
//!
//! The group in link order, without revisiting: every node with at least [`MIN_OPTIONS`]
//! non-group options is a menu, and passing a node marks it SEEN, so the unseen set shrinks
//! as the walk goes and later candidate lists are drawn against what the player has by then
//! read.
//!
//! NOT A PLAYER'S PATH, and worth saying rather than implying. A player takes one route and
//! this takes the whole group, so the menus are ordered by the graph rather than by choice.
//! What that biases is the ORDER two menus are met in, not whether they ask about the same
//! targets.
//!
//! ## What a menu asks, which is much less than what its options list
//!
//! Two reductions, both of them the driver's own and neither optional:
//!
//! DOMINANCE REFUSES MOST OF A CANDIDATE LIST FOR FREE. See [`minimal`]: a candidate with a
//! strict dominator earlier in the list is answered by that dominator's refusal and never
//! costs a fixed point, and de-kqgq measured 87.7 per cent of a whole-game run's candidates
//! to be in that position. A candidate that costs nothing is one there is nothing to
//! remember about.
//!
//! AND A MENU'S OPTIONS ASK ONE ANOTHER'S QUESTIONS. `bridge::answer_within` answers a whole
//! menu against one manager and one compiler, and 83 per cent of the asks are repeats within
//! the menu - so the asks are deduplicated across the options and attributed to the first
//! option that makes each. What is left is this menu's distinct questions, which is the only
//! thing a memo living BETWEEN requests is asked for.

use std::collections::HashSet;

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::symbolic::dominators::Dominators;
use lookahead_engine::symbolic::novelty_search::{Nearest, candidates_from};

/// How many non-group links a node needs before it counts as a menu.
///
/// THREE, because two options is not a menu anyone worries about and de-8hh2.13's realistic
/// figure is three.
pub const MIN_OPTIONS: usize = 3;

/// One menu of a walk: what the driver would ask there, and what is unread when it does.
pub struct Menu {
    /// What the menu asks: the option that makes each ask, and the target it is about.
    ///
    /// Deduplicated across the menu's own options, so a target appears once however many
    /// options list it.
    pub asks: Vec<(DialogueNodeId, DialogueNodeId)>,
    /// What is still unread by the time the walk reaches this menu.
    ///
    /// CARRIED RATHER THAN RECOMPUTED, because the walk is what moves it and nothing outside
    /// the walk can reconstruct it: the novelty function a menu's candidates were drawn
    /// against is a function of every node the walk passed on the way there. So this cannot
    /// live in the caller that reads it, however one-sided the reading is.
    ///
    /// A caller that only counts asks never looks at it, which is what the allow is for.
    #[allow(dead_code)]
    pub unseen: HashSet<DialogueNodeId>,
}

impl Menu {
    /// The distinct targets this menu asks about.
    pub fn targets(&self) -> impl Iterator<Item = DialogueNodeId> + '_ {
        self.asks.iter().map(|(_, target)| *target)
    }
}

/// The first `wanted` menus of a walk through `root`'s group.
///
/// `unseen` is what the player has NOT read when the walk begins - `seen_profile` draws one
/// for a percentage row - and the walk consumes it as it goes. TAKEN RATHER THAN DRAWN HERE
/// so that this module needs no profile of its own: a caller with a different notion of what
/// is unread gets the same walk without this having to know about it.
///
/// A menu whose options ask about nothing is not a disjoint menu, it is no menu, and is
/// neither returned nor counted towards `wanted`.
pub fn menus(
    graph: &LookAheadGraph,
    root: DialogueNodeId,
    mut unseen: HashSet<DialogueNodeId>,
    wanted: usize,
) -> Vec<Menu> {
    let mut found: Vec<Menu> = Vec::new();

    let mut visited: HashSet<DialogueNodeId> = HashSet::new();
    let mut queue: Vec<DialogueNodeId> = vec![root];
    visited.insert(root);

    while let Some(id) = queue.pop() {
        if found.len() >= wanted {
            break;
        }
        let Some(node) = graph.get(id) else { continue };

        // PASSING AN ENTRY READS IT. The seed carries what has been seen, so a walk that did
        // not do this would draw every menu's candidates against the same unseen set and
        // measure a session nobody has.
        unseen.remove(&id);

        let options: Vec<DialogueNodeId> = node
            .links
            .iter()
            .copied()
            .filter(|child| graph.get(*child).is_some_and(|node| !node.is_group))
            .collect();

        for &child in node.links.iter().rev() {
            if visited.insert(child) {
                queue.push(child);
            }
        }

        if options.len() < MIN_OPTIONS {
            continue;
        }

        let novelty = |id: DialogueNodeId| {
            if unseen.contains(&id) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        };

        let mut asks: Vec<(DialogueNodeId, DialogueNodeId)> = Vec::new();
        let mut here: HashSet<DialogueNodeId> = HashSet::new();
        for &start in &options {
            let list = candidates_from(graph, &[start], &novelty, Nearest::First);
            for target in minimal(graph, start, &list) {
                if here.insert(target) {
                    asks.push((start, target));
                }
            }
        }
        if asks.is_empty() {
            continue;
        }

        found.push(Menu {
            asks,
            unseen: unseen.clone(),
        });
    }

    found
}

/// The candidates an option ACTUALLY asks about, once dominance has refused what it can.
///
/// The same rule `novelty_search::search` applies, from the same relation: walking the list
/// in the driver's order, a candidate with a strict dominator EARLIER in it is answered by
/// that dominator's refusal and never costs a fixed point.
///
/// ASSUMES EVERY ASK IS A REFUSAL, which is the all-refusals row - the population de-kqgq
/// measured this to matter in. Where an option proves something early the driver stops, so
/// this over-counts, in the same direction and for the same reason the whole measurement
/// does.
pub fn minimal(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    ordered: &[DialogueNodeId],
) -> Vec<DialogueNodeId> {
    if ordered.len() < 2 {
        return ordered.to_vec();
    }
    let doms = Dominators::of(graph, &[start]);
    let mut refused: HashSet<DialogueNodeId> = HashSet::new();
    let mut asked = Vec::new();
    for &target in ordered {
        if doms.above(target).any(|above| refused.contains(&above)) {
            continue;
        }
        asked.push(target);
        refused.insert(target);
    }
    asked
}

/// How many of a walk's asks an EARLIER menu of the same walk already made.
///
/// The ceiling de-znov.1 reports, and the population `cacheable_asks` takes its fraction of.
/// `asked` is every menu's distinct asks summed, `repeat` how many of those a earlier menu
/// had already asked about, and the two agree with `asked` being `first` plus `repeat`.
#[derive(Debug, Default, Clone, Copy)]
pub struct Recurrence {
    pub menus: usize,
    pub asked: usize,
    pub first: usize,
    pub repeat: usize,
}

impl Recurrence {
    /// Counts one walk's menus.
    pub fn of(menus: &[Menu]) -> Self {
        let mut seen: HashSet<DialogueNodeId> = HashSet::new();
        let mut counted = Self::default();
        for menu in menus {
            counted.menus += 1;
            for target in menu.targets() {
                counted.asked += 1;
                if seen.insert(target) {
                    counted.first += 1;
                } else {
                    counted.repeat += 1;
                }
            }
        }
        counted
    }

    /// Adds another walk's count to this one.
    pub fn add(&mut self, other: &Self) {
        self.menus += other.menus;
        self.asked += other.asked;
        self.first += other.first;
        self.repeat += other.repeat;
    }
}
