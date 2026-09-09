// SPDX-License-Identifier: MIT
//! Work in from the target, under a budget, and say plainly when the budget ran out.
//!
//! ## The backward driver is the whole search, and the last word
//!
//! There is no second stage. An unsettled backward driver is the END, and what it found is
//! reported as what it is - a LOWER BOUND, with [`Answered::Partly`] saying so. Every class
//! it refused, it refused completely; an unasked candidate might have carried a better one.
//!
//! A state-at-a-time search could be run here instead, and would answer some groups this one
//! gives up on. Nothing measured says which: the two lower bounds would differ only in the
//! direction the backward driver is already better at, and paying for a second whole search
//! to find that out is what the budgets below exist to avoid.
//!
//! ## Why one search and not a choice between two
//!
//! Searching in from the start instead is a reasonable thing to want, and it was measured at
//! length. Over the whole game, 395 response menus of eight options each - which is the unit
//! a player waits for - it answered 80% of a menu's options and made the menus 2.4 times
//! slower, 22.9 seconds against 9.5. The options it answers are the EASY ones, the ones where
//! something novel is close, which is exactly why it answers them; and an option easy to
//! reach from the start is cheap to reach from the target too. So what it absorbed was never
//! work this driver would have struggled with.
//!
//! It was slower in every bucket, including the 215 menus where it answered every option, and
//! the per-option reading agreed on all 2,605 rows of a whole-game matrix. There is no group
//! where it is worth carrying, and no reading under which it wins.
//!
//! ## What the budget is protecting against
//!
//! The tail, not the median. Conversation 368's median target takes 247ms and its worst
//! takes 78.8 SECONDS. A median-shaped budget would be far too tight for the groups that
//! win and far too loose for the ones that lose; what makes this work is that a group
//! which is going to be slow is usually slow immediately.

use std::time::Duration;

use oxidd::bdd::BDDFunction;

use crate::core::types::{DialogueNodeId, Novelty, StartBranch};
use crate::graph::graph::LookAheadGraph;
use crate::symbolic::guard_formula::GuardCompiler;
use crate::symbolic::known::GroupShape;
use crate::symbolic::novelty_search::{self, StoppedBy};
use crate::world::world::ILookAheadWorld;

/// What produced an answer, and whether it is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answered {
    /// The start already carried the class being hunted, so nothing was searched.
    ///
    /// Exact, and free: no diagram was touched and no candidate asked about. Kept apart from
    /// [`Self::Backwards`] rather than folded into it because the two cost nothing alike, and
    /// a measurement that could not tell them apart would read a group full of these as a
    /// very fast search.
    AtTheStart,
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
    /// Why the backward driver stopped, or `Nothing` when it never had to run.
    ///
    /// Carried out rather than reduced to [`Answered::Partly`], because a caller reporting
    /// an incomplete answer has to say WHICH ration ran out - the candidates, the clock, or
    /// a single pass that could not finish - and those want different responses.
    pub stopped_by: StoppedBy,
    pub elapsed: Duration,
}

/// How long the search gets, and how that is divided.
pub struct Budget {
    /// THE WALL. Everything below is an estimate; this is the one that binds.
    ///
    /// de-cluo. `LookAheadTimeBudgetMs` is documented to players as "the longest one
    /// option's look-ahead may run for" and it was not: the candidate loop tested its clock
    /// and then allowed a whole `each` past it, so a dial set to 1000 could return at
    /// roughly 1300.
    ///
    /// The rations below are kept as what they are - estimates of what each part should
    /// need - and are narrowed to the time actually left as the answer is assembled. So the
    /// shape of the search is unchanged where it fits, and where it does not the answer
    /// arrives when it said it would.
    pub overall: Duration,
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
}

impl Budget {
    /// The same rations, held to `left` as well as to whatever they already said.
    ///
    /// ONLY [`Self::overall`] MOVES, and that is enough rather than an oversight: every
    /// other clock here is already narrowed against it where it is spent. The backward
    /// driver takes `backwards.min(overall - elapsed)` and narrows each candidate again to
    /// what is left of that. So one number binds them all, and narrowing the rest as well
    /// would restate the same limit in two places for a reader to keep in step.
    ///
    /// `Duration::MAX` is the identity, which is what a caller with no wall of its own
    /// passes.
    pub fn within(&self, left: Duration) -> Self {
        Self {
            overall: self.overall.min(left),
            backwards: self.backwards,
            each: self.each,
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
            // THE BACKWARD ATTEMPT, which is the whole of the search. Stated rather than
            // derived so a reader can see the number the answer is promised in.
            overall: backwards,
            backwards,
            each: backwards,
        }
    }
}

/// The best novelty reachable beyond `start`.
///
/// `hunting` is the class being looked for, and the caller must have established that it is
/// the best class link-reachable from `start` - [`LookAheadGraph::best_linked_class`] is what
/// establishes it. It is what lets a start that already carries that class answer for itself
/// without a search, and a caller that passes a class which is NOT the best reachable would
/// get that shortcut taken with something better still unfound.
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
            by: Answered::AtTheStart,
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
    // WHERE THE SEARCH BEGINS, and for one outcome of a rolled start that is its
    // destinations holding what entering by that outcome left - never the check itself,
    // whose pre-entry states are reachable by either roll and would let a meet there prove
    // the wrong thing.
    let from = novelty_search::Where::of(graph, start, branch, seed, compiler, world, counter_cap);

    // THE GRAPH'S SHAPE, WORKED OUT ONCE. This is what the driver would otherwise rebuild
    // PER CANDIDATE - the parent map and the Tarjan order, once for each of forty questions
    // about one graph - and it is a fact about the links rather than about any search, so
    // one is good for every option of a menu. See `GroupShape::of`.
    let mut known = shape.known_from(graph, start);
    for (id, states) in from.known_pairs() {
        known = known.from(id, states);
    }
    let known = Some(known);

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
            time: budget
                .backwards
                .min(budget.overall.saturating_sub(began.elapsed())),
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
    use crate::test_graph::{Entry, GraphBuilder, node};
    use crate::world::test_world::TestWorld;

    const CAP: i32 = 16;

    fn run<F>(
        graph: &LookAheadGraph,
        world: &TestWorld,
        novelty: F,
        budget: &Budget,
    ) -> PortfolioAnswer
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
            graph,
            node(0),
            StartBranch::Either,
            &seed,
            &mut compiler,
            world,
            CAP as u32,
            novelty,
            hunting,
            budget,
            &GroupShape::of(graph),
            None,
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
            graph,
            node(0),
            branch,
            &seed,
            &mut compiler,
            world,
            CAP as u32,
            novelty,
            hunting,
            &Budget::default(),
            &GroupShape::of(graph),
            None,
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

    /// A novelty function where the named entries are unseen anywhere and nothing else is.
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

    #[test]
    fn a_start_carrying_the_hunted_class_answers_before_any_search() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1))
            .build();

        // Only the start is unseen anywhere, which is the shape a rolled check makes when
        // its outcome opens something already read - see the bridge's own test.
        let answer = run(
            &graph,
            &TestWorld::new(),
            classes(&[0], &[]),
            &Budget::default(),
        );

        assert_eq!(answer.best, Novelty::UnseenAnyGame);
        assert_eq!(answer.by, Answered::AtTheStart);
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
    fn a_group_with_nothing_unseen_is_answered_without_hunting() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build();

        let answer = run(
            &graph,
            &TestWorld::new(),
            classes(&[], &[]),
            &Budget::default(),
        );

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

        let no_forward = Budget {
            ..Default::default()
        };
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
            backwards: Duration::ZERO,
            each: Duration::ZERO,
            ..Budget::default()
        };
        let answer = run(&graph, &TestWorld::new(), unseen(&[2]), &starved);

        assert_eq!(answer.by, Answered::Partly);
        assert_ne!(
            answer.stopped_by,
            StoppedBy::Nothing,
            "and it says which ration ran out"
        );
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
