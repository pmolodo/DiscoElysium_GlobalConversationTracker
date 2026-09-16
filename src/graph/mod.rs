// SPDX-License-Identifier: MIT

pub mod node;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use crate::core::state::StateSymbols;
use crate::core::types::{DialogueNodeId, Novelty};
use crate::graph::node::LookAheadNode;

/// The dialogue entries the look-ahead can walk, indexed by id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookAheadGraph {
    nodes: HashMap<DialogueNodeId, LookAheadNode>,
    /// The order [`Self::nodes`] yields entries in: by conversation, then by entry.
    ///
    /// ## Why a graph carries a list of its own keys
    ///
    /// BECAUSE A HASH MAP'S ORDER IS A FACT ABOUT THE PROCESS, and several things built by
    /// sweeping this graph inherit it - the layout's threshold narrowing, the SCC
    /// decomposition, the order guards are compiled and therefore the order diagram nodes
    /// are created. None of that changes an ANSWER, and measuring says so: three processes
    /// asked the same question of conversation 1030 returned the same verdict, the same
    /// `by` and the same `asked` every time. What moved was the work done to get there -
    /// 126,106, 126,588 and 126,148 diagram nodes - and, once, how deep the recursion went:
    /// the same code overflowed the main thread's stack in one process and not the next.
    ///
    /// So the cost of leaving it was a search whose cost is not reproducible, a `nodes`
    /// column that cannot be compared between runs, and a stack depth that varies for no
    /// reason anybody can see. See de-12wr.3 and `measurements/nodes_repeat.rs`, which is
    /// the experiment.
    ///
    /// A SORTED KEY LIST RATHER THAN A `BTreeMap`, because `get` is the hot operation here
    /// and iteration is not: the sweeps above happen once per group, and lookups happen
    /// per link followed. This keeps both at what they were.
    order: Vec<DialogueNodeId>,
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

        // SORTED ONCE, HERE, so every sweep of the graph sees the same order in every
        // process. See the field for what a hash map's order was costing.
        let mut order: Vec<DialogueNodeId> = map.keys().copied().collect();
        order.sort_unstable_by_key(|id| (id.conversation_id, id.entry_id));

        let mut choices = HashSet::new();
        for parent in map.values() {
            let mut pending = parent.links.clone();
            let mut visited = HashSet::new();
            let mut offered = HashSet::new();
            while let Some(id) = pending.pop() {
                if !visited.insert(id) {
                    continue;
                }
                let Some(node) = map.get(&id) else { continue };
                if node.is_group {
                    pending.extend(node.links.iter().copied());
                } else if node.player {
                    offered.insert(id);
                }
            }
            if offered.len() > 1 {
                choices.extend(offered);
            }
        }
        for id in choices {
            map.get_mut(&id).expect("offered entry exists").choice = true;
        }

        // THE VARIABLES THE GROUP READS, fixed here with the slots and for the same reason: a
        // search reads them and never adds one. Every plain slot, which the seed reads from
        // the world, and every variable a guard names, a flag a `FlagSet` names by a literal
        // included. Nothing else can be read - see `VariableRef`.
        let mut variables: Vec<String> = (0..symbols.count())
            .filter_map(|slot| symbols.name_of(slot))
            .filter(|name| crate::core::state::names_a_variable(name))
            .map(str::to_string)
            .collect();
        for node in map.values() {
            for part in node.guard.nodes() {
                match part.expression() {
                    crate::core::guard::GuardExpression::Variable(name) => {
                        variables.push(name.to_string());
                    }
                    crate::core::guard::GuardExpression::Call(function, arguments)
                        if crate::world::flag_query(function).is_some() =>
                    {
                        if let Some(crate::core::guard::GuardExpression::Literal(value)) =
                            arguments.only().map(|only| only.expression())
                            && value.kind() == crate::core::guard_value::GuardValueKind::Text
                        {
                            variables.push(value.text().to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
        symbols.declare_variables(variables);

        Ok(Self {
            nodes: map,
            order,
            symbols,
        })
    }

    pub fn symbols(&self) -> &StateSymbols {
        &self.symbols
    }

    pub fn count(&self) -> usize {
        self.nodes.len()
    }

    /// Every entry, by conversation and then by entry id.
    ///
    /// THE ORDER IS PART OF THE CONTRACT, not an accident of the storage - see
    /// [`Self::order`]. A caller that sweeps this to build something the search then follows
    /// can rely on two processes building the same thing.
    pub fn nodes(&self) -> impl Iterator<Item = &LookAheadNode> {
        self.order.iter().filter_map(|id| self.nodes.get(id))
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
    /// search when nothing does. `symbolic::answer` asks which class to hunt, because a
    /// search that stops at the best class PRESENT has found the best there is, while one
    /// stopping at the first entry that merely beats "seen" may have walked past a better
    /// one - and it was two different walks, answering these two questions differently,
    /// that made that gap possible.
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

        if let Some(node) = self.get(start)
            && consider(start, node)
        {
            return best;
        }

        expanded.insert(start);
        pending.push_back(start);

        while let Some(id) = pending.pop_front() {
            let Some(node) = self.get(id) else { continue };
            for &child_id in &node.links {
                let Some(child) = self.get(child_id) else {
                    continue;
                };

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
        write!(
            f,
            "LookAheadGraph({} nodes, {} slots)",
            self.nodes.len(),
            self.symbols.count()
        )
    }
}

#[cfg(test)]
mod best_linked_class_tests {
    use super::*;
    use crate::test_graph::{Entry, GraphBuilder, node};

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

#[cfg(test)]
mod iteration_order_tests {
    use super::*;
    use crate::test_graph::{Entry, GraphBuilder};

    /// Entries come back by conversation and then by entry id, whatever order they arrived.
    ///
    /// ## What this is protecting
    ///
    /// A search whose COST depends on the process it runs in. `nodes()` used to hand back a
    /// `HashMap`'s values, and the standard hasher is seeded per process - so the layout's
    /// threshold narrowing, the SCC decomposition and the order guards were compiled all
    /// followed a different order in every run. de-12wr.3 measured what that was worth:
    /// three processes asked the same question of conversation 1030 and all three answered
    /// it identically, having built 126,106, 126,588 and 126,148 diagram nodes to do it -
    /// and one of the three overflowed a stack the other two did not.
    ///
    /// The answers were never wrong, which is exactly why this needs a test rather than
    /// being noticed: nothing failed, the matrix's `nodes` column simply could not be
    /// compared between runs and nobody could say why.
    #[test]
    fn entries_come_back_in_a_stable_order() {
        let shuffled = GraphBuilder::new()
            .add(Entry::new(7))
            .add(Entry::new(1))
            .add(Entry::new(30))
            .add(Entry::new(2))
            .add(Entry::new(4))
            .build();

        let order: Vec<i32> = shuffled.nodes().map(|node| node.id.entry_id).collect();
        assert_eq!(
            order,
            vec![1, 2, 4, 7, 30],
            "entries should come back by id"
        );

        // NUMERIC, NOT LEXICOGRAPHIC, which is what the 30 is there to catch: sorted as text
        // it lands between 2 and 4, and a run ordered that way would be perfectly stable and
        // perfectly confusing to read against an id.
        assert_eq!(shuffled.nodes().count(), shuffled.count());
    }

    /// The same entries in a different arrival order build the same iteration order.
    #[test]
    fn the_order_does_not_depend_on_how_the_graph_was_built() {
        let forwards = GraphBuilder::new()
            .add(Entry::new(1))
            .add(Entry::new(2))
            .add(Entry::new(3))
            .build();
        let backwards = GraphBuilder::new()
            .add(Entry::new(3))
            .add(Entry::new(2))
            .add(Entry::new(1))
            .build();

        let one: Vec<DialogueNodeId> = forwards.nodes().map(|node| node.id).collect();
        let other: Vec<DialogueNodeId> = backwards.nodes().map(|node| node.id).collect();
        assert_eq!(one, other);
    }
}
