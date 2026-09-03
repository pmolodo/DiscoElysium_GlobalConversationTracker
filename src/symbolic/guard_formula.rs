// SPDX-License-Identifier: MIT
//! Turning a guard into a formula over the data variables.
//!
//! ## Two rails, because a guard is three-valued
//!
//! The engine's guards answer True, False or Unknown, and [`crate::core::types::Ternary`]
//! lets Unknown through - a crawl must not refuse a branch merely because it cannot
//! decide one. A single formula cannot express that: negating "may be true" gives
//! "must be false", which is a different thing.
//!
//! So a compiled guard is a PAIR. `may_be_true` is the set of data states in which the
//! guard could hold; `may_be_false` the set in which it could fail. Both are true at once
//! exactly where the guard is undecided, and a guard the compiler cannot read at all is
//! `(⊤, ⊤)` - undecided everywhere, which is the permissive answer the engine already
//! gives.
//!
//! That makes every approximation here one-directional and safe: the compiler may report
//! a branch as possible when it is not, never the reverse, so a reachable set built from
//! these formulas is an over-approximation of the real one and never misses a state.
//!
//! ## What it can and cannot read
//!
//! Reads precisely: literals, variables the symbol table knows, `not`/`and`/`or`, and a
//! comparison of a known variable against a constant with `==` or `~=`.
//!
//! Falls back to undecided: world queries (`Call`), ordering comparisons, and anything
//! naming a variable the graph never mentions. [`GuardCompiler::fallbacks`] counts how
//! often that happened, which is the number that says whether this approach can carry
//! real content.

use std::collections::HashMap;

use oxidd::bdd::{new_manager, BDDFunction, BDDManagerRef};
use oxidd::{BooleanFunction, Manager, ManagerRef};

use crate::core::guard::GuardExpression;
use crate::core::guard_value::GuardValueKind;
use crate::core::state::StateSymbols;
use crate::core::types::Ternary;
use crate::symbolic::data_layout::DataLayout;
use crate::world::world::ILookAheadWorld;

/// A guard as two sets of data states: where it may hold, and where it may fail.
#[derive(Clone)]
pub struct MayBe {
    pub may_be_true: BDDFunction,
    pub may_be_false: BDDFunction,
}

impl MayBe {
    /// Whether this guard is decided everywhere - the two rails never overlap.
    pub fn is_decided(&self) -> bool {
        match self.may_be_true.and(&self.may_be_false) {
            Ok(both) => !both.satisfiable(),
            Err(_) => false,
        }
    }
}

/// Compiles guards over one data layout.
pub struct GuardCompiler<'a> {
    manager: BDDManagerRef,
    vars: Vec<BDDFunction>,
    layout: &'a DataLayout,
    symbols: &'a StateSymbols,
    /// Where a variable no action writes gets its value.
    ///
    /// Such a variable is CONSTANT for the whole crawl - the seed reads it once and
    /// nothing moves it - so with a world in hand it compiles to a literal rather than
    /// falling back. Between 30% and 47% of the distinct variables the biggest
    /// conversations' guards mention are of this kind, so it is not a corner.
    world: Option<&'a dyn ILookAheadWorld>,
    fallbacks: usize,
    compiled: usize,
    reasons: HashMap<&'static str, usize>,
}

impl<'a> GuardCompiler<'a> {
    /// Creates a compiler, declaring one variable per bit of the layout.
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

        Self { manager, vars, layout, symbols, world: None, fallbacks: 0, compiled: 0, reasons: HashMap::new() }
    }

    /// Gives the compiler a world to read untracked variables from.
    pub fn with_world(mut self, world: &'a dyn ILookAheadWorld) -> Self {
        self.world = Some(world);
        self
    }

    /// How many sub-expressions the compiler could not read and had to call undecided.
    pub fn fallbacks(&self) -> usize {
        self.fallbacks
    }

    /// How many sub-expressions it read precisely.
    pub fn compiled(&self) -> usize {
        self.compiled
    }

    /// The everywhere-true formula.
    pub fn top(&self) -> BDDFunction {
        self.manager.with_manager_shared(BDDFunction::t)
    }

    /// The everywhere-false formula.
    pub fn bottom(&self) -> BDDFunction {
        self.manager.with_manager_shared(BDDFunction::f)
    }

    /// Why each fallback happened, most common first.
    ///
    /// Worth keeping rather than a bare count: the reasons are what say whether the
    /// compiler is missing something cheap or genuinely up against the world.
    pub fn fallback_reasons(&self) -> Vec<(&'static str, usize)> {
        let mut rows: Vec<(&'static str, usize)> =
            self.reasons.iter().map(|(k, v)| (*k, *v)).collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1));
        rows
    }

    /// A guard that is undecided everywhere: the permissive answer.
    fn undecided(&mut self, reason: &'static str) -> MayBe {
        self.fallbacks += 1;
        *self.reasons.entry(reason).or_default() += 1;
        MayBe { may_be_true: self.top(), may_be_false: self.top() }
    }

    fn decided(&mut self, holds: BDDFunction) -> MayBe {
        self.compiled += 1;
        let fails = holds.not().expect("negation");
        MayBe { may_be_true: holds, may_be_false: fails }
    }

    /// Compiles a guard into its two rails.
    pub fn compile(&mut self, guard: &GuardExpression) -> MayBe {
        match guard {
            GuardExpression::Literal(value) => match value.as_condition() {
                Ternary::True => {
                    let t = self.top();
                    self.decided(t)
                }
                Ternary::False => {
                    let f = self.bottom();
                    self.decided(f)
                }
                Ternary::Unknown => self.undecided("literal is unknown"),
            },

            GuardExpression::Variable(name) => match self.slot_is_set(name) {
                Some(holds) => self.decided(holds),
                // Not tracked, so constant - ask the world, and fall back only if even
                // the world cannot say.
                None => match self.constant_truth(name) {
                    Some(true) => {
                        let t = self.top();
                        self.decided(t)
                    }
                    Some(false) => {
                        let f = self.bottom();
                        self.decided(f)
                    }
                    None => self.undecided("variable untracked and world cannot say"),
                },
            },

            GuardExpression::Not(inner) => {
                let inner = self.compile(inner);
                MayBe { may_be_true: inner.may_be_false, may_be_false: inner.may_be_true }
            }

            GuardExpression::And(left, right) => {
                let a = self.compile(left);
                let b = self.compile(right);
                MayBe {
                    // Both may hold, so both rails must allow it.
                    may_be_true: a.may_be_true.and(&b.may_be_true).expect("and"),
                    // Either failing is enough to fail the conjunction.
                    may_be_false: a.may_be_false.or(&b.may_be_false).expect("or"),
                }
            }

            GuardExpression::Or(left, right) => {
                let a = self.compile(left);
                let b = self.compile(right);
                MayBe {
                    may_be_true: a.may_be_true.or(&b.may_be_true).expect("or"),
                    may_be_false: a.may_be_false.and(&b.may_be_false).expect("and"),
                }
            }

            GuardExpression::Comparison(op, left, right) => self.compare(op, left, right),

            // A world query - HasItem, IsTaskActive, MoneyAmount, the clock, and every
            // other thing the crawl asks the game rather than its own state. Undecided
            // here, which is the permissive answer, and counted so the fallback rate can
            // be measured against real content.
            // Inventory and journal questions, which the crawl DOES change - GainItem
            // and LoseItem write an `item:` slot, GainTask and FinishTask a `task:` one,
            // and `BoundContext::query` answers these from exactly those slots. This
            // mirrors that.
            //
            // Where the group has no such slot, the item or task is one no action here
            // touches, so it is constant for the crawl and the world answers it - the
            // same rule as an untracked variable. That is also what keeps the variable
            // count down: a slot exists only for something the group actually
            // manipulates, not for every item in the game.
            GuardExpression::Call(name, args)
                if name == "CheckItem" || name == "IsTaskActive" =>
            {
                let is_item = name == "CheckItem";
                match Self::text_argument(args) {
                    Some(subject) => {
                        let slot = format!("{}{subject}", if is_item { "item:" } else { "task:" });
                        match self.slot_is_set(&slot) {
                            Some(holds) => self.decided(holds),
                            // Untracked, so constant - and answered THE WAY THE ENGINE
                            // ANSWERS IT, through `query`, not through `initially_has_item`.
                            //
                            // The difference matters and is easy to get wrong. `initially_has_item`
                            // returns a plain bool, so it is definite for every name,
                            // including one the world has simply never heard of. The
                            // engine does not consult it here: `BoundContext::query`
                            // falls through to `world.query`, which may answer unknown
                            // and leave the branch open. Deciding "not held" where the
                            // engine stays permissive would prune a branch the real crawl
                            // walks, which is the one direction this compiler must never
                            // be wrong in. `initially_has_item` is used only by the seed, and only
                            // for slots the group tracks.
                            None => match self.constant_query(name, args) {
                                Some(true) => {
                                    let t = self.top();
                                    self.decided(t)
                                }
                                Some(false) => {
                                    let f = self.bottom();
                                    self.decided(f)
                                }
                                None => self.undecided("call: untracked, world cannot say"),
                            },
                        }
                    }
                    None => self.undecided("call: subject is not a literal"),
                }
            }

            // A query the CRAWL cannot change is a constant, and the engine says which
            // those are: `BoundContext::query` intercepts MoneyAmount, CheckItem,
            // IsTaskActive and the clock, and lets everything else fall through to the
            // world - which does not change while a crawl runs. So anything not
            // intercepted has the same answer at every state, and asking the world once
            // is exactly what the engine does at every step.
            //
            // Worth the trouble: IsKimHere alone is 691 of the roughly 1,200 world
            // queries the five biggest conversations make.
            GuardExpression::Call(name, args)
                if !Self::crawl_can_change(name) && self.world.is_some() =>
            {
                match self.constant_query(name, args) {
                    Some(true) => {
                        let t = self.top();
                        self.decided(t)
                    }
                    Some(false) => {
                        let f = self.bottom();
                        self.decided(f)
                    }
                    None => self.undecided("call: world cannot say"),
                }
            }

            GuardExpression::Call(name, _) => {
                let reason: &'static str = match name.as_str() {
                    "CheckItem" => "call: CheckItem",
                    "IsTaskActive" => "call: IsTaskActive",
                    "MoneyAmount" => "call: MoneyAmount",
                    "DayCount" | "HourCount" | "IsDayFrom" | "IsMorning" | "IsAfternoon"
                    | "IsEvening" | "IsNight" | "IsMidnight" | "IsHour" => "call: clock",
                    _ => "call: other world query",
                };
                self.undecided(reason)
            }
        }
    }

    /// A comparison, where it can be read.
    ///
    /// Only equality against a constant. An ordering comparison would need the slot's
    /// bits compared against a constant's, which is the arithmetic that makes decision
    /// diagrams blow up and is deliberately not attempted until something measures
    /// whether it is needed.
    fn compare(
        &mut self,
        op: &str,
        left: &GuardExpression,
        right: &GuardExpression,
    ) -> MayBe {
        // `expr == false` is negation and `expr == true` is a no-op, and BOTH are
        // everywhere: 5,994 of the 13,059 distinct guards in the database end in
        // `== false` and another 1,582 in `== true`, because that is how the condition
        // text is written. Reading them as comparisons and giving up - which is what
        // happens when neither side is a bare variable - throws away the whole
        // compilable expression underneath, and was the single largest reason the
        // fallback rate was high.
        if op == "==" || op == "~=" {
            let negated = op == "~=";
            if let Some(truth) = Self::boolean_of(right) {
                return self.against_boolean(left, truth != negated);
            }
            if let Some(truth) = Self::boolean_of(left) {
                return self.against_boolean(right, truth != negated);
            }
        }

        let (Some(slot), Some(value)) = (Self::variable_of(left), Self::constant_of(right))
        else {
            // Also try the other way round: a guard may be written `1 == Variable[..]`.
            if let (Some(slot), Some(value)) = (Self::variable_of(right), Self::constant_of(left)) {
                return self.equality(op, &slot, value);
            }
            return self.undecided("comparison: neither side a known variable");
        };

        self.equality(op, &slot, value)
    }

    /// `expression == truth`, compiled by compiling the expression and, when comparing
    /// against false, swapping its rails.
    fn against_boolean(&mut self, expression: &GuardExpression, truth: bool) -> MayBe {
        let inner = self.compile(expression);
        if truth {
            inner
        } else {
            MayBe { may_be_true: inner.may_be_false, may_be_false: inner.may_be_true }
        }
    }

    /// The boolean a literal stands for, if it is a boolean one.
    fn boolean_of(expression: &GuardExpression) -> Option<bool> {
        let GuardExpression::Literal(value) = expression else { return None };
        match value.kind() {
            GuardValueKind::Boolean => Some(value.boolean()),
            _ => None,
        }
    }

    fn equality(&mut self, op: &str, name: &str, value: i32) -> MayBe {
        let equals = match self.slot_equals(name, value) {
            Some(f) => f,
            None => return self.undecided("comparison: variable not in the layout"),
        };

        match op {
            "==" => self.decided(equals),
            "~=" => {
                let differs = equals.not().expect("negation");
                self.decided(differs)
            }
            _ => self.undecided("comparison: ordering operator"),
        }
    }

    /// The name a `Variable` node carries, if the expression is one.
    fn variable_of(expression: &GuardExpression) -> Option<String> {
        match expression {
            GuardExpression::Variable(name) => Some(name.clone()),
            _ => None,
        }
    }

    /// The integer a literal stands for, if it is one a slot could hold.
    fn constant_of(expression: &GuardExpression) -> Option<i32> {
        let GuardExpression::Literal(value) = expression else { return None };
        match value.kind() {
            GuardValueKind::Boolean => Some(i32::from(value.boolean())),
            GuardValueKind::Number => {
                let number = value.number();
                // Only whole numbers in range: a slot holds an integer.
                if number.fract() == 0.0 && number >= 0.0 && number <= i32::MAX as f64 {
                    Some(number as i32)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// The single text argument a query names its subject with, if that is its shape.
    fn text_argument(args: &[GuardExpression]) -> Option<String> {
        let [GuardExpression::Literal(value)] = args else { return None };
        match value.kind() {
            GuardValueKind::Text => Some(value.text().to_string()),
            _ => None,
        }
    }

    /// Whether a crawl's own actions can change what this query answers.
    ///
    /// Mirrors the interception list in `BoundContext::query`. Anything here is answered
    /// from crawl state and so varies between states; anything else is answered by the
    /// world and is the same at every state.
    fn crawl_can_change(name: &str) -> bool {
        matches!(name, "MoneyAmount" | "CheckItem" | "IsTaskActive")
            || crate::core::clock::ClockTime::owns(name)
    }

    /// What the world says a constant query is, as a condition.
    ///
    /// Only literal arguments: a query whose argument is itself computed would have to be
    /// evaluated per state, which is the thing being avoided.
    fn constant_query(&self, name: &str, args: &[GuardExpression]) -> Option<bool> {
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            let GuardExpression::Literal(value) = arg else { return None };
            values.push(value.clone());
        }

        match self.world?.query(name, &values).as_condition() {
            Ternary::True => Some(true),
            Ternary::False => Some(false),
            Ternary::Unknown => None,
        }
    }

    /// What the world says an untracked variable is, as a condition.
    fn constant_truth(&self, name: &str) -> Option<bool> {
        match self.world?.get_variable(name).as_condition() {
            Ternary::True => Some(true),
            Ternary::False => Some(false),
            Ternary::Unknown => None,
        }
    }

    /// "This slot is non-zero", as a formula.
    fn slot_is_set(&self, name: &str) -> Option<BDDFunction> {
        let slot = self.symbols.find(name)?;
        let (base, bits) = self.layout.slot(slot)?;
        // Non-zero is "any bit set".
        let mut any = self.bottom();
        for bit in 0..bits as u32 {
            any = any.or(&self.vars[(base + bit) as usize]).expect("or");
        }

        Some(any)
    }

    /// "This slot holds exactly this value", as a formula.
    fn slot_equals(&self, name: &str, value: i32) -> Option<BDDFunction> {
        let slot = self.symbols.find(name)?;
        let (base, bits) = self.layout.slot(slot)?;
        if bits < 32 && value as u32 >= (1u32 << bits) {
            // The slot cannot hold it, so the equality is false everywhere - which is
            // decided, not unknown.
            return Some(self.bottom());
        }

        let mut all = self.top();
        for bit in 0..bits as u32 {
            let var = &self.vars[(base + bit) as usize];
            let literal = if (value as u32 >> bit) & 1 == 1 {
                var.clone()
            } else {
                var.not().expect("negation")
            };
            all = all.and(&literal).expect("and");
        }

        Some(all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::action::DialogueAction;
    use crate::core::guard_value::GuardValue;
    use crate::core::guard::GuardExpression;
    use crate::core::types::{DialogueCheckKind, DialogueNodeId};
    use crate::graph::graph::LookAheadGraph;
    use crate::graph::node::LookAheadNode;

    const NODES: usize = 1 << 16;
    const CACHE: usize = 1 << 14;

    /// A graph whose symbol table holds `names`, with `counter` incremented so it is wide.
    fn fixture(names: &[&str], counter: Option<&str>) -> (LookAheadGraph, StateSymbols) {
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

    fn boolean(value: bool) -> GuardExpression {
        GuardExpression::Literal(GuardValue::from_boolean(value))
    }

    fn number(value: f64) -> GuardExpression {
        GuardExpression::Literal(GuardValue::from_number(value))
    }

    #[test]
    fn a_true_literal_holds_everywhere_and_fails_nowhere() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE);

        let compiled = compiler.compile(&boolean(true));
        assert!(compiled.may_be_true.valid());
        assert!(!compiled.may_be_false.satisfiable());
        assert!(compiled.is_decided());
    }

    #[test]
    fn an_unreadable_guard_is_undecided_everywhere_rather_than_false() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE);

        // A world query the compiler cannot read.
        let compiled = compiler.compile(&GuardExpression::Call("IsKimHere".to_string(), vec![]));

        // Permissive in BOTH directions: the crawl may take the branch and may not.
        assert!(compiled.may_be_true.valid());
        assert!(compiled.may_be_false.valid());
        assert!(!compiled.is_decided());
        assert_eq!(compiler.fallbacks(), 1);
    }

    #[test]
    fn a_variable_reads_as_its_slot_being_set() {
        let (graph, symbols) = fixture(&["met_kim"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let slot = symbols.find("met_kim").unwrap();
        let (base, bits) = layout.slot(slot).unwrap();
        assert_eq!(bits, 1);

        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE);
        let compiled = compiler.compile(&GuardExpression::Variable("met_kim".to_string()));

        assert!(compiled.may_be_true.eval([(base, true)]));
        assert!(!compiled.may_be_true.eval([(base, false)]));
        assert!(compiled.is_decided());
    }

    #[test]
    fn a_wide_slot_is_set_when_any_of_its_bits_is() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let slot = symbols.find("counter").unwrap();
        let (base, bits) = layout.slot(slot).unwrap();
        assert_eq!(bits, 5);

        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE);
        let compiled = compiler.compile(&GuardExpression::Variable("counter".to_string()));

        let zero: Vec<(u32, bool)> = (0..bits as u32).map(|b| (base + b, false)).collect();
        assert!(!compiled.may_be_true.eval(zero.iter().copied()));

        // Value 4 is the third bit alone.
        let four: Vec<(u32, bool)> =
            (0..bits as u32).map(|b| (base + b, b == 2)).collect();
        assert!(compiled.may_be_true.eval(four.iter().copied()));
    }

    #[test]
    fn equality_against_a_constant_pins_every_bit() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let slot = symbols.find("counter").unwrap();
        let (base, bits) = layout.slot(slot).unwrap();

        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE);
        let compiled = compiler.compile(&GuardExpression::Comparison(
            "==".to_string(),
            Box::new(GuardExpression::Variable("counter".to_string())),
            Box::new(number(3.0)),
        ));

        let assignment = |value: u32| -> Vec<(u32, bool)> {
            (0..bits as u32).map(|b| (base + b, (value >> b) & 1 == 1)).collect()
        };
        assert!(compiled.may_be_true.eval(assignment(3).iter().copied()));
        assert!(!compiled.may_be_true.eval(assignment(2).iter().copied()));
        assert!(compiled.may_be_false.eval(assignment(2).iter().copied()));
        assert_eq!(compiler.fallbacks(), 0);
    }

    #[test]
    fn inequality_is_the_complement_of_equality() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let base = layout.slot(symbols.find("a").unwrap()).unwrap().0;

        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE);
        let compiled = compiler.compile(&GuardExpression::Comparison(
            "~=".to_string(),
            Box::new(GuardExpression::Variable("a".to_string())),
            Box::new(boolean(true)),
        ));

        assert!(compiled.may_be_true.eval([(base, false)]));
        assert!(!compiled.may_be_true.eval([(base, true)]));
    }

    #[test]
    fn a_constant_on_the_left_reads_the_same_as_on_the_right() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let base = layout.slot(symbols.find("a").unwrap()).unwrap().0;

        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE);
        let compiled = compiler.compile(&GuardExpression::Comparison(
            "==".to_string(),
            Box::new(boolean(true)),
            Box::new(GuardExpression::Variable("a".to_string())),
        ));

        assert!(compiled.may_be_true.eval([(base, true)]));
        assert_eq!(compiler.fallbacks(), 0);
    }

    #[test]
    fn an_ordering_comparison_falls_back_rather_than_guessing() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, 16, None, false);

        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE);
        let compiled = compiler.compile(&GuardExpression::Comparison(
            ">=".to_string(),
            Box::new(GuardExpression::Variable("counter".to_string())),
            Box::new(number(3.0)),
        ));

        assert!(!compiled.is_decided());
        assert_eq!(compiler.fallbacks(), 1);
    }

    /// The rule that makes the whole approximation safe.
    ///
    /// An undecided operand must leave the conjunction takeable, because the engine's
    /// `can_pass` lets Unknown through. Anything else would prune a branch the real crawl
    /// walks, and a reachable set built from these formulas would MISS states.
    #[test]
    fn an_undecided_operand_leaves_a_conjunction_takeable() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let base = layout.slot(symbols.find("a").unwrap()).unwrap().0;

        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE);
        let compiled = compiler.compile(&GuardExpression::And(
            Box::new(GuardExpression::Variable("a".to_string())),
            Box::new(GuardExpression::Call("IsKimHere".to_string(), vec![])),
        ));

        // Where a holds, the conjunction may hold - the query is not read as false.
        assert!(compiled.may_be_true.eval([(base, true)]));
        // And it may fail, because the query may be false.
        assert!(compiled.may_be_false.eval([(base, true)]));
        // Where a does not hold, the conjunction cannot hold.
        assert!(!compiled.may_be_true.eval([(base, false)]));
    }

    #[test]
    fn negation_swaps_the_rails() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let base = layout.slot(symbols.find("a").unwrap()).unwrap().0;

        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE);
        let compiled = compiler.compile(&GuardExpression::Not(Box::new(
            GuardExpression::Variable("a".to_string()),
        )));

        assert!(compiled.may_be_true.eval([(base, false)]));
        assert!(!compiled.may_be_true.eval([(base, true)]));
    }

    /// Negating something undecided leaves it undecided, not decided the other way.
    #[test]
    fn negating_an_unknown_stays_unknown() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);

        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE);
        let compiled = compiler.compile(&GuardExpression::Not(Box::new(
            GuardExpression::Call("IsKimHere".to_string(), vec![]),
        )));

        assert!(compiled.may_be_true.valid());
        assert!(compiled.may_be_false.valid());
    }

    /// An item the group gains or loses is tracked, so the guard reads its slot.
    #[test]
    fn a_tracked_item_compiles_against_its_slot() {
        // GainItem is what interns an `item:` slot, so build the graph through the
        // action parser rather than by naming the slot directly.
        let mut symbols = StateSymbols::new();
        let actions = crate::parser::action_parser::parse_actions(
            r#"GainItem("shoes_faln")"#,
            &mut symbols,
        );
        let node = LookAheadNode::new(
            DialogueNodeId::new(1, 0), false, DialogueCheckKind::None,
            GuardExpression::always_true(), actions, vec![], 0, false, false, -1, -1, false, -1,
        );
        let snapshot = symbols.clone();
        let graph = LookAheadGraph::new(vec![node], symbols).unwrap();
        let layout = DataLayout::for_graph(&graph, 16, None, false);

        let slot = snapshot.find("item:shoes_faln").expect("GainItem interns an item slot");
        let base = layout.slot(slot).unwrap().0;

        let mut compiler = GuardCompiler::new(&layout, &snapshot, NODES, CACHE);
        let compiled = compiler.compile(&GuardExpression::Call(
            "CheckItem".to_string(),
            vec![GuardExpression::Literal(GuardValue::from_text("shoes_faln".to_string()))],
        ));

        assert!(compiled.may_be_true.eval([(base, true)]));
        assert!(!compiled.may_be_true.eval([(base, false)]));
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// An item no action in the group touches is constant, and the WORLD QUERY answers it.
    ///
    /// Through `query`, not `initially_has_item`, matching `BoundContext::query`. Answering from
    /// `initially_has_item` would be more decisive than the engine - it returns a plain bool even
    /// for a name the world never heard of - and deciding "not held" where the engine
    /// stays permissive would prune a branch the real crawl walks.
    #[test]
    fn an_untracked_item_is_answered_by_the_world_query() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let world = crate::world::test_world::TestWorld::new()
            .set_query_bool("CheckItem", true)
            // Set as an item too, to prove that is NOT what is being read.
            .set_item("ledger", false);

        let mut compiler =
            GuardCompiler::new(&layout, &symbols, NODES, CACHE).with_world(&world);
        let compiled = compiler.compile(&GuardExpression::Call(
            "CheckItem".to_string(),
            vec![GuardExpression::Literal(GuardValue::from_text("ledger".to_string()))],
        ));

        assert!(compiled.may_be_true.valid());
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// And where the world query cannot say, it stays open rather than being called false.
    #[test]
    fn an_untracked_item_the_world_cannot_answer_stays_open() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        // Knows the item, but answers no CheckItem query - exactly the shape that would
        // tempt a `initially_has_item` shortcut into deciding.
        let world = crate::world::test_world::TestWorld::new().set_item("ledger", false);

        let mut compiler =
            GuardCompiler::new(&layout, &symbols, NODES, CACHE).with_world(&world);
        let compiled = compiler.compile(&GuardExpression::Call(
            "CheckItem".to_string(),
            vec![GuardExpression::Literal(GuardValue::from_text("ledger".to_string()))],
        ));

        assert!(!compiled.is_decided());
        assert!(compiled.may_be_true.valid());
        assert_eq!(compiler.fallbacks(), 1);
    }

    #[test]
    fn a_task_question_reads_the_task_slot() {
        let mut symbols = StateSymbols::new();
        let actions = crate::parser::action_parser::parse_actions(
            r#"GainTask("TASK.find_ruby")"#,
            &mut symbols,
        );
        let node = LookAheadNode::new(
            DialogueNodeId::new(1, 0), false, DialogueCheckKind::None,
            GuardExpression::always_true(), actions, vec![], 0, false, false, -1, -1, false, -1,
        );
        let snapshot = symbols.clone();
        let graph = LookAheadGraph::new(vec![node], symbols).unwrap();
        let layout = DataLayout::for_graph(&graph, 16, None, false);

        let slot = snapshot.find("task:TASK.find_ruby").expect("GainTask interns a task slot");
        let base = layout.slot(slot).unwrap().0;

        let mut compiler = GuardCompiler::new(&layout, &snapshot, NODES, CACHE);
        let compiled = compiler.compile(&GuardExpression::Call(
            "IsTaskActive".to_string(),
            vec![GuardExpression::Literal(GuardValue::from_text("TASK.find_ruby".to_string()))],
        ));

        assert!(compiled.may_be_true.eval([(base, true)]));
        assert_eq!(compiler.fallbacks(), 0);
    }

    #[test]
    fn a_variable_the_graph_never_mentions_is_undecided() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);

        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE);
        let compiled = compiler.compile(&GuardExpression::Variable("never_heard_of_it".to_string()));

        assert!(!compiled.is_decided());
        assert_eq!(compiler.fallbacks(), 1);
    }
}
