// SPDX-License-Identifier: MIT
//! The question the look-ahead actually asks, answered one target at a time.
//!
//! [`crate::engine::engine::LookAheadEngine::evaluate`] returns the best novelty among the
//! entries a start can reach, and returns early the moment it sees the best there is. So
//! the answer is a MAXIMUM over an ordered enum, and the way to compute a maximum is not
//! to compute the set it is a maximum of.
//!
//! This asks [`Backward`] about one candidate at a time and stops at the first one that
//! can be reached.
//!
//! ## Class before distance
//!
//! The obvious order is nearest first, and it is wrong. Proving a near `UnseenThisGame`
//! entry reachable says nothing about whether a far `UnseenAnyGame` one is, and the far
//! one is the answer if it is - `best` is a maximum, not a first sighting.
//!
//! So candidates are grouped by novelty class, best class first, and only WITHIN a class
//! sorted by how far away they are. The first candidate proved reachable in a class ends
//! the search, because every better class has already been refused entirely. A class every
//! candidate of which is refused drops to the next one down.
//!
//! Distance is the link distance, guards ignored. It is a heuristic about which question
//! is cheap to answer, not a claim about reachability, so it can be as rough as it likes:
//! a near entry has a shorter chain of guards in front of it and a smaller backward
//! fixed point, and that is the whole of the reasoning.
//!
//! ## What it costs when the answer is no
//!
//! One fixed point per candidate, against one crawl for all of them. That is the trade the
//! whole approach rests on and the reason [`Budget`] exists: a group with a long candidate
//! list, every one of them unreachable, is where the forward crawl should win, and
//! de-sze.14.4 is where that crossover gets measured rather than assumed.

use std::collections::{HashMap, VecDeque};

use oxidd::bdd::BDDFunction;

use crate::core::types::{DialogueNodeId, Novelty};
use crate::graph::graph::LookAheadGraph;
use crate::symbolic::backward::Backward;
use crate::symbolic::guard_formula::GuardCompiler;
use crate::symbolic::known::Known;
use crate::world::world::ILookAheadWorld;

/// Every novelty class better than "seen", best first.
///
/// `SeenThisGame` is the floor `evaluate` starts from and never an improvement on itself,
/// so it is not a candidate class - an entry carrying it is not worth asking about.
const CLASSES: [Novelty; 2] = [Novelty::UnseenAnyGame, Novelty::UnseenThisGame];

/// When to stop asking.
pub struct Budget {
    /// The most candidates to ask about before giving up.
    pub targets: usize,
    /// How long to keep asking.
    pub time: std::time::Duration,
    /// The budget each individual backward pass runs under.
    pub each: crate::symbolic::backward::Budget,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            targets: 64,
            time: std::time::Duration::from_secs(5),
            each: crate::symbolic::backward::Budget::default(),
        }
    }
}

/// Why a search stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoppedBy {
    /// Every candidate was asked about, so the answer is final.
    Nothing,
    /// The candidate budget ran out.
    Targets,
    /// The time budget ran out.
    Time,
    /// A backward pass could not finish - out of nodes, or out of its own budget.
    Incomplete,
}

/// What the search found.
#[derive(Debug, Clone)]
pub struct NoveltyAnswer {
    /// The best novelty reachable beyond the start.
    ///
    /// A LOWER BOUND when [`Self::stopped_by`] is anything but `Nothing`: the classes
    /// already refused were refused completely, but an unasked candidate might have
    /// carried a better one.
    pub best: Novelty,
    /// The entry that proved it, when something did.
    ///
    /// Worth returning rather than throwing away. A marker with a reason behind it can be
    /// explained, and a disagreement with the crawl can be investigated from the entry
    /// both engines disagree about rather than from the whole group.
    pub witness: Option<DialogueNodeId>,
    /// How many candidates were asked about.
    pub targets_asked: usize,
    /// How many candidates there were.
    pub candidates: usize,
    pub stopped_by: StoppedBy,
    /// Whether the pass that failed to settle failed by running out of DIAGRAM NODES.
    ///
    /// [`StoppedBy::Incomplete`] covers both ways a pass can fail to settle, and they are
    /// not the same finding: a pass that spent every node it was allowed is a result about
    /// the representation, where one that ran out of steps or seconds is a result about
    /// the clock. A measurement that reported them alike would blame the budget for what
    /// the ration did, which is exactly the mistake de-e33h was raised for.
    pub out_of_nodes: bool,
    /// The entry at which a pass MET what an earlier search already knew, when one did.
    ///
    /// Set only when the meet is what answered the question, so it is the measurement of
    /// whether sharing paid: an answer with this set is one no fixed point had to finish.
    pub met_at: Option<DialogueNodeId>,
    pub elapsed: std::time::Duration,
}

/// The candidates, in the order they should be asked about.
///
/// Non-group entries only: a group is expanded in place and the game never writes its
/// SimStatus, so every group in the database reads as never displayed and treating one as
/// a candidate would make every search succeed instantly on a lie. `evaluate` skips them
/// for the same reason.
///
/// The start itself is a candidate only if a link leads back to it, which mirrors
/// `evaluate` scoring whatever it arrives at rather than where it began.
pub fn candidates<F>(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    novelty: &F,
) -> Vec<DialogueNodeId>
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    let distances = link_distances(graph, start);

    // No filtering of the start here: `link_distances` records an entry only once a link
    // has ARRIVED at it, so the start appears exactly when something leads back to it,
    // which is exactly when `evaluate` would score it.
    let mut worth: Vec<(usize, usize, i32, i32, DialogueNodeId)> = distances
        .iter()
        .filter_map(|(id, distance)| {
            let node = graph.get(*id)?;
            if node.is_group {
                return None;
            }
            let class = novelty(*id);
            let rank = CLASSES.iter().position(|c| *c == class)?;
            Some((rank, *distance, id.conversation_id, id.entry_id, *id))
        })
        .collect();

    // Class first, distance second, and the identifiers last so the order is total: two
    // candidates at the same distance in the same class must still be asked about in the
    // same order on every run, or a measurement is not repeatable.
    worth.sort_by_key(|(rank, distance, conversation, entry, _)| {
        (*rank, *distance, *conversation, *entry)
    });
    worth.into_iter().map(|(_, _, _, _, id)| id).collect()
}

/// The best novelty reachable beyond `start`, by asking about candidates in turn.
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
    known: Option<&Known>,
) -> NoveltyAnswer
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    let began = std::time::Instant::now();
    let ordered = candidates(graph, start, &novelty);
    let mut answer = NoveltyAnswer {
        best: Novelty::SeenThisGame,
        witness: None,
        targets_asked: 0,
        candidates: ordered.len(),
        stopped_by: StoppedBy::Nothing,
        out_of_nodes: false,
        met_at: None,
        elapsed: std::time::Duration::ZERO,
    };

    for target in ordered {
        if answer.targets_asked >= budget.targets {
            answer.stopped_by = StoppedBy::Targets;
            break;
        }
        if began.elapsed() >= budget.time {
            answer.stopped_by = StoppedBy::Time;
            break;
        }

        answer.targets_asked += 1;
        let backward = Backward::reaching_knowing(
            graph, target, compiler, world, counter_cap, &budget.each, known,
        );

        // TWO WAYS TO PROVE IT, and the cheap one is asked first. A meet is a proof that
        // stopped the pass early - a state an earlier search can hold at some entry is one
        // this pass has shown reaches the target - so the fixed point is deliberately
        // incomplete and `reachable_from` would be asking the wrong question of it.
        if backward.stats().met_at.is_some() || backward.reachable_from(start, seed) {
            // The best class is asked about first and exhausted before the next one is
            // begun, so the first candidate that answers yes carries the answer.
            answer.best = novelty(target);
            answer.witness = Some(target);
            answer.met_at = backward.stats().met_at;
            break;
        }

        // A pass that did not settle proves nothing by saying no: it may simply not have
        // got far enough. Say so rather than counting it as a refusal.
        let stats = backward.stats();
        if !stats.reached_fixed_point {
            answer.stopped_by = StoppedBy::Incomplete;
            answer.out_of_nodes = stats.out_of_memory;
            break;
        }
    }

    answer.elapsed = began.elapsed();
    answer
}

/// How far each entry is from `start`, following links and ignoring guards.
fn link_distances(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
) -> HashMap<DialogueNodeId, usize> {
    let mut distance: HashMap<DialogueNodeId, usize> = HashMap::new();
    // The start is walked FROM without being recorded as arrived at, which is what gives
    // it a distance only when a link leads back to it - and so makes it a candidate
    // exactly when `evaluate` would score it. Should that happen it is queued a second
    // time, and expanding it again costs one pass over links already recorded.
    let mut queue = VecDeque::from([(start, 0usize)]);

    while let Some((id, here)) = queue.pop_front() {
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if graph.get(child).is_none() || distance.contains_key(&child) {
                continue;
            }
            distance.insert(child, here + 1);
            queue.push_back((child, here + 1));
        }
    }

    distance
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::budget::DiagramBudget;

    use std::collections::HashSet;

    use crate::core::guard_value::GuardValue;
    use crate::symbolic::data_layout::DataLayout;
    use crate::symbolic::reachability::seed_of;
    use crate::symbolic::vars::DataVars;
    use crate::test_graph::{node, Entry, GraphBuilder};
    use crate::world::test_world::TestWorld;

    const CAP: i32 = 16;

    /// The novelty function the engine's own tests use: everything named is unseen.
    fn novel(unseen: &[i32], best: Novelty) -> impl Fn(DialogueNodeId) -> Novelty + '_ {
        let set: HashSet<i32> = unseen.iter().copied().collect();
        move |id| {
            if set.contains(&id.entry_id) {
                best
            } else {
                Novelty::SeenThisGame
            }
        }
    }

    fn search<F>(graph: &LookAheadGraph, world: &TestWorld, novelty: F) -> NoveltyAnswer
    where
        F: Fn(DialogueNodeId) -> Novelty,
    {
        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(world);
        let seed = seed_of(graph, world, &vars);

        best_novelty(
            graph,
            node(0),
            &seed,
            &mut compiler,
            world,
            CAP as u32,
            novelty,
            &Budget::default(),
            None,
        )
    }

    #[test]
    fn an_unreachable_candidate_does_not_score() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).guard(r#"Variable["shut"]"#).links(&[2]))
            .add(Entry::new(2))
            .build();
        let world = TestWorld::new().set_variable("shut", GuardValue::from_boolean(false));

        let answer = search(&graph, &world, novel(&[2], Novelty::UnseenAnyGame));
        assert_eq!(answer.best, Novelty::SeenThisGame);
        assert_eq!(answer.witness, None);
        assert_eq!(answer.stopped_by, StoppedBy::Nothing);
    }

    #[test]
    fn a_reachable_candidate_scores_and_names_itself() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build();

        let answer = search(&graph, &TestWorld::new(), novel(&[2], Novelty::UnseenAnyGame));
        assert_eq!(answer.best, Novelty::UnseenAnyGame);
        assert_eq!(answer.witness, Some(node(2)));
    }

    /// The case a distance-only order gets wrong, which is why the order is class first.
    ///
    /// A near `UnseenThisGame` entry and a far `UnseenAnyGame` one. Nearest-first would
    /// prove the near one reachable, stop, and report the smaller answer - and it would
    /// look right, because it did find something.
    #[test]
    fn a_far_better_class_beats_a_near_worse_one() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1))
            .add(Entry::new(2).links(&[3]))
            .add(Entry::new(3).links(&[4]))
            .add(Entry::new(4))
            .build();

        let novelty = |id: DialogueNodeId| match id.entry_id {
            1 => Novelty::UnseenThisGame,
            4 => Novelty::UnseenAnyGame,
            _ => Novelty::SeenThisGame,
        };

        let answer = search(&graph, &TestWorld::new(), novelty);
        assert_eq!(answer.best, Novelty::UnseenAnyGame);
        assert_eq!(answer.witness, Some(node(4)), "the near worse candidate won");
        // And it never had to ask about the near one: the better class is exhausted first.
        assert_eq!(answer.targets_asked, 1);
    }

    /// The worse class is only reached once the better one is refused entirely.
    #[test]
    fn a_refused_class_falls_through_to_the_next() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1).guard(r#"Variable["shut"]"#).links(&[3]))
            .add(Entry::new(2))
            .add(Entry::new(3))
            .build();
        let world = TestWorld::new().set_variable("shut", GuardValue::from_boolean(false));

        let novelty = |id: DialogueNodeId| match id.entry_id {
            3 => Novelty::UnseenAnyGame,
            2 => Novelty::UnseenThisGame,
            _ => Novelty::SeenThisGame,
        };

        let answer = search(&graph, &world, novelty);
        assert_eq!(answer.best, Novelty::UnseenThisGame);
        assert_eq!(answer.witness, Some(node(2)));
        assert_eq!(answer.targets_asked, 2, "the shut candidate had to be asked first");
    }

    /// Early exit: a search that settles without asking about everything.
    #[test]
    fn the_search_stops_at_the_first_witness() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2).links(&[3]))
            .add(Entry::new(3).links(&[4]))
            .add(Entry::new(4))
            .build();

        let answer = search(
            &graph,
            &TestWorld::new(),
            novel(&[1, 2, 3, 4], Novelty::UnseenAnyGame),
        );
        assert_eq!(answer.best, Novelty::UnseenAnyGame);
        assert_eq!(answer.candidates, 4);
        assert_eq!(answer.targets_asked, 1, "the nearest witness should have ended it");
        assert_eq!(answer.witness, Some(node(1)));
    }

    /// The answer the engine gives, on the engine's own shapes.
    ///
    /// The acceptance criterion for the whole driver, and the only one that is about the
    /// product rather than about the parts. Each fixture is one the engine's tests already
    /// use, so a disagreement is a disagreement about something already understood.
    ///
    /// AT LEAST, not exactly. The symbolic side over-approximates - undecided guards go
    /// through, no cost can be refused - so it may report a better novelty than the crawl
    /// finds. Reporting a WORSE one would mean a marker lost, and that is what this
    /// forbids.
    #[test]
    fn the_driver_agrees_with_the_engine_on_its_own_fixtures() {
        use crate::engine::engine::{LookAheadEngine, LookAheadOptions};

        let shut = TestWorld::new().set_variable("shut", GuardValue::from_boolean(false));
        let plain = TestWorld::new();

        // Name, graph, world, and which entries are unseen.
        let fixtures: Vec<(&str, LookAheadGraph, &TestWorld, Vec<i32>)> = vec![
            (
                "a false guard blocks",
                GraphBuilder::new()
                    .add(Entry::new(0).links(&[1]))
                    .add(Entry::new(1).guard(r#"Variable["shut"]"#).links(&[2]))
                    .add(Entry::new(2))
                    .build(),
                &shut,
                vec![2],
            ),
            (
                "an unknown guard does not block",
                GraphBuilder::new()
                    .add(Entry::new(0).links(&[1]))
                    .add(Entry::new(1).guard("IsKimHere()").links(&[2]))
                    .add(Entry::new(2))
                    .build(),
                &plain,
                vec![2],
            ),
            (
                "actions unlock their own downstream guards",
                GraphBuilder::new()
                    .add(Entry::new(0).links(&[1]))
                    .add(
                        Entry::new(1)
                            .script(r#"SetVariableValue("opened", true)"#)
                            .links(&[2]),
                    )
                    .add(Entry::new(2).guard(r#"Variable["opened"]"#))
                    .build(),
                &plain,
                vec![2],
            ),
            (
                "groups are traversed but never scored",
                GraphBuilder::new()
                    .add(Entry::new(0).links(&[1]))
                    .add(Entry::new(1).group().links(&[2]))
                    .add(Entry::new(2))
                    .build(),
                &plain,
                vec![1],
            ),
            (
                "cycles terminate",
                GraphBuilder::new()
                    .add(Entry::new(0).links(&[1]))
                    .add(Entry::new(1).links(&[2]))
                    .add(Entry::new(2).links(&[1]))
                    .build(),
                &plain,
                vec![2],
            ),
        ];

        for (name, graph, world, unseen) in fixtures {
            let novelty = novel(&unseen, Novelty::UnseenAnyGame);
            let engine = LookAheadEngine::new(LookAheadOptions {
                counter_cap: CAP,
                ..Default::default()
            });
            let expected = engine.evaluate(&graph, node(0), world, &novelty).best;
            let answer = search(&graph, world, &novelty);

            assert!(
                answer.best >= expected,
                "{name}: the driver said {:?} where the engine said {expected:?}, which \
                 is a marker lost",
                answer.best,
            );
            assert_eq!(
                answer.best, expected,
                "{name}: the driver said {:?} and the engine said {expected:?} - a \
                 surplus rather than a loss, but an unexplained one",
                answer.best,
            );
        }
    }

    /// A group is never a candidate, however novel the save says it is.
    #[test]
    fn a_group_entry_is_not_a_candidate() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).group().links(&[2]))
            .add(Entry::new(2))
            .build();

        let answer = search(&graph, &TestWorld::new(), novel(&[1], Novelty::UnseenAnyGame));
        assert_eq!(answer.best, Novelty::SeenThisGame);
        assert_eq!(answer.candidates, 0);
    }
}
