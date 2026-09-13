// SPDX-License-Identifier: MIT
//! The adversarial menu profile the whole-menu measurements share.
//!
//! ## Why this is a module rather than a paragraph repeated in each file
//!
//! `menu_residue` says of its own copy: "Restated rather than shared because an example
//! cannot import another example's helpers, and the rule is six lines." The first half is
//! not quite true - an example can pull a module in with `#[path]`, which is how every
//! measurement here already reaches `tests/common` - and the second half stopped being true
//! once the profile grew a reachability filter and a deterministic tie-break. Three files
//! wanting the same forty lines is what a module is for.
//!
//! ## What the profile IS, and why it has to be this one
//!
//! ADVERSARIAL. Exactly the structurally deepest entries are unseen and everything else is
//! seen, so every start that can reach one has something better than its own class beyond
//! it, `bridge::class_worth_hunting` refuses none of them, and every start pays for a real
//! search.
//!
//! THE ALTERNATIVE MEASURES NOTHING, and it is an easy mistake to make: the first cut of
//! `menu_residue` took the SHALLOWEST entries in the group and got twenty-four refusals and
//! zero candidates, which reads in a closing line exactly like a clean run. A percentage
//! profile has the same problem for the same reason - it refuses most starts before a
//! diagram is touched.
//!
//! Depth is by EDGE ANALYSIS ALONE - links followed, guards ignored - so it over-approximates
//! reachability, which makes the quarry at least structurally fair.

// The consumers use different halves - some want the novelty function, some build their own
// from `unseen` - and a warning on every build of every one of them would hide the ones
// worth reading. The same reason `seen_profile.rs` carries this.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;

/// A menu to ask about: which entries are unseen, and which starts to ask.
pub struct MenuProfile {
    /// The deepest entries, which are the only unseen ones.
    pub unseen: HashSet<DialogueNodeId>,
    /// Starts that can reach one of them, shallowest first - which is what an option in a
    /// response menu is: an entry with the group's depth still in front of it.
    pub starts: Vec<DialogueNodeId>,
}

impl MenuProfile {
    /// Builds the profile for one group, or `None` if every start would be refused.
    ///
    /// `None` rather than an empty profile, because a run of refusals measures nothing and
    /// a caller has to be able to tell that apart from a group that was simply fast.
    pub fn of(
        graph: &LookAheadGraph,
        root: DialogueNodeId,
        unseen_wanted: usize,
        starts_wanted: usize,
    ) -> Option<Self> {
        let ranked = deepest_first(graph, root);
        let unseen: HashSet<DialogueNodeId> = ranked.iter().take(unseen_wanted).copied().collect();
        if unseen.is_empty() {
            return None;
        }

        let reaching = can_reach(graph, &unseen);
        let starts: Vec<DialogueNodeId> = ranked
            .iter()
            .rev()
            .filter(|id| reaching.contains(*id) && !unseen.contains(*id))
            .copied()
            .take(starts_wanted)
            .collect();

        (!starts.is_empty()).then_some(Self { unseen, starts })
    }

    /// The novelty function this profile describes.
    pub fn novelty(
        &self,
    ) -> impl Fn(DialogueNodeId) -> lookahead_engine::core::types::Novelty + '_ {
        use lookahead_engine::core::types::Novelty;
        move |id| {
            if self.unseen.contains(&id) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        }
    }
}

/// Every entry reachable from `start` by links, deepest first.
fn deepest_first(graph: &LookAheadGraph, start: DialogueNodeId) -> Vec<DialogueNodeId> {
    let mut depth: HashMap<DialogueNodeId, usize> = HashMap::new();
    let mut queue = VecDeque::from([(start, 0usize)]);
    depth.insert(start, 0);
    while let Some((id, here)) = queue.pop_front() {
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if graph.get(child).is_some() && !depth.contains_key(&child) {
                depth.insert(child, here + 1);
                queue.push_back((child, here + 1));
            }
        }
    }

    let mut ranked: Vec<DialogueNodeId> = depth
        .keys()
        .copied()
        // GROUP ENTRIES ARE NOT SCORED - they are walked through and never named as a
        // destination - so they make poor starts and worse quarry.
        .filter(|id| *id != start && graph.get(*id).map(|node| !node.is_group).unwrap_or(false))
        .collect();
    // DETERMINISTIC, so two arms of a comparison rank the same entries the same way.
    // DialogueNodeId is not Ord, so the tie-break is spelled out from its parts.
    ranked.sort_unstable_by_key(|id| {
        (
            std::cmp::Reverse(depth[id]),
            id.conversation_id,
            id.entry_id,
        )
    });
    ranked
}

/// Every entry from which some member of `unseen` is link-reachable.
fn can_reach(graph: &LookAheadGraph, unseen: &HashSet<DialogueNodeId>) -> HashSet<DialogueNodeId> {
    let mut parents: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
    for node in graph.nodes() {
        for &child in &node.links {
            parents.entry(child).or_default().push(node.id);
        }
    }

    let mut reaching: HashSet<DialogueNodeId> = HashSet::new();
    let mut queue: VecDeque<DialogueNodeId> = unseen.iter().copied().collect();
    while let Some(id) = queue.pop_front() {
        for &parent in parents.get(&id).map(|v| v.as_slice()).unwrap_or(&[]) {
            if reaching.insert(parent) {
                queue.push_back(parent);
            }
        }
    }
    reaching
}
