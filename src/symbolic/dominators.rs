// SPDX-License-Identifier: MIT
//! Which entries lie on EVERY path from a start, so a refusal about one refuses the rest.
//!
//! ## What it is for
//!
//! `t` dominates `u` from `s` when every path from `s` to `u` passes through `t`. The
//! backward driver turns that into free answers, and only in one direction:
//!
//! A NO ABOUT `t` REFUSES EVERY `u` THAT `t` DOMINATES. Write `B_x(s)` for the states at
//! `s` from which `x` is still reachable. Any run that reaches `u` reached `t` on the way,
//! so `B_u(s)` is contained in `B_t(s)`; if nothing arriving at `s` is in `B_t(s)` then
//! nothing is in `B_u(s)` either, and `u` is refused with no fixed point at all.
//!
//! The other direction is sound and worthless. A yes about `u` implies a yes about `t`, and
//! the driver stops at the first candidate it proves, so it was never going to ask.
//!
//! ## Over the LINK graph, and that is the safe way round
//!
//! Guards are ignored here. Real paths are a subset of link paths, so an entry on every
//! LINK path to `u` is on every real path to it: ignoring guards can only report FEWER
//! dominators than hold, never more. Taking dominance over some guard-narrowed graph would
//! be the unsound direction, refusing entries a search could genuinely reach.
//!
//! ## Why the simple algorithm
//!
//! The iterative Cooper-Harvey-Kennedy fixed point rather than Lengauer-Tarjan. 1,372 of
//! the game's 1,422 groups hold about forty-three entries and the largest is under five
//! thousand - what `group_census` counted, and `performance/removed_tools.md` says how to
//! count it again - and building every group's tree in the game plus every candidate list
//! takes about a second and a half (de-kqgq).
//! The fast algorithm is a page of machinery bought with nothing to spend it on.
//!
//! ## What it is worth
//!
//! Structurally, over the whole game (de-kqgq): 87.7
//! per cent of the candidates in a driver's list have a strict dominator EARLIER in that
//! same list. That is the ceiling - it is the saving on a row that refuses every candidate,
//! and an over-estimate on one that finds something, since a yes ends the search anyway.
//!
//! MEASURED THROUGH THE DRIVER, de-rn59.4, on the four groups the refusals concentrate in,
//! against the recorded rows of `performance/logs/whole-game` at the same allowance:
//!
//! ```text
//!   conv     profile   cands    verdict   asked before   asked after
//!    640   25pc-seen    1798  not-there           1798            93
//!    825   50pc-seen     599  not-there            599           120
//!    587   50pc-seen     245  not-there            245            44
//!     29   25pc-seen    1312      found            246            36
//!
//!   fixed points over all 28 rows: 3,918 before, 945 after - 75.9 per cent saved
//! ```
//!
//! AND EVERY VERDICT IS UNCHANGED, which is the half that matters more. This makes a search
//! cheaper and must never make it answer differently; a differing verdict is a bug, not a
//! saving, and the comparison above is written to show the verdict beside the count for
//! exactly that reason.

use std::collections::{HashMap, HashSet};

use crate::core::types::DialogueNodeId;
use crate::graph::LookAheadGraph;

/// The immediate dominator of every entry reachable from a set of starts.
///
/// ## Indices rather than identifiers, and a VIRTUAL ROOT above the starts
///
/// Entries are numbered by their postorder position, and one more number is added above all
/// of them for a root that is not an entry at all. Both halves earn their keep.
///
/// THE VIRTUAL ROOT IS WHAT MAKES SEVERAL STARTS SOUND, and leaving it out is a real bug
/// rather than an inelegance. One outcome of a rolled check begins at all of the check's
/// children at once, and there is no entry above them that a run must have passed through.
/// With each start its own root, two starts' chains have nothing in common - and
/// `intersect`, asked to meet two chains that never meet, has to answer SOMETHING. Its
/// answer was one of the two starts, which claims a dominator the search does not have, in
/// the one direction that would refuse a genuinely reachable entry. Given a root above
/// them the chains meet there, at a number that is not an entry and that `immediate` never
/// hands back.
///
/// THE INDICES MAKE `intersect` COMPARE POSITIONS DIRECTLY, which is what the algorithm is
/// written in terms of, instead of going through a hash lookup per step of a walk that
/// happens per parent per pass.
pub struct Dominators {
    /// Postorder: `order[i]` is the entry numbered `i`. The virtual root is `order.len()`
    /// and has no entry.
    order: Vec<DialogueNodeId>,
    /// Where each entry sits in `order`.
    at: HashMap<DialogueNodeId, usize>,
    /// Each number's immediate dominator, `None` until the fixed point reaches it. The
    /// virtual root is its own, which is what ends a walk.
    idom: Vec<Option<usize>>,
}

impl Dominators {
    /// Works out the tree over the links reachable from `starts`.
    pub fn of(graph: &LookAheadGraph, starts: &[DialogueNodeId]) -> Self {
        let order = postorder(graph, starts);
        let at: HashMap<DialogueNodeId, usize> =
            order.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        let root = order.len();

        // Numbers increase towards the root, so the virtual one is above everything.
        let mut idom: Vec<Option<usize>> = vec![None; root + 1];
        idom[root] = Some(root);

        let mut parents: Vec<Vec<usize>> = vec![Vec::new(); root];
        for (i, &id) in order.iter().enumerate() {
            let Some(node) = graph.get(id) else { continue };
            for &child in &node.links {
                if let Some(&child) = at.get(&child) {
                    parents[child].push(i);
                }
            }
        }
        // THE STARTS HANG OFF THE VIRTUAL ROOT, and off nothing else. A link INTO a start
        // stays in its parent list too - a start can sit on a cycle - and the meet of the
        // two is the virtual root, which is the honest answer: an entry reachable both from
        // the start and around a loop back into it is dominated by neither.
        for start in starts {
            if let Some(&start) = at.get(start) {
                parents[start].push(root);
            }
        }

        // Reverse postorder, which is what makes the fixed point converge in a pass or two
        // rather than in as many passes as the graph is deep.
        let mut changed = true;
        while changed {
            changed = false;
            for i in (0..root).rev() {
                let mut new: Option<usize> = None;
                for &parent in &parents[i] {
                    // A parent the fixed point has not reached yet contributes nothing this
                    // pass and will on the next.
                    if idom[parent].is_none() {
                        continue;
                    }
                    new = Some(match new {
                        None => parent,
                        Some(current) => intersect(parent, current, &idom),
                    });
                }
                if new.is_some() && idom[i] != new {
                    idom[i] = new;
                    changed = true;
                }
            }
        }

        Self { order, at, idom }
    }

    /// The entry immediately above `id`, or `None` where `id` is a start or unreachable.
    ///
    /// STRICT, and the virtual root is never returned: a start's dominator is that root,
    /// which is not an entry, so a start reports nothing above it.
    pub fn immediate(&self, id: DialogueNodeId) -> Option<DialogueNodeId> {
        let above = self.idom[*self.at.get(&id)?]?;
        self.order.get(above).copied()
    }

    /// Every strict dominator of `id`, nearest first.
    pub fn above(&self, id: DialogueNodeId) -> impl Iterator<Item = DialogueNodeId> + '_ {
        let mut walk = self.immediate(id);
        std::iter::from_fn(move || {
            let node = walk?;
            walk = self.immediate(node);
            Some(node)
        })
    }

    /// Whether `cut` lies on every path from the starts to `id`.
    pub fn dominates(&self, cut: DialogueNodeId, id: DialogueNodeId) -> bool {
        self.above(id).any(|node| node == cut)
    }

    /// Whether `id` is reachable from the starts along links.
    pub fn reaches(&self, id: DialogueNodeId) -> bool {
        self.at.contains_key(&id)
    }

    /// How many entries the tree covers, which is what was reachable from the starts.
    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// The reachable entries in postorder, so a node follows everything below it.
///
/// ITERATIVE, because a group can be thousands of entries deep and the recursive form is
/// the same stack overflow de-fpax was about.
fn postorder(graph: &LookAheadGraph, starts: &[DialogueNodeId]) -> Vec<DialogueNodeId> {
    let mut order: Vec<DialogueNodeId> = Vec::new();
    let mut seen: HashSet<DialogueNodeId> = HashSet::new();
    let mut stack: Vec<(DialogueNodeId, usize)> = Vec::new();

    for &start in starts {
        if graph.get(start).is_none() || !seen.insert(start) {
            continue;
        }
        stack.push((start, 0));
        while let Some((id, next)) = stack.pop() {
            let links = graph
                .get(id)
                .map(|node| node.links.clone())
                .unwrap_or_default();
            if next < links.len() {
                stack.push((id, next + 1));
                let child = links[next];
                if graph.get(child).is_some() && seen.insert(child) {
                    stack.push((child, 0));
                }
            } else {
                order.push(id);
            }
        }
    }

    order
}

/// The nearest number that dominates both, by walking each up until they meet.
///
/// Postorder numbers increase towards the root, so the LOWER of the two is the deeper one
/// and is the one to lift. Every walk terminates at the virtual root, which is its own
/// dominator and above everything - so two chains ALWAYS meet, and there is no case where
/// this has to invent an answer.
fn intersect(mut a: usize, mut b: usize, idom: &[Option<usize>]) -> usize {
    while a != b {
        while a < b {
            match idom[a] {
                Some(parent) if parent != a => a = parent,
                // Unsettled this pass. The next one lifts it; meanwhile the other side is
                // as close as this gets.
                _ => return b,
            }
        }
        while b < a {
            match idom[b] {
                Some(parent) if parent != b => b = parent,
                _ => return a,
            }
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_graph::{Entry, GraphBuilder, node};

    /// A graph whose entry `id` links to `children`. Guards and actions are left empty:
    /// dominance is over the LINKS, and a fixture carrying guards would suggest otherwise.
    fn graph_of(links: &[(i32, &[i32])]) -> LookAheadGraph {
        let mut builder = GraphBuilder::new();
        for (id, children) in links {
            builder = builder.add(Entry::new(*id).links(children));
        }
        builder.build()
    }

    #[test]
    fn a_chain_dominates_all_the_way_down() {
        let graph = graph_of(&[(0, &[1]), (1, &[2]), (2, &[])]);
        let doms = Dominators::of(&graph, &[node(0)]);

        assert!(doms.dominates(node(1), node(2)));
        assert!(doms.dominates(node(0), node(2)));
        assert!(!doms.dominates(node(2), node(1)));
    }

    #[test]
    fn a_diamond_leaves_neither_branch_dominating_the_join() {
        // 0 -> 1 -> 3, 0 -> 2 -> 3. Both routes reach 3, so neither 1 nor 2 is on every
        // path to it - which is the case a reachability rule would get wrong.
        let graph = graph_of(&[(0, &[1, 2]), (1, &[3]), (2, &[3]), (3, &[])]);
        let doms = Dominators::of(&graph, &[node(0)]);

        assert!(!doms.dominates(node(1), node(3)));
        assert!(!doms.dominates(node(2), node(3)));
        assert!(doms.dominates(node(0), node(3)));
    }

    #[test]
    fn an_entry_reached_through_one_neck_is_dominated_by_it() {
        // The hub shape: 0 -> 1, and everything past 1 goes through it.
        let graph = graph_of(&[(0, &[1]), (1, &[2, 3]), (2, &[4]), (3, &[4]), (4, &[])]);
        let doms = Dominators::of(&graph, &[node(0)]);

        for entry in [2, 3, 4] {
            assert!(
                doms.dominates(node(1), node(entry)),
                "1 should dominate {entry}"
            );
        }
        assert!(!doms.dominates(node(2), node(4)));
    }

    #[test]
    fn a_cycle_does_not_make_its_members_dominate_each_other() {
        // 1 and 2 are a component: each is reachable from the other, and neither is on
        // every path to the other, because the entry into the loop is 0.
        let graph = graph_of(&[(0, &[1, 2]), (1, &[2]), (2, &[1])]);
        let doms = Dominators::of(&graph, &[node(0)]);

        assert!(!doms.dominates(node(1), node(2)));
        assert!(!doms.dominates(node(2), node(1)));
    }

    #[test]
    fn nothing_dominates_itself() {
        let graph = graph_of(&[(0, &[1]), (1, &[])]);
        let doms = Dominators::of(&graph, &[node(0)]);

        assert!(!doms.dominates(node(0), node(0)));
        assert!(!doms.dominates(node(1), node(1)));
        assert_eq!(
            doms.immediate(node(0)),
            None,
            "a root has no strict dominator"
        );
    }

    #[test]
    fn two_starts_are_two_roots_and_neither_dominates_the_others_side() {
        // What a rolled check's outcome looks like: the search begins at 1 and 2 at once,
        // and 0 above them is NOT something a run must have passed through.
        let graph = graph_of(&[(0, &[1, 2]), (1, &[3]), (2, &[3]), (3, &[])]);
        let doms = Dominators::of(&graph, &[node(1), node(2)]);

        assert!(
            !doms.dominates(node(1), node(3)),
            "3 is reachable from 2 as well"
        );
        assert!(!doms.dominates(node(2), node(3)));
        assert_eq!(doms.immediate(node(1)), None);
        assert_eq!(doms.immediate(node(2)), None);
    }

    #[test]
    fn an_unreachable_entry_is_not_in_the_tree() {
        let graph = graph_of(&[(0, &[1]), (1, &[]), (9, &[])]);
        let doms = Dominators::of(&graph, &[node(0)]);

        assert_eq!(doms.immediate(node(9)), None);
        assert!(!doms.dominates(node(0), node(9)));
        assert_eq!(doms.len(), 2);
    }
}
