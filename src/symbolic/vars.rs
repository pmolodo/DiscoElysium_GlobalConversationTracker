// SPDX-License-Identifier: MIT
//! The decision-diagram variables a data layout occupies, and the formulas about them.
//!
//! Shared by everything symbolic, because everything symbolic has to agree about which
//! variable is which. Two formulas built over different managers cannot be combined at
//! all, and two built over the same manager with different ideas about what variable 7
//! means combine into nonsense - so the manager, the variables and the layout travel
//! together.

use oxidd::bdd::{new_manager, BDDFunction, BDDManagerRef};
use oxidd::{BooleanFunction, Manager, ManagerRef};

use crate::core::state::StateSymbols;
use crate::symbolic::data_layout::DataLayout;

/// A manager, its variables, and the layout that says what they mean.
pub struct DataVars<'a> {
    manager: BDDManagerRef,
    vars: Vec<BDDFunction>,
    layout: &'a DataLayout,
    symbols: &'a StateSymbols,
}

impl<'a> DataVars<'a> {
    /// Declares one variable per bit of the layout.
    pub fn new(
        layout: &'a DataLayout,
        symbols: &'a StateSymbols,
        node_capacity: usize,
        cache_capacity: usize,
    ) -> Self {
        let manager = new_manager(node_capacity, cache_capacity, 1);
        let vars = manager.with_manager_exclusive(|m| {
            m.add_vars(layout.total_vars())
                .map(|v| BDDFunction::var(m, v).expect("a freshly added variable"))
                .collect()
        });

        Self { manager, vars, layout, symbols }
    }

    pub fn layout(&self) -> &DataLayout {
        self.layout
    }

    pub fn symbols(&self) -> &StateSymbols {
        self.symbols
    }

    /// The everywhere-true formula: every data state.
    pub fn top(&self) -> BDDFunction {
        self.manager.with_manager_shared(BDDFunction::t)
    }

    /// The everywhere-false formula: no data state.
    pub fn bottom(&self) -> BDDFunction {
        self.manager.with_manager_shared(BDDFunction::f)
    }

    /// One variable, by number.
    pub fn var(&self, number: u32) -> &BDDFunction {
        &self.vars[number as usize]
    }

    /// The slot a name occupies, if the layout has one for it.
    pub fn slot_of(&self, name: &str) -> Option<usize> {
        self.symbols.find(name)
    }

    /// The largest value a slot can hold, given its width.
    pub fn slot_ceiling(&self, slot: usize) -> Option<u32> {
        let (_, bits) = self.layout.slot(slot)?;
        Some(if bits >= 32 { u32::MAX } else { (1u32 << bits) - 1 })
    }

    /// "This slot holds exactly this value", as a formula.
    ///
    /// A value the slot is too narrow to hold gives the empty set rather than nothing:
    /// the equality is false everywhere, which is an answer, not a failure.
    pub fn slot_equals(&self, slot: usize, value: u32) -> Option<BDDFunction> {
        let (base, bits) = self.layout.slot(slot)?;
        if bits < 32 && value >= (1u32 << bits) {
            return Some(self.bottom());
        }

        let mut all = self.top();
        for bit in 0..bits as u32 {
            let var = self.var(base + bit);
            let literal = if (value >> bit) & 1 == 1 {
                var.clone()
            } else {
                var.not().expect("negation")
            };
            all = all.and(&literal).expect("and");
        }

        Some(all)
    }

    /// "This slot is non-zero", as a formula.
    pub fn slot_is_set(&self, slot: usize) -> Option<BDDFunction> {
        let (base, bits) = self.layout.slot(slot)?;
        let mut any = self.bottom();
        for bit in 0..bits as u32 {
            any = any.or(self.var(base + bit)).expect("or");
        }

        Some(any)
    }

    /// The conjunction of a slot's variables, which is the cube to quantify over when
    /// forgetting what it held.
    pub fn slot_cube(&self, slot: usize) -> Option<BDDFunction> {
        let (base, bits) = self.layout.slot(slot)?;
        let mut cube = self.top();
        for bit in 0..bits as u32 {
            cube = cube.and(self.var(base + bit)).expect("and");
        }

        Some(cube)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::core::action::DialogueAction;
    use crate::core::guard::GuardExpression;
    use crate::core::types::{DialogueCheckKind, DialogueNodeId};
    use crate::graph::graph::LookAheadGraph;
    use crate::graph::node::LookAheadNode;

    const NODES: usize = 1 << 16;
    const CACHE: usize = 1 << 14;

    /// A graph whose symbol table holds `names`, with `counter` incremented so it is wide.
    pub(crate) fn fixture(
        names: &[&str],
        counter: Option<&str>,
    ) -> (LookAheadGraph, StateSymbols) {
        let mut symbols = StateSymbols::new();
        let mut actions = Vec::new();
        for name in names {
            let slot = symbols.variable(name);
            if Some(*name) == counter {
                actions.push(DialogueAction::increment(slot, 1, false, "s".to_string()));
            }
        }

        let node = LookAheadNode::new(
            DialogueNodeId::new(1, 0), false, DialogueCheckKind::None,
            GuardExpression::always_true(), actions, vec![], 0, false, false, -1, -1, false, -1,
        );
        let snapshot = symbols.clone();
        (LookAheadGraph::new(vec![node], symbols).unwrap(), snapshot)
    }

    #[test]
    fn a_slot_equals_only_the_value_it_was_given() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, NODES, CACHE);
        let slot = symbols.find("counter").unwrap();
        let (base, bits) = layout.slot(slot).unwrap();

        let three = vars.slot_equals(slot, 3).unwrap();
        let at = |value: u32| -> Vec<(u32, bool)> {
            (0..bits as u32).map(|b| (base + b, (value >> b) & 1 == 1)).collect()
        };

        assert!(three.eval(at(3).iter().copied()));
        assert!(!three.eval(at(2).iter().copied()));
        assert!(!three.eval(at(4).iter().copied()));
    }

    #[test]
    fn a_value_too_wide_for_the_slot_is_false_everywhere() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, NODES, CACHE);
        let slot = symbols.find("a").unwrap();

        // One bit, so it cannot hold 2.
        assert!(!vars.slot_equals(slot, 2).unwrap().satisfiable());
        assert_eq!(vars.slot_ceiling(slot), Some(1));
    }

    #[test]
    fn a_cube_covers_every_bit_of_its_slot() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, NODES, CACHE);
        let slot = symbols.find("counter").unwrap();
        let (base, bits) = layout.slot(slot).unwrap();

        let cube = vars.slot_cube(slot).unwrap();
        // Only the all-ones assignment satisfies it.
        let all_set: Vec<(u32, bool)> = (0..bits as u32).map(|b| (base + b, true)).collect();
        assert!(cube.eval(all_set.iter().copied()));
        let one_clear: Vec<(u32, bool)> =
            (0..bits as u32).map(|b| (base + b, b != 0)).collect();
        assert!(!cube.eval(one_clear.iter().copied()));
    }

    #[test]
    fn a_slot_is_set_when_any_bit_is() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, NODES, CACHE);
        let slot = symbols.find("counter").unwrap();
        let (base, bits) = layout.slot(slot).unwrap();

        let set = vars.slot_is_set(slot).unwrap();
        let zero: Vec<(u32, bool)> = (0..bits as u32).map(|b| (base + b, false)).collect();
        assert!(!set.eval(zero.iter().copied()));
        let four: Vec<(u32, bool)> = (0..bits as u32).map(|b| (base + b, b == 2)).collect();
        assert!(set.eval(four.iter().copied()));
    }
}
