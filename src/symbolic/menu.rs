// SPDX-License-Identifier: MIT
//! Greedy menu marking by nearest distance.
//!
//! Each round removes one nearest target and cuts the option that first reaches it.
//!
//! ## A round
//!
//! One worklist pass asks whether anything in play is still reachable. Where something is,
//! the round finds the least distance over every option and every target, and which option
//! owns it, by branch and bound: one single-target backward pass per target, in bound order,
//! stopping as soon as a target's bound cannot beat the best distance proven this round -
//! ties included, since a tie cannot change which distance is least. See
//! [`Backward::nearest`], which also says why a single pooled search over every target is not
//! used instead.
//!
//! The structural choice distance, guards ignored and cut respected, drops a target no
//! route reaches at all before the search spends anything on it.
//!
//! ## The bound
//!
//! Two things bound a target, and the answer is the larger:
//!
//! - the structural choice distance, which only ever removes routes and so can only be
//!   optimistic;
//! - whatever a previous round proved about that same target, because a round only cuts an
//!   option, claims an entry and drops a winner from the contest, and none of those brings
//!   anything closer.
//!
//! THE SECOND IS WHAT MAKES IT AFFORDABLE. The structural bound alone is far too loose to
//! skip anything: on conversation 631 it reads 11 where the true distance is 20, so a first
//! round evaluates every target it has. From the second round on the proven distance takes
//! over, arrives within one of the truth, and the walk stops after two or three targets.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use super::backward::{Backward, Budget as PassBudget, Nearest, Position};
use super::known::GroupShape;
use super::novelty_search::{StoppedBy, choice_bounds};
use super::search::Search;
use crate::core::types::{DialogueNodeId, Novelty};
use crate::graph::LookAheadGraph;

pub struct Contestant {
    pub position: Position,
    pub baseline: Novelty,
    /// What choosing it lands on directly: the option itself for an ordinary option, the
    /// first entries an outcome opens for one half of a rolled check. `baseline` is the best
    /// novelty among these.
    pub landing: Vec<DialogueNodeId>,
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

/// What a menu falls back to where the onward question stars nothing.
///
/// AN ARM, NOT A SETTING. The default is what the product ships and every default measurement
/// measures; the other is an opt-in comparison, named by `DEGCT_MARKING` in the menu matrix.
/// See CLAUDE.md on keeping one algorithm everywhere by default.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Fallback {
    /// The shipped rule: ask the onward question again with the siblings alone, and only then
    /// the exact marking.
    #[default]
    SiblingsThenExact,
    /// Straight to the exact marking, so a route that goes back is shown only where the walk's
    /// own cut found nothing going on. See de-l88t.
    Exact,
}

/// Marks a menu by the cheap question, falling back to the exact one only where it answers
/// nothing.
///
/// ## The rule
///
/// Up to three steps, stopping at the first that stars anything:
///
/// 1. WALK AND SIBLINGS CUT. Ask each option whether it reaches unread content with its
///    siblings cut and everything the player has passed since their hubs cut too. A route that
///    had to return through another option of this menu, or back through a menu the player
///    already left, cannot survive the cut, so a yes means the option leads onward. Where the
///    walk cuts nothing beyond this menu's own options, this is step 2's question, and is
///    skipped for it.
/// 2. SIBLINGS CUT ONLY, asked where step 1 starred nothing. The walk can leave nothing onward
///    at all - every route to unread content going back through the hub the menu hangs off -
///    and the menu is then answered as though nothing had been walked. [`Fallback::Exact`]
///    leaves this step out.
/// 3. EXACT MARKING, NOTHING CUT, where neither question starred anything and something is
///    reachable. It keeps the routes the cuts cannot see, such as one that sets a variable and
///    comes back through the hub to what the variable opens.
///
/// STEP 2 IS WHAT KEEPS THE EXACT MARKING RARE. Without it a walk cut that empties the onward
/// question sends the menu straight to the exact marking, which knows nothing of the walk and
/// is the costliest arrangement there is: measured on the whole-game matrix, 761's menu then
/// fails to settle at 256 MB, and 640's takes three times as long to star what it did anyway.
///
/// ## What dropping step 2 costs, measured 2026-09-17 (de-l88t)
///
/// [`Fallback::Exact`] is the arm that leaves it out. It is worth having because step 2 hands
/// back exactly the routes step 1's cut exists to refuse - an option whose only way to unread
/// content runs back out through the hub the player just came in by - so a menu answered at
/// step 2 contradicts what step 1 taught the player.
///
/// Over the whole game, three runs each, it moved four menus of 395 and THREE OF THEM GOT
/// TIGHTER at no cost: 517 went from five stars to one and 554 from five to one, both at
/// unchanged milliseconds, and 16 lost a star it should lose.
///
/// The fourth is why it is an arm and not the default. 761 stopped settling at all - 0 of 8
/// options, 2,628 ms against 981 - and the whole game rose 15.8 per cent. The nolimit arm says
/// that is structural rather than a budget to raise: the exact marking answers 761 with the
/// same four stars step 2 gave, in 140,103 ms and 93,353,567 diagram nodes, about 3.7 GB.
///
/// NEITHER READING IS SETTLED, because both were taken on a profile that asks every menu in a
/// state no save holds - see `menu_matrix`'s `seen_world` and de-aqxa.
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
/// answer exactly in between 18 and 80 ms - so step 2 is affordable exactly where it fires,
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
pub fn mark_menu_hybrid<F: Fn(DialogueNodeId) -> Novelty>(
    mut search: Search<'_, '_>,
    novelty: &F,
    contestants: &[Contestant],
    budget: &Budget,
    shape: &GroupShape,
    returned: &HashSet<DialogueNodeId>,
    fallback: Fallback,
) -> MenuAnswer {
    // STEP 1 ONLY WHERE THE WALK CUTS SOMETHING. An option of this menu is never cut on the
    // walk's account, so a walk holding nothing else asks step 2's question, and would ask it
    // twice.
    let siblings_alone = HashSet::new();
    let walk_cuts = returned
        .iter()
        .any(|id| !contestants.iter().any(|c| c.position.option == *id));
    let cuts: Vec<&HashSet<DialogueNodeId>> = match (fallback, walk_cuts) {
        (Fallback::SiblingsThenExact, true) => vec![returned, &siblings_alone],
        (Fallback::SiblingsThenExact, false) => vec![&siblings_alone],
        (Fallback::Exact, true) => vec![returned],
        // With nothing walked the two questions are the same one, so the arm asks it once and
        // differs from the shipped rule in nothing.
        (Fallback::Exact, false) => vec![&siblings_alone],
    };

    let mut passes = 0;
    for cut in cuts {
        let onward = mark_onward(search.reborrow(), novelty, contestants, budget, shape, cut);
        passes += onward.passes;
        if onward.rounds > 0 {
            return MenuAnswer { passes, ..onward };
        }
    }

    // NOTHING LED ONWARD. Either there is nothing to find - in which case the exact marking
    // settles on its own first pass and agrees - or every route loops back, which is the one
    // case the cheap question cannot answer and the expensive one can.
    let mut exact = mark_menu(search, novelty, contestants, budget, shape);
    exact.passes += passes;
    exact
}

/// Marks every option that reaches unread content without returning through the menu.
///
/// One worklist pass decides the whole menu before any option is asked about: where nothing
/// of a class is reachable from ANY option, asking each of them separately would be eight
/// passes to reach the same nothing.
///
/// `returned` is what the player has passed since the hubs they are inside - see
/// [`crate::symbolic::hub::since_current_hub`]. It is cut beside the siblings, so a route back
/// out through a menu the player already left counts as returning too. An option of this
/// menu is never cut on its account.
pub fn mark_onward<F: Fn(DialogueNodeId) -> Novelty>(
    mut search: Search<'_, '_>,
    novelty: &F,
    contestants: &[Contestant],
    budget: &Budget,
    shape: &GroupShape,
    returned: &HashSet<DialogueNodeId>,
) -> MenuAnswer {
    let graph = search.graph;
    let began = Instant::now();
    let mut answer = blank(contestants);
    let options: HashSet<_> = contestants.iter().map(|c| c.position.option).collect();
    let behind = returned_outside(returned, &options);
    let mut marked = HashSet::new();
    let mut failure = None;

    'classes: for class in [Novelty::UnseenAnyGame, Novelty::UnseenThisGame] {
        let hunting: Vec<_> = (0..contestants.len())
            .filter(|i| {
                !marked.contains(i) && worth_hunting(graph, &contestants[*i], novelty, class)
            })
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
            .chain(behind.iter().copied())
            .collect();
        // ONLY WHAT THE MENU REACHES ALONG LINKS WITH THE CUT IN PLACE. Guards can only remove
        // a link route, never make one, so a target no uncut route reaches cannot be reached at
        // all - and a backward pass asked about it spends a fixed point proving what one link
        // walk already says. That is dearest where the cut takes out the hub every route loops
        // through, and leaves most of the group's unread content behind it.
        let reached_by_menu: HashSet<DialogueNodeId> = hunting
            .iter()
            .flat_map(|i| choice_bounds(graph, &contestants[*i].position, &refused).into_keys())
            .collect();
        let targets: Vec<_> = graph
            .nodes()
            .filter(|n| {
                !n.is_group
                    && novelty(n.id) == class
                    && !options.contains(&n.id)
                    && reached_by_menu.contains(&n.id)
            })
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
            search.reborrow(),
            &targets,
            &refused,
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
            // THE SAME FILTER, for this option alone: a target its own uncut routes miss is one
            // it cannot reach, and an option that reaches none of them is settled without a pass.
            let reached = choice_bounds(graph, position, &cut);
            let own_targets: Vec<DialogueNodeId> = targets
                .iter()
                .copied()
                .filter(|id| reached.contains_key(id))
                .collect();
            if own_targets.is_empty() {
                continue;
            }
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
                search.reborrow(),
                &own_targets,
                &cut,
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
            // ONLY WHAT WAS ACTUALLY BEING SEARCHED FOR. An option nothing of either class
            // could improve was settled before a diagram was touched, and a budget that ran
            // out somewhere else does not unsettle it - see `worth_hunting`. Reporting it
            // unfinished draws the uncertain marker over "there is nothing down there".
            let hunted = [Novelty::UnseenAnyGame, Novelty::UnseenThisGame]
                .into_iter()
                .any(|class| worth_hunting(graph, &contestants[index], novelty, class));
            if !marked.contains(&index) && hunted {
                mark.complete = false;
                mark.stopped_by = reason;
                mark.out_of_nodes = memory;
            }
        }
    }
    answer.elapsed = began.elapsed();
    answer
}

/// Whether any of the `onward` options reaches `target`, each asked the way the onward
/// question asks it: with every other option of the menu cut.
///
/// FOR THE LOCKED OPTIONS ANSWERED AFTER A MENU. A star the exact marking gives names the entry
/// it claims, and a locked option is kept off that entry; a star the onward question gives
/// names none, since that question only establishes that an option leads somewhere. So where a
/// locked option's search lands on an entry, this asks whether an onward star already leads
/// there - one backward pass per star, and only for the entries a locked option actually lands
/// on. See `bridge::answer_starts`.
///
/// `Err` says why a pass stopped before it could answer, and whether the manager filled.
///
/// `returned` is cut as [`mark_onward`] cuts it, so a star is asked about the way it was given.
pub fn reached_onward(
    mut search: Search<'_, '_>,
    contestants: &[Contestant],
    onward: &[usize],
    target: DialogueNodeId,
    budget: &Budget,
    shape: &GroupShape,
    returned: &HashSet<DialogueNodeId>,
) -> Result<bool, (StoppedBy, bool)> {
    let graph = search.graph;
    let began = Instant::now();
    let options: HashSet<_> = contestants.iter().map(|c| c.position.option).collect();
    let behind = returned_outside(returned, &options);
    for &i in onward {
        let position = &contestants[i].position;
        let mut cut = options.clone();
        cut.remove(&position.option);
        cut.extend(behind.iter().copied());
        // No uncut route along links, no route at all: skip the pass, as `mark_onward` does.
        if !choice_bounds(graph, position, &cut).contains_key(&target) {
            continue;
        }
        let mut known = shape.known_from(graph, position.option);
        for &entry in &position.entries {
            known = known.from(entry, &position.holding);
        }
        let left = budget.wall.saturating_sub(began.elapsed());
        if left.is_zero() {
            return Err((StoppedBy::Time, false));
        }
        let pass = Backward::reaching_any_knowing(
            search.reborrow(),
            &[target],
            &cut,
            &PassBudget {
                time: budget.each.min(left),
                steps: usize::MAX,
                ..Default::default()
            },
            Some(&known),
        );
        if pass.stats().met_at.is_some() {
            return Ok(true);
        }
        if !pass.stats().reached_fixed_point {
            return Err((StoppedBy::Incomplete, pass.stats().out_of_memory));
        }
    }
    Ok(false)
}

/// What the player passed since their hubs, less this menu's own options - which are what is
/// being asked about, and so are never cut on that account.
fn returned_outside(
    returned: &HashSet<DialogueNodeId>,
    options: &HashSet<DialogueNodeId>,
) -> Vec<DialogueNodeId> {
    returned.difference(options).copied().collect()
}

/// Whether this option could be improved on by the class being hunted.
///
/// TWO REFUSALS, AND THE SECOND IS THE ONE A BUDGET MUST NOT UNDO. The baseline says the
/// option already lands at or above the class, so nothing of that class could outrank it.
/// The link walk says nothing of that class is even LINK-REACHABLE from here - guards
/// ignored, so it can only be optimistic - which settles the option without a diagram.
///
/// WHY IT BELONGS HERE rather than only in the caller. A menu is marked by passes shared
/// across its options, so an option left in the contest is one a budget can strand: when the
/// pass runs out, every option still hunting is reported unfinished, and "the search gave up"
/// is drawn where "there is nothing down there" is the truth. Conversation 451's "Leave." is
/// exactly that - it reaches nothing at all, which is as firmly established at a budget of
/// one as at any other, and it drew the uncertain marker for want of this test.
///
/// `graph.best_linked_class` is the same walk `bridge::class_worth_hunting` makes for an
/// option asked on its own, so a menu and a single option refuse on the same grounds.
fn worth_hunting<F: Fn(DialogueNodeId) -> Novelty>(
    graph: &LookAheadGraph,
    contestant: &Contestant,
    novelty: &F,
    class: Novelty,
) -> bool {
    contestant.baseline < class
        && graph
            .best_linked_class(contestant.position.option, novelty)
            .is_some_and(|reachable| reachable >= class)
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

/// The exact marking: each round a branch and bound over single targets. See the module
/// documentation's section on the bound, and [`Backward::nearest`].
pub fn mark_menu<F: Fn(DialogueNodeId) -> Novelty>(
    search: Search<'_, '_>,
    novelty: &F,
    contestants: &[Contestant],
    budget: &Budget,
    shape: &GroupShape,
) -> MenuAnswer {
    mark_menu_blocking(search, novelty, contestants, budget, shape, &HashSet::new())
}

/// [`mark_menu`], with `blocked` entries neither walkable nor claimable.
///
/// FOR A SEARCH ASKED AFTER THE MENU'S OWN: a locked check's halves are answered once every
/// ordinary star is settled, with the menu's options and every entry those stars claimed
/// blocked, so a half is starred only for content no other option reaches and without
/// cycling back through the menu. See `bridge::answer_starts`.
pub fn mark_menu_blocking<F: Fn(DialogueNodeId) -> Novelty>(
    mut search: Search<'_, '_>,
    novelty: &F,
    contestants: &[Contestant],
    budget: &Budget,
    shape: &GroupShape,
    blocked: &HashSet<DialogueNodeId>,
) -> MenuAnswer {
    let graph = search.graph;
    let began = Instant::now();
    let mut answer = blank(contestants);
    let options: HashSet<_> = contestants.iter().map(|c| c.position.option).collect();
    let mut claimed: HashSet<_> = options.union(blocked).copied().collect();
    let mut marked = HashSet::new();
    let mut failure = None;
    // WHAT A ROUND PROVES, KEPT. A round only ever cuts an option, claims an entry and drops
    // the winner from the contest, and each of those only removes routes, so no target ever
    // comes closer than it was. Its last distance is therefore a bound on it from then on -
    // see the module documentation - and a target found unreachable stays so.
    let mut proven = HashMap::<DialogueNodeId, usize>::new();
    let mut unreachable = HashSet::new();
    'classes: for class in [Novelty::UnseenAnyGame, Novelty::UnseenThisGame] {
        // WHAT AN OPTION ALREADY LANDS ON IS ITS OWN. A contestant whose landing reaches this
        // class is the nearest route there is to it - choosing it is zero steps away - so it
        // claims those entries before any round runs, and its option is cut as a winner's is.
        // Without this an open check whose Pass lands on unread content never competes, and
        // any sibling that loops back through the menu and into the check claims the content
        // the check itself shows.
        let landed: Vec<usize> = (0..contestants.len())
            .filter(|i| contestants[*i].baseline >= class)
            .collect();
        for &i in &landed {
            claimed.extend(
                contestants[i]
                    .landing
                    .iter()
                    .copied()
                    .filter(|id| novelty(*id) == class),
            );
        }
        let mut hunting: Vec<_> = (0..contestants.len())
            .filter(|i| {
                !marked.contains(i) && worth_hunting(graph, &contestants[*i], novelty, class)
            })
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
        cut.extend(blocked.iter().copied());
        cut.extend(landed.iter().map(|i| contestants[*i].position.option));
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
                        .or_insert_with(|| search.compiler.vars().bottom());
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
                search.reborrow(),
                &in_play,
                &cut,
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
            // IN BOUND ORDER, one target at a time, stopping at the first whose bound cannot
            // beat the best distance proven this round - ties included, since a tie cannot
            // change which distance is least.
            let mut best: Option<(usize, usize, DialogueNodeId)> = None;
            for &target in &in_play {
                if best.is_some_and(|(nearest, _, _)| bounds[&target] >= nearest) {
                    break;
                }
                let left = budget.wall.saturating_sub(began.elapsed());
                if left.is_zero() {
                    failure = Some((StoppedBy::Time, false));
                    break 'classes;
                }
                answer.passes += 1;
                match Backward::nearest(
                    search.reborrow(),
                    target,
                    &cut,
                    &pass_budget(left),
                    &known,
                    &positions,
                ) {
                    Nearest::Found { distance, winner } => {
                        proven.insert(target, distance);
                        if best.is_none_or(|(nearest, _, _)| distance < nearest) {
                            best = Some((distance, hunting[winner], target));
                        }
                    }
                    Nearest::Unreachable => {
                        unreachable.insert(target);
                    }
                    Nearest::Unfinished { out_of_memory } => {
                        failure = Some((StoppedBy::Incomplete, out_of_memory));
                        break 'classes;
                    }
                }
            }
            let Some((distance, index, witness)) = best else {
                break;
            };
            answer.rounds += 1;
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
            in_play.retain(|id| !claimed.contains(id));
            hunting.retain(|i| !marked.contains(i));
        }
    }
    if let Some((reason, memory)) = failure {
        for (index, mark) in answer.marks.iter_mut().enumerate() {
            // ONLY WHAT WAS ACTUALLY BEING SEARCHED FOR. An option nothing of either class
            // could improve was settled before a diagram was touched, and a budget that ran
            // out somewhere else does not unsettle it - see `worth_hunting`. Reporting it
            // unfinished draws the uncertain marker over "there is nothing down there".
            let hunted = [Novelty::UnseenAnyGame, Novelty::UnseenThisGame]
                .into_iter()
                .any(|class| worth_hunting(graph, &contestants[index], novelty, class));
            if !marked.contains(&index) && hunted {
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
    use crate::symbolic::guard_formula::GuardCompiler;
    use crate::symbolic::novelty_search::Where;
    use crate::symbolic::reachability::seed_of;
    use crate::symbolic::vars::DataVars;
    use crate::test_graph::{Entry, GraphBuilder, node};
    use crate::world::test_world::TestWorld;

    /// Which of the markings to run.
    ///
    /// AN ENUM RATHER THAN A FUNCTION POINTER. The markings differ only in which one is
    /// called, but `novelty` is a closure and writing the pointer type for it is more
    /// machinery than the thing it parameterises.
    #[derive(Clone, Copy)]
    enum Which {
        Exact,
        Onward,
        /// The shipped rule, siblings alone before the exact marking.
        Hybrid,
        /// The arm that drops the siblings-alone step - see [`Fallback::Exact`].
        HybridOnwardOnly,
    }

    fn mark(graph: &LookAheadGraph, options: &[i32], unread: &[i32]) -> MenuAnswer {
        let answer = marking(graph, options, unread, Which::Exact);
        assert!(answer.marks.iter().all(|mark| mark.complete));
        answer
    }

    /// The apparatus a menu is answered with - world, manager, compiled guards and one
    /// contestant per option - handed to `run`.
    fn with_menu<R>(
        graph: &LookAheadGraph,
        options: &[i32],
        run: impl FnOnce(&mut GuardCompiler<'_>, &TestWorld, &[Contestant]) -> R,
    ) -> R {
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
                landing: vec![node(*id)],
            })
            .collect();
        run(&mut compiler, &world, &contestants)
    }

    fn marking(
        graph: &LookAheadGraph,
        options: &[i32],
        unread: &[i32],
        which: Which,
    ) -> MenuAnswer {
        marking_returning(graph, options, unread, which, &HashSet::new())
    }

    use crate::symbolic::hub::tests::{
        DEEPER_TOPIC_MENU, DEEPER_TOPIC_UNREAD, DEEPER_TOPIC_WALK, deeper_topic,
    };
    use crate::symbolic::hub::tests::{KITCHEN_MENU, KITCHEN_WALK, kitchen};
    use crate::symbolic::hub::{Hubs, since_current_hub};

    /// What a walk has passed since its hubs, as the bridge works it out.
    fn returned_after(graph: &LookAheadGraph, walk: &[i32]) -> HashSet<DialogueNodeId> {
        let walk: Vec<DialogueNodeId> = walk.iter().map(|id| node(*id)).collect();
        since_current_hub(&IterationOrder::of(graph), &Hubs::of(graph), &walk)
    }

    /// Back at a sub-hub after going through a door and out again, the door keeps its star:
    /// what lies behind it is the sub-hub's own topic, not a way back out.
    ///
    /// AND IT WOULD NOT, were everything since the outer hub cut - which is the second half, and
    /// is what the hub stack exists to prevent. Cutting the whole walk leaves only the window.
    #[test]
    fn a_door_off_a_sub_hub_keeps_its_star_after_it_was_opened_once() {
        let graph = deeper_topic();
        let returned = returned_after(&graph, &DEEPER_TOPIC_WALK);
        let answer = marking_returning(
            &graph,
            &DEEPER_TOPIC_MENU,
            &DEEPER_TOPIC_UNREAD,
            Which::Hybrid,
            &returned,
        );
        assert_eq!(
            starred(&answer),
            vec![false, false, true, true],
            "topic one, back, the door, the window"
        );

        let everything: HashSet<DialogueNodeId> = [1, 2, 4, 5, 6, 8, 9, 11, 13, 14]
            .iter()
            .map(|id| node(*id))
            .collect();
        let answer = marking_returning(
            &graph,
            &DEEPER_TOPIC_MENU,
            &DEEPER_TOPIC_UNREAD,
            Which::Hybrid,
            &everything,
        );
        assert_eq!(
            starred(&answer),
            vec![false, false, false, true],
            "the door's route cut with the rest"
        );
    }

    /// Where the walk's cut leaves nothing onward - the only unread line is the bird behind the
    /// main hub, which every kitchen option reaches only by going back out - the menu is asked
    /// again with the siblings alone, and that answers it without the exact marking.
    #[test]
    fn a_walk_that_leaves_nothing_onward_is_answered_with_the_siblings_alone() {
        let graph = kitchen();
        let returned = returned_after(&graph, &KITCHEN_WALK);
        let bird_only = [3];

        let hybrid = marking_returning(&graph, &KITCHEN_MENU, &bird_only, Which::Hybrid, &returned);
        let siblings = marking(&graph, &KITCHEN_MENU, &bird_only, Which::Onward);

        assert!(
            hybrid.rounds > 0,
            "the siblings-alone level stars something"
        );
        assert_eq!(starred(&hybrid), starred(&siblings));
        assert_eq!(
            hybrid.passes, siblings.passes,
            "the walk's cut asked nothing, and the exact marking did not run"
        );
    }

    /// The same menu under [`Fallback::Exact`], which has no siblings-alone level: it goes on
    /// to the exact marking instead, and a star that gives names the entry it claims.
    ///
    /// THE WITNESS IS WHAT TELLS THE TWO APART. Both levels star the same kitchen options for
    /// the same bird, so the stars cannot; only the exact marking computes a witness.
    #[test]
    fn the_onward_only_arm_goes_to_the_exact_marking_instead() {
        let graph = kitchen();
        let returned = returned_after(&graph, &KITCHEN_WALK);
        let bird_only = [3];

        let arm = marking_returning(
            &graph,
            &KITCHEN_MENU,
            &bird_only,
            Which::HybridOnwardOnly,
            &returned,
        );
        let exact = marking(&graph, &KITCHEN_MENU, &bird_only, Which::Exact);

        assert!(arm.rounds > 0, "the exact marking stars something");
        assert_eq!(starred(&arm), starred(&exact));
        assert!(
            arm.marks
                .iter()
                .all(|mark| mark.round.is_none() == mark.witness.is_none()),
            "every star names the entry it claims, which only the exact marking does"
        );
    }

    /// A walk holding nothing but this menu's own options cuts nothing - an option is never cut
    /// on the walk's account - so the menu is answered exactly as one with no walk at all.
    #[test]
    fn a_walk_of_only_the_menus_own_options_cuts_nothing() {
        let graph = every_route_loops_back();
        let options_only: HashSet<DialogueNodeId> = [1, 2, 3].iter().map(|id| node(*id)).collect();

        let walked = marking_returning(&graph, &[1, 2, 3], &[4], Which::Hybrid, &options_only);
        let unwalked = marking(&graph, &[1, 2, 3], &[4], Which::Hybrid);

        assert_eq!(starred(&walked), starred(&unwalked));
        assert_eq!(walked.passes, unwalked.passes);
    }
    use crate::symbolic::order::IterationOrder;

    /// The kitchen's unread content: the bird behind the main hub, and the line past the
    /// warrant.
    const KITCHEN_UNREAD: [i32; 2] = [3, 17];

    /// Told nothing of where the player has been, the cheap question stars every kitchen
    /// option: 11 and 13 reach the bird by going back out through the sub-hub and the main
    /// hub, and neither of those is a sibling.
    #[test]
    fn without_the_walk_every_kitchen_option_is_starred() {
        let answer = marking(&kitchen(), &KITCHEN_MENU, &KITCHEN_UNREAD, Which::Hybrid);

        assert_eq!(
            starred(&answer),
            vec![true, true, true],
            "cook, warrant, hungry"
        );
    }

    /// Told the walk, the sub-hub and the main hub are behind the player, and only the option
    /// that leads onward keeps its star.
    #[test]
    fn with_the_walk_only_the_onward_kitchen_option_is_starred() {
        let graph = kitchen();
        let returned = returned_after(&graph, &KITCHEN_WALK);

        let answer = marking_returning(
            &graph,
            &KITCHEN_MENU,
            &KITCHEN_UNREAD,
            Which::Hybrid,
            &returned,
        );

        assert_eq!(
            starred(&answer),
            vec![false, true, false],
            "cook, warrant, hungry"
        );
        // THE GATE AND ONE PASS. The cook and the hungry reach nothing unread along links once
        // the sub-hub is cut, so neither is asked; the bird behind the main hub is not hunted
        // at all, since no uncut route from the menu reaches it.
        assert_eq!(answer.passes, 2);
    }

    /// [`marking`], told what the player has passed since their hubs.
    fn marking_returning(
        graph: &LookAheadGraph,
        options: &[i32],
        unread: &[i32],
        which: Which,
        returned: &HashSet<DialogueNodeId>,
    ) -> MenuAnswer {
        with_menu(graph, options, |compiler, world, contestants| {
            let novelty = |id: DialogueNodeId| {
                if unread.contains(&id.entry_id) {
                    Novelty::UnseenAnyGame
                } else {
                    Novelty::SeenThisGame
                }
            };
            let budget = Budget {
                wall: Duration::from_secs(10),
                each: Duration::from_secs(10),
            };
            let shape = GroupShape::of(graph);
            let search = Search {
                graph,
                compiler,
                world,
                counter_cap: 16,
            };
            match which {
                Which::Exact => mark_menu(search, &novelty, contestants, &budget, &shape),
                Which::Onward => {
                    mark_onward(search, &novelty, contestants, &budget, &shape, returned)
                }
                Which::Hybrid => mark_menu_hybrid(
                    search,
                    &novelty,
                    contestants,
                    &budget,
                    &shape,
                    returned,
                    Fallback::SiblingsThenExact,
                ),
                Which::HybridOnwardOnly => mark_menu_hybrid(
                    search,
                    &novelty,
                    contestants,
                    &budget,
                    &shape,
                    returned,
                    Fallback::Exact,
                ),
            }
        })
    }

    /// A menu whose every route to the unread line goes back through the menu.
    ///
    /// Option 2 links straight at the unread entry, but its guard is only opened by option 1,
    /// which goes nowhere itself. So no option reaches it ALONE - cutting the siblings cuts
    /// the thing that opens the way - and the menu as a whole does reach it, by taking 1 and
    /// then 2. That is the shape the fallback exists for.
    fn every_route_loops_back() -> LookAheadGraph {
        GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2, 3]))
            .add(
                Entry::new(1)
                    .player()
                    .script("SetVariableValue(\"open\", true)")
                    .links(&[0]),
            )
            .add(Entry::new(2).player().links(&[4]))
            .add(Entry::new(3).player())
            .add(Entry::new(4).guard("Variable[\"open\"] == true"))
            .build()
    }

    #[test]
    fn the_cheap_question_finds_nothing_where_every_route_loops_back() {
        let graph = every_route_loops_back();
        let onward = marking(&graph, &[1, 2, 3], &[4], Which::Onward);

        // THE FIXTURE HAS TO REACH THE FALLBACK, or the test below is quietly checking the
        // cheap question twice.
        assert_eq!(onward.rounds, 0, "no option may lead onward here");
        assert!(onward.marks.iter().all(|mark| mark.round.is_none()));
    }

    #[test]
    fn the_exact_marking_is_taken_whole_where_the_cheap_one_finds_nothing() {
        let graph = every_route_loops_back();
        let exact = marking(&graph, &[1, 2, 3], &[4], Which::Exact);
        let hybrid = marking(&graph, &[1, 2, 3], &[4], Which::Hybrid);

        assert_eq!(exact.rounds, hybrid.rounds);
        let exactly: Vec<_> = exact.marks.iter().map(|mark| mark.distance).collect();
        let hybridly: Vec<_> = hybrid.marks.iter().map(|mark| mark.distance).collect();
        assert_eq!(
            exactly, hybridly,
            "the fallback must not edit what it falls back to"
        );
        assert!(
            hybrid.rounds > 0,
            "the exact marking finds what the cheap one could not"
        );
    }

    #[test]
    fn the_exact_marking_is_not_consulted_where_an_option_leads_onward() {
        // 1 walks straight at unread content with nothing in the way, so the cheap question
        // answers and the expensive one is never asked. Without this a hybrid that always
        // fell back would pass the test above.
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1).player().links(&[3]))
            .add(Entry::new(2).player().links(&[0]))
            .add(Entry::new(3))
            .build();
        let onward = marking(&graph, &[1, 2], &[3], Which::Onward);
        let hybrid = marking(&graph, &[1, 2], &[3], Which::Hybrid);

        assert!(onward.rounds > 0, "option 1 leads onward");
        assert_eq!(
            hybrid.passes, onward.passes,
            "the fallback must not have run"
        );
        assert_eq!(hybrid.marks[0].round, onward.marks[0].round);
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

    /// The menu from `.claude/plans/MovingCloserToNewContent.md`, E included.
    ///
    /// ```text
    ///   S  the menu, every option of it already read
    ///   A  loops back to S - and links at W, which only E's variable opens
    ///   B  six nodes to X, then back to S
    ///   C  five nodes to X, then back to S
    ///   D  four nodes to Y, then back to S
    ///   E  straight back to S, having set the variable that opens A's link
    ///   Z  ends the conversation
    /// ```
    ///
    /// W, X and Y are the unread content and nothing else is. The shape exists for E, which
    /// is the case that says a walk may not be abandoned merely for returning to where it
    /// started: it saw nothing new on the way, but it CHANGED STATE, and that state is what
    /// opens A. A marking that prunes on position alone loses the three-step route E, S, A,
    /// W; one that prunes on position and state together keeps it.
    ///
    /// B and C share a target and D has its own, which is what makes the distances worth
    /// ordering rather than merely counting.
    fn the_e_menu() -> LookAheadGraph {
        let mut builder = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2, 3, 4, 5, 6]))
            .add(Entry::new(1).player().links(&[0, 7]))
            .add(Entry::new(2).player().links(&[10]))
            .add(Entry::new(3).player().links(&[16]))
            .add(Entry::new(4).player().links(&[21]))
            .add(
                Entry::new(5)
                    .player()
                    .script("SetVariableValue(\"opened\", true)")
                    .links(&[0]),
            )
            .add(Entry::new(6).player())
            .add(Entry::new(7).guard("Variable[\"opened\"] == true"))
            .add(Entry::new(8).links(&[0]))
            .add(Entry::new(9).links(&[0]));

        // The three corridors, each ending at the content it leads to. Written as a loop so
        // that their LENGTHS are the only thing that differs, which is the whole point of
        // having three of them.
        for (first, length, target) in [(10, 6, 8), (16, 5, 8), (21, 4, 9)] {
            for step in 0..length {
                let next = if step + 1 == length {
                    target
                } else {
                    first + step + 1
                };
                builder = builder.add(Entry::new(first + step).links(&[next]));
            }
        }

        builder.build()
    }

    /// The menu's options, in the order the letters name them.
    const E_MENU_OPTIONS: [i32; 6] = [1, 2, 3, 4, 5, 6];

    /// W, X and Y, and nothing else.
    const E_MENU_UNREAD: [i32; 3] = [7, 8, 9];

    /// Which of A, B, C, D, E and Z a marking stars.
    fn starred(answer: &MenuAnswer) -> Vec<bool> {
        answer
            .marks
            .iter()
            .map(|mark| mark.round.is_some())
            .collect()
    }

    /// The exact marking keeps the route a state change opens, and stars the nearest of each.
    ///
    /// ```text
    ///   A  no    it only reaches W by coming back to the menu and taking E first
    ///   B  one   no choices to X, the same as C - whichever wins, the other is cut
    ///   C  of    the two
    ///   D  yes   the nearest route to Y
    ///   E  yes   one choice to W by way of the menu, which is nearer than A's two
    ///   Z  no    ends the conversation
    /// ```
    ///
    /// E IS THE ONE THE SHAPE EXISTS FOR. It sees nothing new and comes straight back to
    /// where it started, so a walk pruned on POSITION alone would abandon it - and the route
    /// it opens is the shortest on the menu. Pruning on position and state together keeps
    /// it, and this is what says the shipped exact marking does.
    ///
    /// B AND C ARE A TIE, and this does not pin which of them breaks it. Distance counts
    /// choices, and neither corridor passes one; B's extra entry is not what it measures.
    #[test]
    fn the_exact_marking_keeps_the_route_a_state_change_opens() {
        let answer = marking(&the_e_menu(), &E_MENU_OPTIONS, &E_MENU_UNREAD, Which::Exact);
        let stars = starred(&answer);

        assert_eq!(
            [stars[0], stars[3], stars[4], stars[5]],
            [false, true, true, false],
            "A, D, E, Z",
        );
        assert!(stars[1] != stars[2], "exactly one of B and C: {stars:?}");
        let corridor = if stars[1] { 1 } else { 2 };

        // AND E IS NOT MERELY REACHED, it is ordered behind the two that are already there:
        // the X corridor and D win their own rounds at distance 0, and E wins the round
        // after, one choice further out. A marking that found E by accident would not have
        // it in third place.
        assert_eq!(answer.marks[4].distance, Some(1));
        assert_eq!(answer.marks[corridor].distance, Some(0));
        assert_eq!(answer.marks[3].distance, Some(0));
    }

    /// The cheap question loses E, and stars B in its place.
    ///
    /// ```text
    ///   A  no    its siblings are cut, so nothing opens W
    ///   B  YES   it leads onward, and the cheap question does not ask how far
    ///   C  yes
    ///   D  yes
    ///   E  NO    everything it opens runs through the menu, which the cut removes
    ///   Z  no
    /// ```
    ///
    /// TWO WRONG ANSWERS, not one, and they are the same mistake: the question is whether an
    /// option reaches unread content WITHOUT RETURNING THROUGH THE MENU, which is a yes or no
    /// about routes rather than a comparison of distances. So it cannot see that B is further
    /// from X than C is, and it cannot see E at all - E's whole value is the return.
    ///
    /// This is a record of what the shipped marking does, not an endorsement. See de-2p8j.2.
    #[test]
    fn the_cheap_question_loses_the_route_a_state_change_opens() {
        let answer = marking(
            &the_e_menu(),
            &E_MENU_OPTIONS,
            &E_MENU_UNREAD,
            Which::Onward,
        );

        assert_eq!(
            starred(&answer),
            vec![false, true, true, true, false, false],
            "A, B, C, D, E, Z",
        );
    }

    /// And on this menu the hybrid is the cheap question, because the fallback never fires.
    ///
    /// The fallback runs only where NO option leads onward. Here three of them do, so the
    /// expensive marking is never asked and E goes unstarred - which is the gap de-2p8j.2
    /// exists to close, and is pinned here so that closing it shows up as a changed test
    /// rather than as a quietly different menu.
    ///
    /// BOTH HALVES ARE NEEDED. That the hybrid agrees with the cheap question says which
    /// question answered; that it DISAGREES with the exact one says the choice of question
    /// is what decided the menu.
    #[test]
    fn the_hybrid_answers_the_e_menu_with_the_cheap_question() {
        let graph = the_e_menu();
        let onward = marking(&graph, &E_MENU_OPTIONS, &E_MENU_UNREAD, Which::Onward);
        let hybrid = marking(&graph, &E_MENU_OPTIONS, &E_MENU_UNREAD, Which::Hybrid);
        let exact = marking(&graph, &E_MENU_OPTIONS, &E_MENU_UNREAD, Which::Exact);

        assert!(onward.rounds > 0, "three options lead onward here");
        assert_eq!(starred(&hybrid), starred(&onward));
        assert_eq!(
            hybrid.passes, onward.passes,
            "the fallback must not have run"
        );
        assert_ne!(
            starred(&hybrid),
            starred(&exact),
            "the two questions must disagree here, or this menu pins nothing",
        );
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
