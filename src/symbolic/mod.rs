// SPDX-License-Identifier: MIT
//! Representing a set of search states as one decision diagram.
//!
//! A search that enumerates states holds every `(entry, state)` pair it has visited, and
//! that set is what exhausts the budget: 200,000 states in about half a second, on a
//! shape that then finds nothing. A decision diagram represents a SET rather than its
//! members, so if the reachable states share structure it can hold far more of them than
//! there are nodes in the diagram.
//!
//! Whether they DO share structure is the open question, and this module exists to
//! measure it before anything is built on top. Nothing here computes reachability yet -
//! it encodes states already found, unions them, and reports how big
//! the diagram is against how many states went in. If that ratio is poor there is no
//! point building a transition relation, and finding that out cheaply is the point.
//!
//! ## The variable order
//!
//! Variables are numbered in the order [`StateEncoding`] lays them out, and for a BDD the
//! numbering IS the order, which usually matters more than anything else about the
//! encoding. The current layout - entry, then the moving slots, then money, then the
//! clock - is a starting guess and is meant to be varied.

pub mod action_image;
pub mod answer;
pub mod backward;
pub mod budget;
pub mod data_layout;
pub mod dominators;
pub mod guard_formula;
pub mod isolated;
pub mod known;
pub mod menu;
pub mod novelty_search;
pub mod order;
pub mod reachability;
pub mod register;
pub mod vars;

use std::collections::HashMap;

use oxidd::bdd::{BDDFunction, BDDManagerRef};
use oxidd::{BooleanFunction, Function, Manager, ManagerRef};

use crate::core::state::LookAheadState;
use crate::core::types::DialogueNodeId;

/// Minutes in a day, the range the clock is wrapped into.
const MINUTES_IN_DAY: u32 = 1440;

/// How many bits it takes to represent `0..=max`.
fn bits_for(max: u32) -> usize {
    if max == 0 {
        0
    } else {
        (u32::BITS - max.leading_zeros()) as usize
    }
}

/// What a state looked like across a whole sample, so only what MOVES gets encoded.
///
/// A slot that held the same value in every state carries no information and would only
/// add a variable the diagram has to carry. Measuring the sample first and encoding
/// second is what keeps the variable count near what the search actually uses rather than
/// near the symbol table's size - for the biggest conversation group that is the
/// difference between a few dozen variables and 339.
#[derive(Debug, Default)]
pub struct Profile {
    slot_min: HashMap<usize, i32>,
    slot_max: HashMap<usize, i32>,
    money_min: Option<i32>,
    money_max: i32,
    minute_min: Option<i32>,
    minute_max: i32,
    nodes: Vec<DialogueNodeId>,
    node_seen: HashMap<DialogueNodeId, usize>,
    states: usize,
}

impl Profile {
    pub fn new() -> Self {
        Self::default()
    }

    /// Folds one visited state into the profile.
    pub fn observe(&mut self, node: DialogueNodeId, state: &LookAheadState) {
        self.states += 1;
        // Only the FIRST sighting numbers an entry. HashMap::insert overwrites, so
        // inserting unconditionally renumbers an entry every time it is seen again and
        // then hands the next new entry a number already in use - two entries encode
        // identically and the set silently conflates them.
        if !self.node_seen.contains_key(&node) {
            self.node_seen.insert(node, self.nodes.len());
            self.nodes.push(node);
        }

        for slot in 0..state.slot_count() {
            let value = state.get(slot);
            let min = self.slot_min.entry(slot).or_insert(value);
            *min = (*min).min(value);
            let max = self.slot_max.entry(slot).or_insert(value);
            *max = (*max).max(value);
        }

        let money = state.money();
        self.money_min = Some(self.money_min.map_or(money, |m: i32| m.min(money)));
        self.money_max = self.money_max.max(money);

        let minutes = state.day_minutes();
        self.minute_min = Some(self.minute_min.map_or(minutes, |m: i32| m.min(minutes)));
        self.minute_max = self.minute_max.max(minutes);
    }

    /// How many states were folded in.
    pub fn states(&self) -> usize {
        self.states
    }

    /// The distinct entries those states sat on.
    pub fn distinct_nodes(&self) -> usize {
        self.nodes.len()
    }

    /// The slots that took more than one value, in slot order.
    pub fn moving_slots(&self) -> Vec<usize> {
        let mut moving: Vec<usize> = self
            .slot_min
            .keys()
            .copied()
            .filter(|slot| self.slot_min[slot] != self.slot_max[slot])
            .collect();
        moving.sort_unstable();
        moving
    }

    /// Whether money took more than one value.
    pub fn money_moves(&self) -> bool {
        self.money_min != Some(self.money_max)
    }

    /// Whether the clock took more than one value.
    pub fn clock_moves(&self) -> bool {
        self.minute_min != Some(self.minute_max)
    }
}

/// Which decision-diagram variable stands for which part of a state.
#[derive(Debug)]
pub struct StateEncoding {
    node_index: HashMap<DialogueNodeId, u32>,
    node_bits: usize,
    /// The slots that move, and the first variable of each one's little-endian run.
    slots: Vec<(usize, usize)>,
    slot_bits: usize,
    money: Option<usize>,
    money_bits: usize,
    money_base: i32,
    clock: Option<usize>,
    clock_bits: usize,
    total_vars: usize,
    reversed: bool,
}

impl StateEncoding {
    /// Lays out variables for the states a [`Profile`] saw.
    ///
    /// Order: the entry first, then each moving slot, then money, then the clock. The
    /// entry goes first because it changes on every step and so sits at the top of the
    /// diagram where a shared prefix pays off most - a guess, and the first thing to try
    /// reversing if the numbers disappoint.
    pub fn for_profile(profile: &Profile) -> Self {
        let node_bits = bits_for(profile.nodes.len().saturating_sub(1) as u32);
        let mut next = node_bits;

        let moving = profile.moving_slots();
        let slot_span = moving
            .iter()
            .map(|slot| bits_for(profile.slot_max[slot].max(0) as u32))
            .max()
            .unwrap_or(0);
        let mut slots = Vec::with_capacity(moving.len());
        for slot in moving {
            slots.push((slot, next));
            next += slot_span;
        }

        // Money is encoded as an offset from its lowest observed value, which is what
        // keeps the width down: the interesting quantity is how far it moved, not how
        // large it is, and a search that never spends anything then costs no variables.
        let money_base = profile.money_min.unwrap_or(0);
        let money_bits = if profile.money_moves() {
            bits_for((profile.money_max - money_base).max(0) as u32)
        } else {
            0
        };
        let money = if money_bits > 0 {
            let at = next;
            next += money_bits;
            Some(at)
        } else {
            None
        };

        let clock_bits = if profile.clock_moves() {
            bits_for(MINUTES_IN_DAY - 1)
        } else {
            0
        };
        let clock = if clock_bits > 0 {
            let at = next;
            next += clock_bits;
            Some(at)
        } else {
            None
        };

        Self {
            node_index: profile
                .node_seen
                .iter()
                .map(|(k, v)| (*k, *v as u32))
                .collect(),
            node_bits,
            slots,
            slot_bits: slot_span,
            money,
            money_bits,
            money_base,
            clock,
            clock_bits,
            total_vars: next,
            reversed: false,
        }
    }

    /// The same layout with the variable numbering turned back to front.
    ///
    /// A permutation, not a different encoding: the same states, the same number of
    /// variables, the same distinctions - only the ORDER the diagram branches on them.
    /// Comparing the two node counts is the cheapest evidence there is about whether a
    /// disappointing result is the order's fault or the state set's. If a set has
    /// structure the order can exploit, two orders this different should not agree.
    pub fn reversed(mut self) -> Self {
        self.reversed = !self.reversed;
        self
    }

    /// How many decision-diagram variables the layout uses.
    pub fn total_vars(&self) -> usize {
        self.total_vars
    }

    /// How many of those stand for slots.
    pub fn slot_vars(&self) -> usize {
        self.slots.len() * self.slot_bits
    }

    /// A state as an assignment to every variable.
    ///
    /// Returns `None` for a value the layout cannot hold, which can only happen if a
    /// state is encoded that the profile never saw - the widths come from the profile.
    /// A caller that profiles and encodes the same sample never sees `None`.
    pub fn encode(&self, node: DialogueNodeId, state: &LookAheadState) -> Option<Vec<(u32, bool)>> {
        let mut bits = Vec::with_capacity(self.total_vars);
        push_bits(
            &mut bits,
            0,
            self.node_bits,
            *self.node_index.get(&node)? as u32,
        )?;

        for &(slot, at) in &self.slots {
            let value = state.get(slot);
            if value < 0 {
                return None;
            }

            push_bits(&mut bits, at, self.slot_bits, value as u32)?;
        }

        if let Some(at) = self.money {
            let offset = state.money().checked_sub(self.money_base)?;
            if offset < 0 {
                return None;
            }

            push_bits(&mut bits, at, self.money_bits, offset as u32)?;
        }

        if let Some(at) = self.clock {
            push_bits(
                &mut bits,
                at,
                self.clock_bits,
                state.day_minutes().max(0) as u32,
            )?;
        }

        if self.reversed {
            let last = self.total_vars.saturating_sub(1) as u32;
            for (var, _) in &mut bits {
                *var = last - *var;
            }
        }

        Some(bits)
    }
}

/// Appends `width` little-endian bits of `value` starting at variable `at`.
fn push_bits(bits: &mut Vec<(u32, bool)>, at: usize, width: usize, value: u32) -> Option<()> {
    if width < 32 && value >= (1u32 << width) {
        return None;
    }

    for bit in 0..width {
        bits.push(((at + bit) as u32, (value >> bit) & 1 == 1));
    }

    Some(())
}

/// A set of search states, held as one decision diagram.
///
/// ## Its manager is its own, and that is deliberate
///
/// [`crate::symbolic::guard_formula`] and [`crate::symbolic::action_image`] share one
/// manager, because formulas that have to be combined must live in the same one. This
/// does not join them, and must not: a [`StateEncoding`] puts the ENTRY into variables
/// alongside the data, so its variable space means something different from
/// [`crate::symbolic::data_layout::DataLayout`]'s. Sharing a manager between two
/// different ideas of what a variable number means is worse than not sharing at all -
/// the formulas would combine, and combine into nonsense.
pub struct StateSet {
    _manager: BDDManagerRef,
    vars: Vec<BDDFunction>,
    set: BDDFunction,
}

impl StateSet {
    /// An empty set over `total_vars` variables, within a memory budget.
    pub fn new(total_vars: usize, budget: crate::symbolic::budget::DiagramBudget) -> Self {
        let manager = budget.manager();
        let (vars, empty) = manager.with_manager_exclusive(|m| {
            let range = m.add_vars(total_vars as u32);
            let vars: Vec<BDDFunction> = range
                .map(|v| BDDFunction::var(m, v).expect("a freshly added variable"))
                .collect();
            (vars, BDDFunction::f(m))
        });

        Self {
            _manager: manager,
            vars,
            set: empty,
        }
    }

    /// Adds one state.
    ///
    /// Built as a conjunction over every variable and then unioned in, which costs a
    /// diagram operation per variable per state. That is the honest cost of this
    /// measurement and the reason it is run over a bounded sample: it is how you would
    /// build a set from a list of members, not how symbolic reachability would build one,
    /// which computes whole successor sets at a time.
    pub fn insert(&mut self, assignment: &[(u32, bool)]) {
        let mut minterm = self.set.manager_ref().with_manager_shared(BDDFunction::t);
        for &(var, value) in assignment {
            let literal = if value {
                self.vars[var as usize].clone()
            } else {
                self.vars[var as usize]
                    .not()
                    .expect("negation of a variable")
            };
            minterm = minterm.and(&literal).expect("conjunction of literals");
        }

        self.set = self.set.or(&minterm).expect("union with a minterm");
    }

    /// How many diagram nodes the set costs. The number this whole module exists to get.
    pub fn node_count(&self) -> usize {
        self.set.node_count()
    }

    /// Whether an assignment is in the set, for checking the encoding round-trips.
    pub fn contains(&self, assignment: &[(u32, bool)]) -> bool {
        self.set.eval(assignment.iter().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::budget::DiagramBudget;

    fn state(slots: &[i32], money: i32, minutes: i32) -> LookAheadState {
        let mut s = LookAheadState::empty(slots.len(), money, minutes);
        for (i, &v) in slots.iter().enumerate() {
            s = s.with(i, v);
        }
        s
    }

    #[test]
    fn bit_widths_cover_their_range() {
        assert_eq!(bits_for(0), 0);
        assert_eq!(bits_for(1), 1);
        assert_eq!(bits_for(2), 2);
        assert_eq!(bits_for(16), 5);
        assert_eq!(bits_for(1439), 11);
    }

    #[test]
    fn a_slot_that_never_moves_costs_no_variables() {
        let node = DialogueNodeId::new(1, 0);
        let mut profile = Profile::new();
        // Slot 0 moves, slot 1 never does.
        profile.observe(node, &state(&[0, 7], 100, 60));
        profile.observe(node, &state(&[1, 7], 100, 60));

        let encoding = StateEncoding::for_profile(&profile);
        assert_eq!(profile.moving_slots(), vec![0]);
        assert_eq!(encoding.slot_vars(), 1);
        // One node, so no bits are needed to say which; money and clock are still.
        assert_eq!(encoding.total_vars(), 1);
    }

    #[test]
    fn money_is_encoded_as_an_offset_from_its_lowest_value() {
        let node = DialogueNodeId::new(1, 0);
        let mut profile = Profile::new();
        profile.observe(node, &state(&[], 5000, 60));
        profile.observe(node, &state(&[], 5003, 60));

        // Three apart, so two bits - not the thirteen 5003 would need outright.
        let encoding = StateEncoding::for_profile(&profile);
        assert_eq!(encoding.total_vars(), 2);
    }

    #[test]
    fn distinct_states_round_trip_through_the_set() {
        let node = DialogueNodeId::new(1, 0);
        let other = DialogueNodeId::new(1, 1);
        let members = [
            (node, state(&[0, 0], 10, 60)),
            (node, state(&[1, 0], 10, 60)),
            (other, state(&[1, 2], 12, 75)),
        ];

        let mut profile = Profile::new();
        for (n, s) in &members {
            profile.observe(*n, s);
        }

        let encoding = StateEncoding::for_profile(&profile);
        let mut set = StateSet::new(encoding.total_vars(), DiagramBudget::modest());
        for (n, s) in &members {
            set.insert(&encoding.encode(*n, s).expect("a profiled state encodes"));
        }

        for (n, s) in &members {
            assert!(
                set.contains(&encoding.encode(*n, s).unwrap()),
                "{n} should be in the set"
            );
        }

        // A state that was never inserted must not be.
        let absent = encoding.encode(node, &state(&[1, 2], 12, 75)).unwrap();
        assert!(!set.contains(&absent));
    }

    #[test]
    fn an_empty_set_holds_nothing() {
        let set = StateSet::new(4, DiagramBudget::modest());
        assert!(!set.contains(&[(0, false), (1, false), (2, false), (3, false)]));
    }
}
