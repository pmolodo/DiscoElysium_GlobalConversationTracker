// SPDX-License-Identifier: MIT
//! Arithmetic on a run of decision-diagram variables holding one number.
//!
//! ## What this is for
//!
//! Money and the clock. Every symbolic measurement so far has left both outside the
//! layout, so the arithmetic that de-sze predicted as the likely blowup had never been
//! run once - a different failure arrived first. The reason they were left out is written
//! down in `ActionImage`: an increment is done by splitting the set over the slot's
//! possible values, which is fine for a counter capped at sixteen and absurd for thirteen
//! bits of money or eleven of clock. Eight thousand cases is not an encoding.
//!
//! ## Comparisons cost bits, not values
//!
//! `v >= k` over an n-bit run is a chain of n decisions, not a union of `2^n - k`
//! equalities. Written the second way - which is what a counter's five bits could get
//! away with - money's thirteen bits would build eight thousand conjunctions to say one
//! thing.
//!
//! ## Shifts are substitutions, not relations
//!
//! Adding a constant is a BIJECTION on `0..2^n`, and the image of a set under a bijection
//! is the pre-image under its inverse. Pre-image is exactly what substitution computes:
//! `S[v := v - k]` is the set of `w` with `w - k` in `S`, which is `{ s + k : s in S }`.
//!
//! So no primed variables, no transition relation, and no case split: one substitution
//! whose replacements are a ripple-borrow subtractor, O(n) diagram nodes per bit.
//!
//! The operations that are NOT bijections - saturating at a ceiling, clamping at zero,
//! wrapping at midnight - are each two bijections and a range test, because the set is
//! split on the range first and each piece shifted on its own. Saturation is the one
//! piece that is not a shift at all: every state at or above the ceiling collapses to the
//! ceiling, which is an assignment.

use oxidd::Subst;
use oxidd::bdd::BDDFunction;
use oxidd::{BooleanFunction, BooleanFunctionQuant, FunctionSubst};

/// One number, as a little-endian run of variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Register {
    /// The variable number of the low bit.
    pub base: u32,
    /// How many variables the run covers.
    pub bits: u8,
}

impl Register {
    pub fn new(base: u32, bits: u8) -> Self {
        Self { base, bits }
    }

    /// The largest value the run can hold.
    pub fn ceiling(&self) -> u32 {
        if self.bits >= 32 {
            u32::MAX
        } else {
            (1u32 << self.bits) - 1
        }
    }

    /// The variable numbers, low bit first.
    pub fn vars(&self) -> impl Iterator<Item = u32> + '_ {
        (0..self.bits as u32).map(move |bit| self.base + bit)
    }
}

/// Builds formulas and images over one register.
///
/// Holds the variables rather than looking them up, because every operation here touches
/// all of them and a register is small.
///
/// ## `None` MEANS THE MANAGER RAN OUT OF ROOM
///
/// Every method here returns an option and every `None` says the same thing: a diagram
/// operation could not complete for want of nodes, so there is no answer at all. It is
/// never "the empty set" and never "nothing to do" - a caller that reads it as either
/// reports a settled verdict it has no evidence for, which is the failure that looks like
/// success.
///
/// Running out of nodes is a RESULT rather than a fault: the player's manager is a budget
/// small enough for a wide purse to reach, and a register is where a search stands when
/// it does. Unwrapping here aborts the process - not a panic a host can turn into a
/// partial answer, and not a row a measurement can keep.
///
/// [`Self::compare`] folds one more case into the same `None`: an operator this does not
/// know. Both are "no formula, fall back rather than answer a different question", and
/// every caller does the same thing about them, so they are not worth telling apart.
pub struct RegisterOps<'a> {
    register: Register,
    /// The register's variables, low bit first.
    vars: Vec<&'a BDDFunction>,
    top: BDDFunction,
    bottom: BDDFunction,
}

impl<'a> RegisterOps<'a> {
    /// Creates the operations for `register`, whose variables `var` resolves.
    pub fn new(
        register: Register,
        top: BDDFunction,
        bottom: BDDFunction,
        var: impl Fn(u32) -> &'a BDDFunction,
    ) -> Self {
        let vars = register.vars().map(&var).collect();
        Self {
            register,
            vars,
            top,
            bottom,
        }
    }

    pub fn register(&self) -> Register {
        self.register
    }

    /// The conjunction of every variable, which is the cube to quantify over.
    pub fn cube(&self) -> Option<BDDFunction> {
        let mut cube = self.top.clone();
        for var in &self.vars {
            cube = cube.and(var).ok()?;
        }

        Some(cube)
    }

    /// "The register holds exactly `value`."
    ///
    /// A value the register is too narrow to hold gives the EMPTY SET rather than `None`:
    /// the equality is false everywhere, which is an answer.
    pub fn equals(&self, value: u32) -> Option<BDDFunction> {
        if value > self.register.ceiling() {
            return Some(self.bottom.clone());
        }

        let mut all = self.top.clone();
        for (bit, var) in self.vars.iter().enumerate() {
            let literal = if (value >> bit) & 1 == 1 {
                (*var).clone()
            } else {
                var.not().ok()?
            };
            all = all.and(&literal).ok()?;
        }

        Some(all)
    }

    /// "The register holds at least `value`."
    ///
    /// Built from the top bit down, which is what makes it O(bits): at each bit, either
    /// the register's bit is set where `value`'s is clear - and everything below is then
    /// free - or the two agree and the question moves down one bit.
    pub fn at_least(&self, value: u32) -> Option<BDDFunction> {
        if value == 0 {
            return Some(self.top.clone());
        }

        if value > self.register.ceiling() {
            return Some(self.bottom.clone());
        }

        // `greater` holds where the register is already strictly above `value`'s prefix;
        // `equal` where it has matched it exactly so far. Walking down, a register bit
        // that is set against a clear one in `value` settles it upward for good.
        let mut greater = self.bottom.clone();
        let mut equal = self.top.clone();
        for bit in (0..self.register.bits as usize).rev() {
            let var = self.vars[bit];
            let wanted = (value >> bit) & 1 == 1;
            if wanted {
                // The register must have this bit too to stay equal; there is no way to
                // become strictly greater at a bit where `value` is already 1.
                equal = equal.and(var).ok()?;
            } else {
                let above = equal.and(var).ok()?;
                greater = greater.or(&above).ok()?;
                equal = equal.and(&var.not().ok()?).ok()?;
            }
        }

        // At or above: strictly above at some bit, or equal all the way down.
        greater.or(&equal).ok()
    }

    /// "The register holds at most `value`."
    pub fn at_most(&self, value: u32) -> Option<BDDFunction> {
        if value >= self.register.ceiling() {
            return Some(self.top.clone());
        }

        self.at_least(value + 1)?.not().ok()
    }

    /// A comparison against a constant, spelled as the guard language spells it.
    ///
    /// `None` for an operator this does not know, and for a manager with no room left to
    /// say it in. Either way a caller falls back rather than quietly answering a different
    /// question, which is why the two are not told apart.
    pub fn compare(&self, operator: &str, value: i64) -> Option<BDDFunction> {
        // A negative constant is not representable and every register is unsigned, so the
        // answer is settled by the operator alone without looking at a bit.
        if value < 0 {
            return match operator {
                ">" | ">=" | "~=" | "!=" => Some(self.top.clone()),
                "<" | "<=" | "==" => Some(self.bottom.clone()),
                _ => None,
            };
        }

        let value = value.min(u32::MAX as i64) as u32;
        match operator {
            "==" => self.equals(value),
            "~=" | "!=" => self.equals(value)?.not().ok(),
            ">=" => self.at_least(value),
            ">" => {
                if value == u32::MAX {
                    Some(self.bottom.clone())
                } else {
                    self.at_least(value + 1)
                }
            }
            "<=" => self.at_most(value),
            "<" => {
                if value == 0 {
                    Some(self.bottom.clone())
                } else {
                    self.at_most(value - 1)
                }
            }
            _ => None,
        }
    }

    /// `register := value`, over a whole set.
    ///
    /// Forget what it held, then assert the new value. The quantifier is what makes this
    /// an assignment rather than a filter.
    pub fn assign(&self, states: &BDDFunction, value: u32) -> Option<BDDFunction> {
        let forgotten = states.exists(&self.cube()?).ok()?;
        forgotten.and(&self.equals(value)?).ok()
    }

    /// The image of `states` under `register := register + delta`, modulo `2^bits`.
    ///
    /// Exact and total: adding a constant modulo a power of two is a bijection, so nothing
    /// is lost and nothing is invented. The pieces that are not bijections are built out
    /// of this one by splitting the set on a range first - see [`Self::wrapping_add`] and
    /// [`Self::saturating_add`].
    ///
    /// Returns `None` only if the diagram manager ran out of room.
    pub fn shift(&self, states: &BDDFunction, delta: i64) -> Option<BDDFunction> {
        let width = self.register.bits as u32;
        let modulus = if width >= 32 { 0u64 } else { 1u64 << width };
        let delta = if modulus == 0 {
            (delta as i128).rem_euclid(1i128 << 32) as u64
        } else {
            (delta as i128).rem_euclid(modulus as i128) as u64
        };

        if delta == 0 {
            return Some(states.clone());
        }

        // The image under `+delta` is the pre-image under `-delta`, and substitution
        // computes pre-images. So each variable is replaced by the matching bit of
        // `register - delta`.
        let replacements = self.minus(delta as u32)?;
        let numbers: Vec<oxidd::VarNo> = self.register.vars().collect();
        let substitution = Subst::new(numbers, replacements);
        states.substitute(&substitution).ok()
    }

    /// Each bit of `register - amount`, as a function of the register's own variables.
    ///
    /// A ripple-borrow subtractor. `O(bits)` operations, and the diagrams it produces are
    /// the reason a shift is affordable where a case split over `2^bits` values is not.
    fn minus(&self, amount: u32) -> Option<Vec<BDDFunction>> {
        let mut difference = Vec::with_capacity(self.vars.len());
        let mut borrow = self.bottom.clone();

        for (bit, var) in self.vars.iter().enumerate() {
            let subtrahend = (amount >> bit) & 1 == 1;

            // difference = var XOR subtrahend XOR borrow
            let without_borrow = if subtrahend {
                var.not().ok()?
            } else {
                (*var).clone()
            };
            difference.push(without_borrow.xor(&borrow).ok()?);

            // borrow out: the register's bit is clear and something has to be taken from
            // above, or it is set and both the subtrahend and the incoming borrow take
            // from it.
            let clear = var.not().ok()?;
            borrow = if subtrahend {
                // 1 - borrow needs a borrow whenever the bit is clear, or the bit is set
                // and a borrow was already coming in... which is the same as "clear or
                // borrow".
                clear.or(&borrow).ok()?
            } else {
                clear.and(&borrow).ok()?
            };
        }

        Some(difference)
    }

    /// The image under `register := (register + delta) mod modulus`.
    ///
    /// For the clock, where midnight is not a ceiling but a wrap. States holding a value at
    /// or above `modulus` are outside the model and are dropped rather than wrapped, since
    /// nothing should have put one there.
    pub fn wrapping_add(
        &self,
        states: &BDDFunction,
        delta: u32,
        modulus: u32,
    ) -> Option<BDDFunction> {
        if modulus == 0 {
            return Some(states.clone());
        }

        let delta = delta % modulus;
        let in_range = self.at_most(modulus - 1)?;
        let states = states.and(&in_range).ok()?;
        if delta == 0 {
            return Some(states);
        }

        // Below the wrap: the value simply moves up, and cannot pass the modulus.
        let below = states.and(&self.at_most(modulus - 1 - delta)?).ok()?;
        let moved = self.shift(&below, delta as i64)?;

        // At or above it: the value moves up and then loses a whole modulus, which is one
        // shift by `delta - modulus` rather than two operations.
        let over = states.and(&self.at_least(modulus - delta)?).ok()?;
        let wrapped = self.shift(&over, delta as i64 - modulus as i64)?;

        // Each piece is intersected back into the range it must land in. The shift is a
        // bijection so this cannot remove anything real; it is here so that a register too
        // narrow for `modulus` cannot smuggle a value in from the far side of the wrap.
        let moved = moved.and(&self.at_least(delta)?).ok()?;
        let moved = moved.and(&self.at_most(modulus - 1)?).ok()?;
        let wrapped = wrapped.and(&self.at_most(delta - 1)?).ok()?;

        moved.or(&wrapped).ok()
    }

    /// The image under `register := min(register + amount, ceiling)`.
    ///
    /// For money, where the ceiling is the width the layout gave it. Saturation is not a
    /// shift: every state at or above the ceiling collapses onto it, which is where a
    /// symbolic set genuinely loses information about what it held - and it is the honest
    /// place to lose it, since a register that narrow could not have held the answer.
    pub fn saturating_add(&self, states: &BDDFunction, amount: u32) -> Option<BDDFunction> {
        let ceiling = self.register.ceiling();
        if amount == 0 {
            return Some(states.clone());
        }

        if amount >= ceiling {
            return self.assign(states, ceiling);
        }

        let below = states.and(&self.at_most(ceiling - amount)?).ok()?;
        let moved = self.shift(&below, amount as i64)?;
        let moved = moved.and(&self.at_least(amount)?).ok()?;

        let over = states.and(&self.at_least(ceiling - amount + 1)?).ok()?;
        let saturated = self.assign(&over, ceiling)?;

        moved.or(&saturated).ok()
    }

    /// The states from which `register := (register + delta) mod modulus` lands in `states`.
    ///
    /// A wrap is a bijection, so its pre-image is the image under the opposite shift and
    /// nothing more.
    pub fn pre_wrapping_add(
        &self,
        states: &BDDFunction,
        delta: u32,
        modulus: u32,
    ) -> Option<BDDFunction> {
        if modulus == 0 {
            return Some(states.clone());
        }

        self.wrapping_add(states, modulus - (delta % modulus), modulus)
    }

    /// The states from which `register := min(register + amount, ceiling)` lands in `states`.
    ///
    /// Saturation is many-to-one - every value at or above `ceiling - amount` lands on the
    /// ceiling - so the pre-image of the ceiling is a RANGE rather than a point. Getting
    /// that half wrong would make the backward search report an entry unreachable that the
    /// search walks to, which is the one error direction this file is not allowed.
    pub fn pre_saturating_add(&self, states: &BDDFunction, amount: u32) -> Option<BDDFunction> {
        let ceiling = self.register.ceiling();
        if amount == 0 {
            return Some(states.clone());
        }

        // Landed below the ceiling: exactly one value came here, and it is this one minus
        // the amount. Only values at or above `amount` can have been reached by adding it.
        //
        // AT OR ABOVE, not merely above. Adding the ceiling itself to any value the
        // register can hold reaches the ceiling, so nothing can land below it and the
        // branch is empty - the same answer the forward `saturating_add` gives, which
        // sends everything to the ceiling from `amount >= ceiling` upwards. Guarding only
        // the greater-than case left `ceiling - amount - 1` to evaluate 0 - 1 on unsigned.
        let below = if amount >= ceiling {
            self.bottom.clone()
        } else {
            let landed = states.and(&self.at_least(amount)?).ok()?;
            let landed = landed.and(&self.at_most(ceiling - 1)?).ok()?;
            let came = self.shift(&landed, -(amount as i64))?;
            came.and(&self.at_most(ceiling - amount - 1)?).ok()?
        };

        // Landed ON the ceiling: everything from `ceiling - amount` upwards did, and so
        // did `ceiling - amount` itself only if the addition reaches it. Forgetting the
        // register first is what turns the point into the range.
        let at_top = states.and(&self.equals(ceiling)?).ok()?;
        let forgotten = at_top.exists(&self.cube()?).ok()?;
        let from = ceiling.saturating_sub(amount);
        let above = forgotten.and(&self.at_least(from)?).ok()?;

        below.or(&above).ok()
    }

    /// The states from which `register := max(register - amount, 0)` lands in `states`.
    pub fn pre_saturating_sub(&self, states: &BDDFunction, amount: u32) -> Option<BDDFunction> {
        if amount == 0 {
            return Some(states.clone());
        }

        let ceiling = self.register.ceiling();

        // Landed above zero: one value came here, this one plus the amount, and it has to
        // fit.
        let above = if amount > ceiling {
            self.bottom.clone()
        } else {
            let landed = states.and(&self.at_least(1)?).ok()?;
            let landed = landed.and(&self.at_most(ceiling - amount)?).ok()?;
            let came = self.shift(&landed, amount as i64)?;
            came.and(&self.at_least(amount)?).ok()?
        };

        // Landed on zero: everything at or below the amount did.
        let at_zero = states.and(&self.equals(0)?).ok()?;
        let forgotten = at_zero.exists(&self.cube()?).ok()?;
        let below = forgotten.and(&self.at_most(amount.min(ceiling))?).ok()?;

        above.or(&below).ok()
    }

    /// The image under `register := max(register - amount, 0)`.
    pub fn saturating_sub(&self, states: &BDDFunction, amount: u32) -> Option<BDDFunction> {
        if amount == 0 {
            return Some(states.clone());
        }

        let ceiling = self.register.ceiling();
        if amount > ceiling {
            return self.assign(states, 0);
        }

        let above = states.and(&self.at_least(amount)?).ok()?;
        let moved = self.shift(&above, -(amount as i64))?;
        let moved = moved.and(&self.at_most(ceiling - amount)?).ok()?;

        let below = states.and(&self.at_most(amount - 1)?).ok()?;
        let floored = self.assign(&below, 0)?;

        moved.or(&floored).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use oxidd::Function;
    use oxidd::bdd::new_manager;
    use oxidd::{Manager, ManagerRef};

    const NODES: usize = 1 << 18;
    const CACHE: usize = 1 << 16;

    /// A manager holding one register and nothing else.
    struct Bench {
        _manager: oxidd::bdd::BDDManagerRef,
        vars: Vec<BDDFunction>,
        register: Register,
        top: BDDFunction,
        bottom: BDDFunction,
    }

    impl Bench {
        fn new(bits: u8) -> Self {
            Self::within(bits, NODES)
        }

        /// The same, with the manager held to `nodes` so that it can be filled.
        fn within(bits: u8, nodes: usize) -> Self {
            let manager = new_manager(nodes, CACHE, 1);
            let vars: Vec<BDDFunction> = manager.with_manager_exclusive(|m| {
                m.add_vars(bits as u32)
                    .map(|v| BDDFunction::var(m, v).expect("a fresh variable"))
                    .collect()
            });
            let top = manager.with_manager_shared(BDDFunction::t);
            let bottom = manager.with_manager_shared(BDDFunction::f);
            Self {
                _manager: manager,
                vars,
                register: Register::new(0, bits),
                top,
                bottom,
            }
        }

        fn ops(&self) -> RegisterOps<'_> {
            RegisterOps::new(
                self.register,
                self.top.clone(),
                self.bottom.clone(),
                |number| &self.vars[number as usize],
            )
        }

        /// The assignment that holds `value`.
        fn at(&self, value: u32) -> Vec<(u32, bool)> {
            (0..self.register.bits as u32)
                .map(|bit| (bit, (value >> bit) & 1 == 1))
                .collect()
        }

        /// Every value a formula holds.
        fn values(&self, formula: &BDDFunction) -> Vec<u32> {
            (0..=self.register.ceiling())
                .filter(|value| formula.eval(self.at(*value).iter().copied()))
                .collect()
        }

        /// A formula holding exactly `values`.
        fn set(&self, values: &[u32]) -> BDDFunction {
            let ops = self.ops();
            let mut all = self.bottom.clone();
            for value in values {
                all = all.or(&ops.equals(*value).expect("room")).expect("or");
            }

            all
        }
    }

    /// Every comparison, against every value, at every width up to five bits.
    ///
    /// Brute force on purpose. A comparator is the kind of thing that is right for every
    /// value but one, and the one is always at a boundary - zero, the ceiling, a power of
    /// two - so checking a few interesting cases by hand is exactly how such a bug
    /// survives.
    #[test]
    fn every_comparison_answers_what_arithmetic_answers() {
        for bits in 1..=5u8 {
            let bench = Bench::new(bits);
            let ops = bench.ops();
            let ceiling = ops.register().ceiling();

            for constant in 0..=(ceiling as i64 + 2) {
                for operator in ["==", "~=", "<", "<=", ">", ">="] {
                    let formula = ops.compare(operator, constant).expect("a known operator");
                    for value in 0..=ceiling {
                        let expected = match operator {
                            "==" => value as i64 == constant,
                            "~=" => value as i64 != constant,
                            "<" => (value as i64) < constant,
                            "<=" => value as i64 <= constant,
                            ">" => value as i64 > constant,
                            ">=" => value as i64 >= constant,
                            other => panic!("unlisted operator {other}"),
                        };
                        assert_eq!(
                            formula.eval(bench.at(value).iter().copied()),
                            expected,
                            "{bits} bits: {value} {operator} {constant}",
                        );
                    }
                }
            }
        }
    }

    /// A negative constant is answered by the operator, since no register can hold one.
    #[test]
    fn a_negative_constant_is_decided_without_looking_at_a_bit() {
        let bench = Bench::new(4);
        let ops = bench.ops();

        assert_eq!(bench.values(&ops.compare(">=", -1).unwrap()).len(), 16);
        assert!(bench.values(&ops.compare("<", -1).unwrap()).is_empty());
        assert!(bench.values(&ops.compare("==", -1).unwrap()).is_empty());
        assert_eq!(bench.values(&ops.compare("~=", -1).unwrap()).len(), 16);
        assert!(
            ops.compare("<=>", -1).is_none(),
            "an operator nobody knows must not answer"
        );
    }

    /// A shift moves every value, and moves it back.
    #[test]
    fn a_shift_is_the_bijection_it_claims_to_be() {
        for bits in 1..=5u8 {
            let bench = Bench::new(bits);
            let ops = bench.ops();
            let modulus = 1u32 << bits;

            for delta in 0..modulus as i64 {
                let all = ops.shift(&bench.top, delta).expect("room");
                assert_eq!(
                    bench.values(&all).len(),
                    modulus as usize,
                    "{bits} bits, +{delta}: a bijection must not lose a value",
                );

                for value in 0..modulus {
                    let one = bench.set(&[value]);
                    let moved = ops.shift(&one, delta).expect("room");
                    assert_eq!(
                        bench.values(&moved),
                        vec![(value + delta as u32) % modulus],
                        "{bits} bits: {value} + {delta}",
                    );
                }
            }
        }
    }

    /// Shifting down is shifting up by a negative, and undoes it.
    #[test]
    fn shifting_back_returns_what_was_shifted() {
        let bench = Bench::new(5);
        let ops = bench.ops();
        let some = bench.set(&[0, 1, 7, 30, 31]);

        for delta in -40..40i64 {
            let there = ops.shift(&some, delta).expect("room");
            let back = ops.shift(&there, -delta).expect("room");
            assert_eq!(bench.values(&back), bench.values(&some), "delta {delta}");
        }
    }

    /// The clock's wrap, against the arithmetic it stands for.
    #[test]
    fn a_wrapping_add_is_the_modulus_arithmetic() {
        let bench = Bench::new(5);
        let ops = bench.ops();

        // Not a power of two, which is the whole difficulty: the register holds 32 values
        // and the model uses 24 of them, exactly as eleven bits hold 2,048 and the clock
        // uses 1,440.
        const MODULUS: u32 = 24;
        for delta in 0..MODULUS {
            for value in 0..MODULUS {
                let one = bench.set(&[value]);
                let moved = ops.wrapping_add(&one, delta, MODULUS).expect("room");
                assert_eq!(
                    bench.values(&moved),
                    vec![(value + delta) % MODULUS],
                    "{value} + {delta} mod {MODULUS}",
                );
            }
        }
    }

    /// A whole set wraps as one, and every value in it lands where it should.
    #[test]
    fn a_wrapping_add_moves_a_whole_set_at_once() {
        let bench = Bench::new(5);
        let ops = bench.ops();
        const MODULUS: u32 = 24;

        let some = bench.set(&[0, 5, 22, 23]);
        let moved = ops.wrapping_add(&some, 3, MODULUS).expect("room");
        assert_eq!(bench.values(&moved), vec![1, 2, 3, 8]);
    }

    /// A value outside the modulus is dropped rather than wrapped into the model.
    #[test]
    fn a_value_beyond_the_modulus_is_not_carried_across() {
        let bench = Bench::new(5);
        let ops = bench.ops();

        // 30 is representable in five bits and is not a time of day.
        let bad = bench.set(&[30]);
        let moved = ops.wrapping_add(&bad, 3, 24).expect("room");
        assert!(bench.values(&moved).is_empty());
    }

    /// Saturating addition, against the arithmetic it stands for.
    #[test]
    fn a_saturating_add_stops_at_the_ceiling() {
        for bits in 1..=5u8 {
            let bench = Bench::new(bits);
            let ops = bench.ops();
            let ceiling = ops.register().ceiling();

            for amount in 0..=ceiling + 1 {
                for value in 0..=ceiling {
                    let one = bench.set(&[value]);
                    let moved = ops.saturating_add(&one, amount).expect("room");
                    assert_eq!(
                        bench.values(&moved),
                        vec![value.saturating_add(amount).min(ceiling)],
                        "{bits} bits: {value} + {amount}",
                    );
                }
            }
        }
    }

    /// Saturating subtraction, against the arithmetic it stands for.
    #[test]
    fn a_saturating_sub_stops_at_zero() {
        for bits in 1..=5u8 {
            let bench = Bench::new(bits);
            let ops = bench.ops();
            let ceiling = ops.register().ceiling();

            for amount in 0..=ceiling + 1 {
                for value in 0..=ceiling {
                    let one = bench.set(&[value]);
                    let moved = ops.saturating_sub(&one, amount).expect("room");
                    assert_eq!(
                        bench.values(&moved),
                        vec![value.saturating_sub(amount)],
                        "{bits} bits: {value} - {amount}",
                    );
                }
            }
        }
    }

    /// Two different values that saturate onto the same one become one value.
    ///
    /// The place a symbolic set genuinely loses information, and it is worth pinning: it
    /// is the honest loss, because a register that narrow could not have held the answer
    /// either way.
    #[test]
    fn saturation_merges_what_it_cannot_tell_apart() {
        let bench = Bench::new(4);
        let ops = bench.ops();

        let two = bench.set(&[14, 15]);
        let moved = ops.saturating_add(&two, 3).expect("room");
        assert_eq!(bench.values(&moved), vec![15]);
    }

    /// An assignment forgets what was there, rather than filtering for it.
    #[test]
    fn an_assignment_forgets_what_the_register_held() {
        let bench = Bench::new(4);
        let ops = bench.ops();

        let some = bench.set(&[1, 2, 3]);
        let assigned = ops.assign(&some, 9).expect("room");
        assert_eq!(bench.values(&assigned), vec![9]);
    }

    /// Every pre-image is exactly the set of values the operation maps into the target.
    ///
    /// Brute force over every width, every amount and every target set of one value,
    /// because the backward search's one forbidden error is a pre-image that is TOO SMALL:
    /// it reports an entry unreachable that the search walks to. A missing value here is
    /// that bug, and it would hide behind any test that only checked a few cases.
    #[test]
    fn every_pre_image_is_exactly_what_lands_in_the_target() {
        for bits in 1..=5u8 {
            let bench = Bench::new(bits);
            let ops = bench.ops();
            let ceiling = ops.register().ceiling();

            for amount in 0..=ceiling + 1 {
                for target in 0..=ceiling {
                    let landed = bench.set(&[target]);

                    let expected: Vec<u32> = (0..=ceiling)
                        .filter(|v| v.saturating_add(amount).min(ceiling) == target)
                        .collect();
                    assert_eq!(
                        bench.values(&ops.pre_saturating_add(&landed, amount).expect("room")),
                        expected,
                        "{bits} bits: what lands on {target} after +{amount}",
                    );

                    let expected: Vec<u32> = (0..=ceiling)
                        .filter(|v| v.saturating_sub(amount) == target)
                        .collect();
                    assert_eq!(
                        bench.values(&ops.pre_saturating_sub(&landed, amount).expect("room")),
                        expected,
                        "{bits} bits: what lands on {target} after -{amount}",
                    );
                }
            }
        }
    }

    /// The clock's pre-image is its wrap run the other way.
    #[test]
    fn a_wrapping_pre_image_undoes_the_wrap() {
        let bench = Bench::new(5);
        let ops = bench.ops();
        const MODULUS: u32 = 24;

        for delta in 0..MODULUS {
            for target in 0..MODULUS {
                let landed = bench.set(&[target]);
                let came = ops.pre_wrapping_add(&landed, delta, MODULUS).expect("room");
                let expected: Vec<u32> = (0..MODULUS)
                    .filter(|v| (v + delta) % MODULUS == target)
                    .collect();
                assert_eq!(
                    bench.values(&came),
                    expected,
                    "lands on {target} after +{delta}"
                );
            }
        }
    }

    /// A comparison over thirteen bits is a chain, not a union of eight thousand terms.
    ///
    /// The measurement this module exists for, made small enough to be a test: money's
    /// width is what made the case-split encoding impossible, and the point of a
    /// comparator is that its size follows the BITS.
    #[test]
    fn a_wide_comparison_stays_the_size_of_its_bits() {
        let bench = Bench::new(13);
        let ops = bench.ops();

        let at_least = ops.at_least(4_999).expect("room");
        assert!(
            at_least.node_count() <= 4 * 13,
            "a 13-bit comparator grew to {} nodes",
            at_least.node_count(),
        );

        // And it is still the right answer at the boundary.
        assert!(at_least.eval(bench.at(4_999).iter().copied()));
        assert!(!at_least.eval(bench.at(4_998).iter().copied()));
    }

    /// A manager with no room left answers `None`, rather than taking the process with it.
    ///
    /// These four are where a search stands when the purse is wide and the budget is the
    /// player's, and they are reached before any search starts - `seed_of` conjoins an
    /// equality per slot and then the purse. Unwrapped, running out of nodes here ABORTS:
    /// not a panic a host can turn into a partial answer, and not a row a measurement can
    /// keep. de-nyv2.
    ///
    /// The width and the node count are chosen so that laying the variables out already
    /// spends most of the manager, which is what leaves nothing for the conjunctions.
    #[test]
    fn a_full_manager_is_reported_rather_than_unwrapped() {
        const WIDE: u8 = 24;
        const CRAMPED: usize = 26;

        let bench = Bench::within(WIDE, CRAMPED);
        let ops = bench.ops();

        assert!(
            ops.equals(9_999_999).is_none(),
            "equals should report the full manager"
        );
        assert!(
            ops.at_least(9_999_999).is_none(),
            "at_least should report it too"
        );
        assert!(
            ops.at_most(9_999_999).is_none(),
            "and at_most, which is built on it"
        );
        assert!(ops.cube().is_none(), "and the cube over every variable");
    }
}
