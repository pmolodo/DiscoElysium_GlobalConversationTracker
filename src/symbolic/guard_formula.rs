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
use crate::core::guard_value::{GuardValue, GuardValueKind};
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
    /// Whether clock questions are answered from the world instead of being refused.
    constant_clock: bool,
    /// Whether that is an approximation for this group, rather than exact.
    clock_approximated: bool,
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

        Self {
            manager, vars, layout, symbols, world: None,
            constant_clock: false, clock_approximated: false,
            fallbacks: 0, compiled: 0, reasons: HashMap::new(),
        }
    }

    /// Gives the compiler a world to read untracked variables from.
    pub fn with_world(mut self, world: &'a dyn ILookAheadWorld) -> Self {
        self.world = Some(world);
        self
    }

    /// Answers clock questions from the world, as though the conversation never moved it.
    ///
    /// A DELIBERATE APPROXIMATION where the group can move the clock, and exact where it
    /// cannot - `group_passes_time` says which, and it should be true exactly when some
    /// action in the group is a `PassTime`.
    ///
    /// Why it is worth taking. Modelling the clock means eleven more variables and
    /// magnitude comparisons against them, which is the classic way to make a decision
    /// diagram explode; a guard like `IsHourBetween(14, 18)` is a range test on a
    /// bit-blasted integer. Against that, the engine advances the clock by fifteen
    /// minutes per `PassTime` and by nothing else, so a conversation rarely moves it far
    /// enough to change what a coarse question like `IsNight()` answers.
    ///
    /// What it costs. Where the group does move the clock, this can report a branch
    /// CLOSED that the real crawl would walk - the unsafe direction, and the only place
    /// in this compiler that is true. A guard that only opens once time has passed is
    /// judged against the starting hour and refused. Accepted knowingly; the count is
    /// exposed so the exposure is visible rather than assumed.
    ///
    /// A separate and larger question hangs over this: the engine's clock may not match
    /// the GAME's, which is understood to advance about a minute per unseen entry. The
    /// engine models no such thing, so its clock already lags. See de-sze.10.
    pub fn with_constant_clock(mut self, group_passes_time: bool) -> Self {
        self.constant_clock = true;
        self.clock_approximated = group_passes_time;
        self
    }

    /// Whether treating the clock as constant is an approximation for this group, rather
    /// than exact - true when some action in the group advances it.
    pub fn clock_is_approximated(&self) -> bool {
        self.constant_clock && self.clock_approximated
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
                            // Untracked, so nothing in this group can change it: the
                            // starting value is the only value, and the world answers
                            // directly. `BoundContext::query` does exactly the same, and
                            // the mirroring is the point - a compiler more decisive than
                            // the engine it models would prune branches the real crawl
                            // walks.
                            //
                            // What neither may do is answer this way for a TRACKED
                            // subject. Once GainItem has run the truth is in the state,
                            // and the starting inventory is stale.
                            None => match self.world {
                                Some(world) => {
                                    let held = if is_item {
                                        world.initially_has_item(&subject)
                                    } else {
                                        world.initially_task_active(&subject)
                                    };
                                    let f = if held { self.top() } else { self.bottom() };
                                    self.decided(f)
                                }
                                None => self.undecided("call: untracked and no world"),
                            },
                        }
                    }
                    None => self.undecided("call: subject is not a literal"),
                }
            }

            // The clock, held at whatever the world says and not moved by the
            // conversation. See `with_constant_clock` for why, and what it costs: this is
            // the one approximation here that can close a branch the crawl would walk.
            GuardExpression::Call(name, args)
                if self.constant_clock && crate::core::clock::ClockTime::owns(name) =>
            {
                match self.clock_answer(name, args) {
                    Some(true) => {
                        let t = self.top();
                        self.decided(t)
                    }
                    Some(false) => {
                        let f = self.bottom();
                        self.decided(f)
                    }
                    None => self.undecided("call: clock, world cannot say"),
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

        let (Some(name), Some(literal)) = (Self::variable_of(left), Self::literal_of(right))
        else {
            // Also try the other way round: a guard may be written `1 == Variable[..]`.
            // The operator has to turn with the operands - `3 <= x` is `x >= 3`, and
            // reading it as `x <= 3` would answer the opposite question everywhere the
            // two disagree.
            if let (Some(name), Some(literal)) =
                (Self::variable_of(right), Self::literal_of(left))
            {
                return self.comparison(Self::mirrored(op), &name, literal);
            }
            return self.undecided("comparison: neither side a known variable");
        };

        self.comparison(op, &name, literal)
    }

    /// The operator that means the same thing with its operands swapped.
    fn mirrored(op: &str) -> &str {
        match op {
            "<" => ">",
            "<=" => ">=",
            ">" => "<",
            ">=" => "<=",
            // Equality reads the same either way round.
            other => other,
        }
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

    /// `Variable[name] op literal`, where `op` is an equality.
    fn comparison(&mut self, op: &str, name: &str, literal: &GuardValue) -> MayBe {
        let equality = op == "==" || op == "~=";

        // Tracked: pin the slot's bits against the value.
        if let Some(value) = Self::whole_number(literal) {
            if equality {
                if let Some(equals) = self.slot_equals(name, value) {
                    let holds = if op == "~=" {
                        equals.not().expect("negation")
                    } else {
                        equals
                    };
                    return self.decided(holds);
                }
            } else if let Some(holds) = self.slot_ordered(name, op, value) {
                return self.decided(holds);
            }
        }

        // Untracked, so constant, and compared THE WAY THE ENGINE COMPARES: the world's
        // value against the literal, through GuardValue::equals, which is kind-sensitive
        // - a boolean never equals a number. Doing the comparison on a converted integer
        // instead would answer differently from the crawl for a variable the world
        // reports as a boolean.
        //
        // Without this an equality on an untracked variable fell back while a BARE
        // mention of the same variable did not, which was an inconsistency in this
        // compiler rather than anything about the content.
        let Some(world) = self.world else {
            return self.undecided("comparison: variable untracked and no world");
        };

        let actual = world.get_variable(name);
        if actual.kind() == GuardValueKind::Unknown {
            return self.undecided("comparison: variable untracked and world cannot say");
        }

        let holds = if equality {
            let same = actual.equals(literal);
            same != (op == "~=")
        } else {
            // Ordering on values, the way `GuardExpression::evaluate` does it: both sides
            // through `try_as_number`, and undecided where either will not convert.
            let (Some(a), Some(b)) = (actual.try_as_number(), literal.try_as_number()) else {
                return self.undecided("comparison: ordering on a non-numeric value");
            };
            match op {
                ">=" => a >= b,
                "<=" => a <= b,
                ">" => a > b,
                "<" => a < b,
                _ => return self.undecided("comparison: unknown operator"),
            }
        };

        let formula = if holds { self.top() } else { self.bottom() };
        self.decided(formula)
    }

    /// `Variable[name] op value` for an ordering operator, over a tracked slot.
    ///
    /// ## Why this is not the blowup the epic expected
    ///
    /// Magnitude comparison on bit-blasted integers is the classic way to make a decision
    /// diagram explode, and it is named in de-sze as the likely failure. It is not, for
    /// these slots, because THE COUNTER CAP BOUNDS THE WIDTH: a slot saturates at 16 by
    /// default, so it is five bits and holds 32 values. The set of values satisfying the
    /// comparison is enumerated and unioned, which costs at most one diagram operation
    /// per value and reuses [`Self::slot_equals`] rather than open-coding a comparator.
    ///
    /// What the warning was really about is MONEY and the CLOCK - a thirteen-bit balance
    /// and an eleven-bit minute count, compared against arbitrary constants. Neither is
    /// in this layout, and when one arrives it should get a proper ripple comparator
    /// rather than this.
    fn slot_ordered(&mut self, name: &str, op: &str, value: i32) -> Option<BDDFunction> {
        let slot = self.symbols.find(name)?;
        let (_, bits) = self.layout.slot(slot)?;
        let ceiling: i64 = if bits >= 32 { u32::MAX as i64 } else { (1i64 << bits) - 1 };

        let mut holds = self.bottom();
        for candidate in 0..=ceiling {
            let satisfies = match op {
                ">=" => candidate >= value as i64,
                "<=" => candidate <= value as i64,
                ">" => candidate > value as i64,
                "<" => candidate < value as i64,
                _ => return None,
            };
            if !satisfies {
                continue;
            }

            let at = self.slot_equals(name, candidate as i32)?;
            holds = holds.or(&at).expect("or");
        }

        Some(holds)
    }

    /// The name a `Variable` node carries, if the expression is one.
    fn variable_of(expression: &GuardExpression) -> Option<String> {
        match expression {
            GuardExpression::Variable(name) => Some(name.clone()),
            _ => None,
        }
    }

    /// The value a literal expression carries, if it is a literal.
    fn literal_of(expression: &GuardExpression) -> Option<&GuardValue> {
        match expression {
            GuardExpression::Literal(value) => Some(value),
            _ => None,
        }
    }

    /// The integer a literal stands for, if it is one a slot could hold.
    fn whole_number(value: &GuardValue) -> Option<i32> {
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

    /// What a clock question answers at the world's time, with the conversation ignored.
    ///
    /// Answered by `ClockTime` against the world's `day_minutes` and `day_counter`, which
    /// is exactly what the engine does for a crawl that has not moved the clock - not
    /// through `world.query`, which knows nothing about hours.
    fn clock_answer(&self, name: &str, args: &[GuardExpression]) -> Option<bool> {
        let world = self.world?;
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            let GuardExpression::Literal(value) = arg else { return None };
            values.push(value.clone());
        }

        let answer = crate::core::clock::ClockTime::answer(
            name,
            &values,
            world.day_minutes(),
            world.day_counter(),
        );
        match answer.as_condition() {
            Ternary::True => Some(true),
            Ternary::False => Some(false),
            Ternary::Unknown => None,
        }
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

    /// Which values of a slot a compiled formula admits, by trying them all.
    fn admitted(
        compiler: &GuardCompiler,
        layout: &DataLayout,
        slot: usize,
        formula: &BDDFunction,
    ) -> Vec<u32> {
        let (base, bits) = layout.slot(slot).unwrap();
        let ceiling = (1u32 << bits) - 1;
        let _ = compiler;
        (0..=ceiling)
            .filter(|value| {
                let assignment: Vec<(u32, bool)> =
                    (0..bits as u32).map(|b| (base + b, (value >> b) & 1 == 1)).collect();
                formula.eval(assignment.iter().copied())
            })
            .collect()
    }

    /// The comparison the epic named as the likely blowup, and it compiles.
    ///
    /// It is affordable here because the counter cap bounds the slot to five bits. Money
    /// and the clock, which are not in this layout, are the case the warning was about.
    #[test]
    fn an_ordering_comparison_admits_exactly_the_values_that_satisfy_it() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let slot = symbols.find("counter").unwrap();

        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE);
        let compiled = compiler.compile(&GuardExpression::Comparison(
            ">=".to_string(),
            Box::new(GuardExpression::Variable("counter".to_string())),
            Box::new(number(3.0)),
        ));

        assert!(compiled.is_decided());
        assert_eq!(compiler.fallbacks(), 0);
        assert_eq!(
            admitted(&compiler, &layout, slot, &compiled.may_be_true),
            (3..=31).collect::<Vec<u32>>()
        );
        // The other rail is its complement, which is what makes it decided.
        assert_eq!(
            admitted(&compiler, &layout, slot, &compiled.may_be_false),
            (0..=2).collect::<Vec<u32>>()
        );
    }

    /// `3 <= x` is `x >= 3`, so the operator has to turn with the operands.
    ///
    /// Reading it as `x <= 3` would answer the opposite question on every value but 3,
    /// and it would still look decided - the failure would be silent.
    #[test]
    fn an_ordering_comparison_written_backwards_keeps_its_meaning() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let slot = symbols.find("counter").unwrap();

        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE);
        let compiled = compiler.compile(&GuardExpression::Comparison(
            "<=".to_string(),
            Box::new(number(3.0)),
            Box::new(GuardExpression::Variable("counter".to_string())),
        ));

        assert_eq!(
            admitted(&compiler, &layout, slot, &compiled.may_be_true),
            (3..=31).collect::<Vec<u32>>()
        );
    }

    /// An ordering comparison on a variable no action writes is a constant, and the
    /// world answers it - the same rule equality already followed.
    #[test]
    fn an_ordering_comparison_on_an_untracked_variable_is_answered_by_the_world() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let world = crate::world::test_world::TestWorld::new()
            .set_variable("untracked", GuardValue::from_number(5.0));

        let mut compiler =
            GuardCompiler::new(&layout, &symbols, NODES, CACHE).with_world(&world);
        let holds = compiler.compile(&GuardExpression::Comparison(
            ">=".to_string(),
            Box::new(GuardExpression::Variable("untracked".to_string())),
            Box::new(number(3.0)),
        ));
        let fails = compiler.compile(&GuardExpression::Comparison(
            ">=".to_string(),
            Box::new(GuardExpression::Variable("untracked".to_string())),
            Box::new(number(9.0)),
        ));

        assert_eq!(compiler.fallbacks(), 0);
        assert!(holds.may_be_true.valid());
        assert!(!fails.may_be_true.satisfiable());
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

    /// An item no action in the group touches is constant, and the WORLD answers it.
    ///
    /// From the world's inventory, not from a `CheckItem` query: a world that answers no
    /// query at all still settles this, because the inventory it was built with IS the
    /// answer when nothing can change it. `BoundContext::query` does the same, and the
    /// two must agree.
    #[test]
    fn an_untracked_item_is_answered_from_the_worlds_inventory() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        // Deliberately answers no query, to show that is not what settles it.
        let world = crate::world::test_world::TestWorld::new().set_item("ledger", true);

        let mut compiler =
            GuardCompiler::new(&layout, &symbols, NODES, CACHE).with_world(&world);
        let held = compiler.compile(&GuardExpression::Call(
            "CheckItem".to_string(),
            vec![GuardExpression::Literal(GuardValue::from_text("ledger".to_string()))],
        ));
        let absent = compiler.compile(&GuardExpression::Call(
            "CheckItem".to_string(),
            vec![GuardExpression::Literal(GuardValue::from_text("nothing".to_string()))],
        ));

        assert!(held.may_be_true.valid());
        assert!(held.is_decided());
        assert!(!absent.may_be_true.satisfiable());
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// A TRACKED item is never answered from the world, however tempting.
    ///
    /// The starting inventory is stale the moment GainItem runs, so reading it for a
    /// tracked item would make the crawl blind to its own purchases. That is the mirror
    /// of the mistake the untracked case invites, and the reason both sit behind one
    /// deliberately-named pair of methods.
    #[test]
    fn a_tracked_item_ignores_the_worlds_starting_inventory() {
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
        let base = layout.slot(snapshot.find("item:shoes_faln").unwrap()).unwrap().0;

        // The world says the player does NOT have them. The slot must still decide, so
        // that a path which buys them is seen.
        let world = crate::world::test_world::TestWorld::new().set_item("shoes_faln", false);
        let mut compiler =
            GuardCompiler::new(&layout, &snapshot, NODES, CACHE).with_world(&world);
        let compiled = compiler.compile(&GuardExpression::Call(
            "CheckItem".to_string(),
            vec![GuardExpression::Literal(GuardValue::from_text("shoes_faln".to_string()))],
        ));

        assert!(compiled.may_be_true.eval([(base, true)]));
        assert!(!compiled.may_be_true.eval([(base, false)]));
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

    /// With the clock held constant, a clock question resolves against the world's time.
    #[test]
    fn a_clock_question_is_answered_at_the_worlds_time() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        // Two in the morning.
        let night = crate::world::test_world::TestWorld::new().with_day_minutes(2 * 60);

        let mut compiler = GuardCompiler::new(&layout, &symbols, NODES, CACHE)
            .with_world(&night)
            .with_constant_clock(false);
        let compiled =
            compiler.compile(&GuardExpression::Call("IsNight".to_string(), vec![]));

        assert!(compiled.is_decided());
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// And it is refused, not guessed, when nothing has been told to hold it constant.
    #[test]
    fn a_clock_question_is_undecided_without_the_approximation() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let night = crate::world::test_world::TestWorld::new().with_day_minutes(2 * 60);

        let mut compiler =
            GuardCompiler::new(&layout, &symbols, NODES, CACHE).with_world(&night);
        let compiled =
            compiler.compile(&GuardExpression::Call("IsNight".to_string(), vec![]));

        assert!(!compiled.is_decided());
        assert_eq!(compiler.fallbacks(), 1);
    }

    /// The approximation is only an approximation where the group can move the clock.
    #[test]
    fn holding_the_clock_is_exact_unless_the_group_passes_time() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let world = crate::world::test_world::TestWorld::new();

        let exact = GuardCompiler::new(&layout, &symbols, NODES, CACHE)
            .with_world(&world)
            .with_constant_clock(false);
        assert!(!exact.clock_is_approximated());

        let approximate = GuardCompiler::new(&layout, &symbols, NODES, CACHE)
            .with_world(&world)
            .with_constant_clock(true);
        assert!(approximate.clock_is_approximated());
    }

    /// A graph with a PassTime action is one where holding the clock is an approximation.
    #[test]
    fn a_group_that_passes_time_is_detected() {
        let mut symbols = StateSymbols::new();
        let still = crate::parser::action_parser::parse_actions("", &mut symbols);
        let moving = crate::parser::action_parser::parse_actions("PassTime()", &mut symbols);

        let node = |actions| {
            LookAheadNode::new(
                DialogueNodeId::new(1, 0), false, DialogueCheckKind::None,
                GuardExpression::always_true(), actions, vec![], 0, false, false, -1, -1,
                false, -1,
            )
        };

        let quiet = LookAheadGraph::new(vec![node(still)], StateSymbols::new()).unwrap();
        assert!(!DataLayout::group_passes_time(&quiet));

        let ticking = LookAheadGraph::new(vec![node(moving)], symbols).unwrap();
        assert!(DataLayout::group_passes_time(&ticking));
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
