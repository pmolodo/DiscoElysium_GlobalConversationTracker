// SPDX-License-Identifier: MIT
//! Turning a guard into a formula over the data variables.
//!
//! ## Two rails, because a guard is three-valued
//!
//! The engine's guards answer True, False or Unknown, and [`crate::core::types::Ternary`]
//! lets Unknown through - a search must not refuse a branch merely because it cannot
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

use oxidd::BooleanFunction;
use oxidd::bdd::BDDFunction;

use crate::core::guard::{Arguments, Guard, GuardExpression, GuardRef};
use crate::core::guard_value::{GuardValue, GuardValueKind};
use crate::core::state::{ITEM_PREFIX, TASK_PREFIX, THOUGHT_PREFIX};
use crate::core::types::{DialogueNodeId, Ternary};
use crate::symbolic::data_layout::DeltaSlot;
use crate::symbolic::vars::DataVars;
use crate::world::{ILookAheadWorld, MONEY_QUERY};

/// What a comparison against a rebased slot came to.
///
/// Three answers rather than an `Option`, because the two ways of having no formula want
/// different things said about them: a manager with no room left is a resource failure, and a
/// compiler with no world is a comparison this cannot decide. See
/// [`GuardCompiler::rebased`].
enum Rebase {
    Formula(BDDFunction),
    /// No world, so no value to rebase by.
    Unknown,
    /// The manager could not build the formula.
    NoRoom,
}

/// The distances a rebased comparison is true at, once the arriving value is folded in.
///
/// Only ever a threshold or a point, because the variable is a saturating sum: `v >= c` is
/// `d >= c - v0` and `v == c` is `d == c - v0`, and every other operator is one of those
/// negated. Both are over distances of at least one; the branch where the slot has not moved
/// is arithmetic rather than a set. See [`GuardCompiler::rebased`].
enum Distances {
    All,
    None,
    AtLeast(i64),
    Exactly(i64),
}

/// What compiling one guard node takes, as [`GuardCompiler::plan`] decides it.
enum Plan<'g> {
    /// Its rails, worked out without compiling anything under it.
    Settled(MayBe),
    /// One operand's rails first, then this.
    Unary(Combiner, GuardRef<'g>),
    /// Two operands' rails first, left then right, then this.
    Binary(Combiner, GuardRef<'g>, GuardRef<'g>),
}

/// How a node's rails are made from its operands'.
#[derive(Clone, Copy)]
enum Combiner {
    Not,
    And,
    Or,
    /// `expression == truth`: the operand's rails as they are against true, and swapped
    /// against false.
    AgainstBoolean(bool),
}

/// One thing left for [`GuardCompiler::compile_node`] to do.
enum Step<'g> {
    /// Plan this node, compiling it outright or asking for its operands.
    Compile(GuardRef<'g>),
    /// Join the operands just compiled for this node.
    Combine(Combiner, GuardRef<'g>),
}

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
///
/// ## It borrows the variables rather than declaring its own
///
/// A compiled guard has to be combined with the sets [`crate::symbolic::action_image`]
/// produces - reachability filters a set by a guard and then applies actions to what
/// survives - and TWO FORMULAS BUILT OVER DIFFERENT MANAGERS CANNOT BE COMBINED AT ALL.
/// This compiler used to build a manager of its own over the same layout, which looked
/// harmless because the variable numbering agreed; it was not, and nothing had caught it
/// only because nothing had yet asked a guard and an action about the same set.
pub struct GuardCompiler<'a> {
    vars: &'a DataVars<'a>,
    /// Where a variable no action writes gets its value.
    ///
    /// Such a variable is CONSTANT for the whole search - the seed reads it once and
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
    /// Whether a diagram operation could not complete for want of nodes.
    ///
    /// SEPARATE FROM THE FALLBACK COUNT, which it also lands in. An undecided guard reads
    /// the same whether the language could not express it or the manager could not hold
    /// it, and those want opposite responses: the first is content the model should learn,
    /// the second is a budget at its limit. Nothing could tell them apart before, because
    /// running out of room here did not produce a fallback at all - it aborted the process.
    out_of_memory: bool,
    reasons: HashMap<&'static str, usize>,
    subjects: Vec<(&'static str, String)>,
    /// Queries answered as constants that a DECLARED decision writes.
    ///
    /// A world query is answered once and reused at every state, which is exact only
    /// while nothing moves it. `search_can_change` names the ones the search moves through
    /// slots; [`crate::core::modelling`] names the ones something moves through an action
    /// the model has decided to skip. Those compile cleanly and so appear nowhere in the
    /// fallback counts - which is right, they are not gaps - but they are approximations,
    /// and an approximation nobody can see is the kind that gets forgotten.
    declared_constants: Vec<(&'static str, String)>,
    /// One entry's guard, compiled once and kept for however long this compiler lives.
    ///
    /// ## Why the compiler holds it rather than the search
    ///
    /// A fixed point revisits an entry every time its set grows, and the guard it tests
    /// there is the same guard every time. A search that did not keep them worked out
    /// a map of its own; the backward search had not, and recompiled on every visit.
    ///
    /// Keeping it HERE fixes that and does something the per-search map could not: a
    /// compiler outlives one search, so a group's second question inherits the guards its
    /// first question compiled - which is the cheap, soundness-free half of de-cnjw. A
    /// compiled guard depends on the layout and the world, and both of those are fixed for
    /// the life of a compiler, so there is nothing here that can go stale while it lives.
    ///
    /// ## What it costs
    ///
    /// Node references, which the manager cannot reclaim while they are held. On a group
    /// whose manager fills up that is a real trade rather than a free win, and
    /// [`Self::forget_guards`] is the way out for a caller that would rather have the room.
    guards: HashMap<DialogueNodeId, MayBe>,
    guard_cache_hits: usize,
}

impl<'a> GuardCompiler<'a> {
    /// Creates a compiler over variables somebody else declared.
    pub fn new(vars: &'a DataVars<'a>) -> Self {
        Self {
            vars,
            world: None,
            constant_clock: false,
            clock_approximated: false,
            fallbacks: 0,
            compiled: 0,
            reasons: HashMap::new(),
            subjects: Vec::new(),
            out_of_memory: false,
            declared_constants: Vec::new(),
            guards: HashMap::new(),
            guard_cache_hits: 0,
        }
    }

    /// One entry's guard, compiled the first time it is asked for and remembered after.
    ///
    /// KEYED BY THE ENTRY, not by the expression. Two entries with identical guards each
    /// get their own compilation, which is a small waste and the alternative wants a
    /// hashable normal form for `GuardExpression` that does not exist yet.
    ///
    /// The statistics count the COMPILATION, so a cache hit adds nothing to them. That
    /// makes `compiled()` and `fallbacks()` counts of distinct entries rather than of
    /// visits, which is the number anybody reading them wanted anyway - a fallback counted
    /// once per revisit says how hot the loop was, not how much the compiler cannot read.
    pub fn compile_for(&mut self, id: DialogueNodeId, guard: &Guard) -> MayBe {
        if let Some(compiled) = self.guards.get(&id) {
            self.guard_cache_hits += 1;
            return compiled.clone();
        }

        let compiled = self.compile_node(guard.as_ref());
        self.guards.insert(id, compiled.clone());
        compiled
    }

    /// How many entries have a compiled guard held, and how often one was reused.
    pub fn guard_cache(&self) -> (usize, usize) {
        (self.guards.len(), self.guard_cache_hits)
    }

    /// Drops the compiled guards, releasing the nodes they hold.
    ///
    /// For a caller that has run out of room and would rather recompile than have no
    /// answer. Nothing is lost but time.
    pub fn forget_guards(&mut self) {
        self.guards.clear();
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
    /// CLOSED that the real search would walk - the unsafe direction, and the only place
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

    /// The variables these formulas are built over, and the manager behind them.
    pub fn vars(&self) -> &'a DataVars<'a> {
        self.vars
    }

    /// The everywhere-true formula.
    pub fn top(&self) -> BDDFunction {
        self.vars.top()
    }

    /// The everywhere-false formula.
    pub fn bottom(&self) -> BDDFunction {
        self.vars.bottom()
    }

    /// Why each fallback happened, most common first.
    ///
    /// Worth keeping rather than a bare count: the reasons are what say whether the
    /// compiler is missing something cheap or genuinely up against the world.
    pub fn fallback_reasons(&self) -> Vec<(&'static str, usize)> {
        let mut rows: Vec<(&'static str, usize)> =
            self.reasons.iter().map(|(k, v)| (*k, *v)).collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.1));
        rows
    }

    /// Every sub-expression that fell back, with the reason, in the order met.
    ///
    /// The counts from [`Self::fallback_reasons`] say how big the gap is; these say what
    /// it consists of, which is what somebody closing it needs. Worth carrying rather
    /// than re-deriving in a report: a second copy of these rules is a copy that drifts,
    /// and the first version of the 631 diagnostic did drift - it called every untracked
    /// `CheckItem` unreadable when the compiler answers them from the world.
    pub fn fallback_subjects(&self) -> &[(&'static str, String)] {
        &self.subjects
    }

    /// Every question answered from the world that a declared decision writes.
    ///
    /// Not fallbacks - these compiled, and to a literal. They are the places where the
    /// answer is only as good as the decision behind it, listed so a report can say so.
    pub fn declared_constants(&self) -> &[(&'static str, String)] {
        &self.declared_constants
    }

    /// A query name as the registry spells it, so it can be kept without allocating.
    fn declared_name(name: &str) -> &'static str {
        crate::core::modelling::for_query(name)
            .and_then(|decision| decision.readers.iter().copied().find(|r| *r == name))
            .unwrap_or("declared reader")
    }

    /// A guard that is undecided everywhere: the permissive answer.
    fn undecided(&mut self, reason: &'static str, subject: String) -> MayBe {
        self.fallbacks += 1;
        *self.reasons.entry(reason).or_default() += 1;
        self.subjects.push((reason, subject));
        MayBe {
            may_be_true: self.top(),
            may_be_false: self.top(),
        }
    }

    /// A guard nothing could be built for, because the manager has no room left.
    ///
    /// THE PERMISSIVE ANSWER, like every other fallback here: an undecided guard lets every
    /// branch through, which is the direction both searches are allowed to be wrong in, so
    /// a compile that runs out of nodes still produces a SOUND answer rather than a wrong
    /// one. That is why this records and carries on where the searches stop - see
    /// [`Self::out_of_memory`] for what it costs to be unable to tell it from a gap in the
    /// model.
    ///
    /// RUNNING OUT OF NODES IS A RESULT, not a fault. The player's manager is a budget, and
    /// an unwrapped operation here ABORTS THE PROCESS - not a panic a host can turn into a
    /// partial answer, and not a row a measurement can keep. The searches and the register
    /// already report it; this was the last layer that did not.
    fn no_room(&mut self, subject: String) -> MayBe {
        self.out_of_memory = true;
        self.undecided("no room to build the formula", subject)
    }

    /// Whether a guard could not be compiled for want of diagram nodes.
    ///
    /// A caller that sees this has an answer built partly from undecided guards it did not
    /// ask for. The answer is still sound - see [`Self::no_room`] - but it is coarser than
    /// the content warrants, and a measurement reporting a fallback rate should say so
    /// rather than record it as a property of the guards.
    pub fn out_of_memory(&self) -> bool {
        self.out_of_memory
    }

    fn decided(&mut self, holds: BDDFunction) -> MayBe {
        // THE OTHER RAIL IS DIAGRAM WORK TOO. `may_be_false` is the negation of what was
        // just built, so a manager with room for one and not the other leaves this with
        // half an answer, which is no answer.
        let Ok(fails) = holds.not() else {
            return self.no_room("negating a compiled guard".to_string());
        };

        self.compiled += 1;
        MayBe {
            may_be_true: holds,
            may_be_false: fails,
        }
    }

    /// Compiles a guard into its two rails.
    pub fn compile(&mut self, guard: &Guard) -> MayBe {
        self.compile_node(guard.as_ref())
    }

    /// One node of a guard, and everything under it.
    ///
    /// DEMAND-DRIVEN, WHICH IS WHY IT WALKS DOWN rather than sweeping. A guard is a flat
    /// table whose children always sit earlier than their parents, so a single forward sweep
    /// would compile the whole of it - and that is the wrong walk here, because a comparison
    /// answers from its operands' SHAPE and compiles neither. `MoneyAmount() >= 50` becomes
    /// one comparison over the purse's bits; a sweep would first build a decision diagram for
    /// the call and for the literal, and then throw both away.
    ///
    /// ON A STACK OF ITS OWN, so no guard is too deep for the thread it runs on. Each node is
    /// PLANNED - see [`Self::plan`] - and a node that needs its operands asks for them: they
    /// are compiled first and then COMBINED by [`Self::combine`]. The work stack holds what is
    /// still to do and the results stack what is done. Operands are pushed right first, so
    /// they come off left first - the order a guard is written in, and the order its
    /// fallbacks are recorded in.
    fn compile_node(&mut self, root: GuardRef<'_>) -> MayBe {
        let mut work = vec![Step::Compile(root)];
        let mut results: Vec<MayBe> = Vec::new();
        while let Some(step) = work.pop() {
            match step {
                Step::Compile(node) => match self.plan(node) {
                    Plan::Settled(rails) => results.push(rails),
                    Plan::Unary(combiner, operand) => {
                        work.push(Step::Combine(combiner, node));
                        work.push(Step::Compile(operand));
                    }
                    Plan::Binary(combiner, left, right) => {
                        work.push(Step::Combine(combiner, node));
                        work.push(Step::Compile(right));
                        work.push(Step::Compile(left));
                    }
                },
                Step::Combine(combiner, node) => {
                    let combined = self.combine(combiner, node, &mut results);
                    results.push(combined);
                }
            }
        }
        results
            .pop()
            .expect("a compiled guard leaves exactly one answer")
    }

    /// What compiling one node takes: its rails outright, or its operands' rails first.
    ///
    /// EVERY DECISION ABOUT A NODE IS MADE HERE, and [`Self::compile_node`] only moves
    /// results between its stacks. Four shapes ask for operands - `not`, `and`, `or`, and a
    /// comparison against a boolean - and everything else is settled on the spot, without
    /// compiling what is under it.
    ///
    /// THE LIFETIME IS NAMED, and has to be: with `&mut self` in scope an elided output
    /// lifetime would tie the plan to the borrow of the compiler rather than to the guard.
    fn plan<'g>(&mut self, guard: GuardRef<'g>) -> Plan<'g> {
        Plan::Settled(match guard.expression() {
            GuardExpression::Literal(value) => match value.as_condition() {
                Ternary::True => {
                    let t = self.top();
                    self.decided(t)
                }
                Ternary::False => {
                    let f = self.bottom();
                    self.decided(f)
                }
                Ternary::Unknown => self.undecided("literal is unknown", guard.to_string()),
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
                    None => {
                        self.undecided("variable untracked and world cannot say", guard.to_string())
                    }
                },
            },

            GuardExpression::Not(inner) => return Plan::Unary(Combiner::Not, inner),
            GuardExpression::And(left, right) => {
                return Plan::Binary(Combiner::And, left, right);
            }
            GuardExpression::Or(left, right) => return Plan::Binary(Combiner::Or, left, right),
            GuardExpression::Comparison(op, left, right) => return self.compare(op, left, right),

            // A world query - HasItem, IsTaskActive, MoneyAmount, the clock, and every
            // other thing the search asks the game rather than its own state. Undecided
            // here, which is the permissive answer, and counted so the fallback rate can
            // be measured against real content.
            // Inventory, journal and thought-cabinet questions, which the search DOES
            // change - GainItem and LoseItem write an `item:` slot, GainTask and
            // FinishTask a `task:` one, GainThought a `thought:` one, and
            // `BoundContext::query` answers all three from exactly those slots. This
            // mirrors that.
            //
            // Where the group has no such slot, the subject is one no action here
            // touches, so it is constant for the search and the world answers it - the
            // same rule as an untracked variable. That is also what keeps the variable
            // count down: a slot exists only for something the group actually
            // manipulates, not for every item in the game.
            GuardExpression::Call(name, args) if Self::slot_backed_query(name).is_some() => {
                let prefix = Self::slot_backed_query(name).expect("just matched");
                match Self::text_argument(args) {
                    Some(subject) => {
                        let slot = format!("{prefix}{subject}");
                        match self.slot_is_set(&slot) {
                            Some(holds) => self.decided(holds),
                            // Untracked, so nothing in this group can change it: the
                            // starting value is the only value, and the world answers
                            // directly. `BoundContext::query` does exactly the same, and
                            // the mirroring is the point - a compiler more decisive than
                            // the engine it models would prune branches the real search
                            // walks.
                            //
                            // What neither may do is answer this way for a TRACKED
                            // subject. Once GainItem has run the truth is in the state,
                            // and the starting inventory is stale.
                            None => match self.world {
                                Some(world) => {
                                    let held = match prefix {
                                        ITEM_PREFIX => world.initially_has_item(&subject),
                                        TASK_PREFIX => world.initially_task_active(&subject),
                                        _ => world.initially_has_thought(&subject),
                                    };
                                    let f = if held { self.top() } else { self.bottom() };
                                    self.decided(f)
                                }
                                None => self
                                    .undecided("call: untracked and no world", guard.to_string()),
                            },
                        }
                    }
                    None => self.undecided("call: subject is not a literal", guard.to_string()),
                }
            }

            // The clock, TRACKED: read straight off its own register. Every clock
            // question in the language is a question about the HOUR, so each one becomes
            // the union of the minute ranges of the hours that satisfy it - at most
            // twenty-four ranges, each a pair of comparisons over eleven bits.
            //
            // Which hours satisfy it is decided by asking `ClockTime::answer` at each
            // hour rather than by restating the table here. A second copy of "afternoon
            // runs to the end of the eighteenth hour" is exactly the kind of thing that
            // drifts, and this port has already had those boundaries wrong once.
            GuardExpression::Call(name, args)
                if crate::core::clock::ClockTime::owns(name) && self.vars.clock_ops().is_some() =>
            {
                match self.clock_hours_formula(name, args) {
                    Some(holds) => self.decided(holds),
                    None => {
                        self.undecided("call: clock, not a question of the hour", guard.to_string())
                    }
                }
            }

            // The clock, held at whatever the world says and not moved by the
            // conversation. See `with_constant_clock` for why, and what it costs: this is
            // the one approximation here that can close a branch the search would walk.
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
                    None => self.undecided("call: clock, world cannot say", guard.to_string()),
                }
            }

            // A query the SEARCH cannot change is a constant, and the engine says which
            // those are: `BoundContext::query` intercepts MoneyAmount, CheckItem,
            // IsTaskActive and the clock, and lets everything else fall through to the
            // world - which does not change while a search runs. So anything not
            // intercepted has the same answer at every state, and asking the world once
            // is exactly what the engine does at every step.
            //
            // Worth the trouble: IsKimHere alone is 691 of the roughly 1,200 world
            // queries the five biggest conversations make.
            GuardExpression::Call(name, args)
                if !Self::search_can_change(name) && self.world.is_some() =>
            {
                // Constant for the SEARCH, which is not the same as constant. Something
                // the model has decided to skip may write it, and where that is so the
                // question is noted rather than passed over silently.
                if crate::core::modelling::for_query(name).is_some() {
                    self.declared_constants
                        .push((Self::declared_name(name), guard.to_string()));
                }

                match self.constant_query(name, args) {
                    Some(true) => {
                        let t = self.top();
                        self.decided(t)
                    }
                    Some(false) => {
                        let f = self.bottom();
                        self.decided(f)
                    }
                    None => self.undecided("call: world cannot say", guard.to_string()),
                }
            }

            GuardExpression::Call(name, _) => {
                let reason: &'static str = match name {
                    "CheckItem" => "call: CheckItem",
                    "IsTaskActive" => "call: IsTaskActive",
                    "IsTHCPresent" => "call: IsTHCPresent",
                    "MoneyAmount" => "call: MoneyAmount",
                    "DayCount" | "HourCount" | "IsDayFrom" | "IsMorning" | "IsAfternoon"
                    | "IsEvening" | "IsNight" | "IsMidnight" | "IsHour" => "call: clock",
                    _ => "call: other world query",
                };
                self.undecided(reason, guard.to_string())
            }
        })
    }

    /// Joins the rails of a node's operands, which [`Self::compile_node`] has just compiled
    /// and left on top of `results`, right above left.
    fn combine(
        &mut self,
        combiner: Combiner,
        node: GuardRef<'_>,
        results: &mut Vec<MayBe>,
    ) -> MayBe {
        const COMPILED_FIRST: &str = "a combiner's operands are compiled before it";
        match combiner {
            Combiner::Not => {
                let inner = results.pop().expect(COMPILED_FIRST);
                MayBe {
                    may_be_true: inner.may_be_false,
                    may_be_false: inner.may_be_true,
                }
            }
            Combiner::AgainstBoolean(truth) => {
                let inner = results.pop().expect(COMPILED_FIRST);
                if truth {
                    inner
                } else {
                    MayBe {
                        may_be_true: inner.may_be_false,
                        may_be_false: inner.may_be_true,
                    }
                }
            }
            Combiner::And => {
                let b = results.pop().expect(COMPILED_FIRST);
                let a = results.pop().expect(COMPILED_FIRST);
                // Both may hold, so both rails must allow it; either failing is enough to
                // fail the conjunction. BOTH OR NEITHER: a MayBe with one rail built and
                // the other not is not a weaker answer, it is an inconsistent one.
                match (
                    a.may_be_true.and(&b.may_be_true),
                    a.may_be_false.or(&b.may_be_false),
                ) {
                    (Ok(may_be_true), Ok(may_be_false)) => MayBe {
                        may_be_true,
                        may_be_false,
                    },
                    _ => self.no_room(node.to_string()),
                }
            }
            Combiner::Or => {
                let b = results.pop().expect(COMPILED_FIRST);
                let a = results.pop().expect(COMPILED_FIRST);
                match (
                    a.may_be_true.or(&b.may_be_true),
                    a.may_be_false.and(&b.may_be_false),
                ) {
                    (Ok(may_be_true), Ok(may_be_false)) => MayBe {
                        may_be_true,
                        may_be_false,
                    },
                    _ => self.no_room(node.to_string()),
                }
            }
        }
    }

    /// A comparison, where it can be read.
    ///
    /// Only equality against a constant. An ordering comparison would need the slot's
    /// bits compared against a constant's, which is the arithmetic that makes decision
    /// diagrams blow up and is deliberately not attempted until something measures
    /// whether it is needed.
    fn compare<'g>(&mut self, op: &str, left: GuardRef<'g>, right: GuardRef<'g>) -> Plan<'g> {
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
                return Plan::Unary(Combiner::AgainstBoolean(truth != negated), left);
            }
            if let Some(truth) = Self::boolean_of(left) {
                return Plan::Unary(Combiner::AgainstBoolean(truth != negated), right);
            }
        }

        // Money and the hour are CALLS rather than variables - `MoneyAmount() >= 50`,
        // `HourCount() > 12` - so they never reach the variable path below. Where the
        // layout carries a register for one, this is the arithmetic de-sze named as the
        // likely blowup and never once ran.
        if let Some(compiled) = self.register_comparison(op, left, right) {
            return Plan::Settled(compiled);
        }

        // A query the SEARCH cannot change is a constant, and a comparison against one is
        // arithmetic on two knowns. `DayCount() >= 2` is the shape, and it was the single
        // largest remaining fallback category in the corpus.
        if let Some(compiled) = self.constant_comparison(op, left, right) {
            return Plan::Settled(compiled);
        }

        let (Some(name), Some(literal)) = (Self::variable_of(left), Self::literal_of(right)) else {
            // Also try the other way round: a guard may be written `1 == Variable[..]`.
            // The operator has to turn with the operands - `3 <= x` is `x >= 3`, and
            // reading it as `x <= 3` would answer the opposite question everywhere the
            // two disagree.
            if let (Some(name), Some(literal)) = (Self::variable_of(right), Self::literal_of(left))
            {
                return Plan::Settled(self.comparison(Self::mirrored(op), &name, literal));
            }
            // Money, where the layout does not carry it. It must NOT be answered from
            // the world: the search changes it - `BoundContext::query` reads it from search
            // state - so the world's starting balance would close a branch a richer path
            // opens, which is the unsafe direction. The reason says so rather than blaming
            // the shape of the expression, which is what it used to do and which sent
            // somebody looking at the parser.
            if Self::names_money(left) || Self::names_money(right) {
                return Plan::Settled(self.undecided(
                    "comparison: money, which a search changes and this layout does not carry",
                    format!("({left} {op} {right})"),
                ));
            }

            return Plan::Settled(self.undecided(
                "comparison: neither side a known variable",
                format!("({left} {op} {right})"),
            ));
        };

        Plan::Settled(self.comparison(op, &name, literal))
    }

    /// A comparison written out, for reporting one the compiler could not read.
    ///
    /// Rebuilt from the parts rather than carried down, because by the time a comparison
    /// is being decided the expression it came from has been taken apart. It renders the
    /// way a guard does, so a reported gap can be found in the database by
    /// searching for it.
    fn rendered(op: &str, name: &str, literal: &GuardValue) -> String {
        format!("(Variable[\"{name}\"] {op} {literal})")
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

    /// The boolean a literal stands for, if it is a boolean one.
    fn boolean_of(expression: GuardRef<'_>) -> Option<bool> {
        let GuardExpression::Literal(value) = expression.expression() else {
            return None;
        };
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
            // A REBASED SLOT holds the distance the search has travelled rather than the
            // value the guard is written about, so the comparison is rewritten around the
            // value the search started at. This has to come first: the paths below read the
            // slot's bits as the value itself, which for such a slot they are not.
            if let Some(slot) = self.vars.slot_of(name)
                && let Some(delta) = self.vars.layout().delta_slot(slot)
            {
                return match self.rebased(slot, name, op, value as i64, delta) {
                    Rebase::Formula(holds) => self.decided(holds),
                    Rebase::NoRoom => self.no_room(Self::rendered(op, name, literal)),
                    Rebase::Unknown => self.undecided(
                        "comparison: a rebased slot, and no world to rebase it by",
                        Self::rendered(op, name, literal),
                    ),
                };
            }

            if equality {
                if let Some(equals) = self.slot_equals(name, value) {
                    let holds = if op == "~=" {
                        match equals.not() {
                            Ok(negated) => negated,
                            Err(_) => return self.no_room(Self::rendered(op, name, literal)),
                        }
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
        // instead would answer differently from the search for a variable the world
        // reports as a boolean.
        //
        // Without this an equality on an untracked variable fell back while a BARE
        // mention of the same variable did not, which was an inconsistency in this
        // compiler rather than anything about the content.
        let Some(world) = self.world else {
            return self.undecided(
                "comparison: variable untracked and no world",
                Self::rendered(op, name, literal),
            );
        };

        let actual = world.get_variable(self.declared(name));
        if actual.kind() == GuardValueKind::Unknown {
            return self.undecided(
                "comparison: variable untracked and world cannot say",
                Self::rendered(op, name, literal),
            );
        }

        let holds = if equality {
            let same = actual.equals(literal);
            same != (op == "~=")
        } else {
            // Ordering on values, the way `Guard::evaluate` does it: both sides
            // through `try_as_number`, and undecided where either will not convert.
            let (Some(a), Some(b)) = (actual.try_as_number(), literal.try_as_number()) else {
                return self.undecided(
                    "comparison: ordering on a non-numeric value",
                    Self::rendered(op, name, literal),
                );
            };
            match op {
                ">=" => a >= b,
                "<=" => a <= b,
                ">" => a > b,
                "<" => a < b,
                _ => {
                    return self.undecided(
                        "comparison: unknown operator",
                        Self::rendered(op, name, literal),
                    );
                }
            }
        };

        let formula = if holds { self.top() } else { self.bottom() };
        self.decided(formula)
    }

    /// `Variable[name] op value` for an ordering operator, over a tracked slot.
    ///
    /// ## The blowup the epic expected, and where it went
    ///
    /// Magnitude comparison on bit-blasted integers is the classic way to make a decision
    /// diagram explode, and de-sze names it as the likely failure. This used to enumerate
    /// the satisfying values and union them, which a counter can afford - the cap bounds
    /// it to five bits and 32 values - and which money's thirteen bits and the clock's
    /// eleven cannot: eight thousand conjunctions to say one thing.
    ///
    /// It is now a ripple comparator instead, O(bits) rather than O(2^bits), so the same
    /// routine serves a five-bit counter and a thirteen-bit balance. See
    /// [`crate::symbolic::register`].
    /// `Variable[name] op value` over a slot the layout holds as a DELTA.
    ///
    /// ## What the slot holds, and what the guard asks about
    ///
    /// The slot holds `d`, how far the search's own actions have moved the variable. The
    /// guard asks about the variable, which is `d` added to whatever the save held. So the
    /// rewriting folds the save's value into the CONSTANT and leaves the slot alone:
    /// `v >= c` is `d >= c - v0`. See [`DataLayout::narrow_to_deltas`] for why the slot is
    /// shaped that way and what it saves.
    ///
    /// ## Reproducing the saturation exactly, which is the fiddly half
    ///
    /// The absolute encoding did two different things with two different numbers, and a
    /// rewriting that used one number for both would answer guards the shipped encoding does
    /// not. The seed was CLAMPED into the slot's ceiling. An increment SATURATED at the
    /// counter cap, which is a smaller number wherever the guards left the slot wider than
    /// the cap needs. So a variable arriving above the cap keeps its value until something
    /// increments it, and then drops to the cap.
    ///
    /// That is the split on `d == 0`: at a distance of nothing the variable still reads as
    /// what the save held, and at any other distance it reads as the sum held down to the
    /// cap. Both branches are exact, and the `ite` between them is the whole rewriting.
    ///
    /// ## Why the world is required here and nowhere else
    ///
    /// `v0` comes from the save. A compiler with no world cannot produce it and says so -
    /// undecided, which is the permissive answer - rather than guessing zero, which would
    /// close branches a richer save opens.
    fn rebased(
        &mut self,
        slot: usize,
        name: &str,
        op: &str,
        constant: i64,
        delta: DeltaSlot,
    ) -> Rebase {
        let Some(world) = self.world else {
            return Rebase::Unknown;
        };

        // WHAT `seed_state` WOULD HAVE PUT IN THE SLOT, read the same way it reads it: a
        // boolean is its truth, a number is itself, and anything else is nothing there.
        // `narrow_to_deltas` bars the slots that rule does not cover.
        let held = world.get_variable(self.declared(name));
        let base = match held.kind() {
            GuardValueKind::Boolean => i64::from(held.boolean()),
            GuardValueKind::Number => held.number() as i64,
            _ => 0,
        }
        .clamp(0, delta.ceiling as i64);
        let cap = delta.cap as i64;

        // Every operator is one of three, or the negation of one of three.
        let (positive, negate) = match op {
            ">=" => (">=", false),
            ">" => (">", false),
            "==" => ("==", false),
            "<" => (">=", true),
            "<=" => (">", true),
            "~=" | "!=" => ("==", true),
            _ => return Rebase::Unknown,
        };

        // `min(base + d, cap) >= wanted`, as a bound on `d` alone, for `d` of at least one.
        let at_least = |wanted: i64| -> Distances {
            if wanted <= 0 {
                Distances::All
            } else if wanted > cap {
                Distances::None
            } else {
                Distances::AtLeast((wanted - base).max(1))
            }
        };

        let moved = match positive {
            ">=" => at_least(constant),
            ">" => at_least(constant + 1),
            // Equality against the cap is satisfied by everything at or above it, because
            // that is what saturation means.
            _ if constant == cap => at_least(cap),
            _ if constant < 0 || constant > cap || constant - base < 1 => Distances::None,
            _ => Distances::Exactly(constant - base),
        };

        // At a distance of nothing the variable reads as the save's value, and the
        // comparison is arithmetic on two knowns.
        let unmoved = match positive {
            ">=" => base >= constant,
            ">" => base > constant,
            _ => base == constant,
        };

        let Some(holds) = self.branching(slot, moved, unmoved, negate) else {
            return Rebase::NoRoom;
        };
        Rebase::Formula(holds)
    }

    /// The two branches of a rebased comparison, joined on whether the slot has moved.
    fn branching(
        &mut self,
        slot: usize,
        moved: Distances,
        unmoved: bool,
        negate: bool,
    ) -> Option<BDDFunction> {
        let far = match moved {
            Distances::All => self.top(),
            Distances::None => self.bottom(),
            Distances::AtLeast(bound) => self.vars.slot_ops(slot)?.compare(">=", bound)?,
            Distances::Exactly(bound) => self.vars.slot_equals(slot, u32::try_from(bound).ok()?)?,
        };
        let near = if unmoved { self.top() } else { self.bottom() };

        let still = self.vars.slot_equals(slot, 0)?;
        let holds = still.ite(&near, &far).ok()?;
        if negate {
            holds.not().ok()
        } else {
            Some(holds)
        }
    }

    fn slot_ordered(&mut self, name: &str, op: &str, value: i32) -> Option<BDDFunction> {
        let slot = self.vars.slot_of(name)?;
        self.vars.slot_ops(slot)?.compare(op, value as i64)
    }

    /// A comparison whose subject is money or the hour, where the layout carries it.
    ///
    /// Returns `None` when this is not such a comparison at all, so the caller carries on
    /// to the variable path rather than treating it as a failure.
    fn register_comparison<'g>(
        &mut self,
        op: &str,
        left: GuardRef<'g>,
        right: GuardRef<'g>,
    ) -> Option<MayBe> {
        // Either way round, and the operator turns with the operands: `50 <= MoneyAmount()`
        // is `MoneyAmount() >= 50`, and reading it the other way answers the opposite
        // question everywhere the two disagree.
        let (name, literal, op) = match (Self::call_of(left), Self::literal_of(right)) {
            (Some(name), Some(literal)) => (name, literal, op),
            _ => match (Self::call_of(right), Self::literal_of(left)) {
                (Some(name), Some(literal)) => (name, literal, Self::mirrored(op)),
                _ => return None,
            },
        };

        let value = literal.try_as_number()?;
        match name.as_str() {
            MONEY_QUERY => {
                let ops = self.vars.money_ops()?;
                let holds = ops.compare(op, value as i64)?;
                Some(self.decided(holds))
            }
            // The hour, in minutes: `HourCount() >= 13` is `clock >= 13 * 60`. Exact for
            // every operator, because the hour is the minute count divided by sixty and
            // that division is monotone - `hours >= h` is `minutes >= 60h`, and
            // `hours <= h` is `minutes <= 60h + 59`.
            "HourCount" => {
                let ops = self.vars.clock_ops()?;
                let hour = value as i64;
                let holds = match op {
                    ">=" | ">" => {
                        let first = if op == ">" { hour + 1 } else { hour };
                        ops.compare(">=", first * 60)?
                    }
                    "<=" | "<" => {
                        let last = if op == "<" { hour - 1 } else { hour };
                        ops.compare("<=", last * 60 + 59)?
                    }
                    "==" => {
                        let from = ops.compare(">=", hour * 60)?;
                        let to = ops.compare("<=", hour * 60 + 59)?;
                        from.and(&to).ok()?
                    }
                    "~=" | "!=" => {
                        let from = ops.compare(">=", hour * 60)?;
                        let to = ops.compare("<=", hour * 60 + 59)?;
                        from.and(&to).ok()?.not().ok()?
                    }
                    _ => return None,
                };
                Some(self.decided(holds))
            }
            _ => None,
        }
    }

    /// Every minute of the day at which a clock question is true, as a formula.
    ///
    /// The hours that satisfy it are found by ASKING THE CLOCK MODEL at each hour, so the
    /// boundaries - afternoon running to the end of the eighteenth hour, dusk being the
    /// single hour of nineteen - are stated in exactly one place. This port has already had
    /// them wrong once by restating them.
    fn clock_hours_formula(&mut self, name: &str, args: Arguments<'_>) -> Option<BDDFunction> {
        use crate::core::clock::ClockTime;

        let values = Self::literal_arguments(args)?;
        let ops = self.vars.clock_ops()?;
        let mut holds = self.bottom();
        let mut answered = false;
        for hour in 0..24i32 {
            // The day counter is what the world says; it cannot change within a
            // conversation, so it is the same at every hour.
            let day = self.world.map_or(1, |world| world.day_counter());
            let answer = ClockTime::answer(name, &values, hour * 60, day);
            if answer.kind() == GuardValueKind::Unknown {
                return None;
            }

            answered = true;
            if !answer.boolean() {
                continue;
            }

            let from = ops.compare(">=", hour as i64 * 60)?;
            let to = ops.compare("<=", hour as i64 * 60 + 59)?;
            holds = holds.or(&from.and(&to).ok()?).ok()?;
        }

        if answered { Some(holds) } else { None }
    }

    /// The name a zero-argument call carries, if the expression is one.
    fn call_of(expression: GuardRef<'_>) -> Option<String> {
        match expression.expression() {
            GuardExpression::Call(name, args) if args.is_empty() => Some(name.to_string()),
            _ => None,
        }
    }

    /// The name a `Variable` node carries, if the expression is one.
    fn variable_of(expression: GuardRef<'_>) -> Option<String> {
        match expression.expression() {
            GuardExpression::Variable(name) => Some(name.to_string()),
            _ => None,
        }
    }

    /// The value a literal expression carries, if it is a literal.
    fn literal_of<'g>(expression: GuardRef<'g>) -> Option<&'g GuardValue> {
        match expression.expression() {
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
    fn text_argument(args: Arguments<'_>) -> Option<String> {
        let GuardExpression::Literal(value) = args.only()?.expression() else {
            return None;
        };
        match value.kind() {
            GuardValueKind::Text => Some(value.text().to_string()),
            _ => None,
        }
    }

    /// Whether a search's own actions can change what this query answers.
    ///
    /// Mirrors the interception list in `BoundContext::query`. Anything here is answered
    /// from search state and so varies between states; anything else is answered by the
    /// world and is the same at every state.
    fn search_can_change(name: &str) -> bool {
        matches!(name, MONEY_QUERY)
            || Self::slot_backed_query(name).is_some()
            || crate::core::clock::ClockTime::owns(name)
    }

    /// The slot prefix a query is answered from, for the queries that have one.
    ///
    /// One list, read by both the compiler and `search_can_change`, because a query
    /// answered from a slot in one place and from the world in the other would give two
    /// different answers for the same state. `BoundContext::query` intercepts exactly
    /// these three.
    fn slot_backed_query(name: &str) -> Option<&'static str> {
        match name {
            "CheckItem" => Some(ITEM_PREFIX),
            "IsTaskActive" => Some(TASK_PREFIX),
            "IsTHCPresent" => Some(THOUGHT_PREFIX),
            _ => None,
        }
    }

    /// What a clock question answers at the world's time, with the conversation ignored.
    ///
    /// Answered by `ClockTime` against the world's `day_minutes` and `day_counter`, which
    /// is exactly what the engine does for a search that has not moved the clock - not
    /// through `world.query`, which knows nothing about hours.
    fn clock_answer(&self, name: &str, args: Arguments<'_>) -> Option<bool> {
        let world = self.world?;
        let values = Self::literal_arguments(args)?;

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
    fn constant_query(&self, name: &str, args: Arguments<'_>) -> Option<bool> {
        match self.constant_value(name, args)?.as_condition() {
            Ternary::True => Some(true),
            Ternary::False => Some(false),
            Ternary::Unknown => None,
        }
    }

    /// What a constant query evaluates to, as a value rather than as a truth.
    ///
    /// Split out from [`Self::constant_query`] because a comparison needs the NUMBER:
    /// `DayCount() >= 2` cannot be answered from whether `DayCount()` is truthy. Both go
    /// through here so the two can never disagree about what the query says.
    fn constant_value(&self, name: &str, args: Arguments<'_>) -> Option<GuardValue> {
        let values = Self::literal_arguments(args)?;
        let world = self.world?;

        // The day, which the search cannot move and the world need not be asked about -
        // it is a comparison against `day_counter`, and `BoundContext::query` answers it
        // the same way. Mirroring the engine here is the whole requirement: a compiler
        // that refused a question the search answers would leave a branch open the search
        // closes, and one that answered differently would be worse than either.
        let answer = if crate::core::clock::ClockTime::owns_day(name) {
            crate::core::clock::ClockTime::day_answer(name, &values, world.day_counter())
        } else {
            world.query(name, &values)
        };

        if answer.kind() == GuardValueKind::Unknown {
            None
        } else {
            Some(answer)
        }
    }

    /// A comparison one of whose sides is a query the search cannot change.
    ///
    /// `None` when this is not such a comparison, so the caller carries on rather than
    /// treating it as a failure.
    ///
    /// ## Why this is safe and answering money the same way would not be
    ///
    /// A query the search cannot change has the same answer at every state the search can
    /// reach, so asking the world once is exactly what the engine does at every step -
    /// `BoundContext::query` lets anything it does not intercept fall through to the
    /// world. `MoneyAmount` IS intercepted, so it is excluded here by
    /// [`Self::search_can_change`], and answering it from the world's starting balance
    /// would close a branch a richer path opens.
    fn constant_comparison<'g>(
        &mut self,
        op: &str,
        left: GuardRef<'g>,
        right: GuardRef<'g>,
    ) -> Option<MayBe> {
        let (name, args, literal, op) = match (Self::query_of(left), Self::literal_of(right)) {
            (Some((name, args)), Some(literal)) => (name, args, literal, op),
            _ => match (Self::query_of(right), Self::literal_of(left)) {
                // The operator turns with the operands: `2 <= DayCount()` is
                // `DayCount() >= 2`, and reading it the other way answers the opposite
                // question everywhere the two disagree.
                (Some((name, args)), Some(literal)) => (name, args, literal, Self::mirrored(op)),
                _ => return None,
            },
        };

        if Self::search_can_change(&name) {
            return None;
        }

        let actual = self.constant_value(&name, args)?;
        let rendered = format!("({name}(..) {op} {literal})");

        let holds = if op == "==" || op == "~=" {
            actual.equals(literal) != (op == "~=")
        } else {
            // Ordering the way `Guard::evaluate` does it: both sides through
            // `try_as_number`, and undecided where either will not convert.
            let (Some(a), Some(b)) = (actual.try_as_number(), literal.try_as_number()) else {
                return Some(
                    self.undecided("comparison: ordering on a non-numeric query", rendered),
                );
            };
            match op {
                ">=" => a >= b,
                "<=" => a <= b,
                ">" => a > b,
                "<" => a < b,
                _ => return Some(self.undecided("comparison: unknown operator", rendered)),
            }
        };

        let formula = if holds { self.top() } else { self.bottom() };
        Some(self.decided(formula))
    }

    /// The name and arguments of a call, if the expression is one.
    fn query_of(expression: GuardRef<'_>) -> Option<(String, Arguments<'_>)> {
        match expression.expression() {
            GuardExpression::Call(name, args) => Some((name.to_string(), args)),
            _ => None,
        }
    }

    /// Every argument as a literal value, or `None` if any of them is computed.
    ///
    /// A query whose argument is not a literal would have to be evaluated per state,
    /// which is the thing being avoided - so one such argument settles the whole call.
    fn literal_arguments(args: Arguments<'_>) -> Option<Vec<GuardValue>> {
        args.iter()
            .map(|argument| match argument.expression() {
                GuardExpression::Literal(value) => Some(value.clone()),
                _ => None,
            })
            .collect()
    }

    /// Whether an expression is a call to `MoneyAmount`.
    fn names_money(expression: GuardRef<'_>) -> bool {
        matches!(expression.expression(), GuardExpression::Call(name, _) if name == MONEY_QUERY)
    }

    /// The variable a guard names, as the group declares it.
    ///
    /// Every variable a guard names is declared when the graph is built, so a name that is
    /// not is a compiler handed symbols from some other graph - loud rather than Unknown.
    fn declared(&self, name: &str) -> crate::core::state::VariableRef<'a> {
        let vars: &'a DataVars<'a> = self.vars;
        vars.symbols().variable_ref(name).unwrap_or_else(|| {
            panic!("a guard reads '{name}', which the group does not declare as a variable")
        })
    }

    /// What the world says an untracked variable is, as a condition.
    fn constant_truth(&self, name: &str) -> Option<bool> {
        match self.world?.get_variable(self.declared(name)).as_condition() {
            Ternary::True => Some(true),
            Ternary::False => Some(false),
            Ternary::Unknown => None,
        }
    }

    /// "This slot is non-zero", as a formula, by the variable's name.
    fn slot_is_set(&self, name: &str) -> Option<BDDFunction> {
        self.vars.slot_is_set(self.vars.slot_of(name)?)
    }

    /// "This slot holds exactly this value", as a formula, by the variable's name.
    ///
    /// A value the slot cannot hold gives the empty set rather than nothing: the equality
    /// is false everywhere, which is decided, not unknown. A NEGATIVE value is one of
    /// those - a slot is an unsigned run of bits and holds nothing below zero.
    fn slot_equals(&self, name: &str, value: i32) -> Option<BDDFunction> {
        let slot = self.vars.slot_of(name)?;
        let Ok(value) = u32::try_from(value) else {
            return Some(self.bottom());
        };

        self.vars.slot_equals(slot, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::action::DialogueAction;
    use crate::core::guard::Guard;
    use crate::core::state::StateSymbols;
    use crate::core::types::DialogueNodeId;
    use crate::graph::LookAheadGraph;
    use crate::graph::node::LookAheadNode;
    use crate::symbolic::budget::DiagramBudget;
    use crate::symbolic::data_layout::DataLayout;

    /// A graph whose symbol table holds `names`, with `counter` incremented so it is wide.
    /// A graph whose symbol table holds `names`, with `counter` incremented so it is wide.
    ///
    /// SELF-LINKED, so the increment can fire more than once and the slot keeps the absolute
    /// encoding. An increment that fires at most once is held as a delta - see
    /// `DataLayout::narrow_to_deltas` - and a single `+1` then needs one bit, which is not
    /// the wide slot these tests are about.
    fn fixture(names: &[&str], counter: Option<&str>) -> (LookAheadGraph, StateSymbols) {
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

    fn boolean(value: bool) -> Guard {
        Guard::literal(GuardValue::from_boolean(value))
    }

    fn number(value: f64) -> Guard {
        Guard::literal(GuardValue::from_number(value))
    }

    fn call(name: &str, args: Vec<Guard>) -> Guard {
        Guard::call(name.to_string(), args)
    }

    /// `DayCount() >= 2` is decided by the world's day, not given up on.
    ///
    /// The single largest remaining fallback category in the corpus - de-sze.5.5 - and it
    /// was reported as "neither side a known variable", which describes the SHAPE of the
    /// expression and says nothing about what was actually missing.
    #[test]
    fn a_comparison_against_a_constant_query_is_decided_by_the_world() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let world = crate::world::test_world::TestWorld::new().with_day_counter(1);
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);

        // Day one, so the branch is shut.
        let compiled = compiler.compile(&Guard::comparison(
            ">=".to_string(),
            call("DayCount", vec![]),
            number(2.0),
        ));
        assert!(!compiled.may_be_true.satisfiable());
        assert_eq!(compiler.fallbacks(), 0);

        // And open where the day satisfies it.
        let compiled = compiler.compile(&Guard::comparison(
            ">=".to_string(),
            call("DayCount", vec![]),
            number(1.0),
        ));
        assert!(compiled.may_be_true.satisfiable());
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// Written the other way round, the operator turns with the operands.
    #[test]
    fn a_constant_query_on_the_right_compares_the_same_way() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let world = crate::world::test_world::TestWorld::new().with_day_counter(1);
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);

        // `2 <= DayCount()` is `DayCount() >= 2`, which is false on day one. Read without
        // turning the operator it would be `DayCount() <= 2`, which is true - the opposite
        // answer.
        let compiled = compiler.compile(&Guard::comparison(
            "<=".to_string(),
            number(2.0),
            call("DayCount", vec![]),
        ));
        assert!(!compiled.may_be_true.satisfiable());
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// A money comparison stays undecided where the layout does not carry money.
    ///
    /// The search CHANGES money, so answering it from the world's starting balance would
    /// close a branch a richer path opens - the unsafe direction. What changes is the
    /// reason: it now names money instead of blaming the shape of the expression.
    #[test]
    fn a_money_comparison_is_undecided_and_says_it_is_about_money() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let world = crate::world::test_world::TestWorld::new();
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);

        let compiled = compiler.compile(&Guard::comparison(
            ">=".to_string(),
            call("MoneyAmount", vec![]),
            number(50.0),
        ));

        // Permissive: both outcomes stay open, which is what an undecided guard means.
        assert!(compiled.may_be_true.satisfiable());
        assert!(compiled.may_be_false.satisfiable());
        assert_eq!(compiler.fallbacks(), 1);
        let reasons = compiler.fallback_reasons();
        assert!(
            reasons.iter().any(|(reason, _)| reason.contains("money")),
            "the reason should name money: {reasons:?}",
        );
    }

    /// With money in the layout, the same comparison is decided against its register.
    #[test]
    fn a_money_comparison_is_decided_once_money_has_a_register() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, Some(1000), false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let world = crate::world::test_world::TestWorld::new();
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);

        let compiled = compiler.compile(&Guard::comparison(
            ">=".to_string(),
            call("MoneyAmount", vec![]),
            number(50.0),
        ));

        assert_eq!(compiler.fallbacks(), 0);
        // Decided, and decided as a real condition rather than as everywhere-true: some
        // balances satisfy it and some do not.
        assert!(compiled.may_be_true.satisfiable());
        assert!(compiled.may_be_false.satisfiable());

        let (base, bits) = layout.money().expect("money is in this layout");
        let at = |value: u32| -> Vec<(u32, bool)> {
            (0..bits as u32)
                .map(|b| (base + b, (value >> b) & 1 == 1))
                .collect()
        };
        assert!(compiled.may_be_true.eval(at(50).iter().copied()));
        assert!(!compiled.may_be_true.eval(at(49).iter().copied()));
    }

    #[test]
    fn a_true_literal_holds_everywhere_and_fails_nowhere() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);

        let compiled = compiler.compile(&boolean(true));
        assert!(compiled.may_be_true.valid());
        assert!(!compiled.may_be_false.satisfiable());
        assert!(compiled.is_decided());
    }

    #[test]
    fn an_unreadable_guard_is_undecided_everywhere_rather_than_false() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);

        // A world query the compiler cannot read.
        let compiled = compiler.compile(&Guard::call("IsKimHere".to_string(), vec![]));

        // Permissive in BOTH directions: the search may take the branch and may not.
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

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);
        let compiled = compiler.compile(&Guard::variable("met_kim".to_string()));

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

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);
        let compiled = compiler.compile(&Guard::variable("counter".to_string()));

        let zero: Vec<(u32, bool)> = (0..bits as u32).map(|b| (base + b, false)).collect();
        assert!(!compiled.may_be_true.eval(zero.iter().copied()));

        // Value 4 is the third bit alone.
        let four: Vec<(u32, bool)> = (0..bits as u32).map(|b| (base + b, b == 2)).collect();
        assert!(compiled.may_be_true.eval(four.iter().copied()));
    }

    #[test]
    fn equality_against_a_constant_pins_every_bit() {
        let (graph, symbols) = fixture(&["counter"], Some("counter"));
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let slot = symbols.find("counter").unwrap();
        let (base, bits) = layout.slot(slot).unwrap();

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);
        let compiled = compiler.compile(&Guard::comparison(
            "==".to_string(),
            Guard::variable("counter".to_string()),
            number(3.0),
        ));

        let assignment = |value: u32| -> Vec<(u32, bool)> {
            (0..bits as u32)
                .map(|b| (base + b, (value >> b) & 1 == 1))
                .collect()
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

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);
        let compiled = compiler.compile(&Guard::comparison(
            "~=".to_string(),
            Guard::variable("a".to_string()),
            boolean(true),
        ));

        assert!(compiled.may_be_true.eval([(base, false)]));
        assert!(!compiled.may_be_true.eval([(base, true)]));
    }

    #[test]
    fn a_constant_on_the_left_reads_the_same_as_on_the_right() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let base = layout.slot(symbols.find("a").unwrap()).unwrap().0;

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);
        let compiled = compiler.compile(&Guard::comparison(
            "==".to_string(),
            boolean(true),
            Guard::variable("a".to_string()),
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
                let assignment: Vec<(u32, bool)> = (0..bits as u32)
                    .map(|b| (base + b, (value >> b) & 1 == 1))
                    .collect();
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

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);
        let compiled = compiler.compile(&Guard::comparison(
            ">=".to_string(),
            Guard::variable("counter".to_string()),
            number(3.0),
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

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);
        let compiled = compiler.compile(&Guard::comparison(
            "<=".to_string(),
            number(3.0),
            Guard::variable("counter".to_string()),
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
        let (graph, mut symbols) = fixture(&["a"], None);
        // Declared as a guard of the group would declare it; the fixture's entry has none.
        symbols.declare_variables(["a".to_string(), "untracked".to_string()]);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let world = crate::world::test_world::TestWorld::new()
            .set_variable("untracked", GuardValue::from_number(5.0));

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        let holds = compiler.compile(&Guard::comparison(
            ">=".to_string(),
            Guard::variable("untracked".to_string()),
            number(3.0),
        ));
        let fails = compiler.compile(&Guard::comparison(
            ">=".to_string(),
            Guard::variable("untracked".to_string()),
            number(9.0),
        ));

        assert_eq!(compiler.fallbacks(), 0);
        assert!(holds.may_be_true.valid());
        assert!(!fails.may_be_true.satisfiable());
    }

    /// The rule that makes the whole approximation safe.
    ///
    /// An undecided operand must leave the conjunction takeable, because the engine's
    /// `can_pass` lets Unknown through. Anything else would prune a branch the real search
    /// walks, and a reachable set built from these formulas would MISS states.
    #[test]
    fn an_undecided_operand_leaves_a_conjunction_takeable() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let base = layout.slot(symbols.find("a").unwrap()).unwrap().0;

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);
        let compiled = compiler.compile(&Guard::and(
            Guard::variable("a".to_string()),
            Guard::call("IsKimHere".to_string(), vec![]),
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

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);
        let compiled = compiler.compile(&Guard::not(Guard::variable("a".to_string())));

        assert!(compiled.may_be_true.eval([(base, false)]));
        assert!(!compiled.may_be_true.eval([(base, true)]));
    }

    /// Negating something undecided leaves it undecided, not decided the other way.
    #[test]
    fn negating_an_unknown_stays_unknown() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);
        let compiled = compiler.compile(&Guard::not(Guard::call("IsKimHere".to_string(), vec![])));

        assert!(compiled.may_be_true.valid());
        assert!(compiled.may_be_false.valid());
    }

    /// An item the group gains or loses is tracked, so the guard reads its slot.
    #[test]
    fn a_tracked_item_compiles_against_its_slot() {
        // GainItem is what interns an `item:` slot, so build the graph through the
        // action parser rather than by naming the slot directly.
        let mut symbols = StateSymbols::new();
        let actions =
            crate::parser::action_parser::parse_actions(r#"GainItem("shoes_faln")"#, &mut symbols);
        let node = LookAheadNode {
            actions,
            ..LookAheadNode::new(DialogueNodeId::new(1, 0))
        };
        let graph = LookAheadGraph::new(vec![node], symbols).unwrap();
        let snapshot = graph.symbols().clone();
        let layout = DataLayout::for_graph(&graph, 16, None, false);

        let slot = snapshot
            .find("item:shoes_faln")
            .expect("GainItem interns an item slot");
        let base = layout.slot(slot).unwrap().0;

        let vars = DataVars::new(&layout, &snapshot, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);
        let compiled = compiler.compile(&Guard::call(
            "CheckItem".to_string(),
            vec![Guard::literal(GuardValue::from_text(
                "shoes_faln".to_string(),
            ))],
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

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        let held = compiler.compile(&Guard::call(
            "CheckItem".to_string(),
            vec![Guard::literal(GuardValue::from_text("ledger".to_string()))],
        ));
        let absent = compiler.compile(&Guard::call(
            "CheckItem".to_string(),
            vec![Guard::literal(GuardValue::from_text("nothing".to_string()))],
        ));

        assert!(held.may_be_true.valid());
        assert!(held.is_decided());
        assert!(!absent.may_be_true.satisfiable());
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// A TRACKED item is never answered from the world, however tempting.
    ///
    /// The starting inventory is stale the moment GainItem runs, so reading it for a
    /// tracked item would make the search blind to its own purchases. That is the mirror
    /// of the mistake the untracked case invites, and the reason both sit behind one
    /// deliberately-named pair of methods.
    #[test]
    fn a_tracked_item_ignores_the_worlds_starting_inventory() {
        let mut symbols = StateSymbols::new();
        let actions =
            crate::parser::action_parser::parse_actions(r#"GainItem("shoes_faln")"#, &mut symbols);
        let node = LookAheadNode {
            actions,
            ..LookAheadNode::new(DialogueNodeId::new(1, 0))
        };
        let graph = LookAheadGraph::new(vec![node], symbols).unwrap();
        let snapshot = graph.symbols().clone();
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let base = layout
            .slot(snapshot.find("item:shoes_faln").unwrap())
            .unwrap()
            .0;

        // The world says the player does NOT have them. The slot must still decide, so
        // that a path which buys them is seen.
        let world = crate::world::test_world::TestWorld::new().set_item("shoes_faln", false);
        let vars = DataVars::new(&layout, &snapshot, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        let compiled = compiler.compile(&Guard::call(
            "CheckItem".to_string(),
            vec![Guard::literal(GuardValue::from_text(
                "shoes_faln".to_string(),
            ))],
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
        let node = LookAheadNode {
            actions,
            ..LookAheadNode::new(DialogueNodeId::new(1, 0))
        };
        let graph = LookAheadGraph::new(vec![node], symbols).unwrap();
        let snapshot = graph.symbols().clone();
        let layout = DataLayout::for_graph(&graph, 16, None, false);

        let slot = snapshot
            .find("task:TASK.find_ruby")
            .expect("GainTask interns a task slot");
        let base = layout.slot(slot).unwrap().0;

        let vars = DataVars::new(&layout, &snapshot, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);
        let compiled = compiler.compile(&Guard::call(
            "IsTaskActive".to_string(),
            vec![Guard::literal(GuardValue::from_text(
                "TASK.find_ruby".to_string(),
            ))],
        ));

        assert!(compiled.may_be_true.eval([(base, true)]));
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// A thought the group gains is tracked, and the guard reads its slot rather than the
    /// save.
    ///
    /// The whole point of modelling the thought cabinet. Conversation 631's group gains
    /// `jamais_vu` and then forks on `IsTHCPresent("jamais_vu")`, and while the query was
    /// answered from the save that branch was held shut - the save says no, and the save
    /// never hears about the gain.
    #[test]
    fn a_gained_thought_compiles_against_its_slot() {
        let mut symbols = StateSymbols::new();
        let actions = crate::parser::action_parser::parse_actions(
            r#"GainThought("jamais_vu")"#,
            &mut symbols,
        );
        let node = LookAheadNode {
            actions,
            ..LookAheadNode::new(DialogueNodeId::new(1, 0))
        };
        let graph = LookAheadGraph::new(vec![node], symbols).unwrap();
        let snapshot = graph.symbols().clone();
        let layout = DataLayout::for_graph(&graph, 16, None, false);

        let slot = snapshot
            .find("thought:jamais_vu")
            .expect("GainThought interns a thought slot");
        let base = layout.slot(slot).unwrap().0;

        // The save says the thought is NOT in the cabinet, which is the case that used to
        // decide the guard. The slot must win.
        let world = crate::world::test_world::TestWorld::new().set_thought("jamais_vu", false);
        let vars = DataVars::new(&layout, &snapshot, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        let compiled = compiler.compile(&Guard::call(
            "IsTHCPresent".to_string(),
            vec![Guard::literal(GuardValue::from_text(
                "jamais_vu".to_string(),
            ))],
        ));

        assert!(compiled.may_be_true.eval([(base, true)]));
        assert!(!compiled.may_be_true.eval([(base, false)]));
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// A thought nothing here gains is constant, and comes from the save.
    ///
    /// And the states it cannot be in stay constant whatever happens: internalising is
    /// the cabinet screen and hours of game time, so `IsTHCCooking` and `IsTHCFixed` are
    /// world queries with no slot behind them, now as before.
    #[test]
    fn an_ungained_thought_is_answered_from_the_save() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let world = crate::world::test_world::TestWorld::new()
            .set_thought("guillaume_le_million", true)
            .set_query_bool("IsTHCFixed", false);

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        let present = compiler.compile(&Guard::call(
            "IsTHCPresent".to_string(),
            vec![Guard::literal(GuardValue::from_text(
                "guillaume_le_million".to_string(),
            ))],
        ));
        let internalised = compiler.compile(&Guard::call(
            "IsTHCFixed".to_string(),
            vec![Guard::literal(GuardValue::from_text(
                "guillaume_le_million".to_string(),
            ))],
        ));

        assert!(present.may_be_true.valid());
        assert!(present.is_decided());
        assert!(!internalised.may_be_true.satisfiable());
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// With the clock held constant, a clock question resolves against the world's time.
    #[test]
    fn a_clock_question_is_answered_at_the_worlds_time() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        // Two in the morning.
        let night = crate::world::test_world::TestWorld::new().with_day_minutes(2 * 60);

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&night)
            .with_constant_clock(false);
        let compiled = compiler.compile(&Guard::call("IsNight".to_string(), vec![]));

        assert!(compiled.is_decided());
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// And it is refused, not guessed, when nothing has been told to hold it constant.
    #[test]
    fn a_clock_question_is_undecided_without_the_approximation() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let night = crate::world::test_world::TestWorld::new().with_day_minutes(2 * 60);

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(&night);
        let compiled = compiler.compile(&Guard::call("IsNight".to_string(), vec![]));

        assert!(!compiled.is_decided());
        assert_eq!(compiler.fallbacks(), 1);
    }

    /// The approximation is only an approximation where the group can move the clock.
    #[test]
    fn holding_the_clock_is_exact_unless_the_group_passes_time() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let world = crate::world::test_world::TestWorld::new();

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let exact = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(false);
        assert!(!exact.clock_is_approximated());

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let approximate = GuardCompiler::new(&vars)
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

        let node = |actions| LookAheadNode {
            actions,
            ..LookAheadNode::new(DialogueNodeId::new(1, 0))
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

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);
        let compiled = compiler.compile(&Guard::variable("never_heard_of_it".to_string()));

        assert!(!compiled.is_decided());
        assert_eq!(compiler.fallbacks(), 1);
    }

    /// One step of reachability, which is the operation this compiler exists to take part
    /// in: filter a set of states by a guard, then apply a node's actions to what got
    /// through.
    ///
    /// It could not be written at all until the compiler and the action image shared a
    /// manager. Two formulas over different managers do not combine - so this is the test
    /// that the two halves are actually one system, and not merely two that agree about
    /// variable numbering.
    #[test]
    fn a_guard_and_an_action_can_be_applied_to_the_same_set() {
        let mut symbols = StateSymbols::new();
        let gate = symbols.variable("gate");
        let counter = symbols.variable("counter");
        let actions = vec![DialogueAction::increment(
            counter,
            1,
            false,
            "s".to_string(),
        )];
        let node = LookAheadNode {
            actions: actions.clone(),
            ..LookAheadNode::new(DialogueNodeId::new(1, 0))
        };
        let graph = LookAheadGraph::new(vec![node], symbols).unwrap();
        let snapshot = graph.symbols().clone();
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &snapshot, DiagramBudget::modest());

        // The guard: the gate must be open.
        let mut compiler = GuardCompiler::new(&vars);
        let guard = compiler.compile(&Guard::variable("gate".to_string()));
        assert!(guard.is_decided());

        // Every state where the counter is zero, gate either way.
        let counter_at_zero = vars.slot_equals(counter, 0).unwrap();

        // Filter by the guard, then apply the action - the two operations meeting.
        let allowed = counter_at_zero.and(&guard.may_be_true).expect("and");
        let mut image = crate::symbolic::action_image::ActionImage::new(&vars, 16);
        let after = image.apply(&allowed, &actions, &vars.bottom());

        // Through the gate the counter moved; the states that were refused are simply not
        // in the result, gate closed and counter still zero.
        let open = vars.slot_is_set(gate).unwrap();
        assert!(after.and(&open).expect("and").satisfiable());
        assert!(
            !after
                .and(&open.not().expect("not"))
                .expect("and")
                .satisfiable(),
            "a state that failed the guard should not appear in the image",
        );

        let moved = vars.slot_equals(counter, 1).unwrap();
        assert!(after.and(&moved).expect("and").satisfiable());
        let still = vars.slot_equals(counter, 0).unwrap();
        assert!(!after.and(&still).expect("and").satisfiable());
    }

    /// How much room the squeezed compile below gets, in bytes.
    ///
    /// A WINDOW RATHER THAN A CEILING, like the one in `reachability`: wide enough to lay
    /// the variables out, since a compiler with no variables tests nothing here, and narrow
    /// enough that conjoining them does not fit. A layout or encoding change can move it out
    /// from under this test, and the symptom is the assertion below rather than a crash.
    const SQUEEZED: usize = 4 * 1024;

    /// How many slots the chain conjoins. Enough that the conjunction is real work.
    const CHAINED: usize = 40;

    /// A manager that fills while a guard is being COMPILED reports it, rather than
    /// aborting.
    ///
    /// The last layer of the same defect de-rvxw and de-nyv2 removed from the searches and
    /// the register. Every diagram operation here went through expect, so a guard compiled
    /// against a full manager took the process with it - and this is the layer reached most
    /// often, once per entry per search.
    ///
    /// THE ANSWER IS STILL SOUND, which is why this asserts on both halves. An undecided
    /// guard lets every branch through, so a compile that ran out of room over-approximates
    /// exactly as one the language cannot express does; what the flag adds is being able to
    /// tell those apart. de-iqcc.
    #[test]
    fn a_manager_that_fills_while_compiling_is_reported_rather_than_fatal() {
        let names: Vec<String> = (0..CHAINED).map(|i| format!("v{i}")).collect();
        let borrowed: Vec<&str> = names.iter().map(|n| n.as_str()).collect();
        let (graph, symbols) = fixture(&borrowed, Some("v0"));
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::new(SQUEEZED));
        let mut compiler = GuardCompiler::new(&vars);

        // A conjunction of every slot, which is the shape that has to build a diagram node
        // per term and so the one that runs out first.
        let mut guard = Guard::variable(borrowed[0]);
        for name in &borrowed[1..] {
            guard = Guard::and(guard, Guard::variable(*name));
        }

        let compiled = compiler.compile(&guard);

        assert!(
            compiler.out_of_memory(),
            "a compile that could not finish should say the nodes ran out",
        );
        assert!(
            compiled.may_be_true.satisfiable(),
            "and the answer it falls back to is the permissive one, not the empty set",
        );
    }
}
