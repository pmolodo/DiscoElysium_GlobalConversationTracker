// SPDX-License-Identifier: MIT
//! Work in from the start, then in from the target, and let the halves share what they find.
//!
//! ## The backward driver is the last word
//!
//! There is no third stage. An unsettled backward driver is the END of the search, and what
//! it found is reported as what it is - a LOWER BOUND, with [`Answered::Partly`] saying so.
//! Every class it refused, it refused completely; an unasked candidate might have carried a
//! better one.
//!
//! A state-at-a-time search could be run here instead, and would answer some groups this
//! one gives up on. Nothing measured says which: the two lower bounds would differ only in
//! the direction the backward half is already better at, and paying for a second whole
//! search to find that out is what the budgets below exist to avoid.
//!
//! ## Why a portfolio rather than a choice
//!
//! The two searches are each better on different conversations, and the measurement in
//! de-sze.14.4 says nothing cheap tells them apart in advance. Asked for one unseen entry
//! deep in each group:
//!
//! ```text
//!   conv  entries wholems   medms   maxms  mednodes  maxnodes   afford
//!    368     4724     491     247   78798     60693    362652        1
//!    631     4514     702       7     392      3942     29372      100
//!     14     3594     680     100   11167     13462    110128        6
//!     28     2186     503       1       9        33      9105      503
//!   1030     1476       1     121    6592      4911    336934        0
//! ```
//!
//! `wholems` is one pass over the WHOLE group, which is what a per-candidate search is
//! priced against: it answers about every entry at once, so a driver that asks about enough
//! candidates one at a time eventually costs more than it. `afford` is how many candidates
//! that buys. Conversation 631 affords a hundred; 368 affords one. The
//! largest strongly connected component does not separate them - 368 is 27% and 1030 is
//! 93% and both lose, while 631 at 84% wins - and neither does entry count, since 368 and
//! 631 are within five per cent of each other in size and differ by thirty-five times in
//! cost. What tracks it is how big the diagrams get, which is a fact about the answer and
//! so no use for choosing before running.
//!
//! So this does not choose. It spends a slice going forwards, hands what that reached to
//! the backward driver, and takes what the backward driver returns - settled, or as the
//! lower bound it is. The group that would have been slow pays its budget and says so,
//! rather than paying it and then paying for a whole search as well.
//!
//! ## What the budget is protecting against
//!
//! The tail, not the median. Conversation 368's median target takes 247ms and its worst
//! takes 78.8 SECONDS. A median-shaped budget would be far too tight for the groups that
//! win and far too loose for the ones that lose; what makes this work is that a group
//! which is going to be slow is usually slow immediately.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use oxidd::bdd::BDDFunction;

use crate::core::types::{DialogueNodeId, Novelty, StartBranch};
use crate::graph::graph::LookAheadGraph;
use crate::symbolic::guard_formula::GuardCompiler;
use crate::symbolic::known::GroupShape;
use crate::symbolic::live_slots::LiveSlots;
use crate::symbolic::novelty_search::{self, StoppedBy};
use crate::symbolic::reachability::{self, Reachability};
use crate::world::world::ILookAheadWorld;

/// Which search produced an answer, and whether it is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answered {
    /// The forward pass reached an entry worth reporting and stopped there.
    Forwards,
    /// The backward driver settled within its budget, so the answer is exact.
    Backwards,
    /// The backward driver did not settle, so [`PortfolioAnswer::best`] is a LOWER BOUND.
    ///
    /// The classes it refused it refused completely, but a candidate it never reached
    /// might have carried a better one. Nothing runs after this - see the module note.
    Partly,
}

/// What the portfolio found.
#[derive(Debug, Clone)]
pub struct PortfolioAnswer {
    pub best: Novelty,
    pub by: Answered,
    /// The entry that proved it, when the backward search is what proved it.
    pub witness: Option<DialogueNodeId>,
    /// How many candidates the backward driver asked about before stopping.
    pub targets_asked: usize,
    /// Why the backward driver stopped, or `Nothing` when the slice answered first.
    ///
    /// Carried out rather than reduced to [`Answered::Partly`], because a caller reporting
    /// an incomplete answer has to say WHICH ration ran out - the candidates, the clock, or
    /// a single pass that could not finish - and those want different responses.
    pub stopped_by: StoppedBy,
    pub elapsed: Duration,
}

/// How long each half of the search gets.
pub struct Budget {
    /// The forward slice run BEFORE the backward driver, or zero to skip it.
    ///
    /// SMALL, AND IT EARNS ITS PLACE TWO WAYS. A forward pass halts the moment it reaches
    /// an entry worth reporting - `Reachability::Budget::halt_on` - so on the common shapes
    /// it answers outright in under a millisecond and nothing else runs. Where it does not,
    /// what it reached is not thrown away: it is handed to the backward driver, and a
    /// backward pass that MEETS it stops there having proved the target reachable.
    ///
    /// The meet is sound from a slice that was cut off, which is what makes a small budget
    /// worth spending: forward sets only ever grow, so every state in a partial run is
    /// genuinely reachable and meeting one proves reachable. It is only the CONVERSE that
    /// needs a settled run, and nothing here reads a no out of the slice.
    ///
    /// So the two searches work in from both ends and share what they find, rather than one
    /// running after the other has given up.
    pub forwards: Duration,
    /// THE WALL. Everything below is an estimate; this is the one that binds.
    ///
    /// de-cluo. `LookAheadTimeBudgetMs` is documented to players as "the longest one
    /// option's look-ahead may run for" and it was not: the forward slice was spent before
    /// the backward clock started, and the candidate loop tested its clock and then allowed
    /// a whole `each` past it. At the default of 1000 a search could return at roughly 1300.
    ///
    /// The rations below are kept as what they are - estimates of what each part should
    /// need - and are narrowed to the time actually left as the answer is assembled. So the
    /// shape of the search is unchanged where it fits, and where it does not the answer
    /// arrives when it said it would.
    pub overall: Duration,
    /// What the forward slice may hold, in bytes, and how many steps it may take.
    ///
    /// STATED RATHER THAN INHERITED, since de-xegj. The slice used to build its
    /// `reachability::Budget` with `..Default::default()`, which supplies 2,000,000 steps and
    /// `DiagramBudget::DEFAULT_MEMORY_BUDGET` - and at the player's 256 MB that COINCIDES
    /// with the manager's allowance by accident. The coincidence hides the fact that the
    /// slice has caps of its own, so a caller that raises the manager to six gigabytes gets a
    /// slice still quietly stopped at 256 MB and cannot tell.
    ///
    /// A caller that wants the slice held to the same allowance as the manager says so.
    pub slice_memory: usize,
    /// See [`Self::slice_memory`]. `usize::MAX` for no step limit.
    pub slice_steps: usize,
    /// The whole backward attempt, across every candidate.
    pub backwards: Duration,
    /// One candidate's fixed point.
    ///
    /// A CEILING FOR CALLERS THAT WANT ONE rather than a ration the search imposes on
    /// itself: by default it is the whole backward attempt, so a candidate may spend
    /// whatever the wall has left. The driver narrows it to the time actually remaining
    /// before every pass, so [`Self::overall`] binds a candidate whether this does or not.
    ///
    /// SETTING IT BELOW THE ATTEMPT BUYS A FASTER GIVE-UP AND NOTHING ELSE. The obvious
    /// reading is that it protects the candidates after a heavy one - a group whose sets
    /// explode does so on its first, and a cap would cut that one off and leave the rest
    /// their time. The driver does not work that way: an unsettled pass ends the whole
    /// attempt at [`best_novelty`]'s `StoppedBy::Incomplete`, because the answer is
    /// already a lower bound that no later candidate improves. So a cap decides how long
    /// the search takes to give up, not how many candidates it reaches.
    ///
    /// Two callers still want one. [`crate::bridge::LookAheadRequest::state_budget`] sets
    /// it to zero, which is how a test provokes a search that can establish nothing; and
    /// the measurement columns state their own, since a column is only readable against a
    /// ration it names.
    pub each: Duration,
    /// Whether a SETTLED forward run may narrow the backward passes told about it.
    ///
    /// ON, and self-guarding: [`Known`] narrows nothing without `forward_settled`, so a
    /// group whose slice spends [`Self::forwards`] without settling behaves exactly as it
    /// would with this off. See the note at the call site for why it was off until
    /// de-bnjy.9 and what changed.
    ///
    /// A FIELD RATHER THAN A LITERAL because it is the only way to measure what it is
    /// worth: one run cannot show a difference, and both arms have to be the shipped path
    /// rather than a hand-built search beside it.
    pub pruning: bool,

    /// The group's liveness analysis, for a forward slice that should forget what nothing
    /// onward reads; `None` to hold every set exactly.
    ///
    /// THE SLICE ONLY. The backward driver's sets say "arriving here, the target is
    /// reachable" and their approximation runs the other way, so nothing here reaches them.
    /// See [`reachability::Budget::forget_dead`] for what the abstraction is and why it
    /// cannot lose an answer.
    ///
    /// A FIELD RATHER THAN A LITERAL, for the same reason [`Self::pruning`] is one: what it
    /// is worth can only be read off two runs of the shipped path, and the two effects it
    /// has pull against each other. Smaller sets can let a slice SETTLE where it did not,
    /// which is what lets the backward passes be narrowed at all; larger sets - and an
    /// abstracted set is larger - narrow them less once it has. See
    /// `measurements/dead_quantify.rs`.
    pub forget_dead: Option<Arc<LiveSlots>>,
}

impl Budget {
    /// The same rations, held to `left` as well as to whatever they already said.
    ///
    /// ONLY [`Self::overall`] MOVES, and that is enough rather than an oversight: every
    /// other clock here is already narrowed against it where it is spent. The slice takes
    /// `forwards.min(overall)`, and the backward driver takes
    /// `backwards.min(overall - elapsed)` and narrows each candidate again to what is left
    /// of that. So one number binds them all, and narrowing the rest as well would restate
    /// the same limit in three places for a reader to keep in step.
    ///
    /// `Duration::MAX` is the identity, which is what a caller with no wall of its own
    /// passes.
    pub fn within(&self, left: Duration) -> Self {
        Self {
            forwards: self.forwards,
            overall: self.overall.min(left),
            slice_memory: self.slice_memory,
            slice_steps: self.slice_steps,
            backwards: self.backwards,
            each: self.each,
            pruning: self.pruning,
            forget_dead: self.forget_dead.clone(),
        }
    }
}

impl Default for Budget {
    fn default() -> Self {
        // A CANDIDATE GETS THE WHOLE BACKWARD ATTEMPT, so the wall is the only clock that
        // stops one. See [`Budget::each`] for why a smaller ration is not the protection it
        // looks like: the attempt ends at the first pass that fails to settle either way,
        // so cutting that pass short shortens the search without buying anything for the
        // candidates behind it.
        let backwards = Duration::from_secs(2);
        Self {
            // FIFTY MILLISECONDS, and the shape of the measurement rather than a guess.
            // A forward pass that is going to answer at all answers in under one on every
            // group measured - it halts on the first entry worth reporting - so this is not
            // sized to let it finish. It is sized to be worth the sets it leaves behind for
            // the backward half to meet, and to be small enough that spending all of it and
            // learning nothing costs a twentieth of the backward allowance.
            forwards: Duration::from_millis(50),
            // THE SLICE PLUS THE BACKWARD ATTEMPT, which is what this arrangement was always
            // meant to cost and what it now cannot exceed. Stated rather than derived so a
            // reader can see the number the answer is promised in.
            overall: Duration::from_millis(2050),
            // WHAT THE SLICE WAS ALREADY GETTING, stated instead of inherited, so this
            // default changes nothing while making the numbers visible.
            slice_memory: crate::symbolic::budget::DiagramBudget::DEFAULT_MEMORY_BUDGET,
            slice_steps: 2_000_000,
            backwards,
            each: backwards,
            // ON. `measurements/settles_within.rs` is why: at the fifty milliseconds above,
            // 119 of 120 ordinary groups settle and 25 of the 50 that span conversations
            // do, so most of the game has a settled run to narrow with and nothing was
            // using it.
            pruning: true,
            // OFF, because nothing has yet said it is worth its quantifier. See
            // `measurements/dead_quantify.rs`, which is what would change this.
            forget_dead: None,
        }
    }
}

/// One forward pass, halting on any entry of the class it was asked for.
///
/// `wanted` is the class and not a floor: an entry of a LOWER class is not what this pass
/// was sent to find, and stopping at one would answer a different question - see
/// [`best_novelty`] for where the class comes from and why halting on it is an answer.
#[allow(clippy::too_many_arguments)]
fn forwards_for<'a, F>(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    branch: StartBranch,
    seed: &BDDFunction,
    compiler: &mut GuardCompiler<'a>,
    world: &dyn ILookAheadWorld,
    counter_cap: u32,
    wanted: Novelty,
    novelty: &F,
    within: Duration,
    slice_memory: usize,
    slice_steps: usize,
    forget_dead: Option<Arc<LiveSlots>>,
    shape: &GroupShape,
) -> Reachability<'a>
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    // THE SET RATHER THAN THE CLOSURE, because `halt_on` outlives this call and cannot
    // borrow `novelty`. One pass over the entries to build it, against a search that is
    // thousands of diagram operations.
    let quarry: HashSet<DialogueNodeId> = graph
        .nodes()
        .map(|node| node.id)
        .filter(|id| novelty(*id) == wanted)
        .collect();

    // KNOWING THE ORDER RATHER THAN BUILDING ONE. The convenience form runs Tarjan over the
    // whole group itself, and a menu is a dozen starts asking for the same answer - see
    // [`GroupShape`].
    Reachability::explore_branch_knowing(
        graph,
        start,
        branch,
        seed,
        compiler,
        world,
        counter_cap,
        &reachability::Budget {
            time: within,
            // TOLD, NOT INHERITED - see `Budget::slice_memory`. What these were defaulting to
            // matched the player's manager allowance by coincidence, so nothing revealed that
            // raising the manager left the slice where it was.
            memory: slice_memory,
            steps: slice_steps,
            halt_on: Some(Box::new(move |id| quarry.contains(&id))),
            forget_dead,
            ..Default::default()
        },
        shape.order(),
    )
}

/// The best novelty reachable beyond `start`, from whichever search answers first.
///
/// `hunting` is the class the forward slice looks for, and the caller must have established
/// that it is the best class link-reachable from `start` -
/// [`LookAheadGraph::best_linked_class`] is what establishes it. A caller that passes a
/// class which is NOT the best reachable gets a lower bound where it thinks it has an
/// answer, because the slice would then be able to halt with something better still unseen.
///
/// `shape` is the group's parent map and SCC decomposition, worked out once by the caller.
/// It changes no answer - it is a fact about the links - and it exists because a response
/// menu calls this once per option and every one of them wants the same one. See
/// [`GroupShape`], and [`GroupShape::of`] for what building one per call used to cost.
#[allow(clippy::too_many_arguments)]
pub fn best_novelty<'a, F>(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    branch: StartBranch,
    seed: &BDDFunction,
    compiler: &mut GuardCompiler<'a>,
    world: &dyn ILookAheadWorld,
    counter_cap: u32,
    novelty: F,
    hunting: Novelty,
    budget: &Budget,
    shape: &GroupShape,
    memo: Option<&crate::symbolic::memo::Memo>,
) -> PortfolioAnswer
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    let began = std::time::Instant::now();

    // THE START IS A RESULT LIKE ANY OTHER, and the cheapest one there is: `hunting` is the
    // best class anything reachable carries, the start included, so a start already
    // carrying it settles the question before a diagram is touched.
    //
    // It cannot fire for an ordinary option - there the caller's baseline IS the start's
    // own class, and it refused a search that could only match it. It fires for one outcome
    // of a rolled check, whose baseline is where that outcome LANDS: a check no save has
    // displayed, opening something this save has read, outranks its own outcome.
    if hunting > Novelty::SeenThisGame && novelty(start) == hunting {
        return PortfolioAnswer {
            best: hunting,
            by: Answered::Forwards,
            witness: Some(start),
            targets_asked: 0,
            stopped_by: StoppedBy::Nothing,
            elapsed: began.elapsed(),
        };
    }

    // IN FROM THE START, FIRST, HUNTING ONE CLASS. Where it halts, that is the answer and
    // nothing else runs - which is only true because of WHICH class it hunts, and that is
    // the caller's to establish: the best class any link from here reaches, so nothing the
    // pass could have walked past outranks what it stopped at.
    //
    // PASSED IN RATHER THAN WORKED OUT HERE, because the caller has already walked the
    // links to decide whether to search at all - `bridge::class_worth_hunting` - and the
    // answer to "is anything better than the baseline reachable" and "which class should
    // the slice hunt" is the same walk over the same graph. Doing it here would be doing it
    // twice per start, once per outcome of every rolled check.
    //
    // THE FLOOR IS NEVER HUNTED. `SeenThisGame` is not a quarry - every entry that is not
    // unseen carries it, so a pass sent after it halts on the first thing it touches and
    // calls the floor an answer. A caller with nothing to hunt has nothing to search for
    // and should not be here at all, and this is what makes arriving anyway harmless.
    // NARROWED TO THE WALL, here and at the backward driver below. de-cluo: the slice used
    // to be spent before the backward clock started, so it was outside the player's number
    // entirely rather than inside it.
    let slice = budget.forwards.min(budget.overall);

    let forwards = (hunting > Novelty::SeenThisGame && !slice.is_zero()).then(|| {
        forwards_for(
            graph, start, branch, seed, compiler, world, counter_cap, hunting, &novelty,
            slice, budget.slice_memory, budget.slice_steps, budget.forget_dead.clone(), shape,
        )
    });

    if let Some(found) = &forwards {
        if let Some(halted_at) = found.stats().halted_at {
            return PortfolioAnswer {
                best: novelty(halted_at),
                by: Answered::Forwards,
                witness: Some(halted_at),
                targets_asked: 0,
                stopped_by: StoppedBy::Nothing,
                elapsed: began.elapsed(),
            };
        }
    }

    // AND IN FROM THE TARGET, TOLD WHAT THE FIRST HALF REACHED. A backward pass that meets
    // those sets has proved its target reachable and stops there - the two searches meet in
    // the middle rather than one starting over where the other gave up. See `Known`.
    //
    // WHERE THE SEARCH BEGINS IS WHAT IS HANDED OVER, and for one outcome of a rolled start
    // that is its destinations holding what entering by that outcome left - never the check
    // itself, whose pre-entry states are reachable by either roll and would let a meet
    // there prove the wrong thing.
    let from = novelty_search::Where::of(
        graph, start, branch, seed, compiler, world, counter_cap,
    );
    // AND NARROWED BY IT WHERE THE RUN SETTLED. A settled forward run says exactly what can
    // arrive at an entry, so a backward pass may intersect every pre-image against it, and
    // an entry the run never reached at all is refused without a fixed point. On
    // conversation 28 that takes the backward half from 19 ms to 2 - see de-fawk, which
    // built it, proved it against the explicit crawl in tests/backward_oracle.rs, and then
    // left it off.
    //
    // IT WAS LEFT OFF ON A READING OF FIVE GROUPS, and the reading does not survive the
    // rest of the game. The argument was that the fifty-millisecond slice above almost
    // never settles, which is true of the five heaviest groups and false everywhere else:
    // `measurements/settles_within.rs` puts it at 119 of 120 ordinary groups and 25 of the
    // 50 that span conversations. Pruning was off for all of them.
    //
    // SELF-GUARDING, which is what makes this free rather than a trade. `Known` narrows
    // nothing unless `forward_settled`, so a group that spends its budget without settling
    // behaves exactly as it did - no bound, no intersection, no cost. Nothing here raises
    // the budget; that is de-bnjy.9's other half, and it is spent per start whether or not
    // it is claimed.
    // BUILT WHETHER OR NOT A SLICE RAN, and only the last line depends on one. The graph's
    // shape is what the driver would otherwise rebuild PER CANDIDATE - the parent map and
    // the Tarjan order, once for each of forty questions about one graph - and it has
    // nothing to do with the forward run. Handing that over only when a slice happened
    // charged the sharing to the slice, so a run with `forwards` at zero paid forty walks
    // for the privilege of not searching forwards.
    let mut known = shape.known_from(graph, start).pruning(budget.pruning);
    for (id, states) in from.known_pairs() {
        known = known.from(id, states);
    }
    let known = Some(match &forwards {
        Some(found) => known.with_forward(found),
        None => known,
    });

    let backwards = novelty_search::best_novelty(
        graph,
        start,
        branch,
        seed,
        compiler,
        world,
        counter_cap,
        &novelty,
        // WHAT IS LEFT OF THE WALL, not the whole backward ration. The slice above has
        // already been spent out of it, and the driver narrows each candidate to what
        // remains of THIS in turn - so the three rations compose into one deadline rather
        // than adding up.
        &novelty_search::Budget {
            time: budget.backwards.min(budget.overall.saturating_sub(began.elapsed())),
            each: crate::symbolic::backward::Budget {
                steps: usize::MAX,
                time: budget.each,
                // Nothing watches a pass bounded by a player's wall. The hook exists for
                // measurement passes that run for minutes, which these cannot.
                ..Default::default()
            },
        },
        known.as_ref(),
        // WHAT AN EARLIER REQUEST SETTLED, where a caller holds a manager long enough for
        // there to be one. The driver decides per candidate whether a kept pass answers it
        // and whether a fresh one is worth keeping; nothing here does, because the two
        // decisions are about a target and this call is about a start.
        memo,
    );

    // SETTLED OR NOT, THIS IS THE ANSWER. A backward driver that ran out of candidates or
    // clock has established a lower bound and nothing else here improves on it - see the
    // module note. The caller is told which it is, and an unsettled answer is read as the
    // bound it is rather than as a claim that nothing is there.
    PortfolioAnswer {
        best: backwards.best,
        by: if backwards.stopped_by == StoppedBy::Nothing {
            Answered::Backwards
        } else {
            Answered::Partly
        },
        witness: backwards.witness,
        targets_asked: backwards.targets_asked,
        stopped_by: backwards.stopped_by,
        elapsed: began.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::budget::DiagramBudget;

    use std::collections::HashSet;

    use crate::core::guard_value::GuardValue;
    use crate::core::types::DialogueCheckKind;
    use crate::symbolic::data_layout::DataLayout;
    use crate::symbolic::reachability::seed_of;
    use crate::symbolic::vars::DataVars;
    use crate::test_graph::{node, Entry, GraphBuilder};
    use crate::world::test_world::TestWorld;

    const CAP: i32 = 16;

    fn run<F>(graph: &LookAheadGraph, world: &TestWorld, novelty: F, budget: &Budget)
        -> PortfolioAnswer
    where
        F: Fn(DialogueNodeId) -> Novelty,
    {
        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(world);
        let seed = seed_of(graph, world, &vars).expect("room for a seed");

        // The caller's job, as it is the bridge's: one walk of the links names the class,
        // and the search is not asked to work it out again. The floor when nothing is
        // unseen, which is a fixture the bridge would have refused before calling.
        let hunting = graph
            .best_linked_class(node(0), &novelty)
            .unwrap_or(Novelty::SeenThisGame);

        best_novelty(
            graph, node(0), StartBranch::Either, &seed, &mut compiler, world, CAP as u32,
            novelty, hunting, budget, &GroupShape::of(graph), None,
        )
    }

    /// The whole portfolio, asked about ONE OUTCOME of a rolled start.
    fn run_branch<F>(
        graph: &LookAheadGraph,
        world: &TestWorld,
        branch: StartBranch,
        novelty: F,
    ) -> PortfolioAnswer
    where
        F: Fn(DialogueNodeId) -> Novelty,
    {
        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(world);
        let seed = seed_of(graph, world, &vars).expect("room for a seed");

        // The caller's walk, as the bridge's would be: from what the outcome opens, so the
        // class hunted is the best that outcome can reach.
        let hunting = graph
            .get(node(0))
            .map(|start| start.links.clone())
            .unwrap_or_default()
            .iter()
            .filter_map(|id| graph.best_linked_class(*id, &novelty))
            .max()
            .unwrap_or(Novelty::SeenThisGame);

        best_novelty(
            graph, node(0), branch, &seed, &mut compiler, world, CAP as u32, novelty,
            hunting, &Budget::default(), &GroupShape::of(graph), None,
        )
    }

    /// BOTH HALVES OF THE PORTFOLIO ANSWER ABOUT ONE OUTCOME, and only that one.
    ///
    /// 0 is a white check; 2 lies past what passing opens and no save has read it. Failing
    /// must not find it - and the forward slice, the backward driver and the meet between
    /// them are three separate ways it could, so this asks the whole thing rather than a
    /// half of it.
    #[test]
    fn an_outcome_is_answered_from_its_own_half_of_the_check() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).kind(DialogueCheckKind::White).flag("roll").links(&[1, 3]))
            .add(Entry::new(1).guard(r#"Variable["roll"] == true"#).links(&[2]))
            .add(Entry::new(2))
            .add(Entry::new(3).guard(r#"Variable["roll"] == false"#))
            .build();
        let world = TestWorld::new().set_variable("roll", GuardValue::from_boolean(false));

        let passing = run_branch(&graph, &world, StartBranch::Pass, unseen(&[2]));
        assert_eq!(passing.best, Novelty::UnseenAnyGame);
        assert_eq!(passing.witness, Some(node(2)));

        let failing = run_branch(&graph, &world, StartBranch::Fail, unseen(&[2]));
        assert_eq!(
            failing.best,
            Novelty::SeenThisGame,
            "the pass flag is what opens 1, and failing does not set it",
        );
        assert_eq!(failing.witness, None);
    }

    /// A novelty function over both classes, for the fixtures about which one is hunted.
    fn classes<'a>(
        unseen_anywhere: &'a [i32],
        unseen_here: &'a [i32],
    ) -> impl Fn(DialogueNodeId) -> Novelty + 'a {
        move |id| {
            if unseen_anywhere.contains(&id.entry_id) {
                Novelty::UnseenAnyGame
            } else if unseen_here.contains(&id.entry_id) {
                Novelty::UnseenThisGame
            } else {
                Novelty::SeenThisGame
            }
        }
    }

    fn unseen(ids: &[i32]) -> impl Fn(DialogueNodeId) -> Novelty + '_ {
        let set: HashSet<i32> = ids.iter().copied().collect();
        move |id| {
            if set.contains(&id.entry_id) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        }
    }

    /// The forward slice answers a reachable entry outright, without the backward driver.
    #[test]
    fn the_forward_slice_answers_before_anything_else_runs() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build();

        let answer = run(&graph, &TestWorld::new(), unseen(&[2]), &Budget::default());
        assert_eq!(answer.best, Novelty::UnseenAnyGame);
        assert_eq!(answer.by, Answered::Forwards, "it halts on the unseen entry");
        assert_eq!(answer.witness, Some(node(2)));
        assert_eq!(answer.targets_asked, 0, "and no candidate was asked about");
    }

    /// THE START ANSWERS FOR ITSELF, without a diagram operation.
    ///
    /// The start carries the class being hunted, so there is nothing to search for: it is
    /// already the best anything reachable carries. Reported forwards, witnessed by the
    /// start, with no candidate asked about.
    #[test]
    fn a_start_carrying_the_hunted_class_answers_before_any_search() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1))
            .build();

        // Only the start is unseen anywhere, which is the shape a rolled check makes when
        // its outcome opens something already read - see the bridge's own test.
        let answer = run(&graph, &TestWorld::new(), classes(&[0], &[]), &Budget::default());

        assert_eq!(answer.best, Novelty::UnseenAnyGame);
        assert_eq!(answer.by, Answered::Forwards);
        assert_eq!(answer.witness, Some(node(0)), "the start is what proved it");
        assert_eq!(answer.targets_asked, 0);
    }

    /// And a start on the floor answers nothing, however starved the search is.
    #[test]
    fn a_start_on_the_floor_is_not_an_answer() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1))
            .build();

        let starved = Budget {
            forwards: Duration::ZERO,
            backwards: Duration::ZERO,
            each: Duration::ZERO,
            ..Budget::default()
        };
        let answer = run(&graph, &TestWorld::new(), classes(&[], &[]), &starved);

        assert_eq!(answer.best, Novelty::SeenThisGame);
        assert_ne!(answer.witness, Some(node(0)));
    }

    /// THE SLICE HUNTS THE BEST CLASS REACHABLE, not the first entry that beats "seen".
    ///
    /// 1 is unseen HERE and sits between the start and 2, which is unseen ANYWHERE. A pass
    /// halting on anything above the floor stops at 1 and reports unseen-here as the best
    /// there is, which is a red marker where orange is right. It has to walk past 1.
    #[test]
    fn the_slice_walks_past_a_lower_class_to_reach_the_top_rung() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build();

        let answer = run(&graph, &TestWorld::new(), classes(&[2], &[1]), &Budget::default());

        assert_eq!(answer.best, Novelty::UnseenAnyGame);
        assert_eq!(answer.by, Answered::Forwards);
        assert_eq!(answer.witness, Some(node(2)), "and 2 is what it stopped at, not 1");
    }

    /// And hunts the rung below when the top one is nowhere reachable.
    #[test]
    fn the_slice_hunts_the_lower_class_when_there_is_no_top_rung() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build();

        let answer = run(&graph, &TestWorld::new(), classes(&[], &[1, 2]), &Budget::default());

        assert_eq!(answer.best, Novelty::UnseenThisGame);
        assert_eq!(answer.by, Answered::Forwards);
        assert_eq!(answer.witness, Some(node(1)), "the nearest of the class it hunts");
    }

    /// Nothing unseen anywhere in reach: no slice runs at all, and the answer is settled.
    ///
    /// THE FLOOR IS NOT A QUARRY, which is what this fixture is really about. Every entry
    /// that is not unseen carries `SeenThisGame`, so a slice sent after it would halt on
    /// the first entry it touched and report the floor as an answer found forwards. The
    /// caller refuses a search it cannot improve on before reaching here - see
    /// `bridge::class_worth_hunting` - and this is what makes arriving anyway harmless.
    #[test]
    fn a_group_with_nothing_unseen_is_answered_without_hunting() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build();

        let answer = run(&graph, &TestWorld::new(), classes(&[], &[]), &Budget::default());

        assert_eq!(answer.best, Novelty::SeenThisGame);
        assert_eq!(answer.by, Answered::Backwards);
        assert_eq!(answer.stopped_by, StoppedBy::Nothing);
        assert_eq!(answer.witness, None);
    }

    /// The same, with the forward slice starved, so the backward driver is what answers.
    ///
    /// STARVED RATHER THAN REMOVED, because the two paths have to keep agreeing: this is
    /// the fixture above with the first half switched off, and both must reach the same
    /// verdict and name the same witness.
    #[test]
    fn a_settled_backward_answer_is_taken_as_it_stands() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build();

        let no_forward = Budget { forwards: Duration::ZERO, ..Default::default() };
        let answer = run(&graph, &TestWorld::new(), unseen(&[2]), &no_forward);
        assert_eq!(answer.best, Novelty::UnseenAnyGame);
        assert_eq!(answer.by, Answered::Backwards);
        assert_eq!(answer.witness, Some(node(2)));
    }

    /// A budget of nothing ends the search rather than handing it on.
    ///
    /// NOTHING RUNS AFTER THE BACKWARD DRIVER, so what a starved search returns is the
    /// whole answer: the floor, marked as the lower bound it is. The caller's job is to say
    /// "not established" rather than "nothing there", and `Answered::Partly` tells it which.
    #[test]
    fn an_unsettled_backward_search_reports_a_lower_bound() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build();

        // THE FORWARD SLICE IS STARVED TOO, or it would answer this fixture outright and
        // the unsettled path - which is what this test is about - would never be reached.
        let starved = Budget {
            forwards: Duration::ZERO,
            backwards: Duration::ZERO,
            each: Duration::ZERO,
            ..Budget::default()
        };
        let answer = run(&graph, &TestWorld::new(), unseen(&[2]), &starved);

        assert_eq!(answer.by, Answered::Partly);
        assert_ne!(answer.stopped_by, StoppedBy::Nothing, "and it says which ration ran out");
        assert_eq!(
            answer.best,
            Novelty::SeenThisGame,
            "the floor, because nothing was established - not a claim that 2 is unreachable",
        );
        assert_eq!(answer.witness, None);
    }

    /// A guard nothing can open is a settled NO, not a lower bound.
    ///
    /// The distinction the caller draws everything from: this answer says the floor AND
    /// says it is established, so an option is left unmarked rather than marked uncertain.
    #[test]
    fn a_settled_search_that_finds_nothing_says_so() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).guard(r#"Variable["shut"]"#).links(&[2]))
            .add(Entry::new(2))
            .build();
        let world = TestWorld::new().set_variable("shut", GuardValue::from_boolean(false));

        // Nothing is reachable, so the slice halts on nothing and the backward driver
        // refuses every candidate - completely, which is what makes this an answer.
        let answer = run(&graph, &world, unseen(&[2]), &Budget::default());
        assert_eq!(answer.best, Novelty::SeenThisGame);
        assert_eq!(answer.by, Answered::Backwards);
        assert_eq!(answer.stopped_by, StoppedBy::Nothing);
        assert_eq!(answer.witness, None);
    }
}
