// SPDX-License-Identifier: MIT
//! Ask backwards first, and fall back to the crawl when the answer does not come.
//!
//! ## Why a portfolio rather than a choice
//!
//! The two searches are each better on different conversations, and the measurement in
//! de-sze.14.4 says nothing cheap tells them apart in advance. Asked for one unseen entry
//! deep in each group:
//!
//! ```text
//!   conv  entries crawlms   medms   maxms  mednodes  maxnodes   afford
//!    368     4724     491     247   78798     60693    362652        1
//!    631     4514     702       7     392      3942     29372      100
//!     14     3594     680     100   11167     13462    110128        6
//!     28     2186     503       1       9        33      9105      503
//!   1030     1476       1     121    6592      4911    336934        0
//! ```
//!
//! `afford` is how many candidates the backward driver can be asked about before one crawl
//! would have been cheaper. Conversation 631 affords a hundred; 368 affords one. The
//! largest strongly connected component does not separate them - 368 is 27% and 1030 is
//! 93% and both lose, while 631 at 84% wins - and neither does entry count, since 368 and
//! 631 are within five per cent of each other in size and differ by thirty-five times in
//! cost. What tracks it is how big the diagrams get, which is a fact about the answer and
//! so no use for choosing before running.
//!
//! So this does not choose. It gives the backward driver a budget, takes the answer when
//! it settles, and runs the crawl when it does not. The group that would have been slow
//! pays the budget rather than the whole run.
//!
//! ## What the budget is protecting against
//!
//! The tail, not the median. Conversation 368's median target takes 247ms and its worst
//! takes 78.8 SECONDS. A median-shaped budget would be far too tight for the groups that
//! win and far too loose for the ones that lose; what makes this work is that a group
//! which is going to be slow is usually slow immediately.

use std::time::Duration;

use oxidd::bdd::BDDFunction;

use crate::core::types::{DialogueNodeId, Novelty};
use crate::engine::engine::LookAheadEngine;
use crate::graph::graph::LookAheadGraph;
use crate::symbolic::guard_formula::GuardCompiler;
use crate::symbolic::novelty_search::{self, StoppedBy};
use crate::world::world::ILookAheadWorld;

/// Which search produced an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answered {
    /// The backward driver settled within its budget.
    Backwards,
    /// The backward driver did not settle, and the crawl was run instead.
    Crawl,
    /// The backward driver did not settle and the crawl then gave up too, so the answer
    /// is a lower bound from whichever got furthest.
    NeitherCompletely,
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
    pub elapsed: Duration,
}

/// How long to let the backward driver try before falling back.
pub struct Budget {
    /// The whole backward attempt, across every candidate.
    pub backwards: Duration,
    /// One candidate's fixed point.
    ///
    /// The one that matters. A group whose sets explode does so on its first candidate, so
    /// a per-candidate limit catches it without waiting for the whole attempt to time out.
    pub each: Duration,
    /// The most candidates to ask about.
    pub targets: usize,
}

impl Default for Budget {
    fn default() -> Self {
        // Chosen from the medians rather than invented: 631, 28 and 14 answer in 7ms, 1ms
        // and 100ms, while 368 and 1030 want 247ms and 121ms and have tails in the tens of
        // seconds. A quarter-second per candidate keeps the first three and cuts the other
        // two off early enough to be worth falling back from.
        Self {
            backwards: Duration::from_secs(2),
            each: Duration::from_millis(250),
            targets: 64,
        }
    }
}

/// The best novelty reachable beyond `start`, from whichever search answers first.
#[allow(clippy::too_many_arguments)]
pub fn best_novelty<'a, F>(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    seed: &BDDFunction,
    compiler: &mut GuardCompiler<'a>,
    world: &dyn ILookAheadWorld,
    counter_cap: u32,
    novelty: F,
    budget: &Budget,
    engine: &LookAheadEngine,
) -> PortfolioAnswer
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    let began = std::time::Instant::now();

    let backwards = novelty_search::best_novelty(
        graph,
        start,
        seed,
        compiler,
        world,
        counter_cap,
        &novelty,
        &novelty_search::Budget {
            targets: budget.targets,
            time: budget.backwards,
            each: crate::symbolic::backward::Budget {
                steps: usize::MAX,
                time: budget.each,
            },
        },
    );

    if backwards.stopped_by == StoppedBy::Nothing {
        return PortfolioAnswer {
            best: backwards.best,
            by: Answered::Backwards,
            witness: backwards.witness,
            targets_asked: backwards.targets_asked,
            elapsed: began.elapsed(),
        };
    }

    // It did not settle, so what it found is a lower bound rather than an answer - every
    // class it refused, it refused completely, but an unasked candidate might have carried
    // a better one. Run the crawl and take the better of the two, which is sound because
    // both are lower bounds and neither can overstate a class it never reached.
    let crawled = engine.evaluate(graph, start, world, &novelty);
    let best = crawled.best.max(backwards.best);
    let by = if crawled.budget_exhausted() {
        Answered::NeitherCompletely
    } else {
        Answered::Crawl
    };

    PortfolioAnswer {
        best,
        by,
        // Only when the backward search is what found it: a witness the crawl produced
        // would be a different fact, and there is no reason to guess at one.
        witness: backwards.witness.filter(|_| backwards.best >= crawled.best),
        targets_asked: backwards.targets_asked,
        elapsed: began.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::budget::DiagramBudget;

    use std::collections::HashSet;

    use crate::core::guard_value::GuardValue;
    use crate::engine::engine::LookAheadOptions;
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
        let seed = seed_of(graph, world, &vars);
        let engine = LookAheadEngine::new(LookAheadOptions {
            counter_cap: CAP,
            ..Default::default()
        });

        best_novelty(
            graph,
            node(0),
            &seed,
            &mut compiler,
            world,
            CAP as u32,
            novelty,
            budget,
            &engine,
        )
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

    #[test]
    fn a_settled_backward_answer_is_taken_as_it_stands() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build();

        let answer = run(&graph, &TestWorld::new(), unseen(&[2]), &Budget::default());
        assert_eq!(answer.best, Novelty::UnseenAnyGame);
        assert_eq!(answer.by, Answered::Backwards);
        assert_eq!(answer.witness, Some(node(2)));
    }

    /// A budget of nothing forces the fallback, which is how the fallback gets tested
    /// without a conversation big enough to be slow.
    #[test]
    fn an_unsettled_backward_search_falls_back_to_the_crawl() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build();

        let starved = Budget {
            backwards: Duration::ZERO,
            each: Duration::ZERO,
            targets: 64,
        };
        let answer = run(&graph, &TestWorld::new(), unseen(&[2]), &starved);

        assert_eq!(answer.by, Answered::Crawl);
        assert_eq!(answer.best, Novelty::UnseenAnyGame, "the crawl should have answered");
    }

    /// The fallback must not lose what the backward search had already established.
    ///
    /// Both are lower bounds, so the answer is the better of the two - and a crawl that
    /// gives up early could otherwise throw away a class the backward search had already
    /// proved reachable.
    #[test]
    fn the_answer_is_the_better_of_the_two_lower_bounds() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).guard(r#"Variable["shut"]"#).links(&[2]))
            .add(Entry::new(2))
            .build();
        let world = TestWorld::new().set_variable("shut", GuardValue::from_boolean(false));

        // Nothing is reachable, so both halves agree on the floor and neither invents one.
        let answer = run(&graph, &world, unseen(&[2]), &Budget::default());
        assert_eq!(answer.best, Novelty::SeenThisGame);
        assert_eq!(answer.witness, None);
    }
}
