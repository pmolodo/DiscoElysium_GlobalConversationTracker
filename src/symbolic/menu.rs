// SPDX-License-Identifier: MIT
//! Greedy menu marking by single-target branch and bound.
//!
//! Each round removes one nearest target and cuts the option that first reaches it.
//!
//! ## The bound
//!
//! A round asks one worklist pass whether anything in play is still reachable, then walks
//! the targets in bound order, one single-target pass each, and stops as soon as a target's
//! bound cannot beat the best distance proven this round - ties included, since a tie
//! cannot change which distance is least.
//!
//! Two things bound a target, and the answer is the larger:
//!
//! - the structural choice distance, guards ignored and cut respected, which only ever
//!   removes routes and so can only be optimistic;
//! - whatever a previous round proved about that same target, because a round only cuts an
//!   option and drops a winner from the contest and neither brings anything closer.
//!
//! THE SECOND IS WHAT MAKES THIS AFFORDABLE. The structural bound alone is far too loose to
//! skip anything: on conversation 631 it reads 11 where the true distance is 20, so a first
//! round evaluates every target it has. From the second round on the proven distance takes
//! over, arrives within one of the truth, and the walk stops after two or three targets.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use super::backward::{Backward, Budget as PassBudget, Nearest, Position, Round};
use super::guard_formula::GuardCompiler;
use super::known::GroupShape;
use super::novelty_search::{StoppedBy, choice_bounds};
use crate::core::types::{DialogueNodeId, Novelty};
use crate::graph::graph::LookAheadGraph;
use crate::world::world::ILookAheadWorld;

pub struct Contestant {
    pub position: Position,
    pub baseline: Novelty,
}

#[derive(Debug)]
pub struct Marked {
    pub best: Novelty,
    pub distance: Option<usize>,
    /// The round that claimed this option, starting at one.
    pub round: Option<usize>,
    pub witness: Option<DialogueNodeId>,
    pub complete: bool,
    pub stopped_by: StoppedBy,
    pub out_of_nodes: bool,
}

pub struct MenuAnswer {
    pub marks: Vec<Marked>,
    pub passes: usize,
    pub rounds: usize,
    pub elapsed: Duration,
}

pub struct Budget {
    pub wall: Duration,
    pub each: Duration,
}

/// Marks a menu by the cheap question, falling back to the exact one only where it answers
/// nothing.
///
/// ## The rule
///
/// 1. Ask each option whether it reaches unread content with its SIBLINGS CUT. A route that
///    had to return through another option of this menu cannot survive the cut, so a yes
///    means the option leads onward and a no means it only gets there by looping back.
/// 2. Where any option leads onward, those are the marks and nothing else runs.
/// 3. Where none does and something is reachable, take the exact marking whole.
///
/// ## Why, from the whole game
///
/// The onward question answers all 395 menus in 2.6 seconds at the player's 256 MB, against
/// 17.3 for the exact marking, and it answers conversation 761 - which no exact arrangement
/// answers at that allowance at all. It also DISCRIMINATES where plain reachability does
/// not: nine options in ten are reachable and three in ten lead onward.
///
/// Its one cost is 25 menus of 395 where content is reachable and every route to it returns
/// through the menu, so the cheap question marks nothing. Those are all small - 24 of the 25
/// answer exactly in between 18 and 80 ms - so step 3 is affordable exactly where it fires,
/// and never fires on 761, 631 or 640.
///
/// IT IS NOT A SUPERSET OF THE EXACT MARKING, and it is worth being exact about why, since
/// the shape of the rule invites the assumption. Where the cheap question marks SOME options
/// the exact answer is never consulted, and it may have marked more of them. Over the whole
/// game: 365 menus of 395 mark the same options, 18 mark more, and 12 mark FEWER - 865
/// markers against 837, which is more in total and not a containment either way.
///
/// The twelve are the rule working rather than failing. An option is dropped there because it
/// reaches its content only by returning through the menu while a sibling goes straight on,
/// which is precisely the option this marking exists to stop recommending. 631 is the clearest
/// case: four marks become three, and the menu goes from 2,886 ms to 279.
///
/// An onward mark carries no distance, because none was computed. See de-0jsf.18 for the
/// free structural bound that orders them when an ordering is wanted.
#[allow(clippy::too_many_arguments)]
pub fn mark_menu_hybrid<'a, F: Fn(DialogueNodeId) -> Novelty>(
    graph: &LookAheadGraph,
    compiler: &mut GuardCompiler<'a>,
    world: &dyn ILookAheadWorld,
    counter_cap: u32,
    novelty: &F,
    contestants: &[Contestant],
    budget: &Budget,
    shape: &GroupShape,
) -> MenuAnswer {
    let onward = mark_onward(
        graph,
        compiler,
        world,
        counter_cap,
        novelty,
        contestants,
        budget,
        shape,
    );
    if onward.rounds > 0 {
        return onward;
    }
    // NOTHING LED ONWARD. Either there is nothing to find - in which case the exact marking
    // settles on its own first pass and agrees - or every route loops back, which is the one
    // case the cheap question cannot answer and the expensive one can.
    let mut exact = mark_menu(
        graph,
        compiler,
        world,
        counter_cap,
        novelty,
        contestants,
        budget,
        shape,
    );
    exact.passes += onward.passes;
    exact
}

/// Marks every option that reaches unread content without returning through the menu.
///
/// One worklist pass decides the whole menu before any option is asked about: where nothing
/// of a class is reachable from ANY option, asking each of them separately would be eight
/// passes to reach the same nothing.
#[allow(clippy::too_many_arguments)]
pub fn mark_onward<'a, F: Fn(DialogueNodeId) -> Novelty>(
    graph: &LookAheadGraph,
    compiler: &mut GuardCompiler<'a>,
    world: &dyn ILookAheadWorld,
    counter_cap: u32,
    novelty: &F,
    contestants: &[Contestant],
    budget: &Budget,
    shape: &GroupShape,
) -> MenuAnswer {
    let began = Instant::now();
    let mut answer = blank(contestants);
    let options: HashSet<_> = contestants.iter().map(|c| c.position.option).collect();
    let mut marked = HashSet::new();
    let mut failure = None;

    'classes: for class in [Novelty::UnseenAnyGame, Novelty::UnseenThisGame] {
        let hunting: Vec<_> = (0..contestants.len())
            .filter(|i| !marked.contains(i) && contestants[*i].baseline < class)
            .collect();
        if hunting.is_empty() {
            continue;
        }
        let refused: HashSet<_> = options
            .iter()
            .copied()
            .filter(|option| {
                !hunting
                    .iter()
                    .any(|i| contestants[*i].position.option == *option)
            })
            .collect();
        let targets: Vec<_> = graph
            .nodes()
            .filter(|n| !n.is_group && novelty(n.id) == class && !options.contains(&n.id))
            .map(|n| n.id)
            .collect();
        if targets.is_empty() {
            continue;
        }

        // THE GATE, and it is one pass rather than one per option: what the whole menu can
        // reach with nothing of its own cut. Where that is nothing, no option can do better.
        let mut together = shape.known_from(graph, contestants[hunting[0]].position.option);
        for &i in &hunting {
            let position = &contestants[i].position;
            for &entry in &position.entries {
                together = together.from(entry, &position.holding);
            }
        }
        let left = budget.wall.saturating_sub(began.elapsed());
        if left.is_zero() {
            failure = Some((StoppedBy::Time, false));
            break 'classes;
        }
        answer.passes += 1;
        let reachable = Backward::reaching_any_knowing(
            graph,
            &targets,
            &refused,
            compiler,
            world,
            counter_cap,
            &PassBudget {
                time: budget.each.min(left),
                steps: usize::MAX,
                ..Default::default()
            },
            Some(&together),
        );
        if reachable.stats().met_at.is_none() {
            if reachable.stats().reached_fixed_point {
                continue;
            }
            failure = Some((StoppedBy::Incomplete, reachable.stats().out_of_memory));
            break 'classes;
        }
        drop(reachable);

        for &i in &hunting {
            // EVERY SIBLING CUT, so what is left is what this option reaches on its own.
            let mut cut = refused.clone();
            for &other in &hunting {
                if other != i {
                    cut.insert(contestants[other].position.option);
                }
            }
            let position = &contestants[i].position;
            let mut known = shape.known_from(graph, position.option);
            for &entry in &position.entries {
                known = known.from(entry, &position.holding);
            }
            let left = budget.wall.saturating_sub(began.elapsed());
            if left.is_zero() {
                failure = Some((StoppedBy::Time, false));
                break 'classes;
            }
            answer.passes += 1;
            let alone = Backward::reaching_any_knowing(
                graph,
                &targets,
                &cut,
                compiler,
                world,
                counter_cap,
                &PassBudget {
                    time: budget.each.min(left),
                    steps: usize::MAX,
                    ..Default::default()
                },
                Some(&known),
            );
            if alone.stats().met_at.is_some() {
                answer.rounds += 1;
                answer.marks[i] = Marked {
                    best: class,
                    // NO DISTANCE, because none was computed and inventing one would be a
                    // claim this question cannot support.
                    distance: None,
                    round: Some(answer.rounds),
                    witness: None,
                    complete: true,
                    stopped_by: StoppedBy::Nothing,
                    out_of_nodes: false,
                };
                marked.insert(i);
            } else if !alone.stats().reached_fixed_point {
                failure = Some((StoppedBy::Incomplete, alone.stats().out_of_memory));
                break 'classes;
            }
        }
    }

    if let Some((reason, memory)) = failure {
        for (index, mark) in answer.marks.iter_mut().enumerate() {
            if !marked.contains(&index) && mark.best < Novelty::UnseenAnyGame {
                mark.complete = false;
                mark.stopped_by = reason;
                mark.out_of_nodes = memory;
            }
        }
    }
    answer.elapsed = began.elapsed();
    answer
}

/// Every option answered by its own baseline and nothing else, which is where both markings
/// start.
fn blank(contestants: &[Contestant]) -> MenuAnswer {
    MenuAnswer {
        marks: contestants
            .iter()
            .map(|c| Marked {
                best: c.baseline,
                distance: None,
                round: None,
                witness: None,
                complete: true,
                stopped_by: StoppedBy::Nothing,
                out_of_nodes: false,
            })
            .collect(),
        passes: 0,
        rounds: 0,
        elapsed: Duration::ZERO,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn mark_menu<'a, F: Fn(DialogueNodeId) -> Novelty>(
    graph: &LookAheadGraph,
    compiler: &mut GuardCompiler<'a>,
    world: &dyn ILookAheadWorld,
    counter_cap: u32,
    novelty: &F,
    contestants: &[Contestant],
    budget: &Budget,
    shape: &GroupShape,
) -> MenuAnswer {
    let began = Instant::now();
    let mut answer = MenuAnswer {
        marks: contestants
            .iter()
            .map(|c| Marked {
                best: c.baseline,
                distance: None,
                round: None,
                witness: None,
                complete: true,
                stopped_by: StoppedBy::Nothing,
                out_of_nodes: false,
            })
            .collect(),
        passes: 0,
        rounds: 0,
        elapsed: Duration::ZERO,
    };
    let options: HashSet<_> = contestants.iter().map(|c| c.position.option).collect();
    let mut claimed = options.clone();
    let mut marked = HashSet::new();
    let mut failure = None;
    // WHAT A ROUND PROVES, KEPT. A round only ever cuts an option and drops the winner from
    // the contest, and both of those can only remove routes, so no target ever comes closer
    // than it was. Its own last distance is therefore a bound on it from then on, and a far
    // tighter one than the structural walk can give: on 631 the structural bound reads 11
    // against a true distance of 20, which is too loose to skip anything at all.
    let mut proven = HashMap::<DialogueNodeId, usize>::new();
    let mut unreachable = HashSet::new();
    'classes: for class in [Novelty::UnseenAnyGame, Novelty::UnseenThisGame] {
        let mut hunting: Vec<_> = (0..contestants.len())
            .filter(|i| !marked.contains(i) && contestants[*i].baseline < class)
            .collect();
        let mut cut: HashSet<_> = options
            .iter()
            .copied()
            .filter(|option| {
                !hunting
                    .iter()
                    .any(|i| contestants[*i].position.option == *option)
            })
            .collect();
        let mut in_play: Vec<_> = graph
            .nodes()
            .filter(|n| !n.is_group && novelty(n.id) == class && !claimed.contains(&n.id))
            .map(|n| n.id)
            .collect();
        while !hunting.is_empty() && !in_play.is_empty() {
            let positions: Vec<_> = hunting
                .iter()
                .map(|i| contestants[*i].position.clone())
                .collect();
            let mut bounds = HashMap::<DialogueNodeId, usize>::new();
            for position in &positions {
                for (id, distance) in choice_bounds(graph, position, &cut) {
                    bounds
                        .entry(id)
                        .and_modify(|d| *d = (*d).min(distance))
                        .or_insert(distance);
                }
            }
            for (id, distance) in &proven {
                if let Some(bound) = bounds.get_mut(id) {
                    *bound = (*bound).max(*distance);
                }
            }
            in_play.retain(|id| bounds.contains_key(id) && !unreachable.contains(id));
            if in_play.is_empty() {
                break;
            }
            in_play.sort_by_key(|id| (bounds[id], id.conversation_id, id.entry_id));
            // A shape-only Known preserves every outcome's states separately. Meeting
            // states are unioned explicitly so two outcomes at one entry cannot overwrite.
            let mut known = shape.known_from(graph, positions[0].option);
            let mut beginnings = HashMap::new();
            for position in &positions {
                for &entry in &position.entries {
                    let states = beginnings
                        .entry(entry)
                        .or_insert_with(|| compiler.vars().bottom());
                    use oxidd::BooleanFunction;
                    match states.or(&position.holding) {
                        Ok(union) => *states = union,
                        Err(_) => {
                            failure = Some((StoppedBy::Incomplete, true));
                            break 'classes;
                        }
                    }
                }
            }
            for (id, states) in beginnings {
                known = known.from(id, &states);
            }
            let pass_budget = |left| PassBudget {
                time: budget.each.min(left),
                steps: usize::MAX,
                ..Default::default()
            };
            let left = budget.wall.saturating_sub(began.elapsed());
            if left.is_zero() {
                failure = Some((StoppedBy::Time, false));
                break 'classes;
            }
            answer.passes += 1;
            let pass = Backward::reaching_any_knowing(
                graph,
                &in_play,
                &cut,
                compiler,
                world,
                counter_cap,
                &pass_budget(left),
                Some(&known),
            );
            if pass.stats().met_at.is_none() {
                if pass.stats().reached_fixed_point {
                    break;
                }
                failure = Some((StoppedBy::Incomplete, pass.stats().out_of_memory));
                break 'classes;
            }
            drop(pass);
            let mut best = None;
            let mut chosen = None;
            let mut pooled = false;
            // ONE POOL FOR THE WHOLE ROUND, where the loop below spends a pass per target.
            // The forward half of a meeting search is the same walk for every target in the
            // round - same options, same cut - so asking per target walks it once per target
            // and asking once walks it once. See [`Backward::nearest_of_many`].
            if std::env::var("DEGCTT_MEETING").is_ok() {
                let left = budget.wall.saturating_sub(began.elapsed());
                if left.is_zero() {
                    failure = Some((StoppedBy::Time, false));
                    break 'classes;
                }
                answer.passes += 1;
                match Backward::nearest_of_many(
                    graph,
                    &in_play,
                    &cut,
                    compiler,
                    world,
                    counter_cap,
                    &pass_budget(left),
                    &known,
                    &positions,
                ) {
                    Round::Found {
                        distance,
                        winner,
                        target,
                    } => {
                        best = Some(distance);
                        chosen = Some((hunting[winner], target));
                        pooled = true;
                    }
                    Round::Unreachable => break,
                    Round::Unfinished { out_of_memory } => {
                        failure = Some((StoppedBy::Incomplete, out_of_memory));
                        break 'classes;
                    }
                }
            }
            for &target in &in_play {
                // THE POOL ABOVE ALREADY ANSWERED, if it ran: it asks about every target at
                // once, so there is nothing left for a per-target pass to add.
                if pooled {
                    break;
                }
                if best.is_some_and(|d| bounds[&target] >= d) {
                    break;
                }
                if best.is_some_and(|d| bounds[&target] >= d) {
                    break;
                }
                let left = budget.wall.saturating_sub(began.elapsed());
                if left.is_zero() {
                    failure = Some((StoppedBy::Time, false));
                    break 'classes;
                }
                answer.passes += 1;
                let find = if std::env::var("DEGCTT_MEETING").is_ok() {
                    Backward::nearest_meeting
                } else {
                    Backward::nearest
                };
                let nearest = find(
                    graph,
                    target,
                    &cut,
                    compiler,
                    world,
                    counter_cap,
                    &pass_budget(left),
                    &known,
                    &positions,
                );
                match nearest {
                    Nearest::Found { distance, winner } => {
                        proven.insert(target, distance);
                        if best.is_none_or(|d| distance < d) {
                            best = Some(distance);
                            chosen = Some((hunting[winner], target));
                        }
                    }
                    // Unreachable for the same reason it will stay unreachable: routes only
                    // ever leave, so this one is done being asked about.
                    Nearest::Unreachable => {
                        unreachable.insert(target);
                    }
                    Nearest::Unfinished { out_of_memory } => {
                        failure = Some((StoppedBy::Incomplete, out_of_memory));
                        break 'classes;
                    }
                }
            }
            let distance = best.expect("worklist meet must have a nearest target");
            answer.rounds += 1;
            if let Some((index, witness)) = chosen {
                answer.marks[index] = Marked {
                    best: class,
                    distance: Some(distance),
                    round: Some(answer.rounds),
                    witness: Some(witness),
                    complete: true,
                    stopped_by: StoppedBy::Nothing,
                    out_of_nodes: false,
                };
                marked.insert(index);
                cut.insert(contestants[index].position.option);
                claimed.insert(witness);
            }
            in_play.retain(|id| !claimed.contains(id));
            hunting.retain(|i| !marked.contains(i));
        }
    }
    if let Some((reason, memory)) = failure {
        for (index, mark) in answer.marks.iter_mut().enumerate() {
            if !marked.contains(&index) && mark.best < Novelty::UnseenAnyGame {
                mark.complete = false;
                mark.stopped_by = reason;
                mark.out_of_nodes = memory;
            }
        }
    }
    answer.elapsed = began.elapsed();
    answer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::StartBranch;
    use crate::symbolic::budget::DiagramBudget;
    use crate::symbolic::data_layout::DataLayout;
    use crate::symbolic::novelty_search::Where;
    use crate::symbolic::reachability::seed_of;
    use crate::symbolic::vars::DataVars;
    use crate::test_graph::{Entry, GraphBuilder, node};
    use crate::world::test_world::TestWorld;

    fn mark(graph: &LookAheadGraph, options: &[i32], unread: &[i32]) -> MenuAnswer {
        let world = TestWorld::new();
        let layout = DataLayout::for_graph(graph, 16, None, false);
        let vars = DataVars::new(&layout, graph.symbols(), DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        let seed = seed_of(graph, &world, &vars).unwrap();
        let contestants: Vec<_> = options
            .iter()
            .map(|id| Contestant {
                position: Where::of(
                    graph,
                    node(*id),
                    StartBranch::Either,
                    &seed,
                    &mut compiler,
                    &world,
                    16,
                )
                .position(node(*id)),
                baseline: Novelty::SeenThisGame,
            })
            .collect();
        let novelty = |id: DialogueNodeId| {
            if unread.contains(&id.entry_id) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        };
        let answer = mark_menu(
            graph,
            &mut compiler,
            &world,
            16,
            &novelty,
            &contestants,
            &Budget {
                wall: Duration::from_secs(10),
                each: Duration::from_secs(10),
            },
            &GroupShape::of(graph),
        );
        assert!(answer.marks.iter().all(|mark| mark.complete));
        answer
    }

    #[test]
    fn tied_targets_choose_one_winner_per_round() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2, 3]))
            .add(Entry::new(1).player().links(&[4]))
            .add(Entry::new(2).player().links(&[4, 5]))
            .add(Entry::new(3).player().links(&[5]))
            .add(Entry::new(4))
            .add(Entry::new(5))
            .build();
        let answer = mark(&graph, &[1, 2, 3], &[4, 5]);
        assert_eq!(answer.rounds, 2);
        assert_eq!(
            answer
                .marks
                .iter()
                .filter(|mark| mark.distance == Some(0))
                .count(),
            2
        );
    }

    #[test]
    fn a_guarded_shortcut_is_only_a_lower_bound() {
        // A points to W but must return and choose E to open its guard. E is the
        // nearer route, even though A has the smaller structural lower bound.
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2, 3]))
            .add(Entry::new(1).player().links(&[0, 4]))
            .add(
                Entry::new(2)
                    .player()
                    .script("SetVariableValue(\"open\", true)")
                    .links(&[0]),
            )
            .add(Entry::new(3).player())
            .add(Entry::new(4).guard("Variable[\"open\"] == true"))
            .build();
        let answer = mark(&graph, &[1, 2, 3], &[4]);
        assert_eq!(answer.marks[0].distance, None);
        assert_eq!(answer.marks[1].distance, Some(1));
        assert_eq!(answer.marks[2].distance, None);
    }

    #[test]
    fn a_cut_winner_cannot_be_used_to_reach_other_content() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1).player().links(&[3]))
            .add(Entry::new(2).player().links(&[0]))
            .add(Entry::new(3).links(&[4, 5]))
            .add(Entry::new(4).player().links(&[6]))
            .add(Entry::new(5).player())
            .add(Entry::new(6))
            .build();
        let answer = mark(&graph, &[1, 2], &[3, 6]);
        assert_eq!(answer.marks[0].distance, Some(0));
        assert_eq!(answer.marks[1].distance, None);
    }
}
