// SPDX-License-Identifier: MIT
//! The order the fixed points take their entries in.
//!
//! Both symbolic searches are the same shape: a worklist, one decision diagram per entry,
//! and a neighbour pushed back whenever its set grows. That is a DATAFLOW FIXED POINT over
//! the group's link graph, not a state-space search, and the thing that decides what one
//! costs is the order the worklist hands entries back.
//!
//! ## What FIFO costs, which is the whole reason this exists
//!
//! A queue ignores the graph's shape. Take a diamond - `a` links to `b` and `c`, both link
//! to `d`. FIFO pops `d` once for what `b` sent and again for what `c` sent, and each pop
//! is a real image or pre-image over a delta plus a union, not bookkeeping. Do that at
//! every join and an entry is re-popped about once per incoming edge that happens to
//! deliver at a different time.
//!
//! Take the entries in a TOPOLOGICAL order instead and `d` is popped once, with both
//! contributions already folded into its pending set. On an acyclic stretch that is one
//! pop per entry, which is the floor.
//!
//! Cycles are why a plain topological order is not available: a loop has no first entry.
//! The standard answer is to work on the CONDENSATION - contract each strongly connected
//! component to a point, which always leaves a DAG - and to stabilise a component before
//! touching anything downstream of it. That is reverse postorder over the condensation,
//! and it is the linear order a Bourdoncle weak topological ordering induces.
//!
//! ## One rank serves both directions
//!
//! Reversing every edge leaves the strongly connected components IDENTICAL - mutual
//! reachability does not care which way the arrows point - and simply reverses the
//! condensation DAG. A topological order of the reversed condensation is therefore the
//! reverse of a topological order of the original.
//!
//! So there is one rank per entry, and the direction is a matter of which end a search
//! reads it from: a forward pass pops the SMALLEST rank, a backward pass the LARGEST. That
//! is what lets this sit in [`crate::symbolic::known::Known`] beside the parent map and be
//! computed once for a group rather than once per candidate.
//!
//! ## It cannot change an answer
//!
//! A monotone fixed point is order-independent: the settled sets are the same whichever
//! order they were reached in, and only the number of pops differs. What an order does
//! change is a run that STOPS EARLY - which entry a budget-limited pass got to, or which
//! entry a meet landed on - and those were never promised to be any particular entry. It
//! changes those FOR THE BETTER, which is most of what it turned out to be worth: on
//! conversations 368 and 631 the forward search spends its memory budget unfinished under a
//! plain queue and settles under this. See `tests/iteration_order.rs`.
//!
//! ## There is no unordered path
//!
//! Both searches take an order, always. A caller with nothing to share gets one built for
//! it - that is one pass of Tarjan over the links, against a search that is thousands of
//! diagram operations - and a caller asking about many targets over one group should hand
//! the same [`IterationOrder`] to each, which is what [`crate::symbolic::known::Known`] is
//! for. The measurement that established the order was worth having is recorded in
//! `tests/iteration_order.rs`; the FIFO arrangement it was measured against is in the
//! history and not in the code, because keeping a slower path alive to be able to re-run a
//! settled comparison is how a slower path gets used by accident.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

use crate::core::types::DialogueNodeId;
use crate::graph::graph::LookAheadGraph;

/// A rank per entry, ordered so that a loop is finished before anything after it starts.
///
/// The guarantee, and the only one worth relying on: where a component can reach another
/// through links, every entry of the first ranks strictly below every entry of the second.
/// Entries WITHIN one component are contiguous and their order among themselves is not
/// meaningful - a cycle has no first entry, which is the reason components exist here.
pub struct IterationOrder {
    /// Each entry's component number, in topological order of the condensation.
    ///
    /// The primary key, and the part that carries the ordering guarantee: a component is
    /// finished before anything downstream of it begins. It depends on the links alone, so
    /// a driver asking about hundreds of candidates builds it once.
    component: HashMap<DialogueNodeId, u32>,
    /// Links traversed to arrive at each entry from the group's start, ignoring guards.
    ///
    /// The tie-break INSIDE a component, where a topological order has nothing to say - and
    /// on the groups that are mostly one cycle, that tie-break decides almost everything.
    /// Empty unless the order was built by [`Self::of_from`], and then the members of a
    /// component simply tie.
    ///
    /// MEASURED FROM THE START WHICHEVER WAY THE SEARCH RUNS, and that is a measured choice
    /// rather than an oversight. Measuring a backward pass's distances from its own target
    /// instead is the obvious refinement, it was built, and it lost: one group in five gained
    /// (368, 51 ms against 125), three lost slightly, the rest tied - against a per-candidate
    /// distance map where this is shared across every candidate. The near-ties are the
    /// giveaway: reversed-distance-from-start and distance-to-target coincide on a chain, and
    /// only 368 branches enough on its deep path to tell them apart. It is in the history.
    distance: HashMap<DialogueNodeId, u32>,
    /// How many strongly connected components the group has.
    components: usize,
    /// The largest of them, which is 1 exactly when the group is acyclic.
    largest: usize,
}

impl IterationOrder {
    /// Ranks every entry of `graph`.
    ///
    /// One pass of Tarjan's algorithm plus a sort of the roots, so O(V log V + E) and paid
    /// ONCE for a group - which is the point, since a driver asks about dozens of targets
    /// over the same graph.
    pub fn of(graph: &LookAheadGraph) -> Self {
        // DETERMINISTIC ROOTS. The graph holds its entries in a `HashMap`, so the order it
        // offers them is not stable between runs; Tarjan visits roots in whatever order it
        // is given and any of the resulting orders is correct, but two measurements of the
        // same group must not be measuring two different ones.
        let mut roots: Vec<DialogueNodeId> = graph.nodes().map(|node| node.id).collect();
        roots.sort_unstable_by_key(|id| (id.conversation_id, id.entry_id));

        let mut walk = Tarjan::new(roots.len());
        for root in roots {
            if !walk.index.contains_key(&root) {
                walk.from(graph, root);
            }
        }

        // TARJAN CLOSES A COMPONENT ONLY ONCE EVERYTHING IT CAN REACH IS CLOSED, so the
        // list it leaves is a reverse topological order of the condensation - sinks first.
        // Walking it backwards puts the sources first, which is what a forward pass wants
        // and, read from the other end, what a backward pass wants.
        let mut component_of = HashMap::with_capacity(walk.index.len());
        let mut largest = 0;
        for (number, component) in walk.components.iter().rev().enumerate() {
            largest = largest.max(component.len());
            for &member in component {
                component_of.insert(member, number as u32);
            }
        }

        Self {
            component: component_of,
            distance: HashMap::new(),

            components: walk.components.len(),
            largest,
        }
    }

    /// The same, told where a crawl begins, so distances can be worked out.
    ///
    /// One BFS
    /// over the links on top of the Tarjan pass, and target-independent like everything else
    /// here - so a driver asking about hundreds of candidates pays for it once.
    pub fn of_from(graph: &LookAheadGraph, start: DialogueNodeId) -> Self {
        let mut this = Self::of(graph);

        // The start is walked FROM without being recorded as arrived at, matching
        // `novelty_search::link_distances`: it gets a distance only if a link leads back to
        // it, and otherwise stays at zero, which is where a forward pass wants it anyway.
        let mut queue = VecDeque::from([(start, 0u32)]);
        while let Some((id, here)) = queue.pop_front() {
            let Some(node) = graph.get(id) else { continue };
            for &child in &node.links {
                if graph.get(child).is_none() || this.distance.contains_key(&child) {
                    continue;
                }
                this.distance.insert(child, here + 1);
                queue.push_back((child, here + 1));
            }
        }

        this
    }

    /// Where `id` falls: its component, then how far from the start it sits within it.
    ///
    /// COMPONENT IN THE HIGH HALF, DISTANCE IN THE LOW, so one integer orders by the
    /// component first and by the distance only within it. Packed rather than compared as a
    /// pair because the worklist wants a single key it can read from either end, and a pair
    /// would need the direction flip applied to both halves separately.
    ///
    /// A link that leaves a component always climbs, whatever the distances say - the
    /// component half dominates, and that is the guarantee the searches rest on.
    ///
    /// An entry the graph does not hold ranks first, and is never queued.
    pub fn rank_of(&self, id: DialogueNodeId) -> u64 {
        let component = self.component.get(&id).copied().unwrap_or(0) as u64;
        let within = self.distance.get(&id).copied().unwrap_or(0) as u64;
        (component << 32) | within
    }

    /// Which component `id` is in, or `None` for an entry the graph does not hold.
    ///
    /// Independent of the distances, because the decomposition is. Two entries share it
    /// exactly when each can reach the other, and the ordering guarantee is stated in terms
    /// of it - so a check of that guarantee can say so directly.
    pub fn component_of(&self, id: DialogueNodeId) -> Option<u32> {
        self.component.get(&id).copied()
    }

    /// How many entries are ranked, which is every entry the graph holds.
    pub fn len(&self) -> usize {
        self.component.len()
    }

    pub fn is_empty(&self) -> bool {
        self.component.is_empty()
    }

    /// How many strongly connected components the group has.
    pub fn components(&self) -> usize {
        self.components
    }

    /// The largest component, which is 1 exactly when the group is acyclic.
    ///
    /// The number that says how much this can be expected to buy: an acyclic group is
    /// reduced to one pop per entry, and a group that is one enormous cycle has no order
    /// to exploit and should be expected to gain nothing.
    pub fn largest_component(&self) -> usize {
        self.largest
    }
}

/// Which end of the rank a search reads from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Along links, from the start. Takes the lowest rank first.
    Forward,
    /// Along links reversed, from a target. Takes the highest rank first.
    Backward,
}

/// One entry waiting to be taken, and where it sits in the order.
///
/// `Ord` puts the next entry to pop LAST, because [`BinaryHeap`] is a max-heap: highest
/// priority first, and among equals the one pushed earliest. It is consistent with `Eq`
/// because `seq` is unique to a push - two of these compare equal only if they are the
/// same push, and so carry the same id.
#[derive(PartialEq, Eq)]
struct Queued {
    priority: u64,
    seq: u64,
    id: DialogueNodeId,
}

impl Ord for Queued {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority).then_with(|| other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The entries a fixed point still has to visit, handed back in [`IterationOrder`].
///
/// THE TIE-BREAK IS THE PUSH ORDER, which is what the entries INSIDE one component get:
/// a cycle has no first entry, so there is nothing for the rank to say about its members
/// and they come back first-in-first-out. That is the only place the old queue behaviour
/// survives, and it survives because it is the right answer there.
pub struct Worklist<'a> {
    order: &'a IterationOrder,
    direction: Direction,
    queue: BinaryHeap<Queued>,
    pushed: u64,
}

impl<'a> Worklist<'a> {
    pub fn new(order: &'a IterationOrder, direction: Direction) -> Self {
        Self { order, direction, queue: BinaryHeap::new(), pushed: 0 }
    }

    /// Queues `id`, which may already be waiting.
    ///
    /// A DUPLICATE IS NOT WORTH SUPPRESSING HERE. Both searches keep what is pending for an
    /// entry in a separate map and take it all on the first pop, so a second pop of the
    /// same entry finds nothing and costs one heap operation - where suppressing it would
    /// cost a membership set kept in step with the heap on every push and pop.
    pub fn push(&mut self, id: DialogueNodeId) {
        let rank = self.order.rank_of(id);
        self.queue.push(Queued {
            // WHICH END THIS SEARCH READS FROM. A forward pass wants the entry nearest its
            // source first, so it takes the lowest rank; a backward pass keyed on the START
            // wants the furthest, because far from the start stands in for near the target.
            //
            priority: match self.direction {
                Direction::Forward => u64::MAX - rank,
                Direction::Backward => rank,
            },
            seq: self.pushed,
            id,
        });
        self.pushed += 1;
    }

    pub fn pop(&mut self) -> Option<DialogueNodeId> {
        self.queue.pop().map(|queued| queued.id)
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

/// Tarjan's strongly connected components, ITERATIVE.
///
/// Recursive is the usual way to write it and it is not available here. These graphs run to
/// thousands of entries in one chain, and this repository already loses whole processes to
/// stack overflows inside recursive diagram operations - de-8hh2.13 - so an order computed
/// to make those searches cheaper must not be a second way to hit the same wall.
///
/// `index` doubles as the visited set: an entry has one exactly when it has been opened.
struct Tarjan {
    /// The order entries were first reached in.
    index: HashMap<DialogueNodeId, u32>,
    /// The lowest index reachable from an entry without leaving the current component.
    low: HashMap<DialogueNodeId, u32>,
    /// Entries reached but not yet assigned to a component.
    stack: Vec<DialogueNodeId>,
    on_stack: HashSet<DialogueNodeId>,
    next: u32,
    /// Components as they close, which is a reverse topological order of the condensation.
    components: Vec<Vec<DialogueNodeId>>,
}

impl Tarjan {
    fn new(entries: usize) -> Self {
        Self {
            index: HashMap::with_capacity(entries),
            low: HashMap::with_capacity(entries),
            stack: Vec::new(),
            on_stack: HashSet::new(),
            next: 0,
            components: Vec::new(),
        }
    }

    /// Walks everything reachable from `root` that has not been walked already.
    ///
    /// `calls` is the recursion made explicit: an entry, and how many of its links have
    /// been followed so far.
    fn from(&mut self, graph: &LookAheadGraph, root: DialogueNodeId) {
        let mut calls: Vec<(DialogueNodeId, usize)> = vec![(root, 0)];
        self.open(root);

        while let Some(&(id, followed)) = calls.last() {
            let links = graph.get(id).map(|node| node.links.as_slice()).unwrap_or(&[]);

            let Some(&child) = links.get(followed) else {
                // Every link followed, so this is where a component can close.
                calls.pop();
                self.close(id);
                // What the child could reach, the parent can reach through it.
                if let Some(&(parent, _)) = calls.last() {
                    self.lower(parent, self.low_of(id));
                }
                continue;
            };

            if let Some(frame) = calls.last_mut() {
                frame.1 += 1;
            }

            // A link can name an entry this group does not hold. Both searches skip those,
            // so an order over them would rank a vertex nothing ever visits.
            if graph.get(child).is_none() {
                continue;
            }

            let seen = self.index.get(&child).copied();
            let pending = self.on_stack.contains(&child);
            match seen {
                None => {
                    self.open(child);
                    calls.push((child, 0));
                }
                // A back edge into the component being built. An entry already closed into
                // some other component is deliberately ignored: it is downstream, and
                // reaching it says nothing about where this one begins.
                Some(index) if pending => self.lower(id, index),
                Some(_) => {}
            }
        }
    }

    fn open(&mut self, id: DialogueNodeId) {
        self.index.insert(id, self.next);
        self.low.insert(id, self.next);
        self.next += 1;
        self.stack.push(id);
        self.on_stack.insert(id);
    }

    fn low_of(&self, id: DialogueNodeId) -> u32 {
        self.low.get(&id).copied().unwrap_or(0)
    }

    fn lower(&mut self, id: DialogueNodeId, to: u32) {
        let low = self.low.entry(id).or_insert(to);
        *low = (*low).min(to);
    }

    /// Closes the component rooted at `id`, if `id` is a root.
    ///
    /// It is one exactly when nothing below it reached back past it - so everything pushed
    /// since is in its component, and nothing else is.
    fn close(&mut self, id: DialogueNodeId) {
        if self.low_of(id) != self.index.get(&id).copied().unwrap_or(0) {
            return;
        }

        let mut component = Vec::new();
        while let Some(member) = self.stack.pop() {
            self.on_stack.remove(&member);
            component.push(member);
            if member == id {
                break;
            }
        }
        self.components.push(component);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_graph::{node, Entry, GraphBuilder};

    /// Every entry's rank, by entry id, for a graph whose ids are all in one conversation.
    fn ranks(graph: &LookAheadGraph) -> HashMap<i32, u64> {
        let order = IterationOrder::of(graph);
        graph.nodes().map(|n| (n.id.entry_id, order.rank_of(n.id))).collect()
    }

    #[test]
    fn a_chain_is_ranked_in_its_own_order() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build();

        let rank = ranks(&graph);
        assert!(rank[&0] < rank[&1], "the start ranks before what it links to");
        assert!(rank[&1] < rank[&2]);

        let order = IterationOrder::of(&graph);
        assert_eq!(order.components(), 3, "a chain has a component per entry");
        assert_eq!(order.largest_component(), 1, "and no cycle in it");
    }

    /// The case the whole thing is for: both sides of a join rank before the join.
    #[test]
    fn a_diamond_ranks_both_arms_before_the_join() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1).links(&[3]))
            .add(Entry::new(2).links(&[3]))
            .add(Entry::new(3))
            .build();

        let rank = ranks(&graph);
        assert!(rank[&1] < rank[&3], "an arm ranks before the join");
        assert!(rank[&2] < rank[&3], "and so does the other one");
        assert_eq!(IterationOrder::of(&graph).largest_component(), 1);
    }

    #[test]
    fn a_cycle_is_one_component_and_ranks_before_what_follows_it() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2).links(&[1, 3]))
            .add(Entry::new(3))
            .build();

        let order = IterationOrder::of(&graph);
        assert_eq!(order.largest_component(), 2, "1 and 2 are one component");
        assert_eq!(order.components(), 3, "the start, the loop, and the tail");

        let rank = ranks(&graph);
        assert!(rank[&0] < rank[&1].min(rank[&2]), "the start ranks before the loop");
        assert!(rank[&1].max(rank[&2]) < rank[&3], "and the whole loop before the tail");
    }

    /// The claim that lets one rank serve both searches.
    #[test]
    fn the_backward_reading_is_the_forward_one_reversed() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1).links(&[3]))
            .add(Entry::new(2).links(&[3]))
            .add(Entry::new(3))
            .build();
        let order = IterationOrder::of(&graph);

        let mut backward = Worklist::new(&order, Direction::Backward);
        for id in [node(0), node(1), node(3)] {
            backward.push(id);
        }
        assert_eq!(backward.pop(), Some(node(3)), "a backward pass starts at the join");
        assert_eq!(backward.pop(), Some(node(1)));
        assert_eq!(backward.pop(), Some(node(0)));

        let mut forward = Worklist::new(&order, Direction::Forward);
        for id in [node(3), node(1), node(0)] {
            forward.push(id);
        }
        assert_eq!(forward.pop(), Some(node(0)), "a forward pass starts at the start");
        assert_eq!(forward.pop(), Some(node(1)));
        assert_eq!(forward.pop(), Some(node(3)));
    }

    /// Entries the order does not separate keep the order they were pushed in.
    /// Inside a component, nearer the source ranks lower - and the component still wins.
    #[test]
    fn the_distance_separates_a_component_without_overriding_it() {
        // 1 -> 2 -> 3 -> 1 is one component entered at 1, so the start-distances within it
        // are 1, 2 and 3. 0 leads in and 4 leads out.
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2).links(&[3]))
            .add(Entry::new(3).links(&[1, 4]))
            .add(Entry::new(4))
            .build();
        let order = IterationOrder::of_from(&graph, node(0));

        assert!(order.rank_of(node(1)) < order.rank_of(node(2)), "nearer ranks lower");
        assert!(order.rank_of(node(2)) < order.rank_of(node(3)));

        // THE PART THE DISTANCE MAY NOT BREAK. It is a tie-break inside a component, so it
        // may never lift a member above something the component leads to, however deep.
        assert!(order.rank_of(node(0)) < order.rank_of(node(1)), "the way in stays below");
        assert!(order.rank_of(node(3)) < order.rank_of(node(4)), "the tail stays above");
    }

    /// Entries the distance does not separate keep the order they were pushed in.
    #[test]
    fn equidistant_entries_fall_back_to_the_push_order() {
        // 1 and 2 are both one link from the start and both in the cycle through 3.
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1).links(&[3]))
            .add(Entry::new(2).links(&[3]))
            .add(Entry::new(3).links(&[1, 2]))
            .build();
        let order = IterationOrder::of_from(&graph, node(0));
        assert_eq!(order.rank_of(node(1)), order.rank_of(node(2)), "same component, same step");

        let pushed = [node(2), node(1)];
        let mut queue = Worklist::new(&order, Direction::Forward);
        for id in pushed {
            queue.push(id);
        }
        for id in pushed {
            assert_eq!(queue.pop(), Some(id), "a tie keeps the push order");
        }
    }

    /// An order built without a start has no distances, and must still order by component.
    ///
    /// The silent-degradation guard: `Known::of` builds one of these, and a caller that
    /// forgets `of_from` should get a coarser order rather than a wrong one.
    #[test]
    fn without_a_start_the_component_still_orders() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2).links(&[1]))
            .build();
        let blind = IterationOrder::of(&graph);

        assert_eq!(
            blind.rank_of(node(1)),
            blind.rank_of(node(2)),
            "with no distances the members of a component tie",
        );
        assert!(blind.rank_of(node(0)) < blind.rank_of(node(1)), "and the order still holds");
    }

    /// The same entry queued twice is popped twice; both searches rely on it.
    #[test]
    fn a_duplicate_is_kept_rather_than_merged() {
        let graph = GraphBuilder::new().add(Entry::new(0)).build();
        let order = IterationOrder::of(&graph);

        let mut queue = Worklist::new(&order, Direction::Forward);
        queue.push(node(0));
        queue.push(node(0));

        assert_eq!(queue.pop(), Some(node(0)));
        assert_eq!(queue.pop(), Some(node(0)));
        assert_eq!(queue.pop(), None);
    }

    /// Every entry is ranked, and a rank is its component rather than its own position.
    #[test]
    /// The guarantee, stated as it is stated on the type: across components, rank rises.
    #[test]
    fn a_link_between_components_always_climbs() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 4]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2).links(&[3, 1]))
            .add(Entry::new(3).links(&[4]))
            .add(Entry::new(4))
            .build();
        let order = IterationOrder::of(&graph);

        for node in graph.nodes() {
            for &child in &node.links {
                if order.component_of(node.id) == order.component_of(child) {
                    continue;
                }
                assert!(
                    order.rank_of(node.id) < order.rank_of(child),
                    "{} -> {child} leaves a component and must climb",
                    node.id,
                );
            }
        }
    }

    /// A link out of the group names an entry nothing holds, and it must not be ranked.
    #[test]
    fn a_link_out_of_the_group_is_not_a_vertex() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 99]))
            .add(Entry::new(1))
            .build();

        let order = IterationOrder::of(&graph);
        assert_eq!(order.components(), 2, "the missing entry is not one of them");
        assert!(order.rank_of(node(0)) < order.rank_of(node(1)));
    }
}
