// SPDX-License-Identifier: MIT
//! The decision-diagram variables a data layout occupies, and the formulas about them.
//!
//! Shared by everything symbolic, because everything symbolic has to agree about which
//! variable is which. Two formulas built over different managers cannot be combined at
//! all, and two built over the same manager with different ideas about what variable 7
//! means combine into nonsense - so the manager, the variables and the layout travel
//! together.

use oxidd::bdd::{BDDFunction, BDDManagerRef};
use oxidd::{BooleanFunction, Manager, ManagerRef};

use crate::core::state::StateSymbols;
use crate::symbolic::budget::DiagramBudget;
use crate::symbolic::data_layout::DataLayout;
use crate::symbolic::register::{Register, RegisterOps};

/// A manager, its variables, and the layout that says what they mean.
pub struct DataVars<'a> {
    manager: BDDManagerRef,
    vars: Vec<BDDFunction>,
    layout: &'a DataLayout,
    symbols: &'a StateSymbols,
}

impl<'a> DataVars<'a> {
    /// Declares one variable per bit of the layout.
    ///
    /// A MEMORY BUDGET, not a node count. How many nodes and how many cache entries that
    /// works out to is [`DiagramBudget`]'s business, so that every caller states the one
    /// quantity a budget is actually spent in and no caller has to know what a node costs.
    pub fn new(layout: &'a DataLayout, symbols: &'a StateSymbols, budget: DiagramBudget) -> Self {
        Self::over(layout, symbols, budget.manager())
    }

    /// The same, or None where this machine cannot supply the allowance.
    ///
    /// FOR A BUDGET NOBODY CHECKED FIRST, which in practice means a large one: the manager
    /// preallocates its node store and the allocation ABORTS rather than failing, so a
    /// caller asking for six gigabytes on a machine that has four does not get a wrong
    /// answer, it gets no process. [`DiagramBudget::try_manager`] asks before it spends and
    /// this carries the answer out.
    ///
    /// A None is not a finding about anything being measured - the search never ran - so a
    /// caller should report the row as NOT MEASURED rather than folding it in with results.
    /// See `measurements/menu_matrix.rs`, which does.
    ///
    /// [`Self::new`] stays for the many callers whose budget is a fixed small one chosen in
    /// the same file; there is nothing for them to react to.
    pub fn try_new(
        layout: &'a DataLayout,
        symbols: &'a StateSymbols,
        budget: DiagramBudget,
    ) -> Option<Self> {
        Some(Self::over(layout, symbols, budget.try_manager()?))
    }

    /// Declares the layout's variables in a manager somebody else built.
    fn over(layout: &'a DataLayout, symbols: &'a StateSymbols, manager: BDDManagerRef) -> Self {
        let vars = manager.with_manager_exclusive(|m| {
            m.add_vars(layout.total_vars())
                .map(|v| BDDFunction::var(m, v).expect("a freshly added variable"))
                .collect()
        });

        Self {
            manager,
            vars,
            layout,
            symbols,
        }
    }

    /// How many nodes the diagram manager is holding.
    ///
    /// THE MANAGER'S OWN COUNT, not a sum over the sets. Summing each set's `node_count`
    /// counts a node once per set that uses it, and sharing between sets is most of what
    /// makes a diagram cheap - so that sum is an upper bound that can exceed the truth
    /// several times over. For a budget, and above all for a budget that is meant to be
    /// comparable across runs (de-e23q), the number wanted is what is actually
    /// held.
    pub fn node_count(&self) -> usize {
        self.manager.with_manager_shared(|m| m.num_inner_nodes())
    }

    /// What the manager is holding, in bytes.
    ///
    /// Priced at [`DiagramBudget::BYTES_PER_NODE`], which is the same figure the budget
    /// was divided by to decide how many nodes it could hold - so "used" and "allowed" are
    /// in the same currency and can be compared. It is not an accounting of the process
    /// and nothing should read it as one; the same caveat, for the same reason, as
    /// `state_bytes` on the forward side.
    pub fn memory_used(&self) -> usize {
        self.node_count() * DiagramBudget::BYTES_PER_NODE
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

    /// Arithmetic over one slot's run of variables.
    pub fn slot_ops(&self, slot: usize) -> Option<RegisterOps<'_>> {
        let (base, bits) = self.layout.slot(slot)?;
        Some(self.ops_over(Register::new(base, bits)))
    }

    /// Arithmetic over the money register, if the layout carries one.
    pub fn money_ops(&self) -> Option<RegisterOps<'_>> {
        let (base, bits) = self.layout.money()?;
        Some(self.ops_over(Register::new(base, bits)))
    }

    /// Arithmetic over the clock register, if the layout carries one.
    pub fn clock_ops(&self) -> Option<RegisterOps<'_>> {
        let (base, bits) = self.layout.clock()?;
        Some(self.ops_over(Register::new(base, bits)))
    }

    fn ops_over(&self, register: Register) -> RegisterOps<'_> {
        RegisterOps::new(register, self.top(), self.bottom(), |number| {
            self.var(number)
        })
    }

    /// The largest value a slot can hold, given its width.
    pub fn slot_ceiling(&self, slot: usize) -> Option<u32> {
        let (_, bits) = self.layout.slot(slot)?;
        Some(if bits >= 32 {
            u32::MAX
        } else {
            (1u32 << bits) - 1
        })
    }

    /// "This slot holds exactly this value", as a formula.
    ///
    /// A value the slot is too narrow to hold gives the empty set rather than nothing:
    /// the equality is false everywhere, which is an answer, not a failure.
    ///
    /// `None` MEANS THERE IS NO FORMULA, for either of two reasons - the layout carries no
    /// such slot, or the manager had no room left to build one. See the note on the three
    /// slot formulas below for why they are not told apart.
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
                var.not().ok()?
            };
            all = all.and(&literal).ok()?;
        }

        Some(all)
    }

    /// "This slot is non-zero", as a formula.
    ///
    /// `None` as for [`Self::slot_equals`]: no such slot, or no room to say it in.
    pub fn slot_is_set(&self, slot: usize) -> Option<BDDFunction> {
        let (base, bits) = self.layout.slot(slot)?;
        let mut any = self.bottom();
        for bit in 0..bits as u32 {
            any = any.or(self.var(base + bit)).ok()?;
        }

        Some(any)
    }

    /// The conjunction of a slot's variables, which is the cube to quantify over when
    /// forgetting what it held.
    ///
    /// ## WHY THESE THREE FOLD "NO SLOT" AND "NO ROOM" INTO ONE `None`
    ///
    /// Because every caller does the same thing about them: there is no formula, so carry
    /// on without the constraint or give up, and neither answer changes with the reason.
    /// What is NOT allowed is to read the `None` as an empty set or as a formula that
    /// holds everywhere - a dropped slot constraint is how a seed comes to say the player
    /// is rich and poor at once, and an unwrapped operation here aborts the process.
    ///
    /// A caller that must tell the two apart can: [`Self::slot_ceiling`] answers "is there
    /// such a slot" without touching the manager, so asking it first leaves `None` here
    /// meaning no room and nothing else. `seed_of` does exactly that.
    pub fn slot_cube(&self, slot: usize) -> Option<BDDFunction> {
        let (base, bits) = self.layout.slot(slot)?;
        let mut cube = self.top();
        for bit in 0..bits as u32 {
            cube = cube.and(self.var(base + bit)).ok()?;
        }

        Some(cube)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::core::action::DialogueAction;
    use crate::core::types::DialogueNodeId;
    use crate::graph::LookAheadGraph;
    use crate::graph::node::LookAheadNode;

    /// A graph whose symbol table holds `names`, with `counter` incremented so it is wide.
    ///
    /// THE ENTRY LINKS TO ITSELF, which is what makes the counter wide rather than the
    /// increment alone: an increment that can only fire once is held as a DELTA and needs
    /// exactly the bits its own amount asks for, which for a single `+1` is one.
    /// `DataLayout::narrow_to_deltas` is where that is decided.
    pub(crate) fn fixture(names: &[&str], counter: Option<&str>) -> (LookAheadGraph, StateSymbols) {
        let mut symbols = StateSymbols::new();
        let mut actions = Vec::new();
        for name in names {
            let slot = symbols.variable(name);
            if Some(*name) == counter {
                actions.push(DialogueAction::increment(slot, 1, false, "s".to_string()));
            }
        }

        let node = LookAheadNode {
            actions,
            links: vec![DialogueNodeId::new(1, 0)],
            ..LookAheadNode::new(DialogueNodeId::new(1, 0))
        };
        let graph = LookAheadGraph::new(vec![node], symbols).unwrap();
        let snapshot = graph.symbols().clone();
        (graph, snapshot)
    }

    #[test]
    fn a_slot_equals_only_the_value_it_was_given() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let slot = symbols.find("counter").unwrap();
        let (base, bits) = layout.slot(slot).unwrap();

        let three = vars.slot_equals(slot, 3).unwrap();
        let at = |value: u32| -> Vec<(u32, bool)> {
            (0..bits as u32)
                .map(|b| (base + b, (value >> b) & 1 == 1))
                .collect()
        };

        assert!(three.eval(at(3).iter().copied()));
        assert!(!three.eval(at(2).iter().copied()));
        assert!(!three.eval(at(4).iter().copied()));
    }

    #[test]
    fn a_value_too_wide_for_the_slot_is_false_everywhere() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let slot = symbols.find("a").unwrap();

        // One bit, so it cannot hold 2.
        assert!(!vars.slot_equals(slot, 2).unwrap().satisfiable());
        assert_eq!(vars.slot_ceiling(slot), Some(1));
    }

    #[test]
    fn a_cube_covers_every_bit_of_its_slot() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let slot = symbols.find("counter").unwrap();
        let (base, bits) = layout.slot(slot).unwrap();

        let cube = vars.slot_cube(slot).unwrap();
        // Only the all-ones assignment satisfies it.
        let all_set: Vec<(u32, bool)> = (0..bits as u32).map(|b| (base + b, true)).collect();
        assert!(cube.eval(all_set.iter().copied()));
        let one_clear: Vec<(u32, bool)> = (0..bits as u32).map(|b| (base + b, b != 0)).collect();
        assert!(!cube.eval(one_clear.iter().copied()));
    }

    #[test]
    fn a_slot_is_set_when_any_bit_is() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let slot = symbols.find("counter").unwrap();
        let (base, bits) = layout.slot(slot).unwrap();

        let set = vars.slot_is_set(slot).unwrap();
        let zero: Vec<(u32, bool)> = (0..bits as u32).map(|b| (base + b, false)).collect();
        assert!(!set.eval(zero.iter().copied()));
        let four: Vec<(u32, bool)> = (0..bits as u32).map(|b| (base + b, b == 2)).collect();
        assert!(set.eval(four.iter().copied()));
    }
}
