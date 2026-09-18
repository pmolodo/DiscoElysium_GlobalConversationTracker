// SPDX-License-Identifier: MIT
//! Reachability asked backwards: from which states does ONE entry become reachable?
//!
//! The obvious way to answer a look-ahead is to work out everything a start can reach and
//! read the answer off it. The question actually asked is smaller - `LookAheadResult::best`
//! is a maximum over the novelty of entries a start can reach, and the plugin displays that
//! and nothing else - so the whole reachable set is a means, and an expensive one.
//!
//! Run the fixed point the other way and the question becomes: for each entry, from which
//! data states does the target become reachable onward? Compute that once and every option
//! in the group is answered by testing its seed against the result.
//!
//! ## Why backwards is cheap here in particular
//!
//! Two properties of this database, neither of which holds of transition systems in
//! general.
//!
//! IT PRUNES ITSELF. A variable enters a backward formula only if a guard on some path to
//! the target mentions it. Better than that, a WRITE ERASES its slot:
//! [`ActionImage::pre_assign`] selects and then quantifies away, so a slot assigned on the
//! way to the target and not read again leaves nothing behind. Forward, the same
//! assignment constrains that slot in every set downstream. So the cone-of-influence
//! reduction that a search going the other way needs a separate analysis for is just what the
//! pre-image does.
//!
//! THE PRE-IMAGE IS A COFACTOR. Every action assigns or increments a CONSTANT rather than
//! a function of another slot, so going backwards needs no relation over primed and
//! unprimed variables, exactly as going forwards needs none. See
//! [`super::reachability`], whose entry step rests on the same property.
//!
//! ## Not everything flips: transformations do, conditions do not
//!
//! The rule to hold on to when reading `pre_enter` against `Reachability::enter`, because
//! getting it wrong is a silent bug rather than a compile error.
//!
//! A TRANSFORMATION inverts and moves. `slot := k` forgets the slot and then asserts the
//! value; its pre-image asserts the value and then forgets the slot, and it is undone in
//! the mirror position - last written is first undone.
//!
//! A CONDITION does not invert and does not move. The guard, "this entry has not been
//! seen", "this check has not already passed", "this one-time effect has not fired" are
//! all predicates about the state at one point in the sequence, and they are still
//! predicates about the state at that same point when read backwards. What changes is
//! only WHEN the backward pass can conjoin them: a condition on the incoming state can
//! only be expressed once everything after it has been undone, so it comes last.
//!
//! The order is load-bearing where a condition and a write touch the same slot. `Fake`
//! tests that its seen flag is clear and then sets it. Backwards, the assignment must be
//! undone FIRST - which selects the states where the flag is set and then frees the slot -
//! and only then can "the flag was clear" be conjoined. Conjoining it earlier would put a
//! constraint on a slot that the very next step erases, and the condition would silently
//! vanish. `unseen` is called after `pre_charge` for exactly that reason, and the same
//! goes for a rolled check's pass and fail flags.
//!
//! One assumption holds this together and is worth stating: nothing in a node's action
//! list writes that node's once slot. It is written by `charge`, outside and before the
//! actions, so "has this one-time effect fired" has the same answer at every position in
//! the sequence and can be read at any of them.
//!
//! ## What it is allowed to get wrong
//!
//! The same one-directional approximation as everything else symbolic here: guards come
//! from `may_be_true`, so a state may be included that a real path could not reach the
//! target from. What must never happen is the reverse - a state excluded that can. A NO
//! against a path the reference walk actually takes is a bug in the pre-image rather than
//! an approximation, and `tests/reference_oracle.rs` exists to catch it.
//!
//! ## What it does not do, and what does it instead
//!
//! ONE TARGET PER PASS, deliberately. Ordering candidates and stopping at the first
//! witness is [`crate::symbolic::novelty_search`]'s job - de-sze.14.3 - and it is the
//! thing that turns this into an answer to the question the look-ahead actually asks.
//! Nothing else should call this in a loop of its own.
//!
//! Money is read the way the entry step reads it: where the layout carries a balance, a
//! price is a constraint on the way in and a subtraction on the way through, and both are
//! undone here in the reverse of the order they happen. Where it does not, every price is
//! affordable and the approximation is inherited unchanged.

use std::collections::{HashMap, HashSet, VecDeque};

use oxidd::bdd::BDDFunction;
use oxidd::{BooleanFunction, Function};

use crate::core::types::{DialogueCheckKind, DialogueNodeId, Ternary};
use crate::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::symbolic::action_image::ActionImage;
use crate::symbolic::guard_formula::GuardCompiler;
use crate::symbolic::known::Known;
use crate::symbolic::order::{IterationOrder, Worklist};
use crate::symbolic::reachability::never_displays;
use crate::symbolic::search::Search;
use crate::symbolic::vars::DataVars;
use crate::world::ILookAheadWorld;

/// What one backward fixed point cost.
#[derive(Debug, Clone, Default)]
pub struct BackwardStats {
    /// Whether the sets stopped moving, rather than the budget running out.
    pub reached_fixed_point: bool,
    /// How many times an entry was taken off the queue.
    pub steps: usize,
    /// How many times an entry's set actually grew.
    pub widenings: usize,
    /// Entries from which the target can be reached in at least one state.
    pub entries_reaching: usize,
    /// Diagram nodes across every entry's set, counted with sharing inside each set only.
    pub diagram_nodes: usize,
    /// The largest single entry's set.
    pub largest_set: usize,
    /// Whether the diagram manager ran out of nodes.
    pub out_of_memory: bool,
    /// Actions skipped because the layout does not carry what they touch.
    pub actions_ignored: usize,
    /// The entry where this pass MET what an earlier search already knew.
    ///
    /// A proof that the target is reachable, and the pass stopped on it: a state the search
    /// holds arriving at this entry is one this pass has shown reaches the target. See
    /// [`crate::symbolic::known::Known`].
    ///
    /// The fixed point is NOT complete when this is set - it stopped early, on purpose -
    /// so `reached_fixed_point` is false and the sets are a lower bound. That is the right
    /// way round: the answer is yes, and there is nothing left to prove.
    pub met_at: Option<DialogueNodeId>,
    pub elapsed: std::time::Duration,
}

/// When to give up on a backward pass.
pub struct Budget {
    pub steps: usize,
    pub time: std::time::Duration,
    /// How often the pass should say where it has got to, or ZERO to say nothing.
    ///
    /// A measurement pass runs for minutes on the heavy groups, and one fixed point over
    /// one target is a single call that returns when it is finished - so without this the
    /// only thing a watcher sees is the row ending. Every long-running search grew the
    /// same hook for the same reason.
    pub report_gap: std::time::Duration,
    /// Steps, entries known to reach the target, entries still queued, manager nodes.
    ///
    /// No percentage, because the pass does not know one: it knows what it has spent, and
    /// spending the budget is how these passes end.
    ///
    /// AN `Rc` RATHER THAN A `Box`, since de-cluo, so that this whole budget can be CLONED.
    /// The driver narrows a candidate's clock to what is left of the attempt, which means
    /// building a budget that differs in one field - and copying one that owned its hook
    /// would have had to drop it, which would silence exactly the progress lines a long row
    /// is watched by. A search runs on a thread of its own, so a shared pointer is enough
    /// and costs nothing.
    #[allow(clippy::type_complexity)]
    pub on_progress: Option<std::rc::Rc<dyn Fn(usize, usize, usize, usize)>>,
}

impl Clone for Budget {
    fn clone(&self) -> Self {
        Self {
            steps: self.steps,
            time: self.time,
            report_gap: self.report_gap,
            on_progress: self.on_progress.clone(),
        }
    }
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            steps: 2_000_000,
            time: std::time::Duration::from_secs(120),
            report_gap: std::time::Duration::ZERO,
            on_progress: None,
        }
    }
}

/// A finished backward fixed point, asked the one question a search asks of one.
///
/// TWO THINGS ANSWER IT: a pass that has just run, and one [`crate::symbolic::memo`] kept
/// from an earlier request. They hold the same sets and are read the same way, and the
/// search should not have to know which it has - so the question is a trait rather than an
/// inherent method on the first of them.
pub trait SettledPass {
    /// Whether a search that starts by entering `node` in any state in `states` reaches
    /// the target.
    fn reachable_from(&self, node: DialogueNodeId, states: &BDDFunction) -> bool;
}

/// For each entry, the data states from which the target is reachable.
pub struct Backward<'a> {
    vars: &'a DataVars<'a>,
    /// Per entry: the states at that entry, AFTER entering it, from which the target can
    /// still be reached.
    sets: HashMap<DialogueNodeId, BDDFunction>,
    stats: BackwardStats,
}

/// One menu outcome, before entering any of its starting entries.
#[derive(Clone)]
pub struct Position {
    pub option: DialogueNodeId,
    pub entries: Vec<DialogueNodeId>,
    pub holding: BDDFunction,
}

/// The nearest option to ONE target, and which option it is.
///
/// What a round of the exact marking asks of each target it could not skip - see
/// [`crate::symbolic::menu::mark_menu`].
///
/// ## A pooled search, and why a round is not answered by one
///
/// A round could instead be answered by one pool over every target at once: a forward front
/// from the whole menu, unioned over its options, met part way by a backward crawl from each
/// target. Each side kept its layers apart, so a meeting named the forward and the backward
/// layer it happened in and the distance was their sum; sums were tried in increasing order,
/// whichever side carried less into its next layer grew first, and the entry the two fronts
/// met on paid its own choice charge, since each side charges an entry only when it leaves
/// it. The winning option was found afterwards, by replaying each option's own forward front
/// as far as the meeting.
///
/// It was measured over the whole game and removed, for two reasons. On the same first round
/// of conversation 353 it took 1,191 ms and 2.55 million diagram nodes where one backward pass
/// per target took 268 ms and 378 thousand: the front over the whole menu is the expensive
/// direction on a deep group, and every target's crawl is held at once. And on a tie it
/// credited the option earliest in the menu, which on a chain of options - one reaching the
/// content only through another - starred the outer ones at the cost of further rounds, 353
/// taking three rounds and 2.6 seconds against one round and 0.3. The measurement that argued
/// for meeting in the middle is kept in `performance/bidirectional_headroom.rs`.
#[derive(Debug, PartialEq, Eq)]
pub enum Nearest {
    Found {
        distance: usize,
        winner: usize,
    },
    /// No option still hunting can reach the target.
    Unreachable,
    Unfinished {
        out_of_memory: bool,
    },
}

/// What a single-target walk carries from one step to the next.
///
/// The question it was asked, the image its pre-images go through, and the layer being
/// spread. Held together so that a step is one call with the entry it spreads from, rather
/// than a dozen arguments of which two change.
struct Walk<'w, 'a> {
    graph: &'w LookAheadGraph,
    world: &'w dyn ILookAheadWorld,
    known: &'w Known,
    cut: &'w HashSet<DialogueNodeId>,
    positions: &'w [Position],
    image: ActionImage<'a>,
    /// What is waiting to be spread within the current layer.
    frontier: HashMap<DialogueNodeId, BDDFunction>,
    queue: Worklist<'w>,
    /// The layer being spread, in choices from the target.
    distance: usize,
}

impl<'a> Backward<'a> {
    /// Runs the fixed point backwards from `target`.
    pub fn reaching(
        graph: &LookAheadGraph,
        target: DialogueNodeId,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
    ) -> Self {
        Self::reaching_within(
            graph,
            target,
            compiler,
            world,
            counter_cap,
            &Budget::default(),
        )
    }

    /// The same, under a budget.
    pub fn reaching_within(
        graph: &LookAheadGraph,
        target: DialogueNodeId,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
        budget: &Budget,
    ) -> Self {
        Self::reaching_knowing(graph, target, compiler, world, counter_cap, budget, None)
    }

    /// The same, told what earlier searches over this group already worked out.
    ///
    /// THREE DIFFERENT USES OF THE SAME ARGUMENT, and only one of them changes an answer.
    /// The parent map and the iteration order are facts about the graph that every pass
    /// works out for itself, so taking them from `known` is pure saving - the order changes
    /// how many pops the same settled sets take, and not what is in them. The forward sets
    /// are the one that does: a pass that MEETS one stops there, having shown the target
    /// reachable without finishing - see [`Known`].
    pub fn reaching_knowing(
        graph: &LookAheadGraph,
        target: DialogueNodeId,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
        budget: &Budget,
        known: Option<&Known>,
    ) -> Self {
        Self::reaching_any_knowing(
            Search {
                graph,
                compiler,
                world,
                counter_cap,
            },
            &[target],
            &HashSet::new(),
            budget,
            known,
        )
    }

    /// Worklist reachability from any target, refusing routes through cut options.
    pub fn reaching_any_knowing(
        search: Search<'_, 'a>,
        targets: &[DialogueNodeId],
        cut: &HashSet<DialogueNodeId>,
        budget: &Budget,
        known: Option<&Known>,
    ) -> Self {
        let Search {
            graph,
            compiler,
            world,
            counter_cap,
        } = search;
        let vars = compiler.vars();
        let mut image = ActionImage::for_world(vars, counter_cap, world);
        let mut this = Self {
            vars,
            sets: HashMap::new(),
            stats: BackwardStats::default(),
        };

        // Only the entries that can reach the target through links at all are worth
        // visiting. Guards can refuse an edge but never create one, so this is an upper
        // bound on what the fixed point will touch - and on the hub-shaped groups it is
        // most of them, which is a measured fact rather than a disappointment: see
        // de-sze.14. Where it IS small, it saves visiting the rest entirely.
        // TAKEN RATHER THAN REBUILT where an earlier search left one. A driver that asks
        // about forty candidates walked the whole graph forty times to build the same map.
        let owned_parents;
        let parents = match known {
            Some(known) => known.parents(),
            None => {
                owned_parents = Self::parents_of(graph);
                &owned_parents
            }
        };
        let relevant = Self::can_reach(parents, targets, cut);

        // Only worth asking when something is actually known; `meets` on an empty `Known`
        // is a walk over the parents to conclude nothing.
        let meeting = known.filter(|known| known.can_meet());

        let began = std::time::Instant::now();
        let mut last_report = began;
        // FROM THE FAR END OF THE ORDER. The rank puts a component below everything it can
        // reach through links, and this pass travels the links backwards - so taking the
        // highest rank first finishes a component before the ones that feed it, which is
        // what stops an entry being popped once per contribution that arrives late.
        //
        // TAKEN RATHER THAN REBUILT where an earlier search left one, exactly like the
        // parent map above; a caller with nothing to share pays one Tarjan pass, which is
        // nothing against the diagram work that follows.
        let owned_order;
        let order = match known {
            Some(known) => known.order(),
            None => {
                owned_order = IterationOrder::of(graph);
                &owned_order
            }
        };
        let mut queue = Worklist::new(order);
        let mut ran_out = false;

        // The target's own set: enter it in any state at all and the target has been
        // reached, so there is nothing to ask about what happens afterwards. Everything
        // else is derived from this one set by walking links backwards.
        //
        // EXCEPT WHERE ENTERING IT IS NOT READING IT. A passive check the sheet fails is
        // entered by every state that gets there and displays to none of them, so the set
        // below would be the whole space for a line the player can never read. No seed
        // means no frontier, the pass settles having established nothing, and the refusal
        // is a real one rather than an unfinished search - see `never_displays`.
        let mut frontier: HashMap<DialogueNodeId, BDDFunction> = HashMap::new();
        for &target in targets {
            if cut.contains(&target) {
                continue;
            }
            if let Some(node) = graph
                .get(target)
                .filter(|node| !never_displays(node, world))
            {
                let arriving = this.pre_enter(node, &vars.top(), compiler, world, &mut image);
                if let Some(fresh) = this.widen(target, &arriving) {
                    // The target itself can be the meeting point, and where the search begins
                    // at the target it is: whether what the search holds arriving there is a
                    // state the target's own guard admits is the whole question, and both
                    // halves of that are already in hand.
                    if let Some(known) = meeting
                        && this.meets_known(known, target)
                    {
                        this.stats.met_at = Some(target);
                    }
                    frontier.insert(target, fresh);
                    queue.push(target);
                }
            }
        }

        // What travels is the DELTA, the way a fixed point propagates only the delta
        // and for the same reason: `pre_enter` distributes over union - the guard is a
        // conjunction, every pre-image is a select-and-forget, and each branch unions its
        // cases - so the pre-image of the whole set is the pre-image of what has already
        // been sent plus the pre-image of what is new. Re-sending the whole set at every
        // visit recomputes the first half each time, which on a group of four thousand
        // entries is the difference between finishing and not.
        'search: while let Some(id) = queue.pop() {
            // Proved already, at the target or at an entry reached since. Nothing below
            // can improve on a yes.
            if this.stats.met_at.is_some() {
                break;
            }

            // Take what is pending and leave nothing behind: an entry can be queued more
            // than once, and the second visit has nothing left to do.
            let delta = match frontier.insert(id, vars.bottom()) {
                Some(pending) if pending.satisfiable() => pending,
                _ => continue,
            };

            this.stats.steps += 1;
            if this.stats.steps >= budget.steps || began.elapsed() >= budget.time {
                ran_out = true;
                break;
            }

            // On a clock rather than on a step count, because no step count works: early
            // on the sets are tiny and a step is nothing, and by the time one step is
            // minutes long a step count is either far too chatty or silent for an hour.
            // The clock is read every step regardless, one line above.
            if let Some(report) = &budget.on_progress
                && !budget.report_gap.is_zero()
                && last_report.elapsed() >= budget.report_gap
            {
                last_report = std::time::Instant::now();
                report(
                    this.stats.steps,
                    this.sets.len(),
                    queue.len(),
                    vars.node_count(),
                );
            }

            for &parent in parents.get(&id).into_iter().flatten() {
                if !relevant.contains(&parent) || cut.contains(&parent) {
                    continue;
                }

                let Some(node) = graph.get(parent) else {
                    continue;
                };
                // Entering the parent has to leave the search somewhere that can go on to
                // reach the target through this child.
                let before = this.pre_enter(node, &delta, compiler, world, &mut image);
                if image.out_of_memory() {
                    this.stats.out_of_memory = true;
                    break 'search;
                }
                if !before.satisfiable() {
                    continue;
                }

                let Some(fresh) = this.widen(parent, &before) else {
                    if this.stats.out_of_memory {
                        break 'search;
                    }
                    continue;
                };

                // ON EVERY WIDENING, not only the first sighting. A state that meets what
                // is already known can arrive at any growth of this entry's set, and
                // checking only the first would turn a proof into a maybe for the sake of
                // one conjunction.
                if let Some(known) = meeting
                    && this.meets_known(known, parent)
                {
                    this.stats.met_at = Some(parent);
                    break 'search;
                }

                let pending = frontier
                    .get(&parent)
                    .cloned()
                    .unwrap_or_else(|| vars.bottom());
                let Ok(waiting) = pending.or(&fresh) else {
                    this.stats.out_of_memory = true;
                    break 'search;
                };
                frontier.insert(parent, waiting);
                queue.push(parent);
            }
        }

        this.stats.actions_ignored = image.ignored();
        // A MEET IS NOT A FIXED POINT. The pass stopped early having proved the answer
        // yes, so the sets are a lower bound and nothing may read a no out of them. The
        // caller is expected to look at `met_at` first; this makes the wrong reading of
        // the flag say "incomplete", which is the safe thing for it to say.
        this.stats.reached_fixed_point =
            !ran_out && !this.stats.out_of_memory && this.stats.met_at.is_none();
        this.stats.elapsed = began.elapsed();
        this.finish();
        this
    }

    /// Adds `arriving` to what is known at `node`, and says what part of it was new.
    ///
    /// `None` means nothing changed - which is also what is reported when the manager
    /// runs out of room, with [`BackwardStats::out_of_memory`] set to say so, because
    /// once that happens every set since is the pre-image of nothing in particular and
    /// the caller must stop rather than carry on with a smaller answer.
    fn widen(&mut self, node: DialogueNodeId, arriving: &BDDFunction) -> Option<BDDFunction> {
        let mut out_of_nodes = false;
        let fresh = widen_into(&mut self.sets, self.vars, node, arriving, &mut out_of_nodes);
        if out_of_nodes {
            self.stats.out_of_memory = true;
        }
        if fresh.is_some() {
            self.stats.widenings += 1;
        }
        fresh
    }

    /// The states from which entering `node` lands in `onward`.
    ///
    /// The mirror of `Reachability::enter`, case for case. The requirement is not
    /// elegance but agreement: a backward pass that reads a node differently from the
    /// forward one is answering a different question, and the oracle test would catch it
    /// as an unreachable entry the search walks to.
    fn pre_enter(
        &mut self,
        node: &LookAheadNode,
        onward: &BDDFunction,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        let inner = match node.kind {
            // A hidden test is entered and goes nowhere, so nothing reaches anything
            // through one.
            DialogueCheckKind::Test => return self.vars.bottom(),

            DialogueCheckKind::Fake => {
                let unfailed = self.pre_fail(node, onward, image);
                let charged = self.pre_charge(node, &unfailed, image);
                self.unseen(node, &charged)
            }

            DialogueCheckKind::KimSwitch => {
                let charged = self.pre_charge(node, onward, image);
                if node.boolean_only {
                    charged
                } else {
                    self.unseen(node, &charged)
                }
            }

            DialogueCheckKind::Red | DialogueCheckKind::White => {
                let may_succeed = crate::world::roll_may_succeed(node, world);
                self.pre_rolled(node, may_succeed, onward, image)
            }

            DialogueCheckKind::Passive => {
                let passes = crate::world::passive_outcome(node, world);
                let mut result = self.vars.bottom();
                if passes != Ternary::False {
                    result = self.pre_charge(node, onward, image);
                }
                // The failing branch passes the state through UNCHARGED, so backwards it
                // passes `onward` through untouched - the one branch that does not go
                // through `charge`, forward or back.
                if passes != Ternary::True {
                    result = self.or_no_room(result.or(onward));
                }
                result
            }

            _ => self.pre_charge(node, onward, image),
        };

        if !inner.satisfiable() {
            return self.vars.bottom();
        }

        // The price is tested on the way in, after the guard and before anything is paid,
        // so undoing runs the other way: the payment is already undone inside `pre_charge`
        // and what is left is the constraint the purse had to satisfy BEFORE it.
        let afforded = self.affordable(node, &inner);
        if !afforded.satisfiable() {
            return self.vars.bottom();
        }

        // And the guard is tested first of all, so it is the last thing undone.
        let (may_be_true, _) = self.guard_of(node, compiler);
        self.or_no_room(afforded.and(&may_be_true))
    }

    /// A rolled check, backwards: the states that reach `onward` down either branch.
    fn pre_rolled(
        &mut self,
        node: &LookAheadNode,
        may_succeed: bool,
        onward: &BDDFunction,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        // Success raised the pass flag, so undoing it selects the states where it is
        // raised and then forgets it - where the roll may succeed at all, the same rule as
        // `Reachability::rolled_cases`.
        let mut landed = if may_succeed {
            match node.flag_slot {
                slot if slot >= 0 => image.pre_assign(onward, slot as usize, 1),
                _ => onward.clone(),
            }
        } else {
            self.vars.bottom()
        };

        // Failure: both kinds recorded it where there was a flag to record it with, a
        // white check without one left the state alone, and anything else has no failing
        // branch at all. The same three cases as the entry step, undone - see
        // `Reachability::rolled`, which this has to mirror exactly or the two engines
        // answer different questions.
        let failing = if node.failed_flag_slot >= 0 {
            let unfailed = self.pre_fail(node, onward, image);
            image.pre_assign(&unfailed, node.failed_flag_slot as usize, 1)
        } else if node.kind == DialogueCheckKind::White {
            self.pre_fail(node, onward, image)
        } else {
            self.vars.bottom()
        };
        landed = self.or_no_room(landed.or(&failing));

        let entered = self.pre_charge(node, &landed, image);

        // A check already passed is closed, and one already failed is closed too.
        let mut open = entered;
        if let Some(passed) = self.flag(node.flag_slot) {
            let unpassed = self.or_no_room(passed.not());
            open = self.or_no_room(open.and(&unpassed));
        }
        if let Some(failed) = self.flag(node.failed_flag_slot) {
            let unfailed = self.or_no_room(failed.not());
            open = self.or_no_room(open.and(&unfailed));
        }

        open
    }

    /// A check's failing branch beyond the failure flag, undone - the mirror of
    /// `Reachability::fail`.
    fn pre_fail(
        &mut self,
        node: &LookAheadNode,
        onward: &BDDFunction,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        image.pre_apply(onward, &node.failure_actions, &self.vars.bottom())
    }

    /// Paying, marking seen and acting, undone - in the reverse of the order they happen.
    fn pre_charge(
        &mut self,
        node: &LookAheadNode,
        onward: &BDDFunction,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        if !onward.satisfiable() {
            return self.vars.bottom();
        }

        // The once slot says whether a one-time effect has already fired, and it is read
        // at the moment the actions apply - which is AFTER the two assignments below, so
        // it is undone first and read against the same state the entry step reads it
        // against.
        let already = match self.flag(node.once_slot) {
            Some(flag) => flag,
            None => self.vars.bottom(),
        };

        // The mirror of `Reachability::charge` raising the once flag. Forward, an entry
        // with a one-time action splits: the states that had not fired apply it and come
        // out with the flag up, and the states that had skip it. So backwards there are
        // two ways to have arrived, and they are told apart by the flag - which is why
        // undoing the assignment on the fresh branch comes BEFORE conjoining "it was
        // clear", exactly as it does for `Fake` and its seen marker.
        let fires_once = node.actions.iter().any(|action| action.is_once());
        let mut current = if fires_once && node.once_slot >= 0 {
            let raised = image.pre_assign(onward, node.once_slot as usize, 1);
            let fresh = image.pre_apply(&raised, &node.actions, &self.vars.bottom());
            let unfired = self.or_no_room(already.not());
            let fresh = self.or_no_room(fresh.and(&unfired));

            let spent = image.pre_apply(onward, &node.actions, &self.vars.top());
            let spent = self.or_no_room(spent.and(&already));

            self.or_no_room(fresh.or(&spent))
        } else {
            image.pre_apply(onward, &node.actions, &already)
        };

        if node.seen_slot >= 0 {
            current = image.pre_assign(&current, node.seen_slot as usize, 1);
        }

        if node.is_cost_option() {
            current = self.pre_pay(node, &current, image);
        }

        current
    }

    /// Paying, undone: the mirror of `Reachability::pay`, branch for branch.
    ///
    /// Forward, a price paid once splits - the states that had not paid hand over the money
    /// and come out with the slot raised, and the states that had are left alone. So
    /// backwards there are two ways to have arrived, told apart by that slot, and the
    /// assignment is undone before "it was clear" is conjoined: `pre_assign` frees the
    /// variable, so a conjunction the other way round would constrain the wrong state.
    fn pre_pay(
        &mut self,
        node: &LookAheadNode,
        onward: &BDDFunction,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        let price = node.cost.max(0) as u32;

        let Some(paid) = self.already_paid(node) else {
            return self.pre_spend(onward, price);
        };

        let fresh = image.pre_assign(onward, node.once_slot as usize, 1);
        let fresh = self.pre_spend(&fresh, price);
        let unpaid = self.or_no_room(paid.not());
        let fresh = self.or_no_room(fresh.and(&unpaid));

        let spent = self.or_no_room(onward.and(&paid));

        self.or_no_room(fresh.or(&spent))
    }

    /// The states from which `money := money - amount` lands in `states`.
    ///
    /// The pre-image includes states that SATURATED - a purse below the price, landing on
    /// zero - which forward could never have entered. They are removed by
    /// [`Self::affordable`], which runs after this and is where the price is a constraint
    /// rather than an arithmetic step.
    fn pre_spend(&mut self, states: &BDDFunction, amount: u32) -> BDDFunction {
        match self.vars.money_ops() {
            // No room says `None` where the diagram operations say `Err`, and the two mean
            // the same thing: the manager filled part way through, so what came back is not
            // the purse before paying. A layout with no money is the other case entirely -
            // there is nothing to unspend, and the set is right as it stands.
            Some(money) => {
                let unspent = money.pre_saturating_sub(states, amount);
                self.or_no_room(unspent.ok_or(()))
            }
            None => states.clone(),
        }
    }

    /// Which states could afford this node, exactly as the entry step decides it.
    ///
    /// A backward pass that read a price differently from the forward one would answer a
    /// different question, and the two are checked against each other and against the
    /// reference walk - so this is deliberately the same three lines.
    fn affordable(&mut self, node: &LookAheadNode, states: &BDDFunction) -> BDDFunction {
        if !node.is_cost_option() {
            return states.clone();
        }

        let Some(money) = self.vars.money_ops() else {
            return states.clone();
        };

        // A layout that carries no money cannot refuse anything, which is the branch
        // above. A manager with no room to build the price is not that: there is no
        // constraint to apply and no honest set to apply it to.
        let Some(price) = money.at_least(node.cost.max(0) as u32) else {
            self.stats.out_of_memory = true;
            return self.vars.bottom();
        };
        self.or_no_room(states.and(&price))
    }

    /// "This entry's price has already been paid", where it is one that is paid once.
    fn already_paid(&mut self, node: &LookAheadNode) -> Option<BDDFunction> {
        if !node.is_cost_option() || !node.cost_once {
            return None;
        }
        self.flag(node.once_slot)
    }

    /// The states in which this node has not been seen.
    fn unseen(&mut self, node: &LookAheadNode, states: &BDDFunction) -> BDDFunction {
        match self.flag(node.seen_slot) {
            Some(seen) => {
                let clear = self.or_no_room(seen.not());
                self.or_no_room(states.and(&clear))
            }
            None => states.clone(),
        }
    }

    /// A slot's "is set" formula, for a slot number that may be -1 for "no slot".
    ///
    /// `None` FOR TWO REASONS THAT WANT OPPOSITE THINGS, exactly as on the forward side.
    /// A slot the layout does not carry constrains nothing and the callers here are right
    /// to carry on without it; a manager with no room to build the formula constrains
    /// nothing either, and carrying on then is how a pass comes to report a target
    /// unreachable that a walk reaches. The layout answers the first question without
    /// touching a diagram, so asking it first leaves the second as the only reading left.
    fn flag(&mut self, slot: i32) -> Option<BDDFunction> {
        let slot = usize::try_from(slot).ok()?;
        self.vars.slot_ceiling(slot)?;
        let formula = self.vars.slot_is_set(slot);
        if formula.is_none() {
            self.stats.out_of_memory = true;
        }

        formula
    }

    /// Takes the result of a diagram operation, or records that there was no room.
    ///
    /// The empty set is returned only so the types stay simple. It is not a meaningful
    /// answer, and a caller that sees [`BackwardStats::out_of_memory`] must stop rather
    /// than read what came back - the loop in `to` does exactly that.
    fn or_no_room<E>(&mut self, attempt: Result<BDDFunction, E>) -> BDDFunction {
        match attempt {
            Ok(function) => function,
            Err(_) => {
                self.stats.out_of_memory = true;
                self.vars.bottom()
            }
        }
    }

    /// This node's compiled guard.
    ///
    /// Not cached per pass, and for a reason worth stating: a backward pass visits an entry
    /// once per widening of its own set, which is few enough times that a map of its own
    /// would cost more than it saved. If a measurement shows otherwise this should grow one.
    /// Whether this entry's set has met something an earlier search already established.
    ///
    /// Asked of the WHOLE set rather than of the delta that just arrived: the meet is a
    /// question about what can be held here at all, and a state that arrived two widenings
    /// ago counts exactly as much as one that arrived now.
    fn meets_known(&self, known: &Known, id: DialogueNodeId) -> bool {
        self.sets.get(&id).is_some_and(|set| known.meets(id, set))
    }

    /// This node's compiled guard, from the compiler's cache when it has one.
    ///
    /// THIS USED TO COMPILE ON EVERY VISIT. A pass revisits an entry each time its set
    /// grows, and the guard does not change between visits. `compile_for` is the map that
    /// stops it, and it lives in the COMPILER so that it also outlives a single pass:
    /// `novelty_search` asks about one candidate after another over the same compiler, so
    /// the second candidate's pass inherits every guard the first one compiled.
    fn guard_of(
        &mut self,
        node: &LookAheadNode,
        compiler: &mut GuardCompiler<'a>,
    ) -> (BDDFunction, BDDFunction) {
        let compiled = compiler.compile_for(node.id, &node.guard);
        (compiled.may_be_true, compiled.may_be_false)
    }

    /// Every entry's incoming links, which is the edge direction this search walks.
    fn parents_of(graph: &LookAheadGraph) -> HashMap<DialogueNodeId, Vec<DialogueNodeId>> {
        let mut parents: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
        for node in graph.nodes() {
            for &child in &node.links {
                parents.entry(child).or_default().push(node.id);
            }
        }
        parents
    }

    /// The entries that can reach a target through uncut links, guards ignored.
    fn can_reach(
        parents: &HashMap<DialogueNodeId, Vec<DialogueNodeId>>,
        targets: &[DialogueNodeId],
        cut: &HashSet<DialogueNodeId>,
    ) -> HashSet<DialogueNodeId> {
        let mut seen: HashSet<_> = targets
            .iter()
            .copied()
            .filter(|id| !cut.contains(id))
            .collect();
        let mut queue: VecDeque<_> = targets
            .iter()
            .copied()
            .filter(|id| !cut.contains(id))
            .collect();
        while let Some(id) = queue.pop_front() {
            for &parent in parents.get(&id).into_iter().flatten() {
                if !cut.contains(&parent) && seen.insert(parent) {
                    queue.push_back(parent);
                }
            }
        }
        seen
    }

    /// Totals that can only be taken once the sets have stopped moving.
    fn finish(&mut self) {
        self.stats.entries_reaching = self.sets.len();
        self.stats.diagram_nodes = self.sets.values().map(|s| s.node_count()).sum();
        self.stats.largest_set = self
            .sets
            .values()
            .map(|s| s.node_count())
            .max()
            .unwrap_or(0);
    }

    /// The states from which ENTERING `node` goes on to reach the target.
    ///
    /// Before rather than after, which is what lets a query be answered without running
    /// anything forwards: a search that begins by entering `node` holding the seed reaches
    /// the target exactly when the seed meets this set. An "after entering" set would
    /// need the seed pushed through that first entry by some other means, and the only
    /// thing that could do it is a walk forwards from the start, which is the work this
    /// exists to avoid.
    pub fn states_at(&self, node: DialogueNodeId) -> Option<&BDDFunction> {
        self.sets.get(&node)
    }

    /// The entries from which the target can be reached at all.
    pub fn entries(&self) -> impl Iterator<Item = DialogueNodeId> + '_ {
        self.sets.keys().copied()
    }

    pub fn stats(&self) -> &BackwardStats {
        &self.stats
    }

    /// The nearest option to one target, walking back from the target alone.
    ///
    /// Distance is counted in choices: an entry is charged when it is left, and only where it
    /// is a player line offered beside another. The route's two ends are free.
    ///
    /// ONE FRONT, from the target. It keeps the cumulative set per entry and the layer being
    /// spread, and stops in the first layer whose set meets an option's own states at that
    /// option's entries - so the distance is exact for this target and says nothing about
    /// any other.
    pub fn nearest(
        search: Search<'_, 'a>,
        target: DialogueNodeId,
        cut: &HashSet<DialogueNodeId>,
        budget: &Budget,
        known: &Known,
        positions: &[Position],
    ) -> Nearest {
        let Search {
            graph,
            compiler,
            world,
            counter_cap,
        } = search;
        let vars = compiler.vars();
        let began = std::time::Instant::now();
        let mut this = Self {
            vars,
            sets: HashMap::new(),
            stats: BackwardStats::default(),
        };
        let Some(node) = graph.get(target).filter(|n| !never_displays(n, world)) else {
            return Nearest::Unreachable;
        };
        if cut.contains(&target) {
            return Nearest::Unreachable;
        }
        let mut walk = Walk {
            graph,
            world,
            known,
            cut,
            positions,
            image: ActionImage::for_world(vars, counter_cap, world),
            frontier: HashMap::new(),
            queue: Worklist::new(known.order()),
            distance: 0,
        };
        let seed = this.pre_enter(node, &vars.top(), compiler, world, &mut walk.image);
        let mut next = HashMap::new();
        if let Some(delta) = this.widen(target, &seed) {
            next.insert(target, delta);
        }
        if let Some(found) = this.meeting(target, 0, positions) {
            return found;
        }
        for distance in 0usize.. {
            walk.distance = distance;
            walk.queue = Worklist::new(known.order());
            walk.frontier.clear();
            // A stable order keeps diagram allocation and measurements repeatable.
            let mut incoming: Vec<_> = std::mem::take(&mut next).into_iter().collect();
            incoming.sort_by_key(|(id, _)| (id.conversation_id, id.entry_id));
            for (id, states) in incoming {
                // At the target these states have arrived nowhere yet. Everywhere else they
                // are a choice's, already asked about when the choice was reached, and what
                // this layer buys is leaving it.
                if distance == 0 {
                    walk.frontier.insert(id, states);
                    walk.queue.push(id);
                } else if let Some(found) = this.spread(&mut walk, compiler, id, &states) {
                    return found;
                }
            }
            while let Some(id) = walk.queue.pop() {
                this.stats.steps += 1;
                if began.elapsed() >= budget.time || this.stats.steps >= budget.steps {
                    return Nearest::Unfinished {
                        out_of_memory: false,
                    };
                }
                let Some(delta) = walk.frontier.remove(&id) else {
                    continue;
                };
                // LEAVING A CHOICE COSTS ONE, and the charge is the node's rather than the
                // link's, so every route out of it belongs to the next layer. The target is
                // where the route finishes and is free.
                if id != target && graph.get(id).is_some_and(|n| n.choice) {
                    let pending = next.get(&id).cloned().unwrap_or_else(|| vars.bottom());
                    match pending.or(&delta) {
                        Ok(joined) => {
                            next.insert(id, joined);
                        }
                        Err(_) => {
                            return Nearest::Unfinished {
                                out_of_memory: true,
                            };
                        }
                    }
                    continue;
                }
                if let Some(found) = this.spread(&mut walk, compiler, id, &delta) {
                    return found;
                }
                if this.stats.out_of_memory || walk.image.out_of_memory() {
                    return Nearest::Unfinished {
                        out_of_memory: true,
                    };
                }
            }
            if this.stats.out_of_memory || walk.image.out_of_memory() {
                return Nearest::Unfinished {
                    out_of_memory: true,
                };
            }
            if next.is_empty() {
                return Nearest::Unreachable;
            }
        }
        unreachable!("choice distance exhausted usize")
    }

    /// Whether what is known at `id` meets an option that begins there, and which option.
    fn meeting(
        &self,
        id: DialogueNodeId,
        distance: usize,
        positions: &[Position],
    ) -> Option<Nearest> {
        let states = self.sets.get(&id)?;
        for (winner, position) in positions.iter().enumerate() {
            if !position.entries.contains(&id) {
                continue;
            }
            match states.and(&position.holding) {
                Ok(meet) if meet.satisfiable() => return Some(Nearest::Found { distance, winner }),
                Ok(_) => {}
                Err(_) => {
                    return Some(Nearest::Unfinished {
                        out_of_memory: true,
                    });
                }
            }
        }
        None
    }

    /// Walks back from `id` into every uncut parent, within the walk's current layer.
    ///
    /// `Some` where that settles the walk: an option met, or the manager full.
    fn spread(
        &mut self,
        walk: &mut Walk<'_, 'a>,
        compiler: &mut GuardCompiler<'a>,
        id: DialogueNodeId,
        delta: &BDDFunction,
    ) -> Option<Nearest> {
        let (graph, world, known, cut, positions) =
            (walk.graph, walk.world, walk.known, walk.cut, walk.positions);
        for &parent in known.parents_of(id) {
            if cut.contains(&parent) {
                continue;
            }
            let Some(node) = graph.get(parent) else {
                continue;
            };
            let before = self.pre_enter(node, delta, compiler, world, &mut walk.image);
            let Some(fresh) = self.widen(parent, &before) else {
                continue;
            };
            if self.stats.out_of_memory || walk.image.out_of_memory() {
                return Some(Nearest::Unfinished {
                    out_of_memory: true,
                });
            }
            if let Some(found) = self.meeting(parent, walk.distance, positions) {
                return Some(found);
            }
            let waiting = walk
                .frontier
                .get(&parent)
                .cloned()
                .unwrap_or_else(|| self.vars.bottom());
            match waiting.or(&fresh) {
                Ok(joined) => {
                    walk.frontier.insert(parent, joined);
                    walk.queue.push(parent);
                }
                Err(_) => {
                    return Some(Nearest::Unfinished {
                        out_of_memory: true,
                    });
                }
            }
        }
        None
    }
}

/// Adds `arriving` to what is known at `id`, and says what part of it was new.
///
/// `None` means nothing changed, which is also what is reported when the manager runs out of
/// room - `out_of_nodes` is what tells those apart, and a caller that sees it set must stop
/// rather than carry on with a smaller answer.
fn widen_into(
    sets: &mut HashMap<DialogueNodeId, BDDFunction>,
    vars: &DataVars<'_>,
    id: DialogueNodeId,
    arriving: &BDDFunction,
    out_of_nodes: &mut bool,
) -> Option<BDDFunction> {
    if !arriving.satisfiable() {
        return None;
    }
    let known = sets.get(&id).cloned().unwrap_or_else(|| vars.bottom());
    // Diagrams are canonical for a fixed variable order, so an empty difference is exactly
    // "nothing changed" - no membership test and no approximation in it.
    let Ok(complement) = known.not() else {
        *out_of_nodes = true;
        return None;
    };
    let Ok(fresh) = arriving.and(&complement) else {
        *out_of_nodes = true;
        return None;
    };
    if !fresh.satisfiable() {
        return None;
    }
    let Ok(widened) = known.or(arriving) else {
        *out_of_nodes = true;
        return None;
    };
    sets.insert(id, widened);
    Some(fresh)
}

impl SettledPass for Backward<'_> {
    /// `states` is the SEED - what the search holds on arrival at `node`, before that
    /// node's own guard, cost or actions have been considered. `seed_of` produces one.
    fn reachable_from(&self, node: DialogueNodeId, states: &BDDFunction) -> bool {
        match self.sets.get(&node) {
            // An `Err` here is the manager out of room, and answering "not reachable" on
            // it would be the one wrong direction. Say reachable and let the caller read
            // `out_of_memory`.
            Some(set) => set
                .and(states)
                .map(|both| both.satisfiable())
                .unwrap_or(true),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::budget::DiagramBudget;

    use crate::core::guard_value::GuardValue;
    use crate::symbolic::data_layout::DataLayout;
    use crate::symbolic::reachability::seed_of;
    use crate::test_graph::{Entry, GraphBuilder, node};
    use crate::world::test_world::TestWorld;

    const CAP: i32 = 16;

    /// Does a settled pass say `target` can be reached from the seed?
    ///
    /// EVERY CASE BELOW SAYS WHAT IT EXPECTS, which is what makes these worth having. A
    /// pre-image that dropped every state would answer "no" to everything and look tidy
    /// doing it, so the expectation is written out by hand per shape rather than taken from
    /// another search that could be wrong in the same direction. The shapes are small
    /// enough to read the right answer off by eye.
    fn reaches(entries: Vec<Entry>, world: &TestWorld, target: i32) -> bool {
        let mut builder = GraphBuilder::new();
        for entry in entries {
            builder = builder.add(entry);
        }
        // From the GRAPH, not from what the builder handed back: `LookAheadGraph::new`
        // interns the once slots itself, after the builder's copy was taken, so the
        // builder's copy cannot name them.
        let mut graph = builder.build();
        graph.fit(&crate::graph::Fitting::read(&graph, world));
        let symbols = graph.symbols().clone();

        // MONEY WHERE THE FIXTURE READS IT, by the same rule the product lays out by - so a
        // priced fixture here exercises the balance rather than the approximation that was
        // there before it.
        let layout = DataLayout::for_graph(
            &graph,
            CAP,
            DataLayout::money_ceiling(&graph, world.money()),
            false,
        );
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        // THE CLOCK AS THE PRODUCT HOLDS IT, constant at the world's time - see
        // `workspace` - so a clock question is decided here the way a shipped search decides it.
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(world)
            .with_constant_clock(DataLayout::group_passes_time(&graph));
        let seed = seed_of(&graph, world, &vars).expect("room for a seed");
        let start = node(0);
        let target = node(target);

        let backward = Backward::reaching(&graph, target, &mut compiler, world, CAP as u32);
        assert!(
            backward.stats().reached_fixed_point,
            "the backward pass did not settle",
        );

        backward.reachable_from(start, &seed)
    }

    /// The search must say what the shape plainly says.
    ///
    /// A NO where the answer is yes is the direction that costs a marker: it means the
    /// pre-image lost states. A yes where the answer is no is the over-approximation
    /// working as designed everywhere else, but not on shapes this small, where every guard
    /// is decidable and the answer is exact.
    fn agree(entries: Vec<Entry>, world: &TestWorld, target: i32, expected: bool) {
        let answer = reaches(entries, world, target);
        assert_eq!(
            answer, expected,
            "the backward search said {answer} about entry {target}",
        );
    }

    /// A once-only raise on an entry the save has shown has already fired, so what only it
    /// opens stays shut - the seed carries the `once:` slot as set.
    #[test]
    fn a_once_raise_on_a_shown_entry_opens_nothing() {
        let shape = || {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(r#"SetVariableValue("raised", Variable["raised"] + once(1))"#)
                    .links(&[2]),
                Entry::new(2).guard(r#"Variable["raised"] >= 1"#),
            ]
        };
        let counter = || TestWorld::new().set_variable("raised", GuardValue::from_number(0.0));

        agree(shape(), &counter(), 2, true);
        agree(shape(), &counter().set_seen(node(1), true), 2, false);
    }

    /// A passive check is carried both ways where the group can take off something worn - the
    /// same fixtures as the reference walk's.
    #[test]
    fn a_passive_check_whose_skill_can_move_is_undecided() {
        let shape = || {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(r#"LoseItem("hat_mullen")"#)
                    .links(&[2]),
                Entry::new(2)
                    .kind(DialogueCheckKind::Passive)
                    .script(r#"SetVariableValue("fired", true)"#)
                    .links(&[3]),
                Entry::new(3).guard(r#"Variable["fired"]"#),
            ]
        };
        let failing = || {
            TestWorld::new()
                .set_variable("fired", GuardValue::from_boolean(false))
                .set_check_result(node(2), Ternary::False)
        };

        agree(
            shape(),
            &failing().set_equipped("HAT", "hat_mullen"),
            3,
            true,
        );
        agree(shape(), &failing(), 3, false);
    }

    /// A failing Volition check is carried both ways only where the group's healing can make up
    /// its shortfall - the same fixture as the reference walk's.
    #[test]
    fn healing_unsettles_a_passive_check_only_within_its_margin() {
        let shape = || {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).script("HealVolition(1)").links(&[2]),
                Entry::new(2)
                    .kind(DialogueCheckKind::Passive)
                    .script(r#"SetVariableValue("fired", true)"#)
                    .links(&[3]),
                Entry::new(3).guard(r#"Variable["fired"]"#),
            ]
        };
        let short_by = |shortfall: i32| {
            TestWorld::new()
                .set_variable("fired", GuardValue::from_boolean(false))
                .set_damage("VOLITION", -3.0)
                .set_check_result(node(2), Ternary::False)
                .set_check_margin(node(2), "VOLITION", -shortfall)
        };

        agree(shape(), &short_by(1), 3, true);
        agree(shape(), &short_by(2), 3, false);
    }

    /// A check's result pays out only while its thought is fixed - the same fixtures as the
    /// reference walk's.
    #[test]
    fn a_fixed_thought_pays_out_on_a_check_result() {
        let fixed = |world: TestWorld| world.set_query_bool("IsTHCFixed", true);
        let purse = || TestWorld::new().with_money(100);

        let failed_logic = || {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .kind(DialogueCheckKind::White)
                    .flag("logic_check")
                    .field("SkillType", "0x0100000400000767")
                    .links(&[2]),
                Entry::new(2).cost(150),
            ]
        };
        agree(failed_logic(), &purse(), 2, false);
        agree(failed_logic(), &fixed(purse()), 2, true);

        let encyclopedia = || {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .kind(DialogueCheckKind::Passive)
                    .field("Actor", "399")
                    .links(&[2]),
                Entry::new(2).cost(250),
            ]
        };
        agree(encyclopedia(), &purse(), 2, false);
        agree(encyclopedia(), &fixed(purse()), 2, true);
    }

    /// A reputation action pays out or hurts only while its thought is fixed - the same
    /// fixtures as the reference walk's.
    #[test]
    fn a_fixed_copotype_thought_adds_to_a_reputation_action() {
        let shape = |reputation: &str, reader: Entry| {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(&format!(r#"ReputationGrows("{reputation}")"#))
                    .links(&[2]),
                reader,
            ]
        };
        let fixed = |world: TestWorld| world.set_query_bool("IsTHCFixed", true);

        let priced = || Entry::new(2).cost(150);
        let purse = || TestWorld::new().with_money(100);
        agree(shape("ultraliberal", priced()), &purse(), 2, false);
        agree(shape("ultraliberal", priced()), &fixed(purse()), 2, true);

        let hurt = || Entry::new(2).guard("HasVolitionDamage()");
        let whole = || TestWorld::new().set_damage("VOLITION", 0.0);
        agree(shape("revacholian_nationhood", hurt()), &whole(), 2, false);
        agree(
            shape("revacholian_nationhood", hurt()),
            &fixed(whole()),
            2,
            true,
        );
    }

    /// A blow on the way opens a damage question, and a heal after it shuts it again.
    #[test]
    fn damage_and_healing_move_the_damage_question() {
        let shape = |script: &str| {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).script(script).links(&[2]),
                Entry::new(2).guard("HasVolitionDamage()"),
            ]
        };
        let whole = TestWorld::new().set_damage("VOLITION", 0.0);

        agree(shape("DamageVolition(1)"), &whole, 2, true);
        agree(
            shape(r#"DamageVolition(1);\nHealVolition(1)"#),
            &whole,
            2,
            false,
        );
        agree(
            shape("HealVolition(2)"),
            &TestWorld::new().set_damage("VOLITION", -3.0),
            2,
            true,
        );
    }

    /// Leaving Kim at the church shuts what needs Kim present and opens what needs Kim gone.
    #[test]
    fn leaving_kim_at_the_church_moves_the_kim_questions() {
        let shape = |script: &str, guard: &str| {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).script(script).links(&[2]),
                Entry::new(2).guard(guard),
            ]
        };
        let with_kim = TestWorld::new()
            .set_query_bool("IsKimHere", true)
            .set_query_bool("IsKimInParty", true);
        let left = "RemoveKitsuragiWaitAtChurch()";

        agree(shape("", "IsKimHere()"), &with_kim, 2, true);
        agree(shape(left, "IsKimHere()"), &with_kim, 2, false);
        agree(shape(left, "not IsKimInParty()"), &with_kim, 2, true);
    }

    /// Losing a worn item takes it off, and losing something else does not.
    #[test]
    fn losing_a_worn_item_takes_it_off() {
        let shape = |script: &str, guard: &str| {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).script(script).links(&[2]),
                Entry::new(2).guard(guard),
            ]
        };
        let dressed = TestWorld::new()
            .set_equipped("SHIRT", "shirt_x")
            .set_query_bool("HasShirt", true)
            .set_query_bool("CheckEquipped", true);

        agree(
            shape(r#"LoseItem("hat_y")"#, "HasShirt()"),
            &dressed,
            2,
            true,
        );
        agree(
            shape(r#"LoseItem("shirt_x")"#, "HasShirt()"),
            &dressed,
            2,
            false,
        );
        agree(
            shape(r#"LoseItem("shirt_x")"#, r#"CheckEquipped("shirt_x")"#),
            &dressed,
            2,
            false,
        );
        agree(
            shape(r#"LoseItem("shirt_x")"#, "not HasShirt()"),
            &dressed,
            2,
            true,
        );
    }

    /// A deadline stored from the world's clock is compared against the same clock, so what
    /// opens only once it has passed stays shut - and opens where the deadline is behind.
    #[test]
    fn a_deadline_set_from_the_clock_is_read_back_exactly() {
        let shape = |offset: i32| {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(&format!(
                        r#"SetVariableValue("deadline", TotalHourCount() {offset:+})"#
                    ))
                    .links(&[2]),
                Entry::new(2).guard(r#"TotalHourCount() >= Variable["deadline"]"#),
            ]
        };
        let world = TestWorld::new()
            .with_day_counter(2)
            .with_day_minutes(10 * 60)
            .set_variable("deadline", GuardValue::from_number(0.0));

        agree(shape(8), &world, 2, false);
        agree(shape(-1), &world, 2, true);
    }

    #[test]
    fn an_unguarded_chain_is_reachable_end_to_end() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).links(&[2]),
                Entry::new(2),
            ],
            &TestWorld::new(),
            2,
            true,
        );
    }

    #[test]
    fn a_guard_nothing_can_satisfy_shuts_the_way() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).guard(r#"Variable["shut"]"#).links(&[2]),
                Entry::new(2),
            ],
            &TestWorld::new().set_variable("shut", GuardValue::from_boolean(false)),
            2,
            false,
        );
    }

    /// A red check a thought forces to fail is passed by no path, backwards as forwards.
    ///
    /// 1 is the check: passing opens 2 and failing opens 3. The pre-image has to drop the
    /// success branch exactly where the entry step does, or the two engines disagree about 2.
    #[test]
    fn a_red_check_forced_to_fail_reaches_only_past_its_failure() {
        let entries = || {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .kind(DialogueCheckKind::Red)
                    .flag("roll")
                    .links(&[2, 3]),
                Entry::new(2).guard(r#"Variable["roll"]"#),
                Entry::new(3).guard(r#"Variable["roll_failed"]"#),
            ]
        };
        let world = TestWorld::new().with_red_checks_failing(true);

        agree(entries(), &world, 2, false);
        agree(entries(), &world, 3, true);
    }

    /// The case the whole approach is for: a door the path opens for itself.
    ///
    /// The backward pass has to carry "opened is set" back through the assignment that
    /// sets it and come out with a constraint the seed satisfies - which it does by the
    /// assignment ERASING the constraint rather than by proving anything about it.
    #[test]
    fn an_action_on_the_way_opens_a_guard_further_along() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(r#"SetVariableValue("opened", true)"#)
                    .links(&[2]),
                Entry::new(2).guard(r#"Variable["opened"]"#).links(&[3]),
                Entry::new(3),
            ],
            &TestWorld::new(),
            3,
            true,
        );
    }

    /// And the same shape with the action missing, which must close it.
    #[test]
    fn without_that_action_the_guard_stays_shut() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).links(&[2]),
                Entry::new(2).guard(r#"Variable["opened"]"#).links(&[3]),
                Entry::new(3),
            ],
            &TestWorld::new().set_variable("opened", GuardValue::from_boolean(false)),
            3,
            false,
        );
    }

    /// A counter climbing to a threshold, which needs the increment's pre-image to be a
    /// range rather than a point.
    #[test]
    fn a_counter_reaches_its_threshold_round_a_cycle() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(r#"SetVariableValue("count", Variable["count"] + 1)"#)
                    .links(&[1, 2]),
                Entry::new(2).guard(r#"Variable["count"] >= 3"#).links(&[3]),
                Entry::new(3),
            ],
            &TestWorld::new(),
            3,
            true,
        );
    }

    /// A once increment inside a cycle does NOT climb past its single step.
    ///
    /// It used to, in both directions, because the symbolic passes never raised a once
    /// slot for a once action - `charge` assigned it only for a cost charged once - so
    /// `once_already_fired` was empty at every visit and the action fired every time round
    /// the loop. That was de-sze.15, and this test was written asserting the wrong
    /// behaviour on purpose, so that fixing it would fail here and force both directions
    /// to change together. It did.
    #[test]
    fn a_once_increment_does_not_climb_past_its_single_step() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(r#"SetVariableValue("count", Variable["count"] +once(1))"#)
                    .links(&[1, 2]),
                Entry::new(2).guard(r#"Variable["count"] >= 3"#).links(&[3]),
                Entry::new(3),
            ],
            // Declared numeric, so a comparison against the slot's bits is decidable - see
            // the note on `Reachability`'s guard compilation, which decides it the same way.
            &TestWorld::new().set_variable("count", GuardValue::from_number(0.0)),
            3,
            false,
        );
    }

    /// The pre-image DOES split on the once slot, even though nothing currently sets it.
    ///
    /// Asked directly, because the test above can no longer ask it: with the slot never
    /// raised, both branches of the split behave alike and a pre-image that dropped the
    /// split entirely would pass. Here the slot is set in the world, so the states that
    /// have already fired are real, and a once action must leave them alone.
    #[test]
    fn a_once_action_leaves_a_state_that_has_already_fired_alone() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(
                Entry::new(1)
                    .script(r#"SetVariableValue("count", Variable["count"] +once(1))"#)
                    .links(&[2]),
            )
            .add(Entry::new(2).guard(r#"Variable["count"] >= 1"#).links(&[3]))
            .add(Entry::new(3))
            .build();
        let symbols = graph.symbols().clone();

        let world = TestWorld::new();
        let layout = DataLayout::for_graph(&graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        let backward = Backward::reaching(&graph, node(3), &mut compiler, &world, CAP as u32);

        let entering = backward
            .states_at(node(1))
            .expect("entry 1 can reach the target");
        let once = symbols
            .find(&format!(
                "once:{}:{}",
                node(1).conversation_id,
                node(1).entry_id
            ))
            .expect("a once action interns a once slot");
        let count = symbols.find("count").expect("the counter is interned");

        // Having already fired, the increment does not happen, so only a state that
        // already holds the count can get through.
        let fired = vars.slot_is_set(once).unwrap();
        let empty = vars.slot_equals(count, 0).unwrap();
        let spent_and_empty = fired.and(&empty).unwrap().and(entering).unwrap();
        assert!(
            !spent_and_empty.satisfiable(),
            "a spent once action was allowed to fire again",
        );

        // Not yet fired, and the increment carries the same state through.
        let fresh = fired.not().unwrap();
        let fresh_and_empty = fresh.and(&empty).unwrap().and(entering).unwrap();
        assert!(
            fresh_and_empty.satisfiable(),
            "a once action that has not fired should still be able to",
        );
    }

    /// A hidden test entry is entered and goes nowhere, in both directions.
    #[test]
    fn nothing_is_reachable_through_a_test_entry() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).kind(DialogueCheckKind::Test).links(&[2]),
                Entry::new(2),
            ],
            &TestWorld::new(),
            2,
            false,
        );
    }

    /// A white check leads onward whichever way it rolls.
    #[test]
    fn a_white_check_leads_onward_down_both_branches() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .kind(DialogueCheckKind::White)
                    .flag("check.jump")
                    .links(&[2]),
                Entry::new(2),
            ],
            &TestWorld::new(),
            2,
            true,
        );
    }

    /// A FAILED white check leaves its failure behind, which is the half the test above
    /// cannot see: both branches lead to entry 2 there, so it passes whether the failing
    /// one records anything or not.
    ///
    /// Here entry 3 is guarded on the failure flag, so it is reachable ONLY down the
    /// failing branch and only if that branch writes the flag. de-1uy8: the search and the
    /// forward fixed point started recording it in b15b1aa and the pre-image did not, so
    /// the two engines disagreed about exactly this entry - forward yes, backward no,
    /// which is the direction that loses a marker.
    #[test]
    fn a_failed_white_check_records_its_failure() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .kind(DialogueCheckKind::White)
                    .flag("check.jump")
                    .links(&[2]),
                Entry::new(2)
                    .guard(r#"Variable["check.jump_failed"]"#)
                    .links(&[3]),
                Entry::new(3),
            ],
            &TestWorld::new(),
            3,
            true,
        );
    }

    /// The target itself is the answer: entering it is the whole event, so a start that
    /// IS the target reaches it with no steps at all.
    #[test]
    fn a_start_that_is_the_target_reaches_it() {
        agree(
            vec![Entry::new(0).links(&[1]), Entry::new(1)],
            &TestWorld::new(),
            0,
            true,
        );
    }

    /// An entry no link leads to is unreachable, and the backward pass never visits it.
    #[test]
    fn an_orphan_entry_is_unreachable() {
        agree(
            vec![Entry::new(0).links(&[1]), Entry::new(1), Entry::new(2)],
            &TestWorld::new(),
            2,
            false,
        );
    }

    /// A price the purse cannot meet closes the option, in both directions.
    ///
    /// The whole of de-95t6 in one fixture: before the layout carried a balance this was
    /// reachable, because `affordable` counted the question and let every price through.
    #[test]
    fn a_price_the_purse_cannot_meet_is_not_entered() {
        let entries = || {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).cost(10).links(&[2]),
                Entry::new(2),
            ]
        };

        agree(entries(), &TestWorld::new().with_money(5), 2, false);
        agree(entries(), &TestWorld::new().with_money(10), 2, true);
    }

    /// And what is spent is gone, which is the claim a price CHECK alone does not make.
    ///
    /// The middle fixture of the money suite in miniature: two prices of six out of a purse
    /// of ten. A search that tested each price against the starting balance would take
    /// both, and mark an option leading somewhere the player cannot reach.
    #[test]
    fn spending_leaves_less_for_the_next_price() {
        let entries = || {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).cost(6).links(&[2]),
                Entry::new(2).cost(6).links(&[3]),
                Entry::new(3),
            ]
        };

        agree(entries(), &TestWorld::new().with_money(10), 3, false);
        agree(entries(), &TestWorld::new().with_money(12), 3, true);
    }

    /// A price paid once is not charged again, but the purse must still cover it.
    ///
    /// The shop door a path comes back through, three times over to count to three. With
    /// twelve in hand and a price of six charged once, every visit is affordable and only the
    /// first is paid. Charged every time, the third visit finds the purse empty. And with only
    /// six in hand, the second visit is refused although it would take nothing - the game
    /// disables an option priced above the purse whether or not it has been paid.
    #[test]
    fn a_price_paid_once_is_not_charged_again_but_still_needs_the_purse() {
        let entries = || {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).cost(6).cost_once().links(&[2]),
                Entry::new(2)
                    .script(r#"SetVariableValue("rounds", Variable["rounds"] + 1)"#)
                    .links(&[1, 3]),
                Entry::new(3)
                    .guard(r#"Variable["rounds"] >= 3"#)
                    .links(&[4]),
                Entry::new(4),
            ]
        };

        let purse = |money: i32| {
            TestWorld::new()
                .with_money(money)
                .set_variable("rounds", GuardValue::from_number(0.0))
        };
        agree(entries(), &purse(12), 4, true);

        let every_time = |mut entries: Vec<Entry>| {
            entries[1] = Entry::new(1).cost(6).links(&[2]);
            entries
        };
        agree(every_time(entries()), &purse(12), 4, false);

        agree(entries(), &purse(6), 4, false);
    }

    /// The awkward shape, against the REFERENCE WALK rather than against the forward
    /// symbolic search.
    ///
    /// Everything else here compares the two symbolic searches, which share a layout, a
    /// guard compiler and three deliberate approximations, so a fault in any of those is
    /// invisible to all of them. This asks [`crate::oracle`], which walks one state at a
    /// time and shares none of it, over the shape that has caught the most bugs in this
    /// file: a cycle, a once action inside it, a counter, and a threshold on the counter.
    ///
    /// Containment rather than equality, and in one direction only. The symbolic side may
    /// reach more, and a surplus costs precision; reaching LESS would cost a marker.
    #[test]
    fn an_awkward_shape_reaches_everything_the_reference_walk_does() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            // A cycle that counts, with the increment marked once.
            .add(
                Entry::new(1)
                    .script(r#"SetVariableValue("count", Variable["count"] +once(1))"#)
                    .links(&[1, 2, 4]),
            )
            // A threshold only a repeated increment could pass.
            .add(Entry::new(2).guard(r#"Variable["count"] >= 2"#).links(&[3]))
            .add(Entry::new(3))
            // And a branch behind a flag the cycle never sets, which nothing can open.
            .add(Entry::new(4).guard(r#"Variable["locked"]"#).links(&[5]))
            .add(Entry::new(5))
            .build();
        let symbols = graph.symbols().clone();
        // `count` is declared a NUMBER, and it has to be. The walk reads a tracked slot
        // back through the world's idea of its type - see `BoundContext::get_variable` -
        // so a counter the world has never heard of comes back as a boolean,
        // `try_as_number` refuses it, and `count >= 2` is undecidable and therefore
        // permissive. The symbolic side compiles the same comparison against the slot's
        // bits and decides it. Without this line the two disagree about the fixture rather
        // than about the once slot, which is what this test is for. de-sze.5.4 is the real
        // fix: the index does not carry declared types yet.
        let world = TestWorld::new()
            .set_variable("locked", GuardValue::from_boolean(false))
            .set_variable("count", GuardValue::from_number(0.0));

        let walk = crate::oracle::walk(&graph, node(0), &world, CAP);
        assert!(!walk.exhausted(), "the fixture should be exhaustible");

        let layout = DataLayout::for_graph(&graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        let seed = seed_of(&graph, &world, &vars).expect("room for a seed");

        for id in graph.nodes().map(|n| n.id).collect::<Vec<_>>() {
            if !walk.reached(id) {
                continue;
            }
            let backward = Backward::reaching(&graph, id, &mut compiler, &world, CAP as u32);
            assert!(
                backward.reachable_from(node(0), &seed),
                "the walk got to {id} and the backward pass refused it",
            );
        }

        // The locked branch is out of reach both ways, which is what stops this test
        // passing vacuously by calling everything reachable.
        assert!(!walk.reached(node(5)));
        let backward = Backward::reaching(&graph, node(5), &mut compiler, &world, CAP as u32);
        assert!(
            !backward.reachable_from(node(0), &seed),
            "a branch behind a guard nothing sets should be out of reach",
        );
    }

    /// What the module claims about pruning, asserted rather than described.
    ///
    /// A slot written on the way to the target and never read by any guard must leave no
    /// trace in the backward sets: `pre_assign` selects and then forgets, so the
    /// constraint on it is erased rather than carried. Forward, the same assignment pins
    /// that slot in every set downstream.
    #[test]
    fn a_slot_nothing_reads_leaves_no_trace_in_the_backward_sets() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(
                Entry::new(1)
                    .script(r#"SetVariableValue("noise", true)"#)
                    .links(&[2]),
            )
            .add(Entry::new(2))
            .build();
        let symbols = graph.symbols().clone();

        let world = TestWorld::new();
        let layout = DataLayout::for_graph(&graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);

        let backward = Backward::reaching(&graph, node(2), &mut compiler, &world, CAP as u32);

        // Nothing reads `noise`, so every backward set is everywhere-true: the target is
        // reachable whatever the state holds.
        for id in [node(0), node(1), node(2)] {
            let set = backward
                .states_at(id)
                .expect("every entry can reach the target");
            assert!(set.valid(), "entry {id} carries a constraint it should not");
        }

        // And the whole thing is one diagram node - the constant - which is the concrete
        // form of "the slot left no trace".
        assert_eq!(backward.stats().largest_set, 1);
        let _ = symbols;
    }
}
