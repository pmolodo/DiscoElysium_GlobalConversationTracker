// SPDX-License-Identifier: MIT
//! What a node's actions do to a whole SET of data states at once.
//!
//! The other half of symbolic reachability. A compiled guard says which states may take
//! an edge; this says what they become once they have.
//!
//! ## Image, not transition relation
//!
//! The textbook way to do this is to build a relation over primed and unprimed copies of
//! every variable and take the relational product. That doubles the variable count and
//! makes the order of primed against unprimed variables a design problem of its own.
//!
//! Not needed here. A dialogue action assigns a slot or increments it, and both are
//! FUNCTIONS of the current state rather than relations - one input state gives exactly
//! one output state. So the image can be computed directly: forget what the slot held by
//! quantifying its variables away, then assert the new value.
//!
//! ## Increments are done by cases, and can afford to be
//!
//! `slot := min(slot + amount, cap)` is not expressible as a single conjunction, so it is
//! split over the slot's possible values - at most seventeen of them, because the counter
//! cap is what keeps a counter in a loop finite and it is 16 by default. A case split
//! that small is cheaper than a relation, and it keeps the whole thing in one variable
//! space.

use oxidd::BooleanFunction;
use oxidd::bdd::BDDFunction;
use oxidd::BooleanFunctionQuant;

use crate::core::action::{DialogueAction, DialogueActionKind};
use crate::symbolic::vars::DataVars;

/// Applies actions to sets of data states.
pub struct ActionImage<'a> {
    vars: &'a DataVars<'a>,
    counter_cap: u32,
    /// Actions that changed nothing because this layout does not carry what they touch.
    ignored: usize,
    /// Whether a diagram operation could not complete for want of nodes.
    out_of_memory: bool,
}

impl<'a> ActionImage<'a> {
    pub fn new(vars: &'a DataVars<'a>, counter_cap: u32) -> Self {
        Self { vars, counter_cap, ignored: 0, out_of_memory: false }
    }

    /// Whether the manager ran out of nodes part way through.
    ///
    /// A caller that sees this MUST STOP: once it is set, every set this has produced
    /// since is the image of nothing in particular. It is reported rather than unwrapped
    /// because running out of room is a RESULT - the most decisive one a measurement of a
    /// representation can get - and a panic destroys the numbers that show how it got
    /// there. Conversation 14's group reaches it.
    pub fn out_of_memory(&self) -> bool {
        self.out_of_memory
    }

    /// Takes the result of a diagram operation, or records that there was no room.
    ///
    /// The fallback is returned only so the types stay simple; it is not a meaningful
    /// answer and nothing downstream should be trusted once [`Self::out_of_memory`] is
    /// set.
    fn or_no_room<E>(&mut self, attempt: Result<BDDFunction, E>, fallback: &BDDFunction) -> BDDFunction {
        match attempt {
            Ok(function) => function,
            Err(_) => {
                self.out_of_memory = true;
                fallback.clone()
            }
        }
    }

    /// How many actions were skipped because the layout does not carry their subject.
    ///
    /// Money and the clock when they are not laid out, and anything the action parser
    /// could not model. Worth counting rather than silently dropping: an action that does
    /// not happen is how a symbolic state quietly stops matching the crawl's.
    pub fn ignored(&self) -> usize {
        self.ignored
    }

    /// The states reachable by entering a node whose actions are `actions`, from `states`.
    ///
    /// `once_already_fired` is the set of states in which this node's one-time effects
    /// have already happened; actions marked `once` are applied only outside it. Pass the
    /// empty set when the node has no once slot.
    pub fn apply(
        &mut self,
        states: &BDDFunction,
        actions: &[DialogueAction],
        once_already_fired: &BDDFunction,
    ) -> BDDFunction {
        let mut current = states.clone();
        for action in actions {
            current = if action.is_once() {
                // Only the states that have not fired it yet are changed; the rest carry
                // through untouched. Splitting the set is what keeps a once action from
                // firing twice round a loop.
                let unspent = self.or_no_room(once_already_fired.not(), &current);
                let fresh = self.or_no_room(current.and(&unspent), &current);
                let spent = self.or_no_room(current.and(once_already_fired), &current);
                let changed = self.apply_one(&fresh, action);
                self.or_no_room(changed.or(&spent), &current)
            } else {
                self.apply_one(&current, action)
            };
        }

        current
    }

    /// The states from which entering a node whose actions are `actions` LANDS IN
    /// `states`.
    ///
    /// The exact inverse of [`Self::apply`], and it has to stay exact: a backward search
    /// built on a pre-image that loses states reports an entry unreachable that the crawl
    /// walks to, which is the one error direction nothing here is allowed.
    ///
    /// Actions compose in reverse. Forward they apply in order and the last write is the
    /// one that lands, so backwards the last is undone first.
    pub fn pre_apply(
        &mut self,
        states: &BDDFunction,
        actions: &[DialogueAction],
        once_already_fired: &BDDFunction,
    ) -> BDDFunction {
        let mut current = states.clone();
        for action in actions.iter().rev() {
            current = if action.is_once() {
                // Forward, a once action leaves a spent state alone and changes a fresh
                // one. So a state reaches `current` either by being spent and already
                // there, or by being fresh and landing there - and the two cases are
                // disjoint on the once slot, exactly as forward splits them.
                let spent = self.or_no_room(current.and(once_already_fired), &current);
                let unspent = self.or_no_room(once_already_fired.not(), &current);
                let changed = self.pre_one(&current, action);
                let fresh = self.or_no_room(changed.and(&unspent), &current);
                self.or_no_room(fresh.or(&spent), &current)
            } else {
                self.pre_one(&current, action)
            };
        }

        current
    }

    /// One action's pre-image over a set.
    fn pre_one(&mut self, states: &BDDFunction, action: &DialogueAction) -> BDDFunction {
        let slot = action.slot();
        let Ok(slot) = usize::try_from(slot) else {
            self.ignored += 1;
            return states.clone();
        };

        match action.kind() {
            DialogueActionKind::Assign => {
                let value = action.value().max(0) as u32;
                self.pre_assign(states, slot, value)
            }
            DialogueActionKind::Increment => self.pre_increment(states, slot, action.value()),
            _ => {
                self.ignored += 1;
                states.clone()
            }
        }
    }

    /// The states from which `slot := value` lands in `states`.
    ///
    /// ## Why this is the cheap direction
    ///
    /// Select the states that hold the assigned value, then FORGET the slot. Forward, an
    /// assignment forgets and then asserts; backwards it asserts and then forgets, and
    /// the difference matters more than the symmetry suggests: forward, a write
    /// constrains the slot in everything downstream, so every slot any action touches
    /// ends up in the diagram. Backwards, a write ERASES the constraint on its slot, so a
    /// slot written on the way to the target and never read again leaves no trace.
    ///
    /// That is where the backward search gets its variable pruning: not from a cone
    /// somebody computed, but from the pre-image itself.
    pub fn pre_assign(&mut self, states: &BDDFunction, slot: usize, value: u32) -> BDDFunction {
        let (Some(cube), Some(equals)) =
            (self.vars.slot_cube(slot), self.vars.slot_equals(slot, value))
        else {
            self.ignored += 1;
            return states.clone();
        };

        let landed = self.or_no_room(states.and(&equals), states);
        self.or_no_room(landed.exists(&cube), states)
    }

    /// The states from which `slot := min(slot + amount, cap)` lands in `states`.
    ///
    /// By cases over the slot's values, the way [`Self::increment`] is, and for the same
    /// reason. Saturation makes this many-to-one - every value at or above the cap lands
    /// on the cap - so the pre-image of the cap is a range rather than a point, which the
    /// case split handles without any special pleading.
    pub fn pre_increment(
        &mut self,
        states: &BDDFunction,
        slot: usize,
        amount: i32,
    ) -> BDDFunction {
        let Some(ceiling) = self.vars.slot_ceiling(slot) else {
            self.ignored += 1;
            return states.clone();
        };

        let cap = self.counter_cap.min(ceiling);
        let cube = self.vars.slot_cube(slot).expect("a slot in the layout has a cube");
        let mut result = self.vars.bottom();

        for value in 0..=ceiling {
            let raised = (value as i64 + amount as i64).clamp(0, cap as i64) as u32;
            let becomes = self.vars.slot_equals(slot, raised).expect("in the layout");
            let landed = self.or_no_room(states.and(&becomes), states);
            if self.out_of_memory {
                return states.clone();
            }
            if !landed.satisfiable() {
                continue;
            }

            let holding = self.vars.slot_equals(slot, value).expect("in the layout");
            let forgotten = self.or_no_room(landed.exists(&cube), states);
            let came_from = self.or_no_room(forgotten.and(&holding), states);
            result = self.or_no_room(result.or(&came_from), states);
            if self.out_of_memory {
                return states.clone();
            }
        }

        result
    }

    /// One action applied to a set.
    fn apply_one(&mut self, states: &BDDFunction, action: &DialogueAction) -> BDDFunction {
        if !states.satisfiable() {
            return states.clone();
        }

        let slot = action.slot();
        let Ok(slot) = usize::try_from(slot) else {
            // Money, the clock, and anything the model does not apply - whether declared
            // or unknown: no slot to write.
            self.ignored += 1;
            return states.clone();
        };

        match action.kind() {
            DialogueActionKind::Assign => {
                let value = action.value().max(0) as u32;
                self.assign(states, slot, value)
            }
            DialogueActionKind::Increment => {
                self.increment(states, slot, action.value())
            }
            // GainMoney, LoseMoney, PassTime, Declared and Unmodelled write no slot.
            // Money and the clock are deliberately outside this layout - see DataLayout
            // and de-sze.10.
            _ => {
                self.ignored += 1;
                states.clone()
            }
        }
    }

    /// `slot := value`, over a whole set.
    ///
    /// Forget what the slot held, then assert the new value. Quantifying first is what
    /// makes this an assignment rather than a filter: without it the result would be the
    /// states that ALREADY held the value.
    pub fn assign(&mut self, states: &BDDFunction, slot: usize, value: u32) -> BDDFunction {
        let (Some(cube), Some(equals)) =
            (self.vars.slot_cube(slot), self.vars.slot_equals(slot, value))
        else {
            self.ignored += 1;
            return states.clone();
        };

        let forgotten = self.or_no_room(states.exists(&cube), states);
        self.or_no_room(forgotten.and(&equals), states)
    }

    /// `slot := min(slot + amount, cap)`, over a whole set.
    pub fn increment(&mut self, states: &BDDFunction, slot: usize, amount: i32) -> BDDFunction {
        let Some(ceiling) = self.vars.slot_ceiling(slot) else {
            self.ignored += 1;
            return states.clone();
        };

        // The cap the counter saturates at, and never above what the slot can hold - a
        // value the slot is too narrow for would silently become a different value.
        let cap = self.counter_cap.min(ceiling);
        let cube = self.vars.slot_cube(slot).expect("a slot in the layout has a cube");
        let mut result = self.vars.bottom();

        for value in 0..=ceiling {
            let holding = self.vars.slot_equals(slot, value).expect("in the layout");
            let matching = self.or_no_room(states.and(&holding), states);
            if self.out_of_memory {
                return states.clone();
            }
            if !matching.satisfiable() {
                continue;
            }

            let raised = (value as i64 + amount as i64).clamp(0, cap as i64) as u32;
            let becomes = self.vars.slot_equals(slot, raised).expect("in the layout");
            let forgotten = self.or_no_room(matching.exists(&cube), states);
            let moved = self.or_no_room(forgotten.and(&becomes), states);
            result = self.or_no_room(result.or(&moved), states);
            if self.out_of_memory {
                return states.clone();
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::StateSymbols;
    use crate::symbolic::data_layout::DataLayout;
    use crate::symbolic::vars::tests::fixture;

    const NODES: usize = 1 << 16;
    const CACHE: usize = 1 << 14;
    const CAP: u32 = 16;

    /// Reads back which values of `slot` a set allows, by trying them all.
    fn values_of(
        vars: &DataVars,
        set: &BDDFunction,
        slot: usize,
    ) -> Vec<u32> {
        let ceiling = vars.slot_ceiling(slot).unwrap();
        (0..=ceiling)
            .filter(|v| {
                let holding = vars.slot_equals(slot, *v).unwrap();
                set.and(&holding).unwrap().satisfiable()
            })
            .collect()
    }

    #[test]
    fn an_assignment_replaces_whatever_was_there() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, CAP as i32, None, false);
        let vars = DataVars::new(&layout, &symbols, NODES, CACHE);
        let slot = symbols.find("counter").unwrap();
        let mut image = ActionImage::new(&vars, CAP);

        // Every state at all, so the slot could be anything.
        let everything = vars.top();
        assert!(values_of(&vars, &everything, slot).len() > 1);

        let assigned = image.assign(&everything, slot, 3);
        assert_eq!(values_of(&vars, &assigned, slot), vec![3]);
    }

    /// The mistake this is written to avoid: filtering instead of assigning.
    #[test]
    fn an_assignment_does_not_merely_select_states_already_holding_the_value() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, CAP as i32, None, false);
        let vars = DataVars::new(&layout, &symbols, NODES, CACHE);
        let slot = symbols.find("counter").unwrap();
        let mut image = ActionImage::new(&vars, CAP);

        // A set holding only 1. Assigning 3 must give 3, not nothing.
        let only_one = vars.slot_equals(slot, 1).unwrap();
        let assigned = image.assign(&only_one, slot, 3);

        assert!(assigned.satisfiable());
        assert_eq!(values_of(&vars, &assigned, slot), vec![3]);
    }

    #[test]
    fn an_increment_moves_every_value_in_the_set() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, CAP as i32, None, false);
        let vars = DataVars::new(&layout, &symbols, NODES, CACHE);
        let slot = symbols.find("counter").unwrap();
        let mut image = ActionImage::new(&vars, CAP);

        // A set holding 1 or 4 - two values at once, which is the point of doing this
        // over sets rather than states.
        let one = vars.slot_equals(slot, 1).unwrap();
        let four = vars.slot_equals(slot, 4).unwrap();
        let both = one.or(&four).unwrap();

        let raised = image.increment(&both, slot, 1);
        assert_eq!(values_of(&vars, &raised, slot), vec![2, 5]);
    }

    #[test]
    fn an_increment_saturates_at_the_cap() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, CAP as i32, None, false);
        let vars = DataVars::new(&layout, &symbols, NODES, CACHE);
        let slot = symbols.find("counter").unwrap();
        let mut image = ActionImage::new(&vars, CAP);

        // Five bits hold up to 31, but the cap is 16.
        assert_eq!(vars.slot_ceiling(slot), Some(31));
        let at_cap = vars.slot_equals(slot, 16).unwrap();
        let raised = image.increment(&at_cap, slot, 1);

        assert_eq!(values_of(&vars, &raised, slot), vec![16]);
    }

    /// The cap is what makes a counter in a dialogue loop terminate.
    #[test]
    fn repeated_increments_reach_a_fixed_point() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, CAP as i32, None, false);
        let vars = DataVars::new(&layout, &symbols, NODES, CACHE);
        let slot = symbols.find("counter").unwrap();
        let mut image = ActionImage::new(&vars, CAP);

        let mut set = vars.slot_equals(slot, 0).unwrap();
        for _ in 0..40 {
            set = image.increment(&set, slot, 1);
        }

        assert_eq!(values_of(&vars, &set, slot), vec![16]);
    }

    #[test]
    fn a_once_action_changes_only_the_states_that_have_not_fired_it() {
        let mut symbols = StateSymbols::new();
        let counter = symbols.variable("counter");
        let fired = symbols.variable("fired");
        let actions = vec![DialogueAction::increment(counter, 1, true, "s".to_string())];
        let node = crate::graph::node::LookAheadNode::new(
            crate::core::types::DialogueNodeId::new(1, 0), false,
            crate::core::types::DialogueCheckKind::None,
            crate::core::guard::GuardExpression::always_true(), actions.clone(), vec![],
            0, false, false, -1, -1, false, -1,
        );
        let snapshot = symbols.clone();
        let graph = crate::graph::graph::LookAheadGraph::new(vec![node], symbols).unwrap();
        let layout = DataLayout::for_graph(&graph, CAP as i32, None, false);
        let vars = DataVars::new(&layout, &snapshot, NODES, CACHE);
        let mut image = ActionImage::new(&vars, CAP);

        let spent = vars.slot_is_set(fired).unwrap();
        let counter_at_zero = vars.slot_equals(counter, 0).unwrap();

        // Not yet fired: the counter moves.
        let fresh = counter_at_zero.and(&spent.not().unwrap()).unwrap();
        let after_fresh = image.apply(&fresh, &actions, &spent);
        assert_eq!(values_of(&vars, &after_fresh, counter), vec![1]);

        // Already fired: it does not.
        let used = counter_at_zero.and(&spent).unwrap();
        let after_used = image.apply(&used, &actions, &spent);
        assert_eq!(values_of(&vars, &after_used, counter), vec![0]);
    }

    #[test]
    fn an_action_the_layout_cannot_carry_is_counted_rather_than_dropped_silently() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, CAP as i32, None, false);
        let vars = DataVars::new(&layout, &symbols, NODES, CACHE);
        let mut image = ActionImage::new(&vars, CAP);

        let money = vec![DialogueAction::money(true, 50, false, "GainMoney".to_string())];
        let before = vars.top();
        let after = image.apply(&before, &money, &vars.bottom());

        assert_eq!(image.ignored(), 1);
        // Unchanged, because this layout carries no money: nothing in the result lies
        // outside what went in, and nothing that went in was lost.
        assert!(!after.and(&before.not().unwrap()).unwrap().satisfiable());
        assert!(!before.and(&after.not().unwrap()).unwrap().satisfiable());
    }
}
