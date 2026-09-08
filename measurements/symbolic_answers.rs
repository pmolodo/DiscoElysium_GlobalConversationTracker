// SPDX-License-Identifier: MIT
//! How fast does the backward driver answer, and what does sharing buy it?
//!
//! THE ONLY QUESTION THE LOOK-AHEAD ASKS is whether any unseen entry is reachable from
//! here. Not which entries, not the states at them, not a fixed point over the group. So
//! the driver stops at the first candidate it can prove, and this measures the clock from
//! the call to the answer.
//!
//! ## The profiles, and why the unreachable ones are the point
//!
//! `deepest-N` takes the N entries furthest from the start by links. That is the shape the
//! epic is about - a handful of unseen entries, buried - but it is not the hard case on its
//! own, because a deep entry that IS reachable is proved the moment the backward pass meets
//! the seed, and that is usually instant.
//!
//! `deepest-unreachable-N` takes the N deepest entries no path can reach.
//! Those are the expensive ones: a no has to be proved, which means driving the fixed point
//! to completion rather than stumbling on a yes. Where a group has fewer than N unreachable
//! entries the set is topped up from the deepest remaining, and where none can be
//! classified within the budget it falls back to `deepest-N` outright and says so - a
//! profile that silently became a different profile is worse than one that admits it.
//!
//! Classification costs one bounded backward pass per candidate and is done once per group,
//! deepest first, stopping as soon as N unreachable entries are in hand.
//!
//! ## A thread per search, which is not a style choice
//!
//! Something accumulates per-thread inside the diagram manager: twelve identical searches
//! die on the third when they share a thread and all twelve survive on a thread each
//! (de-8hh2.13). This measurement lost whole groups to that before it span each search off -
//! 1030, 368 and 14 all took the process down mid-run. de-fpax is the fix; this is it
//! applied here, and the fat stack is belt and braces for releasing a large diagram.
//!
//! Everything the diagram touches is built INSIDE the thread and dropped there. Only plain
//! numbers come back out.

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::backward::Budget as BackwardBudget;
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated::on_its_own_thread;
use lookahead_engine::symbolic::known::Known;
use lookahead_engine::symbolic::novelty_search::{
    best_novelty, classify_candidates, Budget as SearchBudget, StoppedBy,
};
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::world::world::ILookAheadWorld;

#[path = "../tests/common/mod.rs"]
mod common;

const COUNTER_CAP: i32 = 16;

/// The groups every symbolic measurement in this repository is taken on.
const MEASURED: [i32; 5] = [368, 631, 14, 28, 1030];

/// How long the whole driver may run before its answer is called a failure.
const ANSWER_CAP: std::time::Duration = std::time::Duration::from_secs(120);

/// How long ONE candidate may run while a profile is being classified.
///
/// Short on purpose. Classification asks about many candidates and only needs the ones it
/// can settle quickly; anything slower is recorded as unknown rather than waited out.
pub const CLASSIFY_CAP: std::time::Duration = std::time::Duration::from_secs(5);


fn main() {
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index; nothing to measure");
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    let asked: Vec<i32> = match std::env::var("CONVERSATION") {
        Ok(named) => named.split(',').filter_map(|id| id.trim().parse().ok()).collect(),
        Err(_) => MEASURED.to_vec(),
    };

    println!(
        "{:>6} {:>24} {:>7}  {:9} {:>9} {:>16} {:>7}",
        "conv", "profile", "unseen", "sharing", "ms", "verdict", "asked",
    );

    for conversation in asked {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let deepest = deepest_first(&graph, start);
        if deepest.is_empty() {
            continue;
        }

        for wanted in [1usize, 5, 10] {
            for (name, unseen) in profiles(&graph, start, &world, &deepest, wanted) {
                for shared in [false, true] {
                    let answer = answer(&graph, start, &world, &unseen, shared, ANSWER_CAP);
                    println!(
                        "{conversation:>6} {name:>24} {:>7}  {:9} {:>9} {:>16} {:>7}",
                        unseen.len(),
                        if shared { "shared" } else { "alone" },
                        answer.millis,
                        answer.verdict,
                        answer.asked,
                    );
                }
            }
        }
    }
}

/// Every entry a link walk can arrive at, furthest from the start first.
fn deepest_first(graph: &LookAheadGraph, start: DialogueNodeId) -> Vec<DialogueNodeId> {
    let mut depth: HashMap<DialogueNodeId, u32> = HashMap::new();
    let mut queue = VecDeque::from([(start, 0u32)]);
    while let Some((id, here)) = queue.pop_front() {
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if graph.get(child).is_none() || depth.contains_key(&child) {
                continue;
            }
            depth.insert(child, here + 1);
            queue.push_back((child, here + 1));
        }
    }

    let mut all: Vec<DialogueNodeId> = depth
        .keys()
        .copied()
        .filter(|id| *id != start)
        .filter(|id| graph.get(*id).is_some_and(|node| !node.is_group))
        .collect();
    all.sort_by_key(|id| {
        (
            std::cmp::Reverse(depth.get(id).copied().unwrap_or(0)),
            id.conversation_id,
            id.entry_id,
        )
    });
    all
}

/// `deepest-N`, and `deepest-unreachable-N` where enough entries can be classified.
fn profiles(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    deepest: &[DialogueNodeId],
    wanted: usize,
) -> Vec<(String, HashSet<DialogueNodeId>)> {
    let mut out = Vec::new();
    if deepest.len() < wanted {
        return out;
    }
    out.push((
        format!("deepest-{wanted}"),
        deepest.iter().copied().take(wanted).collect(),
    ));

    let (unreachable, unknown) = classify(graph, start, world, deepest, wanted);
    if unreachable.is_empty() {
        // NOTHING TO BUILD IT FROM, and saying so is the point: a profile that quietly
        // becomes `deepest-N` again would be two rows claiming to be different measurements.
        println!(
            "{:>6} {:>24}  no entry classified unreachable ({unknown} undecided); \
             deepest-{wanted} stands alone",
            start.conversation_id, format!("deepest-unreachable-{wanted}"),
        );
        return out;
    }

    // TOPPED UP FROM THE DEEPEST REMAINING where too few were classified, because a set of
    // three when ten were asked for is a different profile again. The name says how many of
    // it are genuinely unreachable.
    let mut set: HashSet<DialogueNodeId> = unreachable.iter().copied().take(wanted).collect();
    let short = wanted.saturating_sub(set.len());
    if short > 0 {
        for &id in deepest {
            if set.len() == wanted {
                break;
            }
            set.insert(id);
        }
    }

    let proven = unreachable.len().min(wanted);
    out.push((format!("deepest-unreach-{wanted} ({proven} proved)"), set));
    out
}

/// Which of the deepest entries the search provably cannot reach.
///
/// Deepest first, stopping once `wanted` are in hand. A candidate whose pass does not settle
/// inside [`CLASSIFY_CAP`] is counted undecided rather than assumed either way - a backward
/// pass that ran out of budget has proved nothing, and treating that as unreachable would
/// build the profile out of the very cases it is meant to exclude.
/// PUBLIC BECAUSE THE CENSUS SHARES IT. measurements/performance_matrix.rs pulls this file
/// in with `#[path]` and asks the same question over every group in the game (de-thlz.2), so
/// that the entries a census names unreachable and the entries a `deepest-unreach-N` profile
/// is built from are decided by one piece of code rather than two that could drift.
pub fn classify(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    deepest: &[DialogueNodeId],
    wanted: usize,
) -> (Vec<DialogueNodeId>, usize) {
    // ONE APPARATUS FOR THE WHOLE GROUP, not one per candidate. de-x8ms.11.
    //
    // This used to call `reachable` per target, and each of those was a separate `answer`
    // on a thread of its own building a fresh diagram manager, a fresh guard compiler and a
    // fresh seed. On groups small enough that the pass itself is trivial that was 19 to 25
    // ms of rebuilding PER CANDIDATE; group 631 meant 2,845 managers to ask 2,845 questions
    // about one graph in one world.
    //
    // IT GOES THROUGH THE SAME SEARCH THE GAME USES rather than growing a loop of its own.
    // `novelty_search::classify_candidates` is `best_novelty` with one option: report every
    // candidate and do not stop at the first yes. So the guard reuse, the candidate
    // ordering and the `Known` narrowing are inherited rather than reimplemented, and a
    // change to any of them reaches the census automatically.
    let symbols = graph.symbols().clone();
    let layout = DataLayout::for_group(graph, world, COUNTER_CAP);

    // ONE THREAD FOR THE WHOLE GROUP, holding ONE manager, which is the arrangement
    // src/symbolic/isolated.rs measured cleanest of all - 0 deaths in 35 - and the one the
    // per-candidate version could not use.
    on_its_own_thread(|| {
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
        let seed = seed_of(graph, world, &vars);

        // EVERY CANDIDATE IS THE QUARRY, because the question is which of them can be
        // reached rather than whether any can.
        let asking: HashSet<DialogueNodeId> = deepest.iter().copied().collect();
        let novelty = |id: DialogueNodeId| {
            if asking.contains(&id) { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
        };

        // SHARED, which the per-candidate version could not be: what a run over this group
        // establishes is handed to every pass instead of being rebuilt for each.
        let known = Known::of_from(graph, start).from(start, &seed);

        let mut unreachable = Vec::new();
        let mut undecided = 0;
        {
            let mut record = |target: DialogueNodeId, verdict: Option<bool>| match verdict {
                Some(false) => unreachable.push(target),
                Some(true) => {}
                None => undecided += 1,
            };

            classify_candidates(
                graph,
                start,
                StartBranch::Either,
                &seed,
                &mut compiler,
                world,
                COUNTER_CAP as u32,
                &novelty,
                &SearchBudget {
                    // THE WHOLE GROUP'S ALLOWANCE NOW, where it used to be one candidate's.
                    // Every candidate is asked about in this single call, so a per-candidate
                    // cap here would end the census rather than one question.
                    targets: usize::MAX,
                    time: std::time::Duration::MAX,
                    each: BackwardBudget {
                        steps: usize::MAX,
                        time: CLASSIFY_CAP,
                        ..Default::default()
                    },
                },
                Some(&known),
                &mut record,
            );
        }

        // BACK INTO DEEPEST-FIRST ORDER BEFORE THE CAP, and this is not cosmetic.
        //
        // The search orders its own candidates - `candidates_from`, best class first - and
        // that is not the order this function promises. What a census records is "the
        // deepest unreachable entries, in the order `candidates()` defines", and the
        // profiles built from it take the first N. Truncating the search's order would
        // hand a different set to every deepest-unreach row.
        //
        // Caught by comparing group 436 against the census taken before this change: same
        // count, entirely different list.
        let rank: std::collections::HashMap<DialogueNodeId, usize> =
            deepest.iter().enumerate().map(|(at, id)| (*id, at)).collect();
        unreachable.sort_by_key(|id| rank.get(id).copied().unwrap_or(usize::MAX));
        unreachable.truncate(wanted);
        (unreachable, undecided)
    })
}

/// Whether one target is reachable, or `None` where the pass could not settle it.
fn reachable(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    target: DialogueNodeId,
) -> Option<bool> {
    let unseen = HashSet::from([target]);
    let found = answer(graph, start, world, &unseen, false, CLASSIFY_CAP);
    match found.verdict.as_str() {
        "UnseenAnyGame" => Some(true),
        "SeenThisGame" => Some(false),
        _ => None,
    }
}

/// What the driver answered, and how long it took.
struct Answer {
    verdict: String,
    millis: u128,
    asked: String,
}

/// The backward driver over one profile, on its own thread.
///
/// `shared` decides whether the passes are told what earlier work over this group
/// established. Without it each candidate rebuilds the parent map and the iteration order
/// and recompiles every guard it touches; with it they are built once and handed on.
fn answer(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    unseen: &HashSet<DialogueNodeId>,
    shared: bool,
    cap: std::time::Duration,
) -> Answer {
    let symbols = graph.symbols().clone();

    // THE LAYOUT THE GAME USES, which this did not use until de-x8ms.4.
    //
    // It was `for_graph(graph, COUNTER_CAP, None, false).keeping_only_read(..)`, and the
    // `None` is the whole story: that argument is the MONEY CEILING, and for_group takes it
    // from the world (see the note at the top of src/workspace.rs). Without one, money is
    // unbounded, so a guard asking whether the player can afford something is satisfiable,
    // so entries behind it look REACHABLE when the world says they are not.
    //
    // MEASURED, on group 436 - money-gated content, which is why it showed there first.
    // Same graph, same world, same start, changing only this line:
    //
    //     for_graph(.., None, false).keeping_only_read(..)   4 of 30 candidates unreachable
    //     for_group(graph, world, ..)                        10+ of 30, including 436:14
    //
    // and 436:14 is the entry de-x8ms.4 was filed about, where this measurement said
    // reachable and all three of the matrix's engines said not-there. They were right.
    //
    // SO THE CENSUS WAS OVER-ESTIMATING REACHABILITY, and therefore UNDER-counting what no
    // path can reach. The entries it did name were sound - a for_graph unreachable is a
    // stronger claim and stays unreachable here - but it found too few of them.
    let layout = DataLayout::for_group(graph, world, COUNTER_CAP);

    // A THREAD PER SEARCH. See the note at the top: this is de-fpax's remedy, and without it
    // this measurement loses whole groups partway through.
    on_its_own_thread(|| {
                let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
                let mut compiler = GuardCompiler::new(&vars)
                    .with_world(world)
                    .with_constant_clock(DataLayout::group_passes_time(graph));
                let seed = seed_of(graph, world, &vars);

                let novelty = |id: DialogueNodeId| {
                    if unseen.contains(&id) {
                        Novelty::UnseenAnyGame
                    } else {
                        Novelty::SeenThisGame
                    }
                };

                let known = shared.then(|| Known::of_from(graph, start).from(start, &seed));

                let began = std::time::Instant::now();
                let found = best_novelty(
                    graph,
                    start,
                    StartBranch::Either,
                    &seed,
                    &mut compiler,
                    world,
                    COUNTER_CAP as u32,
                    &novelty,
                    &SearchBudget {
                        targets: usize::MAX,
                        time: cap,
                        each: BackwardBudget {
                            steps: usize::MAX,
                            time: cap,
                            ..Default::default()
                        },
                    },
                    known.as_ref(),
                );

                Answer {
                    verdict: match found.stopped_by {
                        StoppedBy::Nothing => format!("{:?}", found.best),
                        _ => "Incomplete".to_string(),
                    },
                    millis: began.elapsed().as_millis(),
                    asked: found.targets_asked.to_string(),
                }
    })
}
