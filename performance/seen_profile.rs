// SPDX-License-Identifier: MIT
//! The percentage-seen profile a matrix row is measured under, and the draw behind it.
//!
//! ## Why this is a module rather than a copy in each file
//!
//! The same reason `menu_profile.rs` is one. A row is identified by a conversation and a
//! profile name, and everything downstream - a resume, a `DEGCT_PROFILE=` on the command line,
//! two folders compared against each other - assumes that pair names ONE set of unseen
//! entries. Two copies of the draw is two chances for it to stop being one set, silently,
//! because nothing checks and the row would still have a plausible number in it.
//!
//! What lives here is the structural half, which needs no world, no guards and no diagram:
//! how deep each entry is from the start, which entries a profile may name, and which of
//! them a given percentage leaves unseen.

// The consumers use different halves - the matrix draws profiles, a structural measurement
// may only want the depths - and a warning on every build of the matrix would hide the ones
// worth reading.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;

/// How far each entry is from `start`, following links and ignoring guards.
///
/// An over-approximation of what any search can reach: a guard can refuse a link, it cannot
/// create one.
pub fn structurally_reachable(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
) -> HashMap<DialogueNodeId, usize> {
    let mut depth = HashMap::new();
    let mut queue = VecDeque::new();
    depth.insert(start, 0usize);
    queue.push_back(start);

    while let Some(id) = queue.pop_front() {
        let here = depth[&id];
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if graph.get(child).is_some() && !depth.contains_key(&child) {
                depth.insert(child, here + 1);
                queue.push_back(child);
            }
        }
    }

    depth
}

/// The entries a profile can be built from: reachable, not the start, and not groups.
///
/// GROUPS ARE EXCLUDED because the game never writes a group's SimStatus, so every group in
/// the database reads as never displayed and naming one as unseen would make a search
/// succeed instantly on a lie. `seen_state_search::candidates_from` excludes them for the same
/// reason.
///
/// DEEPEST FIRST, which is what makes `deepest-N` mean something; the identifiers break the
/// tie so the list is the same list on every run and every machine.
pub fn candidates(graph: &LookAheadGraph, start: DialogueNodeId) -> Vec<DialogueNodeId> {
    let depths = structurally_reachable(graph, start);
    let mut all: Vec<(DialogueNodeId, usize)> = depths
        .into_iter()
        .filter(|(id, _)| *id != start)
        .filter(|(id, _)| graph.get(*id).is_some_and(|node| !node.is_group))
        .collect();

    all.sort_unstable_by_key(|(id, depth)| {
        (std::cmp::Reverse(*depth), id.conversation_id, id.entry_id)
    });
    all.into_iter().map(|(id, _)| id).collect()
}

/// Which of `candidates` a `<p>pc-seen` profile leaves unseen.
///
/// The seed IS the percentage, as de-raed asks: reproducible, and different for every row so
/// two rows are not accidentally the same draw.
pub fn percent_unseen(candidates: &[DialogueNodeId], percent: u32) -> HashSet<DialogueNodeId> {
    let mut rng = Rng::new(percent as u64);
    let mut shuffled = candidates.to_vec();

    // Fisher-Yates, so every subset of the right size is equally likely. Taking the first n
    // of a sorted list after a partial shuffle would not be.
    for i in (1..shuffled.len()).rev() {
        let j = (rng.next() % (i as u64 + 1)) as usize;
        shuffled.swap(i, j);
    }

    let seen = (shuffled.len() * percent as usize) / 100;
    shuffled.into_iter().skip(seen).collect()
}

/// A small deterministic generator, so a row is the same row on every machine.
///
/// Written out rather than taken from a crate: what is wanted is repeatability across runs
/// and platforms, and a named algorithm with the arithmetic in view gives that without a
/// dependency whose version could change the draw underneath a recorded measurement.
/// This is xorshift64*, which is more than good enough for choosing which entries to mark.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        // Zero is a fixed point of xorshift, so it can never be the state.
        Self(seed.wrapping_mul(2685821657736338717).max(1))
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(2685821657736338717)
    }
}
