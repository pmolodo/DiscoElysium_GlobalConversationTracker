// SPDX-License-Identifier: MIT
//! Reachability over state SETS, without enumerating the states.
//!
//! The measurement the epic exists for. The explicit crawl keeps every `(entry, state)`
//! pair it has visited in a hash set and exhausts a 200,000-state budget in about half a
//! second on the shapes that matter; this holds one decision diagram per entry and grows
//! them until nothing changes.
//!
//! ## Explicit control, symbolic data
//!
//! The entry a crawl sits on stays an ordinary value - there are a few thousand of them
//! and they are enumerated anyway - while everything carried WITH it is symbolic. So the
//! state of the search is a map from entry to a set of data states, and a step is a
//! formula about data alone.
//!
//! That is not the textbook encoding, which would build a transition relation over primed
//! and unprimed copies of every variable and take the relational product. It is not
//! needed here: a dialogue action assigns a slot or increments it, and both are FUNCTIONS
//! of the state rather than relations, so [`ActionImage`] computes the image directly.
//! The variable count does not double and the question of how to interleave primed with
//! unprimed variables never arises.
//!
//! ## Why it terminates
//!
//! Every set only ever grows, and the lattice they live in is finite because the layout
//! is: each slot is a bounded run of bits, bounded because a counter saturates at its
//! cap. An entry is re-queued only when its set actually grew, so the loop runs at most
//! once per entry per new state and stops.
//!
//! ## What it is allowed to get wrong
//!
//! The same one-directional approximation as the guard compiler, and for the same reason:
//! `may_be_true` lets an undecided guard through, so a set here is an OVER-approximation
//! of what the crawl reaches. It may include a data state the real crawl cannot get to;
//! it may never miss one. Anything else would make the answer useless, because a missed
//! state is a missed marker.
//!
//! Money is the exception worth naming, and it is not in the layout at all - see
//! [`Reachability::unaffordable_unknown`].

use std::collections::HashMap;

use oxidd::bdd::BDDFunction;
use oxidd::{BooleanFunction, Function};

use crate::core::types::{DialogueCheckKind, DialogueNodeId, StartBranch, Ternary};
use crate::engine::engine::LookAheadEngine;
use crate::graph::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::symbolic::action_image::ActionImage;
use crate::symbolic::guard_formula::GuardCompiler;
use crate::symbolic::order::{Direction, IterationOrder, Worklist};
use crate::symbolic::vars::DataVars;
use crate::world::world::ILookAheadWorld;

/// The single data state a crawl starts in, as a set of one.
///
/// Mirrors nothing: it ENCODES [`LookAheadEngine::seed`]'s answer, so the two searches
/// cannot disagree about where they begin. Seeding is not a detail - a symbolic run
/// started from every data state walks paths that need an item the player has not got,
/// and reports entries the real crawl cannot reach.
///
/// A slot the layout is too narrow for is clamped to what it can hold rather than
/// dropped. That can only happen where the world reports a value larger than any action
/// in the group writes, and the alternative - an empty seed - would report nothing
/// reachable at all, which is the failure that looks like success.
pub fn seed_of(
    graph: &LookAheadGraph,
    world: &dyn ILookAheadWorld,
    vars: &DataVars<'_>,
) -> BDDFunction {
    let state = LookAheadEngine::seed(graph, world);
    let mut set = vars.top();

    for slot in 0..vars.layout().slot_count() {
        let Some(ceiling) = vars.slot_ceiling(slot) else { continue };
        let value = state.get(slot).max(0) as u32;
        let Some(holds) = vars.slot_equals(slot, value.min(ceiling)) else { continue };
        set = set.and(&holds).expect("and");
    }

    set
}

/// How far a search is allowed to go, and what it should say while it goes.
///
/// A budget for the same reason the explicit crawl has one: without it a search that is
/// too slow is indistinguishable from one that has hung, and neither reports anything. A
/// partial answer with `reached_fixed_point` false is worth having - it still says the
/// entries found SO FAR are genuinely reachable, because every set only grows.
pub struct Budget {
    /// The most entries to take off the queue before giving up.
    pub steps: usize,
    /// How long to keep going.
    pub time: std::time::Duration,

    /// The most memory the diagram may hold, in bytes, or 0 for no such limit.
    ///
    /// The counterpart of the forward crawl's memory budget, and the reason both exist: a
    /// search that gives up after 500,000 steps and one that gives up after 200,000 states
    /// cannot be compared, because neither number says what either search SPENT. Two
    /// searches held to the same number of bytes can be. See de-e23q.
    ///
    /// CHECKED ON THE PROGRESS CADENCE rather than every step, because asking the manager
    /// its node count is not free and a budget that costs more than it saves is not a
    /// saving. So the diagram can overshoot by up to `report_every` steps' worth of growth -
    /// the same bargain the forward crawl makes with its clock.
    pub memory: usize,
    /// Called every `report_every` steps with the step count, entries reached, the
    /// diagram nodes held in total, the LARGEST single set, and the BYTES the manager is
    /// using.
    ///
    /// The bytes are the last argument and the only one that can be read against
    /// [`Self::memory`], because they are the same quantity the check above uses. The node
    /// sums are not: they count what the entry sets REFERENCE, and the manager holds more
    /// than that - nodes not yet collected, and the apply cache. On conversation 362 the
    /// sets summed to ten million nodes while the manager held thirty-eight million, so a
    /// progress line built from the sums reads a quarter of the true figure and a row looks
    /// far from a budget it is about to spend.
    ///
    /// Of the two sums, the LARGEST SINGLE SET is the one to watch. The total sums each
    /// entry's set separately, so it climbs both when sets get harder and merely when more
    /// entries have one, and those are different problems - the first says the
    /// representation is failing, the second only says the search is making progress.
    #[allow(clippy::type_complexity)]
    pub on_progress: Option<Box<dyn Fn(usize, usize, usize, usize, usize)>>,
    pub report_every: usize,

    /// The least wall time between two calls to [`Self::on_progress`]; zero for none.
    ///
    /// ## Why a second cadence, when `report_every` is already one
    ///
    /// Because the two are paid for differently and wanted at different rates. Asking
    /// whether the memory budget is spent is cheap and wants asking OFTEN, since everything
    /// between two asks is overshoot. Gathering the report is not: it walks every entry's
    /// set for a node count, which is O(entries) and grows with them.
    ///
    /// With one cadence those trade against each other, and a step count cannot settle it
    /// anyway - twenty thousand steps is a moment early on and minutes once the sets are
    /// large, so a run either says nothing for half an hour or floods the log at the start.
    /// A CLOCK is the thing a person watching actually wants, so the step count decides how
    /// often the question is asked and this decides how often it is answered.
    ///
    /// Zero keeps the old behaviour exactly: report on every cadence hit.
    pub report_gap: std::time::Duration,

    /// The wall time [`Self::report_every`] should aim at, retuning itself to hold it;
    /// zero to leave the step count fixed.
    ///
    /// ## Why a step count cannot be set by hand
    ///
    /// Because what a step costs moves by orders of magnitude WITHIN one search. Early on
    /// the sets are small and twenty thousand steps go by in a moment; once they are large
    /// the same twenty thousand take minutes. So any fixed number is both far too often at
    /// the start and far too rare at the end, and the end is when somebody is watching.
    ///
    /// This makes the count follow the clock instead: after each check the interval is
    /// scaled toward this target, damped to at most a halving or a doubling each time so
    /// one unusually slow step cannot collapse it.
    ///
    /// IT ALSO BOUNDS THE MEMORY OVERSHOOT IN TIME RATHER THAN IN STEPS, which is the more
    /// useful guarantee: the budget can be overspent by about this long's worth of growth,
    /// whatever the sets happen to cost at that moment.
    pub check_gap: std::time::Duration,

    /// Called with each entry as it is taken off the queue, for a measurement that wants
    /// the shape of the work rather than its total.
    ///
    /// EXISTS TO TELL TWO FAILURES APART, which the totals cannot. A search doing 14,000
    /// steps over 960 entries might be re-propagating the same handful of entries
    /// thousands of times - which SCC-ordered iteration fixes - or stepping each entry a
    /// dozen times where every step is expensive, which it does not. Only the per-entry
    /// distribution separates them, and de-3x76.3 should not be built before it is known
    /// which one is happening.
    ///
    /// None in every production path, where it costs an `is_some` per step.
    #[allow(clippy::type_complexity)]
    pub on_step: Option<Box<dyn Fn(DialogueNodeId)>>,

    /// The fraction of the MACHINE's memory to leave free, or zero for no such guard.
    ///
    /// ## Why this is needed when the store is preallocated
    ///
    /// It looks unnecessary, because the node store is one allocation made before the
    /// search starts (de-0a3a) and cannot grow - so there appears to be nothing left to
    /// watch. There is. de-mnrb measured what a node really costs and found the unique
    /// table is 15.7 bytes of it, growing as nodes are inserted: about forty per cent of a
    /// full manager is allocated DURING the search, not before it.
    ///
    /// That growth is unbounded by anything except [`Self::memory`], which is a budget
    /// rather than a fact about the machine. Another process can take the memory this one
    /// was counting on, and the failure is not a graceful one - it is an allocation that
    /// aborts, which the plugin cannot catch and which takes the game with it.
    ///
    /// The forward crawl has had this guard since d43670d and it measured at about no cost
    /// (-0.1 per cent), so the same reserve is used here rather than a second number.
    pub system_reserve: f64,
    /// Stop as soon as this says yes about an entry the search has just reached.
    ///
    /// THE MOST IMPORTANT KNOB HERE, and the one the first measurement lacked. The
    /// look-ahead never wants the reachable data states; it wants to know whether an
    /// unseen entry can be reached, and `LookAheadEngine::evaluate` already returns the
    /// instant it sees one worth the maximum score. Computing a fixed point over the data
    /// answers a far harder question that nobody asked - on conversation 368 the reachable
    /// ENTRIES stopped changing at fifteen thousand steps while the diagrams went on
    /// doubling, so everything after that was wasted.
    ///
    /// Called once per entry, when it is first reached.
    #[allow(clippy::type_complexity)]
    pub halt_on: Option<Box<dyn Fn(DialogueNodeId) -> bool>>,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            steps: 2_000_000,
            time: std::time::Duration::from_secs(120),
            // THE SAME NUMBER THE FORWARD CRAWL GETS, which is the point of stating either
            // of them in bytes: two searches held to the same allowance can be compared,
            // and a step count against a state count cannot. See de-e23q.
            memory: crate::engine::engine::DEFAULT_MEMORY_BUDGET,
            on_progress: None,
            report_every: 20_000,
            report_gap: std::time::Duration::ZERO,
            check_gap: std::time::Duration::ZERO,
            on_step: None,
            system_reserve: crate::engine::system_memory::DEFAULT_RESERVE,
            halt_on: None,
        }
    }
}

/// What a fixed point cost to reach.
#[derive(Debug, Clone, Default)]
pub struct ReachabilityStats {
    /// Whether the search finished, or stopped because it ran out of budget.
    ///
    /// The most important field here. Everything else describes a set; this says whether
    /// the set is the whole answer or a lower bound on it. A search that HALTED is
    /// complete for the question it was asked even though this is false - see
    /// [`Self::halted_at`].
    pub reached_fixed_point: bool,
    /// The entry whose arrival stopped the search, if the halt condition fired.
    ///
    /// Its presence is a positive answer, and the strongest kind: the entry is reachable
    /// and here is the one that proves it. A search that halts has done no less work than
    /// the question needed, however far short of a fixed point it stopped.
    pub halted_at: Option<DialogueNodeId>,
    /// How long it ran.
    pub elapsed: std::time::Duration,
    /// How many times an entry was taken off the queue.
    pub steps: usize,
    /// How many times a child's set actually grew.
    pub widenings: usize,
    /// Entries whose set is not empty.
    pub entries_reached: usize,
    /// Diagram nodes across every entry's set, counted with sharing inside each set but
    /// not between them - so an upper bound on what the whole frontier costs.
    pub diagram_nodes: usize,
    /// The largest single entry's set.
    pub largest_set: usize,
    /// Whether the diagram manager ran out of nodes.
    ///
    /// A real outcome and not a crash, which is why it is reported rather than left to
    /// `expect`. It says the representation did not fit, which is the most decisive thing
    /// a measurement of a representation can say - and a run that dies on an unwrap says
    /// the same thing while destroying the numbers that would have shown how it got there.
    pub out_of_memory: bool,
    /// Whether the MACHINE ran out, which is the opposite kind of answer.
    ///
    /// [`Self::out_of_memory`] says the search spent the allowance it was given: a result,
    /// and about the representation. This says the machine could not supply what the search
    /// was still entitled to ask for, which says nothing about the representation at all -
    /// the row is not a measurement and wants running again with the memory free.
    ///
    /// Flattening the two into one word is how a gap in a table gets read as a finding
    /// (de-e33h), so they are separate fields and separate verdicts.
    pub out_of_system_memory: bool,
    /// Cost checks that could not be decided because money is not in the layout.
    pub unaffordable_unknown: usize,
    /// Actions skipped because the layout does not carry what they touch.
    pub actions_ignored: usize,
}

/// The reachable data states, one set per entry.
pub struct Reachability<'a> {
    vars: &'a DataVars<'a>,
    sets: HashMap<DialogueNodeId, BDDFunction>,
    stats: ReachabilityStats,
}

impl<'a> Reachability<'a> {
    /// Runs the fixed point from `start`, seeded with `seed` as its set of data states.
    ///
    /// `seed` is the set the crawl begins in - normally the single state the world seeds,
    /// but any set will do, which is what makes this usable for "everything reachable from
    /// anywhere in this group".
    pub fn explore(
        graph: &LookAheadGraph,
        start: DialogueNodeId,
        seed: &BDDFunction,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
    ) -> Self {
        Self::explore_within(
            graph, start, seed, compiler, world, counter_cap, &Budget::default(),
        )
    }

    /// The same, under a budget that says when to stop trying.
    #[allow(clippy::too_many_arguments)]
    pub fn explore_within(
        graph: &LookAheadGraph,
        start: DialogueNodeId,
        seed: &BDDFunction,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
        budget: &Budget,
    ) -> Self {
        let order = IterationOrder::of(graph);
        Self::explore_knowing(graph, start, seed, compiler, world, counter_cap, budget, &order)
    }

    /// What entering `start` by one outcome leaves, without exploring anything.
    ///
    /// THE STATE THE OUTCOME HANDS ON, which is what a search about that outcome is really
    /// seeded with: its links are walked from here, and everything past them knows nothing
    /// about the roll except what this carries.
    ///
    /// The backward driver is what wants it. Its sets say "arriving HERE, the target is
    /// reachable", and a check's set unions both ways in - so asking it about the check
    /// answers about either roll. Asked instead about the check's children, with this, it
    /// answers about one. See `novelty_search::best_novelty`.
    #[allow(clippy::too_many_arguments)]
    pub fn entry_states(
        graph: &LookAheadGraph,
        start: DialogueNodeId,
        branch: StartBranch,
        seed: &BDDFunction,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
    ) -> BDDFunction {
        let vars = compiler.vars();
        let mut image = ActionImage::new(vars, counter_cap);
        let mut this = Self {
            vars,
            sets: HashMap::new(),
            stats: ReachabilityStats::default(),
        };

        match graph.get(start) {
            Some(node) => this.enter_branch(node, branch, seed, compiler, world, &mut image),
            None => vars.bottom(),
        }
    }

    /// The same, exploring ONE OUTCOME of a start that rolls.
    ///
    /// A white or red check is entered in two ways, and the mod draws its halves apart -
    /// see [`StartBranch`] and de-8hh2.6. Everything after the start's own entry is
    /// identical: the branch selects which of the two cases entering the check leaves, and
    /// the search carries on from there knowing nothing about how it began.
    ///
    /// [`StartBranch::Either`] is the ordinary search and takes both, so
    /// [`Self::explore_within`] is this with `Either` and no caller has to say so.
    #[allow(clippy::too_many_arguments)]
    pub fn explore_branch_within(
        graph: &LookAheadGraph,
        start: DialogueNodeId,
        branch: StartBranch,
        seed: &BDDFunction,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
        budget: &Budget,
    ) -> Self {
        let order = IterationOrder::of(graph);
        Self::explore_branch_knowing(
            graph, start, branch, seed, compiler, world, counter_cap, budget, &order,
        )
    }

    /// The same, given an order worked out for this group already.
    ///
    /// For a caller that runs several searches over one graph: the order depends on nothing
    /// but the links, so building it per search is waste rather than a difference. See
    /// [`IterationOrder`] for what it is and why one of them serves this search and the
    /// backward one both. There is no unordered form of this - an order changes no settled
    /// answer, so there would be nothing for a caller to choose.
    #[allow(clippy::too_many_arguments)]
    pub fn explore_knowing(
        graph: &LookAheadGraph,
        start: DialogueNodeId,
        seed: &BDDFunction,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
        budget: &Budget,
        order: &IterationOrder,
    ) -> Self {
        Self::explore_branch_knowing(
            graph, start, StartBranch::Either, seed, compiler, world, counter_cap, budget,
            order,
        )
    }

    /// One outcome of a rolled start, given an order worked out for this group already.
    #[allow(clippy::too_many_arguments)]
    pub fn explore_branch_knowing(
        graph: &LookAheadGraph,
        start: DialogueNodeId,
        branch: StartBranch,
        seed: &BDDFunction,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
        budget: &Budget,
        order: &IterationOrder,
    ) -> Self {
        let vars = compiler.vars();
        let mut image = ActionImage::new(vars, counter_cap);
        let mut this = Self {
            vars,
            sets: HashMap::new(),
            stats: ReachabilityStats::default(),
        };

        // Entering the start node is a step like any other, so the seed is what arrives
        // AT it rather than what leaves it - and where the start ROLLS, which of the two
        // ways in this search is about.
        let Some(start_node) = graph.get(start) else { return this };
        let entered = this.enter_branch(start_node, branch, seed, compiler, world, &mut image);
        if !entered.satisfiable() {
            return this;
        }

        // What has been reached, and what has not yet been pushed onward. Propagating
        // only the DELTA is what makes this finish: entering a node DISTRIBUTES OVER
        // UNION - the guard is a conjunction, the image quantifies and asserts, and every
        // branch of `enter` unions its cases - so the image of the whole set is the image
        // of what was already sent plus the image of what is new. Sending the whole set
        // every time recomputes the first half at every visit, and on a group of four
        // thousand entries that is the difference between minutes and not finishing.
        let mut frontier: HashMap<DialogueNodeId, BDDFunction> = HashMap::new();
        this.sets.insert(start, entered.clone());
        frontier.insert(start, entered);

        // The start node counts as reached, so a halt condition it satisfies must fire
        // here rather than being missed for having arrived before the loop.
        if let Some(halt) = &budget.halt_on {
            if halt(start) {
                this.stats.halted_at = Some(start);
                this.stats.actions_ignored = image.ignored();
                this.finish();
                return this;
            }
        }

        // FROM THE NEAR END OF THE ORDER. The rank puts a component below everything it can
        // reach, and this pass travels the links forwards - so taking the lowest rank first
        // means an entry is popped once with every arm of a join already folded into what is
        // pending, rather than once per arm as they arrive.
        let mut queue = Worklist::new(order, Direction::Forward);
        queue.push(start);

        let began = std::time::Instant::now();
        let mut last_report = began;
        let mut ran_out = false;

        // THE CADENCE RETUNES ITSELF. See `Budget::check_gap`: a step is microseconds early
        // on and seconds once the sets are large, so a fixed count cannot hold a time
        // target. `cadence` is steps between checks and moves toward whatever holds
        // `check_gap`; a countdown rather than a modulo, because the divisor changes.
        let mut cadence = budget.report_every.max(1);
        let mut until_check = cadence;
        let mut last_check = began;

        // THE MACHINE'S OWN LIMIT, watched on the same cadence as the budget's. See
        // `Budget::system_reserve`: the unique table grows during the search, so there is
        // real allocation to guard even though the node store is not.
        //
        // Built with an interval of one because the cadence above is already the throttle -
        // it retunes toward `check_gap`, about a second - so a reading here is a syscall a
        // second rather than the crawl's every four thousand states. None means the
        // platform cannot be asked, which turns the guard off rather than faking it.
        let mut runway = if budget.system_reserve > 0.0 {
            crate::engine::system_memory::Runway::every(budget.system_reserve, 1)
        } else {
            None
        };
        let mut charged = this.vars.memory_used();

        'search: while let Some(id) = queue.pop() {
            // Take the pending states and leave nothing behind. An entry can be queued
            // more than once before it is reached, and the second visit has nothing to do.
            let delta = match frontier.insert(id, vars.bottom()) {
                Some(pending) if pending.satisfiable() => pending,
                _ => continue,
            };

            this.stats.steps += 1;
            if let Some(watch) = &budget.on_step {
                watch(id);
            }

            until_check -= 1;
            if until_check == 0 {
                until_check = cadence;

                if !budget.check_gap.is_zero() {
                    // Aim the next run of steps at `check_gap`, damped so one slow step
                    // cannot collapse the cadence to nothing and one fast stretch cannot
                    // send it somewhere it will never come back from. Halve or double at
                    // most, and never leave the range where the check is worth making.
                    let took = last_check.elapsed();
                    last_check = std::time::Instant::now();
                    let wanted = if took.is_zero() {
                        cadence * 2
                    } else {
                        let scale = budget.check_gap.as_secs_f64() / took.as_secs_f64();
                        (cadence as f64 * scale.clamp(0.5, 2.0)) as usize
                    };
                    cadence = wanted.clamp(1, 1_000_000);
                    until_check = cadence;
                }

                let used = this.vars.memory_used();

                if budget.memory > 0 && used >= budget.memory {
                    this.stats.out_of_memory = true;
                    ran_out = true;
                    break;
                }

                // CHECKED SECOND, so that a search which has spent its own allowance is
                // reported as having spent it. The two mean opposite things and only one
                // of them is a result.
                if let Some(runway) = runway.as_mut() {
                    let since = used.saturating_sub(charged) as u64;
                    charged = used;
                    if runway.is_low(since) {
                        this.stats.out_of_system_memory = true;
                        ran_out = true;
                        break;
                    }
                }

                if let Some(report) = &budget.on_progress {
                    // The gathering below is the expensive half, so the clock is consulted
                    // before it rather than inside the callback. See `Budget::report_gap`.
                    if last_report.elapsed() >= budget.report_gap {
                        last_report = std::time::Instant::now();
                        let sizes: Vec<usize> =
                            this.sets.values().map(|s| s.node_count()).collect();
                        report(
                            this.stats.steps,
                            this.sets.len(),
                            sizes.iter().sum(),
                            sizes.iter().copied().max().unwrap_or(0),
                            this.vars.memory_used(),
                        );
                    }
                }
            }

            if this.stats.steps >= budget.steps || began.elapsed() >= budget.time {
                ran_out = true;
                break;
            }

            let Some(node) = graph.get(id) else { continue };

            for &child_id in &node.links {
                let Some(child) = graph.get(child_id) else { continue };
                let arriving = this.enter(child, &delta, compiler, world, &mut image);
                // The image gave up for want of nodes, so what it just returned is the
                // image of nothing in particular and everything after it would be built
                // on that. Stop here and say so.
                if image.out_of_memory() {
                    this.stats.out_of_memory = true;
                    break 'search;
                }
                if !arriving.satisfiable() {
                    continue;
                }

                let known = this.sets.get(&child_id).cloned().unwrap_or_else(|| vars.bottom());
                // Only what is genuinely new. Diagrams are canonical for a fixed variable
                // order, so this difference being empty is exactly "nothing changed" -
                // there is no membership test to do and no approximation in the check.
                //
                // Every step from here can run the manager out of nodes, and on the big
                // groups it does. That is an ANSWER - the representation did not fit -
                // so it is reported rather than unwrapped, and the numbers showing how it
                // got there survive.
                let Ok(complement) = known.not() else {
                    this.stats.out_of_memory = true;
                    break 'search;
                };
                let Ok(fresh) = arriving.and(&complement) else {
                    this.stats.out_of_memory = true;
                    break 'search;
                };
                if !fresh.satisfiable() {
                    continue;
                }

                this.stats.widenings += 1;
                // Newly reached, as opposed to newly widened: the halt condition is about
                // whether an entry can be reached at all, so it is asked once, the first
                // time the entry has any states.
                let first_sighting = !known.satisfiable();
                let Ok(widened) = known.or(&fresh) else {
                    this.stats.out_of_memory = true;
                    break 'search;
                };
                this.sets.insert(child_id, widened);

                let pending = frontier
                    .get(&child_id)
                    .cloned()
                    .unwrap_or_else(|| vars.bottom());
                let Ok(waiting) = pending.or(&fresh) else {
                    this.stats.out_of_memory = true;
                    break 'search;
                };
                frontier.insert(child_id, waiting);
                queue.push(child_id);

                if first_sighting {
                    if let Some(halt) = &budget.halt_on {
                        if halt(child_id) {
                            this.stats.halted_at = Some(child_id);
                            break 'search;
                        }
                    }
                }
            }
        }

        this.stats.actions_ignored = image.ignored();
        // EVERY WAY OF STOPPING EARLY, not just the budget. This read `!ran_out`, and
        // `ran_out` is set only by the step and time budgets - so a run that exhausted the
        // diagram manager, or one stopped by `halt_on`, broke out of the loop above and then
        // reported that it had SETTLED.
        //
        // THAT IS A SOUNDNESS BUG AND NOT A REPORTING ONE. `Known::with_forward` copies this
        // into `forward_settled`, and a settled forward run is the one thing allowed to
        // REFUSE a state: `Known::restricted` intersects a backward pass against it. Pruning
        // against a set that merely stopped growing early removes states the crawl can
        // genuinely reach, which is how a marker gets lost. It is latent today only because
        // pruning is off by default. `Backward` has always got this right.
        //
        // Caught on conversation 14, which reported a fixed point over 893 entries where a
        // run that admitted to being incomplete had reached 905 - and then panicked on the
        // next diagram operation, out of nodes. A settled run is a superset of every partial
        // one over the same graph and seed, so that pair cannot both be true.
        this.stats.reached_fixed_point =
            !ran_out && !this.stats.out_of_memory && this.stats.halted_at.is_none();
        this.stats.elapsed = began.elapsed();
        this.finish();
        this
    }

    /// The data states that entering `node` from `states` can leave the crawl in.
    ///
    /// Mirrors `LookAheadEngine::enter`, which is the requirement rather than a nicety: a
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
            states.and(&may_be_true).expect("and")
        };
        if !allowed.satisfiable() {
            return self.vars.bottom();
        }

        let allowed = self.affordable(node, &allowed);
        if !allowed.satisfiable() {
            return self.vars.bottom();
        }

        let (success, failure) = self.rolled_cases(node, &allowed, image);
        match branch {
            StartBranch::Pass => success,
            StartBranch::Fail => failure,
            StartBranch::Either => success.or(&failure).expect("or"),
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
            states.and(&may_be_true).expect("and")
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
                self.charge(node, &fresh, image)
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
                self.rolled(node, &allowed, image)
            }

            DialogueCheckKind::Passive => {
                let passes = world.check_passes(node.id);
                let mut result = self.vars.bottom();
                if passes != Ternary::False {
                    result = self.charge(node, &allowed, image);
                }
                // The failing branch passes the incoming state through UNCHARGED - the one
                // branch in the engine that does not go through `charge`, so it must not
                // go through it here either.
                if passes != Ternary::True {
                    result = result.or(&allowed).expect("or");
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
        states: &BDDFunction,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        let (success, failure) = self.rolled_cases(node, states, image);
        success.or(&failure).expect("or")
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
        states: &BDDFunction,
        image: &mut ActionImage<'a>,
    ) -> (BDDFunction, BDDFunction) {
        // A check already passed is closed, and one already failed is closed too - neither
        // kind can be retried once the roll has been recorded.
        let mut open = states.clone();
        if let Some(passed) = self.flag(node.flag_slot) {
            open = open.and(&passed.not().expect("not")).expect("and");
        }
        if let Some(failed) = self.flag(node.failed_flag_slot) {
            open = open.and(&failed.not().expect("not")).expect("and");
        }

        if !open.satisfiable() {
            return (self.vars.bottom(), self.vars.bottom());
        }

        let entered = self.charge(node, &open, image);

        // Success raises the pass flag.
        let success = match node.flag_slot {
            slot if slot >= 0 => image.assign(&entered, slot as usize, 1),
            _ => entered.clone(),
        };

        // Failure: both kinds record it where there is a flag to record it with, and a
        // white check without one leaves the state alone, so only that case is retryable.
        let failure = if node.failed_flag_slot >= 0 {
            image.assign(&entered, node.failed_flag_slot as usize, 1)
        } else if node.kind == DialogueCheckKind::White {
            entered
        } else {
            self.vars.bottom()
        };

        (success, failure)
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

        // A cost paid once records that in its own slot. The money itself is not in the
        // layout, so what is modelled is the RECORD of having paid - which is what stops
        // a loop charging twice, and is the half that affects reachability.
        if node.is_cost_option() && node.cost_once && node.once_slot >= 0 {
            current = image.assign(&current, node.once_slot as usize, 1);
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
        // increment inside a loop climbed to the counter cap. The explicit crawl does
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

        let fresh = current.and(&already.not().expect("not")).expect("and");
        let spent = current.and(&already).expect("and");

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
            result = result.or(&acted).expect("or");
        }

        result
    }

    /// The states in which this node has not been seen.
    fn unseen(&self, node: &LookAheadNode, states: &BDDFunction) -> BDDFunction {
        match self.flag(node.seen_slot) {
            Some(seen) => states.and(&seen.not().expect("not")).expect("and"),
            None => states.clone(),
        }
    }

    /// Which states can afford this node.
    ///
    /// MONEY IS NOT IN THE LAYOUT, so for a node with a cost this cannot be decided, and
    /// the permissive answer is the only safe one: refusing would prune a branch a richer
    /// path opens, and this may only over-approximate. Counted rather than passed over
    /// silently, because an affordability check that never refuses is a difference from
    /// the explicit crawl that should be visible in the numbers.
    fn affordable(&mut self, node: &LookAheadNode, states: &BDDFunction) -> BDDFunction {
        if node.is_cost_option() {
            self.stats.unaffordable_unknown += 1;
        }

        states.clone()
    }

    /// A slot's "is set" formula, for a slot number that may be -1 for "no slot".
    fn flag(&self, slot: i32) -> Option<BDDFunction> {
        let slot = usize::try_from(slot).ok()?;
        self.vars.slot_is_set(slot)
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

    /// Totals that can only be taken once the sets have stopped moving.
    fn finish(&mut self) {
        self.stats.entries_reached = self.sets.len();
        self.stats.diagram_nodes = self.sets.values().map(|s| s.node_count()).sum();
        self.stats.largest_set =
            self.sets.values().map(|s| s.node_count()).max().unwrap_or(0);
    }

    /// The entries the crawl can reach.
    pub fn entries(&self) -> impl Iterator<Item = DialogueNodeId> + '_ {
        self.sets.keys().copied()
    }

    /// The data states reachable at one entry.
    pub fn states_at(&self, node: DialogueNodeId) -> Option<&BDDFunction> {
        self.sets.get(&node)
    }

    pub fn stats(&self) -> &ReachabilityStats {
        &self.stats
    }
}

#[cfg(test)]
mod branch_tests {
    use super::*;
    use crate::core::guard_value::GuardValue;
    use crate::symbolic::budget::DiagramBudget;
    use crate::symbolic::data_layout::DataLayout;
    use crate::symbolic::vars::DataVars;
    use crate::test_graph::{node, Entry, GraphBuilder};
    use crate::world::test_world::TestWorld;

    const CAP: i32 = 16;

    /// 0 is a white check. Passing opens 1 and 2 beyond it; failing opens 3.
    fn check() -> LookAheadGraph {
        GraphBuilder::new()
            .add(Entry::new(0).kind(DialogueCheckKind::White).flag("roll").links(&[1, 3]))
            .add(Entry::new(1).guard(r#"Variable["roll"] == true"#).links(&[2]))
            .add(Entry::new(2))
            .add(Entry::new(3).guard(r#"Variable["roll"] == false"#))
            .build()
    }

    /// Which entries the search reached, from the given outcome of the start.
    fn reached(graph: &LookAheadGraph, branch: StartBranch) -> Vec<i32> {
        let world = TestWorld::new().set_variable("roll", GuardValue::from_boolean(false));
        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        let seed = seed_of(graph, &world, &vars);

        let found = Reachability::explore_branch_within(
            graph,
            node(0),
            branch,
            &seed,
            &mut compiler,
            &world,
            CAP as u32,
            &Budget::default(),
        );

        let mut entries: Vec<i32> = found
            .entries()
            .filter(|id| {
                found.states_at(*id).map(|states| states.satisfiable()).unwrap_or(false)
            })
            .map(|id| id.entry_id)
            .collect();
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
