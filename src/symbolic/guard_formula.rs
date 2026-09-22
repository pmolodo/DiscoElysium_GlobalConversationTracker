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

use crate::core::clock::ClockTime;
use crate::core::guard::{Arguments, Guard, GuardExpression, GuardRef};
use crate::core::guard_value::{GuardValue, GuardValueKind};
use crate::core::state::{ITEM_PREFIX, THOUGHT_PREFIX};
use crate::core::types::{DialogueNodeId, Ternary};
use crate::graph::LookAheadGraph;
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
/// Only ever a threshold or a point, because the variable is a sum: `v >= c` is
/// `d >= c - v0` and `v == c` is `d == c - v0`, and every other operator is one of those
/// negated. See [`GuardCompiler::rebased`].
enum Distances {
    All,
    None,
    AtLeast(i64),
    Exactly(i64),
}

/// Why [`GuardCompiler::amounts_of`] could not say what a variable reads as.
enum Amounts {
    /// The world cannot give the number the search starts from.
    Unknown,
    /// The manager had no room for a formula.
    NoRoom,
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
/// The integer a literal stands for, if it is one a slot could hold.
///
/// Free rather than a method because it decides which comparisons the compiler can rewrite,
/// and `DataLayout` has to ask the same question about a guard it is deciding whether to drop
/// a slot for. Two spellings of "a constant a slot could be compared against" would let the
/// layout drop a slot the compiler then cannot rewrite.
pub(crate) fn whole_number(value: &GuardValue) -> Option<i32> {
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
    /// The reputation ranges no search from the menu's starts can change the winner of, keyed
    /// by the range's first index, each with the world's winner. See
    /// [`Self::settle_reputation`].
    settled_reputation: HashMap<usize, Option<&'static str>>,
    /// How many reputation questions were answered from [`Self::settled_reputation`].
    reputation_from_world: usize,
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
            settled_reputation: HashMap::new(),
            reputation_from_world: 0,
        }
    }

    /// Settles the reputation ranges whose winner no search from `starts` can change, so their
    /// questions are answered from the world. Needs [`Self::with_world`] first.
    ///
    /// `graph` is the group AS THE MENU CAN WALK IT - see `bridge::walkable_menu` - so a write
    /// behind a door this world keeps shut is already gone from it, and is not counted here.
    /// Asked of the whole group, such a write would count as reachable.
    ///
    /// ## When nothing can change the winner
    ///
    /// Raising the reputation that is winning never changes the winner: it was ahead of every
    /// reputation before it in the loop and strictly above every one after, and a larger amount
    /// is still both. So the winner holds for a whole search if every write to the range that
    /// the starts can reach is a raise, and each raise is to a reputation that is winning when
    /// it happens. A raise is known to be to the winner if its reputation is the world's winner,
    /// or if some entry on EVERY link path from the starts to it - a dominator, other than a
    /// start - has a guard that requires that reputation to be winning.
    ///
    /// By induction along any route: before the first write that changed the winner, the winner
    /// was the world's; that write is dominated by a guard passed earlier on the route, which
    /// required its reputation to be winning then, and so it still was. Contradiction.
    ///
    /// ## Why this is worth having beside the tracked comparison
    ///
    /// The comparison in [`Self::highest_reputation`] is exact already. A settled range is
    /// exact too, and it costs nothing per state. It is also the common shape: Evrart's folder
    /// (785) raises each copotype only behind the guard that it is winning.
    ///
    /// ## Why the starts, and why not a start as the dominator
    ///
    /// A request's searches all begin at its starts or at the entries one outcome of a rolled
    /// start opens, and every route from those is the tail of a route from the start. A start's
    /// own guard is not passed by a route that begins below it, so a start does not count as a
    /// dominator here - the world's winner covers the menu the player is standing at.
    pub fn settle_reputation(&mut self, graph: &LookAheadGraph, starts: &[DialogueNodeId]) {
        let Some(world) = self.world else {
            return;
        };
        let asked: Vec<std::ops::Range<usize>> = [
            crate::core::reputation::COPOTYPE,
            crate::core::reputation::POLITICAL,
        ]
        .into_iter()
        .filter(|range| {
            graph
                .nodes()
                .any(|node| Self::asks_about_range(&node.guard, range))
        })
        .collect();
        if asked.is_empty() {
            return;
        }

        let dominators = super::dominators::Dominators::of(graph, starts);
        let symbols = self.vars.symbols();
        for range in asked {
            let amounts: Option<HashMap<&'static str, i64>> =
                crate::core::reputation::IN_ENUM_ORDER[range.clone()]
                    .iter()
                    .map(|&name| {
                        let variable =
                            symbols.variable_ref(&crate::core::reputation::variable_of(name))?;
                        let amount = world.get_variable(variable).try_as_number()?;
                        Some((name, i64::from(amount as i32)))
                    })
                    .collect();
            let Some(amounts) = amounts else {
                continue;
            };
            let winner = crate::core::reputation::highest(range.clone(), |name| {
                amounts.get(name).map(|&amount| amount as i32)
            })
            .expect("every amount in the range was read");

            if self.no_write_changes_winner(graph, starts, &dominators, &range, winner, &amounts) {
                self.settled_reputation.insert(range.start, winner);
            }
        }
    }

    /// Whether every write to `range` the starts can reach leaves `winner` winning. See
    /// [`Self::settle_reputation`].
    ///
    /// ## Which raises cannot matter
    ///
    /// The game's loop has a winner exactly where one reputation's amount is above zero and
    /// strictly above every other in the range: an equal amount earlier or later clears it, and
    /// a larger one takes over. So with raises only, the winner stays the winner while every
    /// other reputation stays below it. A raise is left out of that count where it cannot
    /// happen while the winner holds: behind a guard that requires its own reputation to be
    /// winning, which only the winner is - so it raises the winner, or never runs.
    ///
    /// Every other reachable raise is summed per reputation, and the range is settled where
    /// no reputation but the winner can get to the winner's amount. With nothing winning, any
    /// such raise could break the tie, so none may be left.
    ///
    /// ONLY AN ORDINARY ENTRY'S GUARD CLOSES A ROUTE. A passive check whose condition fails is
    /// stepped over onto its links rather than refusing them.
    fn no_write_changes_winner(
        &self,
        graph: &LookAheadGraph,
        starts: &[DialogueNodeId],
        dominators: &super::dominators::Dominators,
        range: &std::ops::Range<usize>,
        winner: Option<&'static str>,
        amounts: &HashMap<&'static str, i64>,
    ) -> bool {
        let symbols = self.vars.symbols();
        let mut reachable_raises: HashMap<&'static str, i64> = HashMap::new();
        for node in graph.nodes().filter(|node| dominators.reaches(node.id)) {
            for action in node.all_actions() {
                let Some(raised) = Self::reputation_written(action, symbols, range) else {
                    continue;
                };
                if action.kind() != crate::core::action::DialogueActionKind::Increment
                    || action.value() <= 0
                {
                    return false;
                }
                if winner == Some(raised) {
                    continue;
                }

                let behind_its_own_lead = std::iter::once(node.id)
                    .chain(dominators.above(node.id))
                    .filter(|id| !starts.contains(id))
                    .filter_map(|id| graph.get(id))
                    .filter(|gate| gate.kind == crate::core::types::DialogueCheckKind::None)
                    .any(|gate| Self::requires_winning(gate.guard.as_ref(), range, raised));
                if !behind_its_own_lead {
                    *reachable_raises.entry(raised).or_default() += i64::from(action.value());
                }
            }
        }

        let Some(winner) = winner else {
            return reachable_raises.is_empty();
        };
        let leading = amounts[winner];
        reachable_raises
            .iter()
            .all(|(reputation, raises)| amounts[reputation] + raises < leading)
    }

    /// How many reputation questions were answered from the world because no search could
    /// change their winner.
    pub fn reputation_from_world(&self) -> usize {
        self.reputation_from_world
    }

    /// Whether a guard asks a reputation question over either range.
    ///
    /// For a caller that compiles guards before [`Self::settle_reputation`] has run: such a
    /// guard compiled and kept then would keep the per-state answer after its range settles.
    pub fn asks_reputation(guard: &Guard) -> bool {
        [
            crate::core::reputation::COPOTYPE,
            crate::core::reputation::POLITICAL,
        ]
        .iter()
        .any(|range| Self::asks_about_range(guard, range))
    }

    /// Whether a guard asks a reputation question over `range`.
    fn asks_about_range(guard: &Guard, range: &std::ops::Range<usize>) -> bool {
        guard.nodes().any(|node| match node.expression() {
            GuardExpression::Call(name, _) => {
                crate::core::reputation::range_of(name).as_ref() == Some(range)
            }
            _ => false,
        })
    }

    /// The reputation in `range` an action writes, if it writes one.
    fn reputation_written(
        action: &crate::core::action::DialogueAction,
        symbols: &crate::core::state::StateSymbols,
        range: &std::ops::Range<usize>,
    ) -> Option<&'static str> {
        let name = symbols.name_of(usize::try_from(action.slot()).ok()?)?;
        crate::core::reputation::IN_ENUM_ORDER[range.clone()]
            .iter()
            .copied()
            .find(|reputation| crate::core::reputation::variable_of(reputation) == name)
    }

    /// Whether a guard can only hold where `reputation` is winning `range`: the question
    /// itself, compared against true, or a conjunction with it on either side.
    fn requires_winning(
        guard: GuardRef<'_>,
        range: &std::ops::Range<usize>,
        reputation: &str,
    ) -> bool {
        match guard.expression() {
            GuardExpression::Call(name, args) => {
                crate::core::reputation::range_of(name).as_ref() == Some(range)
                    && Self::text_argument(args).as_deref() == Some(reputation)
            }
            GuardExpression::And(left, right) => {
                Self::requires_winning(left, range, reputation)
                    || Self::requires_winning(right, range, reputation)
            }
            GuardExpression::Comparison("==", left, right) => {
                match (Self::boolean_of(left), Self::boolean_of(right)) {
                    (None, Some(true)) => Self::requires_winning(left, range, reputation),
                    (Some(true), None) => Self::requires_winning(right, range, reputation),
                    _ => false,
                }
            }
            _ => false,
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

    /// Answers clock questions from the world, WHERE THE LAYOUT CARRIES NO CLOCK.
    ///
    /// Where it carries one, the questions are answered over the register instead and this
    /// decides nothing; `DataLayout::clock_can_move` is what puts a clock in the layout, and
    /// it does so exactly where a `PassTime` in the group can move an unlocked one.
    ///
    /// So this is the answer for the rest: a group with no `PassTime`, or one whose world
    /// sends the clock locked, and in both the clock is a constant rather than an
    /// approximation. `group_passes_time` says which of the two, since it is only the first
    /// that leaves anything to be wrong about.
    ///
    /// Why it is worth taking. Modelling the clock means eleven more variables and
    /// magnitude comparisons against them, which is the classic way to make a decision
    /// diagram explode; a guard like `IsHourBetween(14, 18)` is a range test on a
    /// bit-blasted integer. Against that, the engine advances the clock by fifteen
    /// minutes per `PassTime` and by nothing else, so a conversation rarely moves it far
    /// enough to change what a coarse question like `IsNight()` answers.
    ///
    /// What it costs, WHERE IT IS STILL AN APPROXIMATION - a group that passes time whose
    /// layout was built without a clock anyway, which is a caller varying the arm rather than
    /// the shipped path. There it can report a branch CLOSED that the real search would walk,
    /// the unsafe direction and the only place in this compiler that is true: a guard that
    /// only opens once time has passed is judged against the starting hour and refused.
    /// [`Self::clock_is_approximated`] is how such a caller sees that it is exposed.
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
    /// than exact.
    ///
    /// THREE THINGS HAVE TO LINE UP, and the third is what keeps this from crying wolf. The
    /// clock has to be answered from the world at all; some action in the group has to be a
    /// `PassTime`, or there is nothing to be wrong about; and the layout has to be carrying
    /// no clock, because where it carries one the questions are answered over the register
    /// and there is no approximation left to report. See [`DataLayout::clock_can_move`].
    ///
    /// [`DataLayout::clock_can_move`]: crate::symbolic::data_layout::DataLayout::clock_can_move
    pub fn clock_is_approximated(&self) -> bool {
        self.constant_clock && self.clock_approximated && self.vars.clock_ops().is_none()
    }

    /// How many diagram nodes the manager is holding.
    ///
    /// What the work COST, in the unit the budget is denominated in - so a caller reporting it
    /// can say whether a slow menu was a large diagram or a busy machine, which a clock alone
    /// cannot. Read twice and subtracted, since a manager outlives one request:
    /// `workspace::Workspace` keeps one across a conversation, so the count at any moment
    /// includes whatever earlier questions left behind.
    pub fn diagram_nodes(&self) -> usize {
        self.vars.node_count()
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

            // AN ITEM GROUP, as `BoundContext::query` answers it: held when any member is,
            // each member read off its `item:` slot where the group moves it and from the
            // starting inventory where it does not. So the rail is the union of the tracked
            // members' slots, or everything where an untracked member is already held.
            //
            // An untracked member the world cannot answer, or a group whose members it cannot
            // name, is undecided - permissive, like the engine's Unknown.
            GuardExpression::Call(name, args)
                if name == crate::core::item_group::CHECK_ITEM_GROUP =>
            {
                let (Some(group), Some(world)) = (Self::text_argument(args), self.world) else {
                    return Plan::Settled(
                        self.undecided("call: item group not answerable", guard.to_string()),
                    );
                };
                let Some(members) = world.items_in_group(&group) else {
                    return Plan::Settled(
                        self.undecided("call: item group members unknown", guard.to_string()),
                    );
                };
                let held = world.initially_held_in_group(&group);

                let mut holds = self.bottom();
                let mut unanswered = false;
                for item in &members {
                    if let Some(slot) = self.slot_is_set(&format!("{ITEM_PREFIX}{item}")) {
                        match holds.or(&slot) {
                            Ok(joined) => holds = joined,
                            Err(_) => return Plan::Settled(self.no_room(guard.to_string())),
                        }
                        continue;
                    }

                    match held.as_ref().map(|held| held.iter().any(|h| h == item)) {
                        Some(true) => {
                            let t = self.top();
                            return Plan::Settled(self.decided(t));
                        }
                        Some(false) => {}
                        None => unanswered = true,
                    }
                }

                if unanswered {
                    self.undecided("call: item group member unknown", guard.to_string())
                } else {
                    self.decided(holds)
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

            // The clock, held at whatever the world says, for the groups whose layout carries
            // none - see `with_constant_clock` for when that is and what it costs. The arm
            // above takes every group that does carry one, so the two can never both apply.
            GuardExpression::Call(name, args)
                if self.clock_from_the_world() && ClockTime::owns(name) =>
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

            // A FLAG, which is a dialogue variable written as a call - so it is read off the
            // variable's own slot, exactly as `Variable[name]` is, and `FlagNotSet` is that
            // negated. `SetFlag` and `UnsetFlag` are modelled writes, so this is a question
            // the search really does change, and answering it from the world would report
            // the value the crawl started with.
            //
            // Where the group declares no such variable the flag is named by something other
            // than a literal, so no slot exists and there is nothing to read; that falls
            // through to the arms below and ends up undecided, which is permissive.
            GuardExpression::Call(name, args)
                if crate::world::flag_query(name).is_some()
                    && Self::text_argument(args)
                        .is_some_and(|flag| self.vars.slot_of(&flag).is_some()) =>
            {
                let negated = crate::world::flag_query(name).expect("just matched");
                let flag = Self::text_argument(args).expect("just matched");
                match self.slot_is_set(&flag) {
                    // NEGATED BY BUILDING THE COMPLEMENT, which is diagram work like any
                    // other and can run out of room - so it is reported as no_room rather
                    // than as a guard the language could not express.
                    Some(set) if negated => match set.not() {
                        Ok(clear) => self.decided(clear),
                        Err(_) => self.no_room(guard.to_string()),
                    },
                    Some(set) => self.decided(set),
                    None => self.undecided("call: flag has no slot", guard.to_string()),
                }
            }

            // An ACTION the database calls from a guard, which the plugin is never asked to
            // run because running it writes to the player's save. `BoundContext::query`
            // answers it false - a function returning nothing answers nil, and `nil == true`
            // is false - and this has to agree. Left to fall through, the world has no
            // answer for a question nobody asked, so the compiler would go undecided and
            // stay permissive exactly where the search is decisive.
            GuardExpression::Call(name, _)
                if crate::core::modelling::is_action_used_as_guard(name) =>
            {
                let f = self.bottom();
                self.decided(f)
            }

            // WHETHER A SKILL IS DAMAGED: its `damage:` slot where the group damages or heals
            // it, and the world's answer where nothing does - as `BoundContext::query` has it.
            GuardExpression::Call(name, args)
                if crate::core::damage::skill_read_by(name).is_some() =>
            {
                let skill = crate::core::damage::skill_read_by(name).expect("just matched");
                let slot = format!("{}{skill}", crate::core::state::DAMAGE_PREFIX);
                if self.vars.slot_of(&slot).is_some() {
                    match self.slot_is_set(&slot) {
                        Some(holds) => self.decided(holds),
                        None => self.no_room(guard.to_string()),
                    }
                } else {
                    match self.constant_query(name, args) {
                        Some(true) => {
                            let t = self.top();
                            self.decided(t)
                        }
                        Some(false) => {
                            let f = self.bottom();
                            self.decided(f)
                        }
                        None => self.undecided("call: damage, world cannot say", guard.to_string()),
                    }
                }
            }

            // WHAT IS WORN, where the group can take away an item a slot the question reads
            // holds: one case per combination of those items lost or kept, the all-kept case
            // the world's answer - as `BoundContext::query` has it. See `core::equipment`.
            GuardExpression::Call(name, args)
                if crate::core::equipment::reads_equipment(name)
                    && !self.losable_worn_items(name, args).is_empty() =>
            {
                let items = self.losable_worn_items(name, args);
                self.equipment_after_losses(name, args, &items, guard.to_string())
            }

            // WHETHER KIM IS HERE OR IN THE PARTY, where the group takes Kim out of it: the
            // world's answer while the removal slot is clear, and false once it is set - as
            // `BoundContext::query` has it. See `core::party`.
            GuardExpression::Call(name, args)
                if crate::core::party::reads_kim_removal(name)
                    && self
                        .vars
                        .slot_of(crate::core::party::KIM_REMOVED_SLOT)
                        .is_some() =>
            {
                let kept = self
                    .slot_is_set(crate::core::party::KIM_REMOVED_SLOT)
                    .and_then(|removed| removed.not().ok());
                match (kept, self.constant_query(name, args)) {
                    (None, _) => self.no_room(guard.to_string()),
                    (Some(kept), Some(true)) => self.decided(kept),
                    (Some(_), Some(false)) => {
                        let f = self.bottom();
                        self.decided(f)
                    }
                    // Unknown until the removal, and false after it.
                    (Some(kept), None) => {
                        let open =
                            self.undecided("call: party, world cannot say", guard.to_string());
                        MayBe {
                            may_be_true: kept,
                            may_be_false: open.may_be_false,
                        }
                    }
                }
            }

            // WHICH REPUTATION IS WINNING, from the amounts as they stand in each state - the
            // group's own reputation actions move them. See `Self::highest_reputation`.
            GuardExpression::Call(name, args)
                if crate::core::reputation::range_of(name).is_some() =>
            {
                self.highest_reputation(name, args, guard.to_string())
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
        if let Some(compiled) = self.variable_against_query(op, left, right) {
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
        if let Some(value) = whole_number(literal) {
            // A COUNTER THE LAYOUT DROPPED has no bits to pin: its value is how many of some
            // `once` slots are set, and those slots are still carried. This has to come before
            // every path below, all of which read bits that are no longer there - and a
            // comparison against a slot the layout does not carry reads as undecided, which is
            // the permissive answer and would let the gate through. See
            // `DataLayout::dropping_redundant_counters`.
            if let Some(slot) = self.vars.slot_of(name)
                && self.vars.layout().counter_onces(slot).is_some()
            {
                return match self.counted(slot, op, value as i64) {
                    Some(holds) => self.decided(holds),
                    None => self.no_room(Self::rendered(op, name, literal)),
                };
            }

            // A REBASED SLOT holds the distance the search has travelled rather than the
            // value the guard is written about, so the comparison is rewritten around the
            // value the search started at. This has to come first: the paths below read the
            // slot's bits as the value itself, which for such a slot they are not.
            if let Some(slot) = self.vars.slot_of(name)
                && self.vars.layout().is_delta(slot)
            {
                return match self.rebased(slot, name, op, value as i64) {
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

    /// `Variable[name] op value` over a slot the layout holds as a DELTA.
    ///
    /// ## What the slot holds, and what the guard asks about
    ///
    /// The slot holds `d`, how far the search's own actions have moved the variable. The
    /// guard asks about the variable, which is `d` added to whatever the save held. So the
    /// rewriting folds the save's value into the CONSTANT and leaves the slot alone:
    /// `v >= c` is `d >= c - v0`. See `DataLayout::lay_out_counters` for why the slot is
    /// shaped that way and what it saves.
    ///
    /// NOTHING SATURATES. Such a counter cannot loop, so neither engine caps it, and the sum
    /// is the value at every distance - no distance is a special case.
    ///
    /// ## Why the world is required here and nowhere else
    ///
    /// `v0` comes from the save. A compiler with no world cannot produce it and says so -
    /// undecided, which is the permissive answer - rather than guessing zero, which would
    /// close branches a richer save opens.
    fn rebased(&mut self, slot: usize, name: &str, op: &str, constant: i64) -> Rebase {
        let Some(world) = self.world else {
            return Rebase::Unknown;
        };
        let base = self.rebase_start(world, name);

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

        // `base + d >= wanted`, as a bound on `d` alone.
        let at_least = |wanted: i64| -> Distances {
            if wanted - base <= 0 {
                Distances::All
            } else {
                Distances::AtLeast(wanted - base)
            }
        };
        let distances = match positive {
            ">=" => at_least(constant),
            ">" => at_least(constant + 1),
            _ if constant - base < 0 => Distances::None,
            _ => Distances::Exactly(constant - base),
        };

        let Some(holds) = self.at_distances(slot, distances, negate) else {
            return Rebase::NoRoom;
        };
        Rebase::Formula(holds)
    }

    /// `counter op constant`, where the counter is how many of some `once` slots are set.
    ///
    /// ## What the rebase here is, and what it is not
    ///
    /// A delta slot holds how far the SEARCH has moved a variable, so the save's whole value
    /// folds into the constant. This rebases by less than that. The `once` slots are seeded
    /// from the world by `state::seed_state`, so a site that fired before the save was written
    /// arrives with its slot already set, and the count covers this group's own history without
    /// help. What it cannot cover is a writer in some OTHER conversation - these are global
    /// variables - so the layout records what the save holds beyond the sites it marks as
    /// shown, and that offset, and only it, comes off the constant:
    ///
    /// ```text
    /// counter op k   is   (offset + set count) op k   is   set count op (k - offset)
    /// ```
    ///
    /// A threshold at or below zero is then met by every state, which is the right answer and
    /// not a degenerate one: the outside writes alone already satisfy the gate.
    fn counted(&mut self, slot: usize, op: &str, constant: i64) -> Option<BDDFunction> {
        let (onces, offset) = self.vars.layout().counter_onces(slot)?;
        let onces = onces.to_vec();
        let constant = constant - offset as i64;

        let at_least = |compiler: &Self, wanted: i64| -> Option<BDDFunction> {
            if wanted <= 0 {
                return Some(compiler.top());
            }
            compiler.at_least_set(&onces, wanted as usize)
        };

        match op {
            ">=" => at_least(self, constant),
            ">" => at_least(self, constant + 1),
            "<" => at_least(self, constant).and_then(|held| held.not().ok()),
            "<=" => at_least(self, constant + 1).and_then(|held| held.not().ok()),
            "==" | "~=" => {
                let reached = at_least(self, constant)?;
                let beyond = at_least(self, constant + 1)?;
                let exactly = reached.and(&beyond.not().ok()?).ok()?;
                if op == "==" {
                    Some(exactly)
                } else {
                    exactly.not().ok()
                }
            }
            _ => None,
        }
    }

    /// "At least `wanted` of `slots` are set", counted rather than enumerated.
    ///
    /// ONE PASS PER SLOT, carrying `wanted + 1` formulas: after each slot, entry `j` holds "at
    /// least j of the slots so far". Taking the slot advances by one and skipping it does not,
    /// which is the whole recurrence. That is `slots * wanted` diagram operations, where
    /// enumerating the subsets would be `slots choose wanted` - thirty slots and a threshold of
    /// three is ninety operations against four thousand terms.
    fn at_least_set(&self, slots: &[usize], wanted: usize) -> Option<BDDFunction> {
        if wanted > slots.len() {
            return Some(self.bottom());
        }
        let mut reached = vec![self.top()];
        reached.extend(std::iter::repeat_n(self.bottom(), wanted));

        for &slot in slots {
            let set = self.vars.slot_is_set(slot)?;
            let clear = set.not().ok()?;
            let mut next = vec![self.top()];
            for count in 1..=wanted {
                let taking = set.and(&reached[count - 1]).ok()?;
                let skipping = clear.and(&reached[count]).ok()?;
                next.push(taking.or(&skipping).ok()?);
            }
            reached = next;
        }
        reached.pop()
    }

    /// Where a rebased slot's variable starts: what `seed_state` would have put in the slot,
    /// read the same way it reads it - a boolean is its truth, a number is itself, anything else
    /// is nothing there - and held at zero or above, as a slot's run of bits is.
    fn rebase_start(&self, world: &dyn ILookAheadWorld, name: &str) -> i64 {
        let held = world.get_variable(self.declared(name));
        match held.kind() {
            GuardValueKind::Boolean => i64::from(held.boolean()),
            GuardValueKind::Number => held.number() as i64,
            _ => 0,
        }
        .max(0)
    }

    /// The states a rebased comparison holds in, from the distances it holds at.
    fn at_distances(
        &mut self,
        slot: usize,
        distances: Distances,
        negate: bool,
    ) -> Option<BDDFunction> {
        let holds = match distances {
            Distances::All => self.top(),
            Distances::None => self.bottom(),
            Distances::AtLeast(bound) => self.vars.slot_ops(slot)?.compare(">=", bound)?,
            // A distance the slot is too narrow for is one no state is at.
            Distances::Exactly(bound) => match u32::try_from(bound) {
                Ok(bound) => self.vars.slot_equals(slot, bound)?,
                Err(_) => self.bottom(),
            },
        };
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
            // A READING AGAINST A LITERAL, over the register that carries the clock. Both of
            // these are numbers the clock model gives for a minute, so the question is which
            // step counts give a number the comparison accepts - and the model is asked rather
            // than the arithmetic restated here.
            "HourCount" | "TotalHourCount" => {
                let day = self.day() as i32;
                let reading = name.clone();
                let holds = self.steps_where(|minute| {
                    ClockTime::answer(&reading, &[], minute, day)
                        .try_as_number()
                        .and_then(|at| Self::compares(at, op, value))
                        .unwrap_or(false)
                })?;
                Some(self.decided(holds))
            }
            _ => None,
        }
    }

    /// The story's day, which no search can move: a conversation is over long before midnight.
    fn day(&self) -> i64 {
        self.world.map_or(1, |world| i64::from(world.day_counter()))
    }

    /// Whether clock questions are answered from the world rather than over a register.
    ///
    /// The register wins wherever the layout has one, because it is the state's own hour
    /// rather than the hour the search started at. See `DataLayout::clock_can_move` for when
    /// a layout carries a clock, which is rarely.
    fn clock_from_the_world(&self) -> bool {
        self.constant_clock && self.vars.clock_ops().is_none()
    }

    /// The minute of the day a walk is at once it has taken `steps` of them.
    ///
    /// The register counts steps and the WORLD says where the count is read from, which is why
    /// the base lives here rather than in the layout - see `DataLayout::clock_run`.
    fn minute_after(&self, start: i32, steps: u32) -> i32 {
        let passed = i64::from(start) + i64::from(steps) * i64::from(ClockTime::PASS_TIME_MINUTES);
        passed.rem_euclid(i64::from(ClockTime::MINUTES_IN_DAY)) as i32
    }

    /// The states whose clock satisfies `holds`, as a formula over the step count.
    ///
    /// ## Every clock question reduces to this
    ///
    /// A state's clock is decided entirely by how many `PassTime` steps got it there, so a
    /// question about the clock is a question about WHICH COUNTS satisfy it. There are at most
    /// ninety-six of those and usually two or three, so the answer is found by asking the
    /// clock model at each one rather than by restating what the model knows - which is what
    /// keeps the boundaries, afternoon running to the end of the eighteenth hour and dusk
    /// being the single hour of nineteen, stated in exactly one place. This port has had them
    /// wrong once by restating them.
    ///
    /// ## Gathered as runs
    ///
    /// Consecutive counts are consecutive quarter-hours, so the counts satisfying an hour
    /// question come in runs - four of them to an hour - and each run is one comparison pair
    /// rather than one term per count.
    ///
    /// A run covering every count is every state, and is returned as such: the width holds at
    /// least the ceiling and often more, so a comparison against the ceiling would leave the
    /// counts above it on the other rail and a guard every real state satisfies would read as
    /// undecided. Those counts do not occur - the seed pins the register to zero and the image
    /// stops or wraps at the ceiling.
    fn steps_where(&self, holds: impl Fn(i32) -> bool) -> Option<BDDFunction> {
        let run = self.vars.layout().clock_run()?;
        let ops = self.vars.clock_ops()?;
        let start = self.world?.day_minutes();

        let range = |from: u32, to: u32| -> Option<BDDFunction> {
            if from == 0 && to >= run.ceiling {
                return Some(self.top());
            }
            let upper = ops.compare("<=", i64::from(to))?;
            if from == 0 {
                return Some(upper);
            }
            let lower = ops.compare(">=", i64::from(from))?;
            lower.and(&upper).ok()
        };

        let mut formula = self.bottom();
        let mut began: Option<u32> = None;
        for steps in 0..=run.ceiling {
            if holds(self.minute_after(start, steps)) {
                began.get_or_insert(steps);
                continue;
            }
            if let Some(from) = began.take() {
                formula = formula.or(&range(from, steps - 1)?).ok()?;
            }
        }
        if let Some(from) = began {
            formula = formula.or(&range(from, run.ceiling)?).ok()?;
        }

        Some(formula)
    }

    /// Every step count at which a clock question is true, as a formula.
    fn clock_hours_formula(&mut self, name: &str, args: Arguments<'_>) -> Option<BDDFunction> {
        let values = Self::literal_arguments(args)?;
        let day = self.day() as i32;

        // A QUESTION THE MODEL CANNOT ANSWER is refused here rather than read as "at no count",
        // which would be a definite answer it has no right to. Whether it can answer depends on
        // the name and its arguments, not on the time, so one minute settles it.
        if ClockTime::answer(name, &values, 0, day).kind() == GuardValueKind::Unknown {
            return None;
        }

        self.steps_where(|minute| ClockTime::answer(name, &values, minute, day).boolean())
    }

    /// Whether `a op b` holds, for the operators a guard writes between two numbers.
    fn compares(a: f64, op: &str, b: f64) -> Option<bool> {
        Some(match op {
            ">=" => a >= b,
            "<=" => a <= b,
            ">" => a > b,
            "<" => a < b,
            "==" => a == b,
            "~=" | "!=" => a != b,
            _ => return None,
        })
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

    /// The items in the slots an equipment question reads that the group can take away.
    fn losable_worn_items(&self, name: &str, args: Arguments<'_>) -> Vec<String> {
        let Some(world) = self.world else {
            return Vec::new();
        };
        let argument = Self::text_argument(args);
        let mut items: Vec<String> =
            crate::core::equipment::slots_read_by(name, argument.as_deref())
                .into_iter()
                .filter_map(|slot| world.item_in_slot(slot))
                .filter(|item| {
                    !item.is_empty()
                        && self
                            .vars
                            .slot_of(&format!("{}{item}", crate::core::state::UNEQUIPPED_PREFIX))
                            .is_some()
                })
                .collect();
        items.sort();
        items.dedup();
        items
    }

    /// An equipment question over the items the group can take away, one case per
    /// combination of them lost or kept.
    ///
    /// A handful of items at most - one per slot the question reads - so enumerating the
    /// combinations is cheaper than it sounds. With nothing lost the answer is the world's,
    /// exactly as `BoundContext::query` falls through to it.
    fn equipment_after_losses(
        &mut self,
        name: &str,
        args: Arguments<'_>,
        items: &[String],
        subject: String,
    ) -> MayBe {
        let Some(world) = self.world else {
            return self.undecided("call: equipment, no world", subject);
        };
        let argument = Self::text_argument(args);
        let lost_slots: Option<Vec<BDDFunction>> = items
            .iter()
            .map(|item| {
                self.slot_is_set(&format!("{}{item}", crate::core::state::UNEQUIPPED_PREFIX))
            })
            .collect();
        let Some(lost_slots) = lost_slots else {
            return self.no_room(subject);
        };

        let mut may_be_true = self.bottom();
        let mut may_be_false = self.bottom();
        let mut unknown = false;
        for case in 0..1u32 << items.len() {
            let lost = |item: &str| {
                items
                    .iter()
                    .position(|i| i == item)
                    .is_some_and(|index| case & (1 << index) != 0)
            };
            let answer = if case == 0 {
                self.constant_query(name, args)
            } else {
                crate::core::equipment::answer_after_losses(
                    name,
                    argument.as_deref(),
                    |slot| world.item_in_slot(slot),
                    lost,
                    |group| world.items_in_group(group),
                )
                .flatten()
            };

            let mut cube = Ok(self.top());
            for (index, slot) in lost_slots.iter().enumerate() {
                cube = cube.and_then(|c| {
                    if case & (1 << index) != 0 {
                        c.and(slot)
                    } else {
                        slot.not().and_then(|clear| c.and(&clear))
                    }
                });
            }
            let Ok(cube) = cube else {
                return self.no_room(subject);
            };

            let joined = match answer {
                Some(true) => may_be_true.or(&cube).map(|t| may_be_true = t),
                Some(false) => may_be_false.or(&cube).map(|f| may_be_false = f),
                None => {
                    unknown = true;
                    may_be_true
                        .or(&cube)
                        .and_then(|t| may_be_false.or(&cube).map(|f| (t, f)))
                        .map(|(t, f)| {
                            may_be_true = t;
                            may_be_false = f;
                        })
                }
            };
            if joined.is_err() {
                return self.no_room(subject);
            }
        }

        // Counted as a fallback where some case is unknowable, but the rails stay exact: the
        // unknowable cases are already on both.
        if unknown {
            self.undecided("call: equipment, world cannot say", subject);
        } else {
            self.compiled += 1;
        }
        MayBe {
            may_be_true,
            may_be_false,
        }
    }

    /// `IsHighestCopotype(wanted)` or `IsHighestPolitical(wanted)`, over the amounts each state
    /// holds.
    ///
    /// ## The game's loop, run over sets of states
    ///
    /// The answer is `core::reputation::highest`, which is not a maximum: a tie clears the
    /// winner, the running best starts at zero, and a later higher amount wins it back. So it
    /// is computed the way the game computes it, one reputation at a time in enum order, with
    /// the loop's two variables - which reputation is ahead, and by what amount - carried as a
    /// map from each possible pair to the states in which the loop reaches it. Each
    /// reputation's step splits every entry by the amounts that reputation can hold. What is
    /// left under a winner of `wanted` is where the question holds.
    ///
    /// It stays small because the amounts do: an untracked reputation has one, a rebased one
    /// as many as the group's own raises plus one, and a wide slot at most what its bits hold.
    ///
    /// ## Why not compare the slots directly
    ///
    /// A comparator between two registers is exactly the arithmetic the rest of this compiler
    /// avoids, and the tie rule would need one per pair on top. Splitting by value costs a
    /// handful of conjunctions and states the rule once, in the order the game states it.
    fn highest_reputation(&mut self, name: &str, args: Arguments<'_>, rendered: String) -> MayBe {
        let range = crate::core::reputation::range_of(name).expect("the caller matched");
        let Some(wanted) = Self::text_argument(args) else {
            return self.undecided("reputation: the argument is not a literal name", rendered);
        };

        if let Some(winner) = self.settled_reputation.get(&range.start).copied() {
            self.reputation_from_world += 1;
            let holds = if winner == Some(wanted.as_str()) {
                self.top()
            } else {
                self.bottom()
            };
            return self.decided(holds);
        }

        // (index of the reputation ahead, the amount it is ahead by) -> where the loop is there.
        let mut ahead: HashMap<(Option<usize>, i64), BDDFunction> = HashMap::new();
        ahead.insert((None, 0), self.top());

        for index in range {
            let variable =
                crate::core::reputation::variable_of(crate::core::reputation::IN_ENUM_ORDER[index]);
            let amounts = match self.amounts_of(&variable) {
                Ok(amounts) => amounts,
                Err(Amounts::Unknown) => {
                    return self.undecided("reputation: world cannot say an amount", rendered);
                }
                Err(Amounts::NoRoom) => return self.no_room(rendered),
            };

            let mut next: HashMap<(Option<usize>, i64), BDDFunction> = HashMap::new();
            for (&(best, best_amount), reached) in &ahead {
                for (amount, holding) in &amounts {
                    let Ok(both) = reached.and(holding) else {
                        return self.no_room(rendered);
                    };
                    let step = if *amount == best_amount {
                        (None, best_amount)
                    } else if *amount > best_amount {
                        (Some(index), *amount)
                    } else {
                        (best, best_amount)
                    };
                    let joined = match next.remove(&step) {
                        Some(already) => already.or(&both),
                        None => Ok(both),
                    };
                    let Ok(joined) = joined else {
                        return self.no_room(rendered);
                    };
                    next.insert(step, joined);
                }
            }
            ahead = next;
        }

        let mut holds = self.bottom();
        for ((best, _), reached) in ahead {
            let wins = best.is_some_and(|index| {
                crate::core::reputation::IN_ENUM_ORDER[index] == wanted.as_str()
            });
            if !wins {
                continue;
            }
            let Ok(joined) = holds.or(&reached) else {
                return self.no_room(rendered);
            };
            holds = joined;
        }
        self.decided(holds)
    }

    /// Every amount a variable can read as, each with the states in which it does.
    ///
    /// READ THE WAY THE SEARCH READS IT, so the answer is the one `BoundContext::query` gives
    /// state by state:
    ///
    /// - a variable the layout carries no slot for is the world's number everywhere;
    /// - a slot held as a delta is the world's number plus the distance - the same reading
    ///   [`Self::rebased`] rewrites a comparison around;
    /// - any other slot is its own bits.
    ///
    /// A number the world cannot give makes the whole question unknown, as it does for the
    /// search: a zero invented in its place is a participant in the comparison and can clear
    /// a winner.
    fn amounts_of(&self, variable: &str) -> Result<Vec<(i64, BDDFunction)>, Amounts> {
        let slot = self
            .vars
            .slot_of(variable)
            .filter(|slot| self.vars.layout().slot(*slot).is_some());

        let Some(slot) = slot else {
            let world = self.world.ok_or(Amounts::Unknown)?;
            let declared = self
                .vars
                .symbols()
                .variable_ref(variable)
                .ok_or(Amounts::Unknown)?;
            let amount = world
                .get_variable(declared)
                .try_as_number()
                .ok_or(Amounts::Unknown)?;
            return Ok(vec![(amount as i32 as i64, self.top())]);
        };

        let top = self.vars.slot_ceiling(slot).ok_or(Amounts::NoRoom)?;
        let base = if self.vars.layout().is_delta(slot) {
            let world = self.world.ok_or(Amounts::Unknown)?;
            Some(self.rebase_start(world, variable))
        } else {
            None
        };

        let mut by_amount: HashMap<i64, BDDFunction> = HashMap::new();
        for held in 0..=top {
            let amount = match base {
                Some(base) => base + held as i64,
                None => held as i64,
            };
            let at = self.vars.slot_equals(slot, held).ok_or(Amounts::NoRoom)?;
            let joined = match by_amount.remove(&amount) {
                Some(already) => already.or(&at).map_err(|_| Amounts::NoRoom)?,
                None => at,
            };
            by_amount.insert(amount, joined);
        }
        Ok(by_amount.into_iter().collect())
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
    /// A reputation question is here because `ReputationGrows` writes the variables it
    /// compares, so the search really can change the answer; it is built from the amounts
    /// each state holds, by [`Self::highest_reputation`].
    fn search_can_change(name: &str) -> bool {
        matches!(name, MONEY_QUERY)
            || Self::slot_backed_query(name).is_some()
            || crate::world::flag_query(name).is_some()
            || crate::core::reputation::range_of(name).is_some()
            || name == crate::core::item_group::CHECK_ITEM_GROUP
            || crate::core::damage::skill_read_by(name).is_some()
            || crate::core::clock::ClockTime::owns(name)
    }

    /// The slot prefix a query is answered from, for the queries that have one.
    ///
    /// One list, read by both the compiler and `search_can_change`, because a query
    /// answered from a slot in one place and from the world in the other would give two
    /// different answers for the same state. `BoundContext::query` intercepts exactly
    /// these two.
    fn slot_backed_query(name: &str) -> Option<&'static str> {
        match name {
            "CheckItem" => Some(ITEM_PREFIX),
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

    /// The one value a query has at every state this compiler decides for, or `None` where it
    /// is not one value.
    ///
    /// Two kinds qualify. A query the search cannot change at all, asked of the world once. And
    /// a CLOCK query under [`Self::with_constant_clock`], answered by `ClockTime` at the world's
    /// time exactly as the conditions `IsNight()` and the like are - a number such as
    /// `TotalHourCount()` is the same approximation read as a value.
    fn fixed_value(&self, name: &str, args: Arguments<'_>) -> Option<GuardValue> {
        if ClockTime::owns(name) {
            // NOT ONE VALUE WHERE THE CLOCK MOVES. Every question `owns` covers is a question
            // about the hour, so where the layout carries a clock the answer differs between
            // states and there is no constant to hand back. `owns_day` is the other half -
            // the day cannot change within a conversation - and it goes through
            // `constant_value` below, which stays exact either way.
            if !self.clock_from_the_world() {
                return None;
            }
            let world = self.world?;
            let values = Self::literal_arguments(args)?;
            let answer = crate::core::clock::ClockTime::answer(
                name,
                &values,
                world.day_minutes(),
                world.day_counter(),
            );
            return (answer.kind() != GuardValueKind::Unknown).then_some(answer);
        }
        if Self::search_can_change(name) {
            return None;
        }
        // Nor what is worn, where the group can take a worn item away.
        if crate::core::equipment::reads_equipment(name)
            && !self.losable_worn_items(name, args).is_empty()
        {
            return None;
        }
        // The Kim questions are fixed only where the group never takes Kim out of the party.
        if crate::core::party::reads_kim_removal(name)
            && self
                .vars
                .slot_of(crate::core::party::KIM_REMOVED_SLOT)
                .is_some()
        {
            return None;
        }
        self.constant_value(name, args)
    }

    /// `Variable[name] op query()`, or the other way round, where the query is one value.
    ///
    /// The shape a stored deadline is read back in: `TotalHourCount() >=
    /// Variable["plaza.alice_serial_next_meeting_time"]`. With the query fixed it is an ordinary
    /// comparison of a slot against a constant.
    ///
    /// A CLOCK READING IS NOT ONE VALUE where the layout carries a clock, so such a guard is
    /// undecided there rather than answered hour by hour. Not reached: surveyed 2026-09-21
    /// over the whole index, no group that passes time holds a guard mentioning `HourCount`
    /// or `TotalHourCount` at all - every hour question in one is a condition like
    /// `IsMorning()` or `IsHourBetween(22, 6)`, which the arm above compiles over the
    /// register. Undecided is the permissive direction, so the gap is safe as well as empty.
    fn variable_against_query<'g>(
        &mut self,
        op: &str,
        left: GuardRef<'g>,
        right: GuardRef<'g>,
    ) -> Option<MayBe> {
        let (name, (query, args), op) = match (Self::variable_of(left), Self::query_of(right)) {
            (Some(name), Some(query)) => (name, query, op),
            _ => match (Self::variable_of(right), Self::query_of(left)) {
                (Some(name), Some(query)) => (name, query, Self::mirrored(op)),
                _ => return None,
            },
        };
        let value = self.fixed_value(&query, args)?;
        Some(self.comparison(op, &name, &value))
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
        } else if crate::core::scene::variable_read_by(name).is_some() {
            // The weather is a variable nothing in a group writes - read the way
            // `BoundContext::query` reads it.
            let vars: &'a DataVars<'a> = self.vars;
            crate::core::scene::weather_answer(name, |variable| {
                Some(world.get_variable(vars.symbols().variable_ref(variable)?))
            })
            .expect("just matched")
        } else if crate::core::substance::owns(name) {
            // A substance count is a variable nothing in a group writes, so the world's value
            // is the value at every state - read the way `BoundContext::query` reads it.
            let vars: &'a DataVars<'a> = self.vars;
            crate::core::substance::answer(name, &values, |variable| {
                Some(world.get_variable(vars.symbols().variable_ref(variable)?))
            })
            .expect("just matched")
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

        let actual = self.fixed_value(&name, args)?;
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
    /// `DataLayout::lay_out_counters` - and a single `+1` then needs one bit, which is not
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
        let world = crate::world::GameWorld::blank().with_day_counter(1);
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

    /// A flag is read off its own slot, and asking the other way round is the complement.
    ///
    /// `FlagSet(f)` and `FlagNotSet(f)` are `Variable[f]` written as calls, so both are
    /// decided against the slot rather than handed to the world. TRACKED ON PURPOSE: the
    /// fixture's counter argument is what gives the flag a slot at all, since the layout
    /// spends one only on what the group moves - and a flag nothing moves is constant
    /// anyway, which is the case this test is not about.
    #[test]
    fn a_flag_is_decided_against_its_slot_either_way_round() {
        let (graph, symbols) = fixture(&["f"], Some("f"));
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);
        let flag = || Guard::literal(GuardValue::from_text("f".to_string()));

        let set = compiler.compile(&call("FlagSet", vec![flag()]));
        let clear = compiler.compile(&call("FlagNotSet", vec![flag()]));

        // NEITHER WAS GIVEN UP ON. Left to fall through, both would have been asked of a
        // world that was never told about this flag - it is asked for as a variable - and
        // would have gone undecided, which is permissive.
        assert_eq!(compiler.fallbacks(), 0);
        assert!(set.may_be_true.satisfiable(), "the flag can be set");
        assert!(clear.may_be_true.satisfiable(), "and can be clear");

        // And they are opposites rather than merely both decided.
        let both = set
            .may_be_true
            .and(&clear.may_be_true)
            .expect("room to intersect two slot reads");
        assert!(
            !both.satisfiable(),
            "a flag cannot be set and not set at once"
        );
    }

    /// Written the other way round, the operator turns with the operands.
    #[test]
    fn a_constant_query_on_the_right_compares_the_same_way() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let world = crate::world::GameWorld::blank().with_day_counter(1);
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
        let world = crate::world::GameWorld::blank();
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
        let world = crate::world::GameWorld::blank();
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
        let world = crate::world::GameWorld::blank()
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
        let world = crate::world::GameWorld::blank().set_item("ledger", true);

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
        let world = crate::world::GameWorld::blank().set_item("shoes_faln", false);
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
        let world = crate::world::GameWorld::blank().set_thought("jamais_vu", false);
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
        let world = crate::world::GameWorld::blank()
            .set_thought("guillaume_le_million", true)
            .set_fixed(Vec::<&str>::new());

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
        let night = crate::world::GameWorld::blank().with_day_minutes(2 * 60);

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&night)
            .with_constant_clock(false);
        let compiled = compiler.compile(&Guard::call("IsNight".to_string(), vec![]));

        assert!(compiled.is_decided());
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// A clock NUMBER compared against a literal is decided at the world's time too.
    ///
    /// Day 2 at 10:00 is total hour 34 - so `TotalHourCount() >= 30` holds and `HourCount() > 12`
    /// does not.
    #[test]
    fn a_clock_number_is_compared_at_the_worlds_time() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let morning = crate::world::GameWorld::blank()
            .with_day_counter(2)
            .with_day_minutes(10 * 60);

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&morning)
            .with_constant_clock(false);

        let late = compiler.compile(&Guard::comparison(
            ">=".to_string(),
            call("TotalHourCount", vec![]),
            number(30.0),
        ));
        assert!(late.may_be_true.satisfiable() && !late.may_be_false.satisfiable());
        let afternoon = compiler.compile(&Guard::comparison(
            ">".to_string(),
            call("HourCount", vec![]),
            number(12.0),
        ));
        assert!(!afternoon.may_be_true.satisfiable());
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// And it is refused, not guessed, when nothing has been told to hold it constant.
    #[test]
    fn a_clock_question_is_undecided_without_the_approximation() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let night = crate::world::GameWorld::blank().with_day_minutes(2 * 60);

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(&night);
        let compiled = compiler.compile(&Guard::call("IsNight".to_string(), vec![]));

        assert!(!compiled.is_decided());
        assert_eq!(compiler.fallbacks(), 1);
    }

    /// A graph a walk can take `steps` `PassTime` calls over, one to an entry in a chain.
    ///
    /// The register counts steps, so a layout is only as wide as the walk is long - which
    /// means a test about a clock two hours from now has to give the walk the calls to get
    /// there.
    fn ticking(steps: i32) -> (LookAheadGraph, StateSymbols) {
        let mut symbols = StateSymbols::new();
        let nodes = (0..steps)
            .map(|entry| LookAheadNode {
                actions: crate::parser::action_parser::parse_actions("PassTime()", &mut symbols),
                links: vec![DialogueNodeId::new(1, entry + 1)],
                ..LookAheadNode::new(DialogueNodeId::new(1, entry))
            })
            .collect();
        let graph = LookAheadGraph::new(nodes, symbols).unwrap();
        let snapshot = graph.symbols().clone();
        (graph, snapshot)
    }

    /// The states a given number of `PassTime` steps along, for asking what a formula says.
    fn after_steps(vars: &DataVars<'_>, steps: u32) -> BDDFunction {
        vars.clock_ops()
            .expect("this layout carries a clock")
            .equals(steps)
            .expect("the manager has room to pin it")
    }

    /// Whether a formula holds once a walk has taken that many steps.
    fn holds_after(vars: &DataVars<'_>, formula: &BDDFunction, steps: u32) -> bool {
        formula
            .and(&after_steps(vars, steps))
            .expect("the manager has room")
            .satisfiable()
    }

    /// With the clock carried, a clock question is answered at the STATE's hour rather than
    /// at the world's - so one question decides both ways over the walk.
    ///
    /// A quarter to seven is night; one `PassTime` later it is seven, which is dawn. That
    /// single step is the whole difference between the two answers, and the world's own hour
    /// is the one that must NOT decide it.
    #[test]
    fn a_clock_question_is_answered_at_the_states_hour() {
        let (graph, symbols) = ticking(1);
        let layout = DataLayout::for_graph(&graph, 16, None, true);
        let night = crate::world::GameWorld::blank().with_day_minutes(6 * 60 + 45);

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&night)
            .with_constant_clock(true);
        let compiled = compiler.compile(&Guard::call("IsNight".to_string(), vec![]));

        assert!(compiled.is_decided());
        assert_eq!(compiler.fallbacks(), 0);
        assert!(holds_after(&vars, &compiled.may_be_true, 0));
        assert!(!holds_after(&vars, &compiled.may_be_true, 1));
        assert!(holds_after(&vars, &compiled.may_be_false, 1));
    }

    /// A clock NUMBER against a literal is a question about which step counts satisfy it.
    ///
    /// On day 2 the total hour is 24 plus the hour of the day, so `TotalHourCount() >= 30` is
    /// "the hour is 6 or later". From a quarter to six that is one `PassTime` away, and the
    /// boundary is what a stale reading gets wrong.
    #[test]
    fn a_clock_number_is_compared_over_the_carried_clock() {
        let (graph, symbols) = ticking(1);
        let layout = DataLayout::for_graph(&graph, 16, None, true);
        let day_two = crate::world::GameWorld::blank()
            .with_day_counter(2)
            .with_day_minutes(5 * 60 + 45);

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&day_two)
            .with_constant_clock(true);
        let late = compiler.compile(&Guard::comparison(
            ">=".to_string(),
            call("TotalHourCount", vec![]),
            number(30.0),
        ));

        assert!(late.is_decided());
        assert_eq!(compiler.fallbacks(), 0);
        assert!(!holds_after(&vars, &late.may_be_true, 0));
        assert!(holds_after(&vars, &late.may_be_true, 1));
    }

    /// An hour no clock can be at is answered rather than asked of the register.
    #[test]
    fn an_hour_outside_the_day_settles_without_a_comparison() {
        let (graph, symbols) = ticking(4);
        let layout = DataLayout::for_graph(&graph, 16, None, true);
        let world = crate::world::GameWorld::blank();

        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(true);

        let never = compiler.compile(&Guard::comparison(
            ">=".to_string(),
            call("HourCount", vec![]),
            number(30.0),
        ));
        assert!(!never.may_be_true.satisfiable());

        let always = compiler.compile(&Guard::comparison(
            "<".to_string(),
            call("HourCount", vec![]),
            number(30.0),
        ));
        assert!(!always.may_be_false.satisfiable());
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// THE DEADLINE SHAPE gives way rather than answering wrongly, where the clock moves.
    ///
    /// `TotalHourCount() >= Variable["..."]` has both sides deciding per state once the
    /// clock is carried, and nothing here compares two registers. So it is undecided, which
    /// is the permissive direction - and it is a gap no shipped guard falls into: see
    /// [`GuardCompiler::variable_against_query`]. The same guard with the clock HELD is
    /// answered, which is the other half of the contract and the line below it.
    #[test]
    fn a_deadline_gives_way_to_a_moving_clock_and_is_answered_by_a_held_one() {
        let (graph, symbols) = fixture(&["plaza.alice_serial_next_meeting_time"], None);
        let day_two = crate::world::GameWorld::blank().with_day_counter(2);
        let deadline = Guard::comparison(
            ">=".to_string(),
            call("TotalHourCount", vec![]),
            Guard::variable("plaza.alice_serial_next_meeting_time".to_string()),
        );

        let carried = DataLayout::for_graph(&graph, 16, None, true);
        let vars = DataVars::new(&carried, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&day_two)
            .with_constant_clock(true);
        assert!(!compiler.compile(&deadline).is_decided());
        assert_eq!(compiler.fallbacks(), 1);

        let held = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&held, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&day_two)
            .with_constant_clock(true);
        assert!(compiler.compile(&deadline).is_decided());
        assert_eq!(compiler.fallbacks(), 0);
    }

    /// The approximation is only an approximation where the group can move the clock.
    #[test]
    fn holding_the_clock_is_exact_unless_the_group_passes_time() {
        let (graph, symbols) = fixture(&["a"], None);
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let world = crate::world::GameWorld::blank();

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

        // AND NOT AN APPROXIMATION AT ALL where the layout carries a clock, however the
        // group behaves: the questions are answered over the register, not at a fixed hour.
        let carried = DataLayout::for_graph(&graph, 16, None, true);
        let vars = DataVars::new(&carried, &symbols, DiagramBudget::modest());
        let exact = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(true);
        assert!(!exact.clock_is_approximated());
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
