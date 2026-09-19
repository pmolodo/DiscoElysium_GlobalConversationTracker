// SPDX-License-Identifier: MIT
//! What order the data slots take their decision-diagram variables in.
//!
//! ## The order we have is better than it looks, and that is the finding
//!
//! A diagram's size depends on its variable order, often by orders of magnitude. The order
//! this project has is slot index order - [`DataLayout`] lays the slots out end to end - and
//! slot index is interning order: a variable's name while its script was parsed, a `once:`
//! slot later, in `LookAheadGraph::new`. That looks like an accident of parsing, and it was
//! natural to expect a deliberate order to beat it.
//!
//! IT DOES NOT, measured on 761 (de-2knp). Interning order is not arbitrary - scripts are
//! parsed in conversation and entry order, and the per-entry markers are interned in graph
//! order - so it already groups slots by WHERE IN THE DIALOGUE they are decided. Two
//! alternatives were built and measured against it:
//!
//! ```text
//! slot (interned)            9,735 ms   6,444,119 nodes   the incumbent
//! dialogue (this file)       9,505 ms   6,781,791 nodes   a wash
//! force, guards only        15,313 ms   9,154,253 nodes   much worse
//! force with entry markers  15,826 ms   9,499,173 nodes   much worse
//! ```
//!
//! [`Ordering::Dialogue`] orders by that locality deliberately and lands where interning order
//! already was, which is the evidence that interning order IS that locality. [`Ordering::Force`]
//! shortens the guards' spans instead - by 92 per cent on 761 - and pays 59 per cent more time
//! for it, because it spends the dialogue locality to buy guard locality and the dialogue
//! locality was worth more.
//!
//! SPAN IS THEREFORE A PROXY THAT MISLED HERE, and [`span`] is kept to show that rather than to
//! steer by: read a short span as a reason to measure, never as a result.
//!
//! What this does NOT say is that no order beats interning order - only that neither of these
//! does, and that a candidate has to keep the dialogue locality rather than trade it away. The
//! relations that ARE stretched are real: in 761 the counter `seafort.deserter_charge_counter`
//! sat at variables 75..77 with the five `once` slots that determine it at 245..253, spanning
//! 179 of 264. Removing those relations outright was worth 2.16x (de-bfs0). Rearranging the
//! layout to shorten them was not.
//!
//! ## What the order is, mechanically
//!
//! One permutation of slot indices. [`DataLayout::renumber`] hands each slot in turn the next
//! run of variables, so the sequence it walks IS the variable order, and nothing else needs to
//! change - `DataVars` adds the manager's variables in layout order, so level `i` is whatever
//! the layout numbered `i`th. No reordering of a built diagram is involved, and none is
//! available: `oxidd-reorder` offers `set_var_order` for an existing diagram, and sifting is
//! work the OxiDD project has planned rather than shipped.
//!
//! ## Choosing one
//!
//! AN ORDER CANNOT CHANGE AN ANSWER. It changes how much room the answer takes to reach, so
//! the oracle staying green is a requirement rather than a hope: an answer that moves under a
//! reordering is a bug in the permutation.
//!
//! The literature calls the thing to minimise SPAN - how far apart the ends of a constraint
//! sit - and Meijer and van de Pol (arXiv:1511.08678) show span is bounded by twice the
//! bandwidth of the dependency matrix, which makes ordinary sparse-matrix bandwidth reduction
//! apply. [`Ordering::Force`] is the cheap end of that family: Aloul, Markov and Sakallah's
//! FORCE, a force-directed placement where each constraint pulls its slots towards the
//! constraint's centre of gravity. Their Sloan result is the stronger one and was not tried,
//! because the cheap end of the family lost badly enough here to make the expensive end a poor
//! next bet.
//!
//! Dynamic reordering is not an option in any case: `oxidd-reorder` establishes a GIVEN order
//! for a built diagram, and sifting is work the OxiDD project has planned rather than shipped.
//! The order is an input we choose, not something the library will improve for us.

use std::collections::{HashMap, HashSet};

use crate::core::guard::GuardExpression;
use crate::core::state::StateSymbols;
use crate::graph::LookAheadGraph;

/// How many passes [`Ordering::Force`] makes before it stops.
///
/// It converges quickly and an order that has stopped moving is detected and stops it sooner;
/// this is the ceiling for one that oscillates between two arrangements rather than settling.
const FORCE_PASSES: usize = 20;

/// Which ordering to lay the slots out in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ordering {
    /// Slot index order, which is the order the names were interned in.
    #[default]
    Slot,
    /// Force-directed placement over what the guards read - see [`force`].
    Force,
    /// The same, with each entry's own `seen` and `once` markers joined to what its guard
    /// reads, on the argument that a search passing the entry raises them while deciding the
    /// guard.
    ForceEntries,
    /// By where the dialogue decides each slot - see [`dialogue`].
    Dialogue,
}

impl Ordering {
    /// The ordering `DEGCT_VAR_ORDER` asks for, or the default where it says nothing.
    ///
    /// An unrecognised value is refused rather than ignored: a measurement that silently ran
    /// the default because an arm was misspelled is a row that says the wrong thing.
    pub fn asked_for() -> Self {
        match crate::core::env::var("VAR_ORDER").as_deref() {
            Ok("slot") | Err(_) => Self::Slot,
            Ok("force") => Self::Force,
            Ok("force-entries") => Self::ForceEntries,
            Ok("dialogue") => Self::Dialogue,
            Ok(other) => panic!("DEGCT_VAR_ORDER: no ordering called {other:?}"),
        }
    }

    /// The permutation of `0..slots` this ordering puts the slots in.
    pub fn of(self, graph: &LookAheadGraph, slots: usize) -> Vec<usize> {
        match self {
            Self::Slot => (0..slots).collect(),
            Self::Force => force(&constraints(graph, slots, false), slots),
            Self::ForceEntries => force(&constraints(graph, slots, true), slots),
            Self::Dialogue => dialogue(graph, slots),
        }
    }
}

/// By where the dialogue decides each slot: whatever is settled first takes the first
/// variables.
///
/// ## The order over entries
///
/// Reverse postorder over the links. That is the order with the property asked for - IF ONE
/// ENTRY DOMINATES ANOTHER IT COMES FIRST - and it holds without building the dominator tree,
/// because every route to a dominated entry passes through its dominator, so the dominator is
/// finished first in any depth-first walk. [`crate::symbolic::dominators`] builds the tree
/// where the relation itself is wanted; here only the sequence is.
///
/// ## The order over slots
///
/// A slot takes the place of the FIRST entry that writes it, or of the first that reads it
/// where nothing writes it. So a flag raised in the opening lines sits at the top of the
/// diagram and one raised at the end sits at the bottom, which is the order a search settles
/// them in.
///
/// The bet this makes, against [`Ordering::Force`]: what matters is not how near a guard's
/// slots are to each other but how near a slot is to the point the search decides it. FORCE
/// shortens the first and measured worse.
fn dialogue(graph: &LookAheadGraph, slots: usize) -> Vec<usize> {
    let settled = reverse_postorder(graph);

    // The first entry to write a slot, then - for slots nothing writes - the first to read it.
    let mut rank = vec![usize::MAX; slots];
    let symbols = graph.symbols();
    for (place, id) in settled.iter().enumerate() {
        let Some(node) = graph.get(*id) else {
            continue;
        };
        let mut touch = |slot: usize| {
            if slot < slots {
                rank[slot] = rank[slot].min(place);
            }
        };
        for action in node.all_actions() {
            if action.slot() >= 0 {
                touch(action.slot() as usize);
            }
        }
        for marker in [node.seen_slot, node.once_slot] {
            if marker >= 0 {
                touch(marker as usize);
            }
        }
        for expression in node.guard.nodes() {
            if let GuardExpression::Variable(name) = expression.expression()
                && let Some(slot) = symbols.find(name)
            {
                touch(slot);
            }
        }
    }

    let mut order: Vec<usize> = (0..slots).collect();
    // A slot nothing in the group touches keeps its own index as its key, so the slots the
    // order has no opinion about stay in the order they were interned in rather than piling
    // up at one end in an arbitrary arrangement.
    order.sort_by_key(|slot| (rank[*slot], *slot));
    order
}

/// Every entry, dominators before what they dominate.
///
/// ITERATIVE, because the recursion this replaces is over dialogue links and conversation 28's
/// group is deep enough to take the stack down - the same reason the guard compiler walks on a
/// stack of its own.
///
/// Entries are seeded in graph order rather than from a start set, so a part of the group that
/// nothing links to is still placed, after whatever reaches it.
fn reverse_postorder(graph: &LookAheadGraph) -> Vec<crate::core::types::DialogueNodeId> {
    let mut seen: HashSet<crate::core::types::DialogueNodeId> = HashSet::new();
    let mut postorder = Vec::new();

    for root in graph.nodes() {
        if seen.contains(&root.id) {
            continue;
        }
        // (entry, how many of its links have been taken)
        let mut stack = vec![(root.id, 0usize)];
        seen.insert(root.id);
        while let Some((id, taken)) = stack.pop() {
            let links = graph
                .get(id)
                .map(|node| node.links.as_slice())
                .unwrap_or(&[]);
            match links.get(taken) {
                Some(next) => {
                    stack.push((id, taken + 1));
                    if seen.insert(*next) {
                        stack.push((*next, 0));
                    }
                }
                None => postorder.push(id),
            }
        }
    }

    postorder.reverse();
    postorder
}

/// The sets of slots that are constrained together, one per guard that names more than one.
///
/// A guard naming a single slot says nothing about order - wherever that slot goes, the guard
/// is one variable wide - so only guards naming two or more are kept. Duplicates are kept
/// rather than merged, because a relation written into ten guards pulls ten times as hard as
/// one written once, which is the weighting FORCE wants.
fn constraints(graph: &LookAheadGraph, slots: usize, with_markers: bool) -> Vec<Vec<usize>> {
    let symbols = graph.symbols();
    let mut found = Vec::new();
    for node in graph.nodes() {
        let mut named: HashSet<usize> = HashSet::new();
        for expression in node.guard.nodes() {
            match expression.expression() {
                GuardExpression::Variable(name) => {
                    if let Some(slot) = slot_of(symbols, name, slots) {
                        named.insert(slot);
                    }
                }
                // A REPUTATION QUESTION reads a whole range and constrains all of it at once,
                // which is exactly the kind of relation an order should keep short.
                GuardExpression::Call(function, _) => {
                    for variable in crate::core::reputation::variables_read_by(function) {
                        if let Some(slot) = slot_of(symbols, &variable, slots) {
                            named.insert(slot);
                        }
                    }
                }
                _ => {}
            }
        }
        // THE ENTRY'S OWN MARKERS, only where asked. The argument for joining them to the
        // guard is that a search passing the entry raises them while deciding it. The argument
        // against is that there is one such constraint PER ENTRY - thousands of them - so they
        // dominate the placement and pull the markers in among the variables, breaking up the
        // run of marker bits that the interned order leaves contiguous.
        if with_markers {
            for marker in [node.seen_slot, node.once_slot] {
                if marker >= 0 && (marker as usize) < slots {
                    named.insert(marker as usize);
                }
            }
        }
        if named.len() > 1 {
            let mut set: Vec<usize> = named.into_iter().collect();
            set.sort_unstable();
            found.push(set);
        }
    }
    found
}

fn slot_of(symbols: &StateSymbols, name: &str, slots: usize) -> Option<usize> {
    symbols.find(name).filter(|slot| *slot < slots)
}

/// Force-directed placement: each constraint pulls its slots towards its centre of gravity.
///
/// One pass computes every constraint's centre of gravity from where its slots currently sit,
/// gives each slot the mean of the centres it belongs to, and re-ranks the slots by that. A
/// slot in no constraint is left where it was, which keeps the order stable rather than
/// scattering the slots nothing relates.
///
/// See Aloul, Markov and Sakallah, FORCE (GLSVLSI 2003). It is a heuristic and makes no claim
/// to an optimum - the optimal order is NP-hard to find - but it is cheap enough to run on
/// every group and needs nothing outside this file.
fn force(constraints: &[Vec<usize>], slots: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..slots).collect();
    if constraints.is_empty() {
        return order;
    }

    // Where each slot currently sits, which is what a centre of gravity is measured in.
    let mut at: Vec<f64> = vec![0.0; slots];
    for (position, slot) in order.iter().enumerate() {
        at[*slot] = position as f64;
    }

    for _ in 0..FORCE_PASSES {
        let mut pull: HashMap<usize, (f64, usize)> = HashMap::new();
        for constraint in constraints {
            let centre =
                constraint.iter().map(|slot| at[*slot]).sum::<f64>() / constraint.len() as f64;
            for slot in constraint {
                let entry = pull.entry(*slot).or_insert((0.0, 0));
                entry.0 += centre;
                entry.1 += 1;
            }
        }

        let mut wanted: Vec<f64> = at.clone();
        for (slot, (total, count)) in pull {
            wanted[slot] = total / count as f64;
        }

        // RANK, NOT THE PULL ITSELF. The wanted positions are a cloud of fractions; what the
        // layout needs is the sequence they put the slots in.
        let mut next = order.clone();
        next.sort_by(|a, b| {
            wanted[*a]
                .partial_cmp(&wanted[*b])
                .expect("no position is NaN")
                // Slots pulled to the same place keep the order they had, so a pass cannot
                // shuffle slots it has no opinion about.
                .then(a.cmp(b))
        });
        if next == order {
            break;
        }
        order = next;
        for (position, slot) in order.iter().enumerate() {
            at[*slot] = position as f64;
        }
    }
    order
}

/// How far the constraints reach, summed - the number an ordering is trying to make small.
///
/// For reporting and for tests. A constraint's span is the distance from its first slot to its
/// last in the given order, so a relation whose ends sit next to each other spans 1.
pub fn span(graph: &LookAheadGraph, slots: usize, order: &[usize]) -> usize {
    let mut position = vec![0usize; slots];
    for (place, slot) in order.iter().enumerate() {
        position[*slot] = place;
    }
    constraints(graph, slots, false)
        .iter()
        .map(|constraint| {
            let places = constraint.iter().map(|slot| position[*slot]);
            let (low, high) = places.fold((usize::MAX, 0), |(low, high), place| {
                (low.min(place), high.max(place))
            });
            high - low + 1
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two pairs, interleaved by the initial order, are separated into two runs.
    ///
    /// The slots start as 0,1,2,3 with 0 related to 2 and 1 to 3 - each relation spanning
    /// three positions - and there is an order where each spans two.
    #[test]
    fn force_pulls_a_relations_ends_together() {
        let constraints = vec![vec![0, 2], vec![1, 3]];
        let order = force(&constraints, 4);

        let mut position = vec![0usize; 4];
        for (place, slot) in order.iter().enumerate() {
            position[*slot] = place;
        }
        let reach = |a: usize, b: usize| position[a].abs_diff(position[b]);
        assert!(
            reach(0, 2) + reach(1, 3) < 4,
            "the two relations started three apart each; {order:?} is no better"
        );
    }

    /// Nothing to relate, nothing to move.
    #[test]
    fn force_leaves_an_unconstrained_layout_alone() {
        assert_eq!(force(&[], 5), vec![0, 1, 2, 3, 4]);
    }

    /// Every slot appears exactly once, whatever the constraints say.
    #[test]
    fn force_returns_a_permutation() {
        let constraints = vec![vec![0, 5], vec![5, 1], vec![2, 3, 4], vec![0, 4]];
        let mut order = force(&constraints, 6);
        order.sort_unstable();
        assert_eq!(order, vec![0, 1, 2, 3, 4, 5]);
    }

    /// A slot in no constraint keeps its place rather than being swept to one end.
    #[test]
    fn force_leaves_an_unrelated_slot_where_it_was() {
        // Slot 1 is in nothing; 0 and 2 are related to each other.
        let order = force(&[vec![0, 2]], 3);
        assert_eq!(order.iter().position(|slot| *slot == 1), Some(1));
    }
}
