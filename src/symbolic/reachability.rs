// SPDX-License-Identifier: MIT
//! Entering one node, over state SETS rather than one state at a time.
//!
//! The step every search over a group is built out of, and the seed it starts from. A
//! search that enumerates states holds every `(entry, state)` pair it has visited, and on
//! the shapes that matter there are hundreds of thousands of them; here the states carried
//! into and out of an entry are one decision diagram, and the entry itself stays an
//! ordinary value.
//!
//! ## Explicit control, symbolic data
//!
//! The entry a search sits on stays an ordinary value - there are a few thousand of them
//! and they are enumerated anyway - while everything carried WITH it is symbolic. So a step
//! is a formula about data alone, which is what this module computes.
//!
//! That is not the textbook encoding, which would build a transition relation over primed
//! and unprimed copies of every variable and take the relational product. It is not
//! needed here: a dialogue action assigns a slot or increments it, and both are FUNCTIONS
//! of the state rather than relations, so [`ActionImage`] computes the image directly.
//! The variable count does not double and the question of how to interleave primed with
//! unprimed variables never arises.
//!
//! ## What it is allowed to get wrong
//!
//! The same one-directional approximation as the guard compiler, and for the same reason:
//! `may_be_true` lets an undecided guard through, so a set here is an OVER-approximation
//! of what is truly reachable. It may include a data state no path can produce; it may
//! never miss one. Anything else would make the answer useless, because a missed state is
//! a missed marker.
//!
//! Money is the exception worth naming: a layout built without a balance cannot refuse a
//! price at all, and lets every one of them through - see [`Reachability::affordable`].
//!
//! ## Where the walking happens
//!
//! Not here. Reaching a target is asked backwards, from the target towards the start, in
//! [`crate::symbolic::backward`] - which mirrors every case below and must go on doing so.

use oxidd::BooleanFunction;
use oxidd::bdd::BDDFunction;

use crate::core::types::{DialogueCheckKind, DialogueNodeId, StartBranch, Ternary};
use crate::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::symbolic::action_image::ActionImage;
use crate::symbolic::guard_formula::GuardCompiler;
use crate::symbolic::vars::DataVars;
use crate::world::ILookAheadWorld;

/// Whether the world has already decided this node's line can never be shown.
///
/// A passive check is a comparison rather than a roll, so a character sheet that fails one
/// fails it for the whole search: the state walks through the node UNCHARGED and the line
/// is never displayed. Both directions model that pass-through already, and for a node on
/// the way past it is all that is needed.
///
/// This is the question to ask where the node is the thing being MEASURED rather than a
/// step towards something else, because that is where entered and displayed stop meaning
/// the same. A search hunting an unread line, and a baseline naming where an outcome
/// lands, are both about what the player would read.
pub fn never_displays(node: &LookAheadNode, world: &dyn ILookAheadWorld) -> bool {
    node.kind == DialogueCheckKind::Passive
        && crate::world::passive_outcome(node, world) == Ternary::False
}

/// The single data state a search starts in, as a set of one.
///
/// Mirrors nothing: it ENCODES [`crate::core::state::seed_state`]'s answer, so every search
/// cannot disagree about where they begin. Seeding is not a detail - a symbolic run
/// started from every data state walks paths that need an item the player has not got,
/// and reports entries no real path reaches.
///
/// A slot the layout is too narrow for is clamped to what it can hold rather than
/// dropped. That can only happen where the world reports a value larger than any action
/// in the group writes, and the alternative - an empty seed - would report nothing
/// reachable at all, which is the failure that looks like success.
///
/// ## `None` WHERE THE MANAGER FILLED, because there is no honest fallback
///
/// A seed is diagram work like any other and the player's budget is small enough for a
/// wide purse to reach it: squeezing a twenty-bit purse into eight kilobytes fills the
/// manager here, before the search has started. Neither shape of failure can be returned
/// in a set. The empty seed reports nothing reachable at all, which the comment above
/// calls the failure that looks like success; dropping the constraint that would not
/// build leaves the register free, which starts the search rich AND poor at once and
/// undoes the whole of `affordable`. So a seed that cannot be built is not a seed, and
/// this says so rather than aborting the process on an unwrapped operation.
///
/// The slot loop asks [`DataVars::slot_ceiling`] first, which answers out of the layout
/// without touching the manager - so a slot that gets past it and then yields no formula
/// yielded none for want of room, and there is no third case to confuse it with.
pub fn seed_of(
    graph: &LookAheadGraph,
    world: &dyn ILookAheadWorld,
    vars: &DataVars<'_>,
) -> Option<BDDFunction> {
    let state = crate::core::state::seed_state(graph, world);
    let mut set = vars.top();

    for slot in 0..vars.layout().slot_count() {
        let Some(ceiling) = vars.slot_ceiling(slot) else {
            continue;
        };
        // A REBASED SLOT STARTS AT NOTHING, because it holds the distance the search has
        // travelled rather than where it started - see `DataLayout::lay_out_counters`. The
        // save's value is not lost; it moves into the guards, which are rebased by it.
        let value = if vars.layout().is_delta(slot) {
            0
        } else {
            state.get(slot).max(0) as u32
        };
        let holds = vars.slot_equals(slot, value.min(ceiling))?;
        set = set.and(&holds).ok()?;
    }

    // AND THE PURSE, where the layout carries one. A seed that left money free would start
    // the search rich AND poor at once, which is the over-approximation that makes every
    // price affordable down some path and undoes the whole of `affordable` - see de-95t6.
    // Clamped like a slot: `DataLayout::money_ceiling` is built to be wide enough, and an
    // empty seed would report nothing reachable at all.
    if let Some(money) = vars.money_ops() {
        let ceiling = money.register().ceiling();
        let holds = money.equals((state.money().max(0) as u32).min(ceiling))?;
        set = set.and(&holds).ok()?;
    }

    Some(set)
}

/// Entering one node: its guard tested, its roll taken, its price paid, its actions applied.
///
/// A HOLDER RATHER THAN A SEARCH. Every step of a walk over a group is this one operation,
/// and the walking itself is the backward pass's business - see [`crate::symbolic::backward`].
/// What lives here is the step, and the one thing a caller has to be told about it, which is
/// whether the manager filled part way through.
pub struct Reachability<'a> {
    vars: &'a DataVars<'a>,
    /// Whether the manager ran out of nodes part way through entering.
    ///
    /// The set that comes back when this is set is a fragment rather than an answer, which
    /// is why [`Self::entry_states`] returns nothing at all rather than handing it over.
    out_of_memory: bool,
}

impl<'a> Reachability<'a> {
    /// What entering `start` by one outcome leaves, without exploring anything.
    ///
    /// THE STATE THE OUTCOME HANDS ON, which is what a search about that outcome is really
    /// seeded with: its links are walked from here, and everything past them knows nothing
    /// about the roll except what this carries.
    ///
    /// The backward driver is what wants it. Its sets say "arriving HERE, the target is
    /// reachable", and a check's set unions both ways in - so asking it about the check
    /// answers about either roll. Asked instead about the check's children, with this, it
    /// answers about one. See `seen_state_search::best_novelty`.
    ///
    /// `None` WHERE THE MANAGER FILLED, and the distinction is the whole point of
    /// returning an option. Entering the start is diagram work like any other and can run
    /// out of nodes; the empty set it then holds is indistinguishable from "this outcome
    /// opens nothing", and a caller that read it as the latter would refuse every
    /// candidate on no evidence and report a settled verdict. There is no stats channel
    /// here to say it in, so the return type says it.
    pub fn entry_states(
        graph: &LookAheadGraph,
        start: DialogueNodeId,
        branch: StartBranch,
        seed: &BDDFunction,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
    ) -> Option<BDDFunction> {
        let vars = compiler.vars();
        let mut image = ActionImage::for_world(vars, counter_cap, world);
        let mut this = Self {
            vars,
            out_of_memory: false,
        };

        let entered = match graph.get(start) {
            Some(node) => this.enter_branch(node, branch, seed, compiler, world, &mut image),
            None => vars.bottom(),
        };

        match this.out_of_memory || image.out_of_memory() {
            true => None,
            false => Some(entered),
        }
    }

    /// The empty set is returned only so the types stay simple. It is not a meaningful
    /// answer, and a caller that sees [`Self::out_of_memory`] must stop
    /// rather than read what came back.
    fn or_no_room<E>(&mut self, attempt: Result<BDDFunction, E>) -> BDDFunction {
        match attempt {
            Ok(function) => function,
            Err(_) => {
                self.out_of_memory = true;
                self.vars.bottom()
            }
        }
    }

    /// The data states that entering `node` from `states` can leave the search in.
    ///
    /// The one place a node is entered, which is the requirement rather than a nicety: a
    /// symbolic search that disagrees with the explicit one is measuring a different
    /// question. Guard first, then affordability, then the node's kind.
    /// Entering a node, keeping only one outcome where it rolls.
    ///
    /// ONLY THE START IS ENTERED THIS WAY. Every node the search walks ON to is entered
    /// both ways, because a check met in the middle of a path can be passed or failed and
    /// the search is asking what is reachable, not what one roll does. The branch is a fact
    /// about the QUESTION - which half of the option the mod is drawing - and so belongs to
    /// the one node the question is about.
    fn enter_branch(
        &mut self,
        node: &LookAheadNode,
        branch: StartBranch,
        states: &BDDFunction,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        if branch == StartBranch::Either {
            return self.enter(node, states, compiler, world, image);
        }

        let rolls = matches!(node.kind, DialogueCheckKind::Red | DialogueCheckKind::White);
        if !rolls {
            // A start that does not roll has one way in, and every branch names it - except
            // `Fail`, which names a failure that does not exist. That is the definition
            // that keeps a failing outcome from quietly exploring the passing one.
            return match branch {
                StartBranch::Fail => self.vars.bottom(),
                _ => self.enter(node, states, compiler, world, image),
            };
        }

        let allowed = {
            let (may_be_true, _) = self.guard_of(node, compiler);
            self.or_no_room(states.and(&may_be_true))
        };
        if !allowed.satisfiable() {
            return self.vars.bottom();
        }

        let allowed = self.affordable(node, &allowed);
        if !allowed.satisfiable() {
            return self.vars.bottom();
        }

        let may_succeed = crate::world::roll_may_succeed(node, world);
        let (success, failure) = self.rolled_cases(node, may_succeed, &allowed, image);
        match branch {
            StartBranch::Pass => success,
            StartBranch::Fail => failure,
            StartBranch::Either => self.or_no_room(success.or(&failure)),
        }
    }

    fn enter(
        &mut self,
        node: &LookAheadNode,
        states: &BDDFunction,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        let allowed = {
            let (may_be_true, _) = self.guard_of(node, compiler);
            self.or_no_room(states.and(&may_be_true))
        };
        if !allowed.satisfiable() {
            return self.vars.bottom();
        }

        let allowed = self.affordable(node, &allowed);
        if !allowed.satisfiable() {
            return self.vars.bottom();
        }

        match node.kind {
            // A hidden test is entered and goes nowhere: the engine produces no successor
            // state for one at all.
            DialogueCheckKind::Test => self.vars.bottom(),

            // Closes once seen, so only states that have not seen it may enter.
            DialogueCheckKind::Fake => {
                let fresh = self.unseen(node, &allowed);
                let entered = self.charge(node, &fresh, image);
                self.fail(node, &entered, image)
            }

            DialogueCheckKind::KimSwitch => {
                let fresh = if node.boolean_only {
                    allowed
                } else {
                    self.unseen(node, &allowed)
                };
                self.charge(node, &fresh, image)
            }

            DialogueCheckKind::Red | DialogueCheckKind::White => {
                let may_succeed = crate::world::roll_may_succeed(node, world);
                self.rolled(node, may_succeed, &allowed, image)
            }

            DialogueCheckKind::Passive => {
                let passes = crate::world::passive_outcome(node, world);
                let mut result = self.vars.bottom();
                if passes != Ternary::False {
                    result = self.charge(node, &allowed, image);
                }
                // The failing branch passes the incoming state through UNCHARGED - the one
                // branch in the engine that does not go through `charge`, so it must not
                // go through it here either.
                if passes != Ternary::True {
                    result = self.or_no_room(result.or(&allowed));
                }

                result
            }

            _ => self.charge(node, &allowed, image),
        }
    }

    /// A rolled check's two branches, which are the states after passing and after
    /// failing.
    fn rolled(
        &mut self,
        node: &LookAheadNode,
        may_succeed: bool,
        states: &BDDFunction,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        let (success, failure) = self.rolled_cases(node, may_succeed, states, image);
        self.or_no_room(success.or(&failure))
    }

    /// The two ways a roll can go, kept apart.
    ///
    /// ONE PLACE BUILDS BOTH, and [`Self::rolled`] unions them. A search about one outcome
    /// takes one of them instead - see [`Self::enter_branch`] - and taking it here rather
    /// than re-deriving the roll elsewhere is what keeps the two from drifting: a rule
    /// added to the failing case reaches both callers at once.
    fn rolled_cases(
        &mut self,
        node: &LookAheadNode,
        may_succeed: bool,
        states: &BDDFunction,
        image: &mut ActionImage<'a>,
    ) -> (BDDFunction, BDDFunction) {
        // A check already passed is closed, and one already failed is closed too - neither
        // kind can be retried once the roll has been recorded.
        let mut open = states.clone();
        if let Some(passed) = self.flag(node.flag_slot) {
            let unpassed = self.or_no_room(passed.not());
            open = self.or_no_room(open.and(&unpassed));
        }
        if let Some(failed) = self.flag(node.failed_flag_slot) {
            let unfailed = self.or_no_room(failed.not());
            open = self.or_no_room(open.and(&unfailed));
        }

        if !open.satisfiable() {
            return (self.vars.bottom(), self.vars.bottom());
        }

        let entered = self.charge(node, &open, image);

        // Success raises the pass flag - where the roll may succeed at all; see
        // `world::roll_may_succeed`.
        let success = if may_succeed {
            match node.flag_slot {
                slot if slot >= 0 => image.assign(&entered, slot as usize, 1),
                _ => entered.clone(),
            }
        } else {
            self.vars.bottom()
        };

        // Failure: both kinds record it where there is a flag to record it with, and a
        // white check without one leaves the state alone, so only that case is retryable.
        let failure = if node.failed_flag_slot >= 0 {
            let failed = image.assign(&entered, node.failed_flag_slot as usize, 1);
            self.fail(node, &failed, image)
        } else if node.kind == DialogueCheckKind::White {
            self.fail(node, &entered, image)
        } else {
            self.vars.bottom()
        };

        (success, failure)
    }

    /// A check's failing branch beyond the failure flag - see `LookAheadNode::failure_actions`,
    /// and `oracle::fail`, which this mirrors.
    fn fail(
        &mut self,
        node: &LookAheadNode,
        states: &BDDFunction,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        image.apply(states, &node.failure_actions, &self.vars.bottom())
    }

    /// Paying the cost, marking the entry seen, and applying its actions.
    fn charge(
        &mut self,
        node: &LookAheadNode,
        states: &BDDFunction,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        if !states.satisfiable() {
            return self.vars.bottom();
        }

        let mut current = states.clone();

        if node.is_cost_option() {
            current = self.pay(node, &current, image);
        }

        if node.seen_slot >= 0 {
            current = image.assign(&current, node.seen_slot as usize, 1);
        }

        // The once slot is what says whether a one-time action has already fired.
        let already = match self.flag(node.once_slot) {
            Some(flag) => flag,
            None => self.vars.bottom(),
        };

        // A one-time action has to RECORD that it fired, or it is not one.
        //
        // `apply` splits on `already` and leaves spent states alone, which is only half of
        // it: nothing was raising the flag, so no state was ever spent, and a once
        // increment inside a loop climbed to the counter cap. The explicit search does
        // raise it - `DialogueAction::apply` pushes the once slot when something fired -
        // and the two have to agree.
        //
        // Split here rather than inside `apply`, because the flag is raised once per
        // ENTRY rather than once per action: an entry with three once actions fires all
        // three together the first time and none of them afterwards.
        let fires_once = node.actions.iter().any(|action| action.is_once());
        if !fires_once || node.once_slot < 0 {
            return image.apply(&current, &node.actions, &already);
        }

        let unfired = self.or_no_room(already.not());
        let fresh = self.or_no_room(current.and(&unfired));
        let spent = self.or_no_room(current.and(&already));

        // Fresh: nothing has fired, so the one-time actions apply - and the flag goes up
        // afterwards, on the states that just used them.
        let mut result = self.vars.bottom();
        if fresh.satisfiable() {
            let acted = image.apply(&fresh, &node.actions, &self.vars.bottom());
            result = image.assign(&acted, node.once_slot as usize, 1);
        }

        // Spent: everything has fired already, so the one-time actions are skipped and
        // the rest still apply. Passing the everywhere-true set says exactly that.
        if spent.satisfiable() {
            let acted = image.apply(&spent, &node.actions, &self.vars.top());
            result = self.or_no_room(result.or(&acted));
        }

        result
    }

    /// The states in which this node has not been seen.
    fn unseen(&mut self, node: &LookAheadNode, states: &BDDFunction) -> BDDFunction {
        match self.flag(node.seen_slot) {
            Some(seen) => {
                let not_seen = self.or_no_room(seen.not());
                self.or_no_room(states.and(&not_seen))
            }
            None => states.clone(),
        }
    }

    /// Paying this entry's price, and recording the payment where it is made only once.
    ///
    /// The states that have already paid keep their money and are left exactly as they
    /// are; the rest hand over the price and come out with the slot raised. Splitting is
    /// what makes the middle fixture of the money suite work: a search that checked a
    /// price without subtracting what the path had already spent would mark an option
    /// leading somewhere the player can no longer afford.
    ///
    /// Nothing here can go below zero, because [`Self::affordable`] has already removed
    /// the states that could not pay.
    fn pay(
        &mut self,
        node: &LookAheadNode,
        states: &BDDFunction,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        let price = node.cost.max(0) as u32;

        // Payable every time, or with nowhere to record having paid: everyone pays.
        let Some(paid) = self.already_paid(node) else {
            return self.spend(states, price);
        };

        let spent = self.or_no_room(states.and(&paid));
        let unpaid = self.or_no_room(paid.not());
        let fresh = self.or_no_room(states.and(&unpaid));
        let fresh = self.spend(&fresh, price);
        let fresh = image.assign(&fresh, node.once_slot as usize, 1);

        self.or_no_room(spent.or(&fresh))
    }

    /// `money := money - amount`, where the layout carries money and otherwise nothing.
    fn spend(&mut self, states: &BDDFunction, amount: u32) -> BDDFunction {
        match self.vars.money_ops() {
            // Subtracting says "no room" with `None` where the diagram operations say it
            // with `Err`, and the two mean the same thing: the manager filled part way
            // through, so what came back is not the purse after paying.
            Some(money) => self.or_no_room(money.saturating_sub(states, amount).ok_or(())),
            None => states.clone(),
        }
    }

    /// Which states can afford this node.
    ///
    /// The states holding at least the price, where the layout carries money - and every
    /// state, counted as undecided, where it does not. A layout without money cannot refuse
    /// anything, and the permissive answer is the only safe one there: refusing would prune
    /// a branch a richer path opens, and this may only over-approximate.
    ///
    /// A COST CHARGED ONCE IS STILL PRICED THE SECOND TIME. Entering it again takes nothing from
    /// the purse (`CostOptionNode.HandleEntry` skips the charge for a seen once-cost entry), but
    /// the option is disabled whenever the price is above the purse
    /// (`CostOptionNode.HandleResponseText`, with no exception for having paid) - so a path back
    /// through a shop door it has paid at still needs the price in hand. The same in the
    /// pre-final-cut export and Final Cut's ISIL dump.
    fn affordable(&mut self, node: &LookAheadNode, states: &BDDFunction) -> BDDFunction {
        if !node.is_cost_option() {
            return states.clone();
        }

        let Some(money) = self.vars.money_ops() else {
            return states.clone();
        };

        // The price formula is diagram work and can be the operation that fills the
        // manager, which is a different thing from a layout that carries no money: that
        // one is undecided and permissive, this one is no answer at all.
        let Some(price) = money.at_least(node.cost.max(0) as u32) else {
            self.out_of_memory = true;
            return self.vars.bottom();
        };
        self.or_no_room(states.and(&price))
    }

    /// "This entry's price has already been paid", where it is one that is paid once.
    ///
    /// `None` where the question does not arise - an unpriced entry, a price payable every
    /// time, or one with no slot to remember the payment in - so a caller can tell "no
    /// state has paid" from "there is nothing to have paid".
    fn already_paid(&mut self, node: &LookAheadNode) -> Option<BDDFunction> {
        if !node.is_cost_option() || !node.cost_once {
            return None;
        }
        self.flag(node.once_slot)
    }

    /// A slot's "is set" formula, for a slot number that may be -1 for "no slot".
    ///
    /// `None` FOR TWO REASONS THAT WANT OPPOSITE THINGS, so the second is recorded. A slot
    /// the layout does not carry constrains nothing, and every caller here is right to
    /// carry on without it. A manager with no room to build the formula also constrains
    /// nothing, and carrying on then is how a search comes to widen a set it never
    /// narrowed - so the stats say so and the loop stops on it.
    ///
    /// The two are told apart by asking the layout first, which touches no diagram.
    fn flag(&mut self, slot: i32) -> Option<BDDFunction> {
        let slot = usize::try_from(slot).ok()?;
        self.vars.slot_ceiling(slot)?;
        let formula = self.vars.slot_is_set(slot);
        if formula.is_none() {
            self.out_of_memory = true;
        }

        formula
    }

    /// This node's compiled guard, compiled once and remembered.
    ///
    /// THE COMPILER REMEMBERS IT, not this search. It used to be a map here, which meant
    /// the cache died with the run - so a second question about the same group recompiled
    /// every guard, and the backward search, which had no such map, recompiled on every
    /// visit within one run. See `GuardCompiler::compile_for`.
    fn guard_of(
        &mut self,
        node: &LookAheadNode,
        compiler: &mut GuardCompiler<'a>,
    ) -> (BDDFunction, BDDFunction) {
        let compiled = compiler.compile_for(node.id, &node.guard);
        (compiled.may_be_true, compiled.may_be_false)
    }
}

#[cfg(test)]
mod branch_tests {
    use super::*;
    use crate::core::guard_value::GuardValue;
    use crate::symbolic::backward::{Backward, SettledPass};
    use crate::symbolic::budget::DiagramBudget;
    use crate::symbolic::data_layout::DataLayout;
    use crate::symbolic::vars::DataVars;
    use crate::test_graph::{Entry, GraphBuilder, node};
    use crate::world::test_world::TestWorld;

    const CAP: i32 = 16;

    /// 0 is a white check. Passing opens 1 and 2 beyond it; failing opens 3.
    fn check() -> LookAheadGraph {
        GraphBuilder::new()
            .add(
                Entry::new(0)
                    .kind(DialogueCheckKind::White)
                    .flag("roll")
                    .links(&[1, 3]),
            )
            .add(
                Entry::new(1)
                    .guard(r#"Variable["roll"] == true"#)
                    .links(&[2]),
            )
            .add(Entry::new(2))
            .add(Entry::new(3).guard(r#"Variable["roll"] == false"#))
            .build()
    }

    /// Which entries a search reaches, entering the start by the given outcome.
    ///
    /// THROUGH THE ENTRY STEP AND THEN A BACKWARD PASS, which is how the driver asks. What
    /// is under test is [`Reachability::entry_states`]: the outcome selects which of the
    /// two cases entering the check leaves, and everything past the start is a question
    /// about what that state can go on to reach. One pass per entry is nothing on a graph
    /// of four.
    fn reached(graph: &LookAheadGraph, branch: StartBranch) -> Vec<i32> {
        let world =
            TestWorld::declaring_nothing().set_variable("roll", GuardValue::from_boolean(false));
        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        let seed = seed_of(graph, &world, &vars).expect("room for a seed");

        let start = node(0);
        let entered = Reachability::entry_states(
            graph,
            start,
            branch,
            &seed,
            &mut compiler,
            &world,
            CAP as u32,
        )
        .expect("room to enter the start");

        // An outcome the start has not got leaves nothing, and nothing goes on from
        // nothing - the start included, which is why this returns before counting it.
        if !entered.satisfiable() {
            return Vec::new();
        }

        let children: Vec<DialogueNodeId> = graph
            .get(start)
            .map(|node| node.links.clone())
            .unwrap_or_default();

        let mut entries: Vec<i32> = vec![start.entry_id];
        for id in graph.nodes().map(|node| node.id) {
            if id == start {
                continue;
            }
            let backward = Backward::reaching(graph, id, &mut compiler, &world, CAP as u32);
            if children
                .iter()
                .any(|child| backward.reachable_from(*child, &entered))
            {
                entries.push(id.entry_id);
            }
        }
        entries.sort();
        entries
    }

    /// The passing outcome walks the passing half, and only that half.
    #[test]
    fn passing_reaches_what_the_pass_flag_opens() {
        assert_eq!(reached(&check(), StartBranch::Pass), vec![0, 1, 2]);
    }

    /// And the failing outcome the other, which is the whole point of asking twice.
    #[test]
    fn failing_reaches_what_the_pass_flag_shuts() {
        assert_eq!(reached(&check(), StartBranch::Fail), vec![0, 3]);
    }

    /// An ordinary search takes both, which is what it did before there were branches.
    #[test]
    fn either_reaches_both_halves() {
        assert_eq!(reached(&check(), StartBranch::Either), vec![0, 1, 2, 3]);
    }

    /// How much room the squeezed search below gets, in bytes.
    ///
    /// A WINDOW RATHER THAN A CEILING, and it is narrow: wide enough to lay the seed out,
    /// since a run that cannot build one dies in `seed_of` and tests nothing here, and
    /// narrow enough that pricing the purse does not fit. Measured at 8 KB the seed itself
    /// cannot be built and at 64 KB the whole search completes.
    ///
    /// So a change to the layout, the register encoding or the manager can move this out
    /// from under the test, and the symptom is either a panic in `seed_of` or an assertion
    /// that the nodes did not run out. Re-tune it to the new window; the test is about
    /// what a full manager DOES, not about this number.
    const SQUEEZED: usize = 16 * 1024;

    /// Half of [`SQUEEZED`], which is not enough to lay the seed out at all.
    ///
    /// The other side of the same window, and the reason [`SQUEEZED`] is a window: at this
    /// budget the manager fills while the purse's equality is being built, before the
    /// search has a starting point to explore from. Re-tune it with [`SQUEEZED`] if a
    /// layout change moves the boundary; the two are one measurement read at both ends.
    const TOO_SQUEEZED: usize = SQUEEZED / 2;

    /// A purse wide enough that arithmetic over it does not fit in [`SQUEEZED`].
    ///
    /// THE ROOM IS TAKEN BY A REGISTER RATHER THAN BY A BIG GRAPH, because the two cost
    /// differently to write down: a graph whose sets genuinely explode is a real
    /// conversation, and a twenty-bit purse is one number. Affording a price and paying it
    /// are a comparison and a shift across every bit of it, which is diagram work in
    /// exactly the place this test is about.
    const DEEP_PURSE: u32 = 1_000_000;

    /// A manager that fills part way through entering a node REPORTS, rather than taking
    /// the process with it.
    ///
    /// Entering opens with the guard conjunction, and everything under it is diagram work
    /// too - the roll's two cases, the price, the seen flag, the actions - all of it
    /// reached once per link per step. Unwrapped, running out of nodes there ABORTS: not a
    /// panic a host can turn into a partial answer, and not a row a measurement can keep.
    /// Reported, the search says what it reached and that it did not settle. de-rvxw.
    #[test]
    fn a_manager_that_fills_while_entering_is_reported_rather_than_fatal() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).cost(7).cost_once().links(&[1]))
            .add(Entry::new(1).cost(11).links(&[2]))
            .add(Entry::new(2))
            .build();

        let world = TestWorld::declaring_nothing().with_money(DEEP_PURSE as i32 / 2);
        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(&graph, CAP, Some(DEEP_PURSE), false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::new(SQUEEZED));
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        let seed = seed_of(&graph, &world, &vars).expect("room for a seed");

        // THE PRICE IS ON THE START, so that the manager fills inside the one step this
        // module still takes. Pricing an entry further on would fill it inside the pass
        // that walks there instead, which is a different module's report to make.
        assert!(
            Reachability::entry_states(
                &graph,
                node(0),
                StartBranch::Either,
                &seed,
                &mut compiler,
                &world,
                CAP as u32,
            )
            .is_none(),
            "a step taken on a manager that filled is a fragment, and must not be handed \
             back as the states the start leaves",
        );
    }

    /// A manager too full to lay the SEED out says so, rather than taking the process.
    ///
    /// One layer below the search, and on a path with no stats to report through: a seed
    /// is a slot equality per slot conjoined with the purse, and over a wide register that
    /// is the arithmetic the module exists for. Unwrapped, it aborts before the search
    /// starts, which is the failure a player meets as a mod that vanished.
    ///
    /// The empty set is not an answer here and neither is a seed with the purse left out -
    /// the first reports nothing reachable at all and the second starts the search rich
    /// and poor at once - so the option is the whole of the fix. de-nyv2.
    #[test]
    fn a_manager_too_full_for_the_seed_says_so_rather_than_aborting() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).cost(7).cost_once().links(&[2]))
            .add(Entry::new(2).cost(11).links(&[3]))
            .add(Entry::new(3))
            .build();

        let world = TestWorld::declaring_nothing().with_money(DEEP_PURSE as i32 / 2);
        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(&graph, CAP, Some(DEEP_PURSE), false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::new(TOO_SQUEEZED));

        assert!(
            seed_of(&graph, &world, &vars).is_none(),
            "a seed the manager has no room for is no seed, and must not be a set",
        );
    }

    /// A start that does not roll has one way in, and `Fail` names a failure it has not got.
    #[test]
    fn a_start_that_does_not_roll_has_no_failing_outcome() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1))
            .build();

        assert_eq!(reached(&graph, StartBranch::Pass), vec![0, 1]);
        assert!(
            reached(&graph, StartBranch::Fail).is_empty(),
            "a failure that does not exist reaches nothing, rather than passing twice",
        );
    }
}
