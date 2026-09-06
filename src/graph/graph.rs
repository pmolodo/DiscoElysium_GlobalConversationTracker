// SPDX-License-Identifier: MIT
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use serde::{Deserialize, Serialize};

use crate::core::types::{DialogueNodeId, Novelty};
use crate::core::state::StateSymbols;
use crate::graph::node::LookAheadNode;

/// The dialogue entries the look-ahead can walk, indexed by id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookAheadGraph {
    nodes: HashMap<DialogueNodeId, LookAheadNode>,
    symbols: StateSymbols,
}

impl LookAheadGraph {
    /// Builds a graph, assigning the once slots and then freezing the symbol table.
    ///
    /// Interning happens HERE and nowhere later. A search reads slots by index and never
    /// creates one, which is what lets the symbol table be shared as `&StateSymbols`
    /// throughout the search, keeps the state vector's width fixed before the first
    /// state exists, and is a precondition for any symbolic encoding: a decision diagram
    /// has to fix its variable order up front, and cannot if a new variable can appear
    /// halfway through.
    pub fn new(nodes: Vec<LookAheadNode>, mut symbols: StateSymbols) -> Result<Self, String> {
        let mut map = HashMap::new();
        for mut node in nodes {
            if map.contains_key(&node.id) {
                return Err(format!("Duplicate dialogue entry {}", node.id));
            }
            if node.needs_once_slot() {
                node.once_slot = symbols.once(node.id) as i32;
            }
            map.insert(node.id, node);
        }
        Ok(Self { nodes: map, symbols })
    }

    pub fn symbols(&self) -> &StateSymbols {
        &self.symbols
    }

    pub fn count(&self) -> usize {
        self.nodes.len()
    }

    pub fn nodes(&self) -> impl Iterator<Item = &LookAheadNode> {
        self.nodes.values()
    }

    pub fn get(&self, id: DialogueNodeId) -> Option<&LookAheadNode> {
        self.nodes.get(&id)
    }

    /// The best novelty class carried by anything LINK-REACHABLE beyond `start`.
    ///
    /// GUARDS ARE IGNORED, which is the whole point: this is a few thousand pointer-follows
    /// against a search that is thousands of diagram operations, and it is run before every
    /// one of them. So it OVER-APPROXIMATES - a class it names may sit behind a guard
    /// nothing can open - and the two answers mean different things. `None`, or a class no
    /// better than a baseline, is DEFINITE: no walk of the links reaches anything better,
    /// so no search can either, and the question is settled without building a state. A
    /// class it does name is a maybe, and what the searches are then sent to establish.
    ///
    /// TWO CALLERS, ONE WALK, and they want the same fact for opposite reasons.
    /// `bridge::class_worth_hunting` asks whether anything outranks a baseline, and refuses to
    /// search when nothing does. `symbolic::portfolio` asks which class to hunt, because a
    /// forward pass that halts on the best class PRESENT has found the best there is, while
    /// one halting on the first entry that merely beats "seen" may have walked past a
    /// better one - and it was two different walks, answering these two questions
    /// differently, that made that gap possible.
    ///
    /// THE START IS A RESULT LIKE ANY OTHER, and is scored before a single link is walked.
    /// It reads as a wasted comparison for an ordinary option, where the baseline IS the
    /// start's own class and nothing can outrank itself - but for one outcome of a rolled
    /// check the baseline is where that OUTCOME LANDS, which sits below the check's own
    /// class whenever the outcome opens something already read. There the check entry
    /// genuinely outranks the baseline, and a walk that skipped it would refuse a search
    /// that had its answer in hand before it started. One rule, no special case, and the
    /// cost is one comparison in the case where it cannot fire.
    ///
    /// Groups are skipped, as everywhere else - the game never writes their SimStatus, so
    /// every group reads as never displayed and counting one would make every question
    /// succeed on a lie.
    ///
    /// Stops the moment it meets the top rung, since nothing outranks it.
    pub fn best_linked_class<F>(&self, start: DialogueNodeId, novelty: F) -> Option<Novelty>
    where
        F: Fn(DialogueNodeId) -> Novelty,
    {
        let mut expanded = HashSet::new();
        let mut pending = VecDeque::new();
        let mut best: Option<Novelty> = None;

        // Every entry this walk meets goes through the same three lines, the start
        // included. SCORED ON ARRIVAL AND EXPANDED ONCE are two different questions, and
        // answering them with one set is what made `reaches_potential_improvement` skip an
        // entry a link led back to.
        let mut consider = |id: DialogueNodeId, node: &LookAheadNode| -> bool {
            if node.is_group {
                return false;
            }
            let class = novelty(id);
            if class > Novelty::SeenThisGame && Some(class) > best {
                best = Some(class);
            }
            best == Some(Novelty::UnseenAnyGame)
        };

        if let Some(node) = self.get(start) {
            if consider(start, node) {
                return best;
            }
        }

        expanded.insert(start);
        pending.push_back(start);

        while let Some(id) = pending.pop_front() {
            let Some(node) = self.get(id) else { continue };
            for &child_id in &node.links {
                let Some(child) = self.get(child_id) else { continue };

                if consider(child_id, child) {
                    return best;
                }

                if expanded.insert(child_id) {
                    pending.push_back(child_id);
                }
            }
        }

        best
    }

    pub fn get_mut(&mut self, id: DialogueNodeId) -> Option<&mut LookAheadNode> {
        self.nodes.get_mut(&id)
    }

    pub fn contains(&self, id: DialogueNodeId) -> bool {
        self.nodes.contains_key(&id)
    }
}

impl fmt::Display for LookAheadGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LookAheadGraph({} nodes, {} slots)", self.nodes.len(), self.symbols.count())
    }
}

#[cfg(test)]
mod best_linked_class_tests {
    use super::*;
    use crate::test_graph::{node, Entry, GraphBuilder};

    /// A novelty function from two lists, so a fixture can say which entry is which class.
    fn classes<'a>(
        unseen_anywhere: &'a [i32],
        unseen_here: &'a [i32],
    ) -> impl Fn(DialogueNodeId) -> Novelty + 'a {
        move |id| {
            if unseen_anywhere.contains(&id.entry_id) {
                Novelty::UnseenAnyGame
            } else if unseen_here.contains(&id.entry_id) {
                Novelty::UnseenThisGame
            } else {
                Novelty::SeenThisGame
            }
        }
    }

    fn chain() -> LookAheadGraph {
        GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build()
    }

    #[test]
    fn everything_read_leaves_no_class_to_hunt() {
        let graph = chain();

        assert_eq!(graph.best_linked_class(node(0), classes(&[], &[])), None);
    }

    /// THE CASE THE WHOLE THING EXISTS FOR. A search that stopped at the first entry
    /// beating "seen" would stop at 1 and report unseen-here, which is a red marker where
    /// orange is right.
    #[test]
    fn the_top_rung_wins_even_when_a_lower_one_is_nearer() {
        let graph = chain();

        assert_eq!(
            graph.best_linked_class(node(0), classes(&[2], &[1])),
            Some(Novelty::UnseenAnyGame),
        );
    }

    #[test]
    fn the_rung_below_is_named_only_when_the_top_one_is_absent() {
        let graph = chain();

        assert_eq!(
            graph.best_linked_class(node(0), classes(&[], &[1, 2])),
            Some(Novelty::UnseenThisGame),
        );
    }

    /// THE START IS A RESULT LIKE ANY OTHER, scored before a link is walked.
    ///
    /// Whether that can ever fire is the caller's business, not this walk's: for an
    /// ordinary option the baseline is the start's own class, so it cannot outrank itself
    /// and the comparison is wasted. For one outcome of a rolled check the baseline is
    /// where that outcome LANDS, and the check entry can outrank it.
    #[test]
    fn the_start_is_a_candidate_for_itself() {
        let graph = chain();

        assert_eq!(
            graph.best_linked_class(node(0), classes(&[0], &[])),
            Some(Novelty::UnseenAnyGame),
        );
    }

    /// And no loop is needed for that, though one changes nothing.
    #[test]
    fn the_start_counts_once_a_link_leads_back_to_it() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[0]))
            .build();

        assert_eq!(
            graph.best_linked_class(node(0), classes(&[0], &[])),
            Some(Novelty::UnseenAnyGame),
        );
    }

    /// The game never writes a group's SimStatus, so every group reads as never displayed
    /// and counting one would make every question succeed on a lie.
    #[test]
    fn a_group_is_never_a_candidate() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).group())
            .build();

        assert_eq!(graph.best_linked_class(node(0), classes(&[1], &[])), None);
    }

    /// GUARDS ARE IGNORED, deliberately: this walk is what decides whether to spend a
    /// search at all, so it over-approximates and lets the search find out.
    #[test]
    fn a_guard_nothing_can_open_is_still_counted() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).guard(r#"Variable["shut"]"#).links(&[2]))
            .add(Entry::new(2))
            .build();

        assert_eq!(
            graph.best_linked_class(node(0), classes(&[2], &[])),
            Some(Novelty::UnseenAnyGame),
        );
    }

    /// A loop is walked once, and an entry nothing links to is not reached at all.
    #[test]
    fn it_walks_links_rather_than_the_whole_group() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[0]))
            .add(Entry::new(2))
            .build();

        assert_eq!(graph.best_linked_class(node(0), classes(&[2], &[])), None);
    }
}
