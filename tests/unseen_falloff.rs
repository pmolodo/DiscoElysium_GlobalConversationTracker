// SPDX-License-Identifier: MIT
//! How fast does a crawl get cheaper as more of the group goes unread?
//!
//! The companion to `finding_one_unseen_entry_in_a_group_that_is_otherwise_seen`, which
//! pins the extreme. That one asks what the worst case costs; this asks whether the worst
//! case is a CLIFF or a CORNER - whether cost collapses the moment a second unread entry
//! appears, or slides down gently across the range a real save actually occupies.
//!
//! ## Why the answer decides how much the budget matters
//!
//! The crawl stops the instant it reaches an entry that outranks the option, so more
//! unread entries means finding one sooner. If cost falls off a cliff, only a nearly
//! exhausted profile is ever expensive and the state budget is a safety net that almost
//! never catches anything. If it falls slowly, the budget is load bearing in ordinary play
//! and its default is a number worth arguing about.
//!
//! Neither of those was known. The all-seen case is free (the no-improvement shortcut
//! refuses it outright), the one-unseen case is the measured extreme, and everything
//! between them - which is where every real save lives - had never been looked at.
//!
//! ## How the question is kept fair
//!
//! The unread entries are drawn from what one full crawl ACTUALLY REACHES, latest first.
//! Everything seeded is therefore findable - the crawl just found it - and its position in
//! the list is exactly how hard it is to find, because that is how long the same search
//! took to get there. Seeding the last-reached entries poses the hardest question the group
//! has, and each further one is strictly easier, which is what makes the series monotone.
//!
//! Structural depth was tried first and does not work here. It is the right way to choose
//! ONE hard quarry, which is what the one-unseen measurement does with it; as a series it
//! seeds entries the guards shut, so every row explores the same space and finds nothing.
//!
//! ## What it found, 2026-09-04
//!
//! States explored, against a 200,000 state budget, on the measurement save:
//!
//! ```text
//!   conv  entries        1        2        4        8       16       32       64
//!    368     4724   196362   189892   183730   170288   153240    86990    14469
//!    362     1860   192929   192904   192902   192891   167874   125970    46780
//!     28     2186   187005   187004   187002   186480   138862   102511    46613
//!    631     4514   183754   183753   183554   158704   155861   101851    64917
//!     14     3594   175676   173671   173664   173654   159141   137728    85750
//!   1030     1476      339      321      285      189      117       32        -
//! ```
//!
//! A CORNER, NOT A CLIFF, and the answer matters. Cost is very nearly FLAT from one unread
//! entry to eight - conversation 14 falls one per cent across that range, and 28 falls
//! three tenths of one per cent - and only starts moving past sixteen. So the expensive
//! case is not a knife edge that a single extra unread entry falls off; it is a plateau
//! that a real save sits on for as long as it has fewer than a dozen or so unread entries
//! left in a big conversation.
//!
//! AND EVERY EXPENSIVE GROUP SITS JUST UNDER THE BUDGET ON THAT PLATEAU: 175,676 to
//! 196,362 states against a budget of 200,000, which is 88 to 98 per cent of it. 368 clears
//! it by two per cent. That settles the question this file was written to ask - the state
//! budget is LOAD BEARING in ordinary play rather than a safety net that never catches
//! anything, and its default is a number worth arguing about (see de-e23q, which proposes
//! measuring it in memory instead, and de-f9gt on 368 specifically).
//!
//! The plateau also explains why the all-seen case being free was misleading about cost.
//! The no-improvement shortcut refuses a fully-read group outright, so the profile that
//! looks most expensive is free; the profile that is ACTUALLY most expensive is the one
//! just short of it, which nothing was measuring.
//!
//! 1030 is in the table as the control: its whole reachable space is a few hundred states,
//! so it is cheap at every level and stops early because it has fewer than 64 reachable
//! entries to seed.
//!
//! Run it with `--ignored --release`, one conversation per process:
//!
//!     CONVERSATION=368 cargo test --release --test unseen_falloff -- --ignored --nocapture

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions};
use lookahead_engine::engine::statistics::LookAheadStatistics;
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::world::world::ILookAheadWorld;
use lookahead_engine::index::{build_group_graph, read_index};

mod common;

/// The same groups the other cost measurements use, so the rows can be read together.
const EXPENSIVE: [i32; 6] = [362, 368, 631, 14, 28, 1030];

/// How many entries are unread at each step.
///
/// Geometric rather than linear, and stopping well short of the whole group, because the
/// interesting part is the bottom of the range: a save with a thousand unread entries in
/// one conversation is a save that has barely touched it, and the crawl there is trivial
/// for reasons nobody needs a measurement to believe.
const UNSEEN_COUNTS: [usize; 7] = [1, 2, 4, 8, 16, 32, 64];

/// The budget these runs are held to, matching the shipped default.
const STATE_BUDGET: usize = 200_000;

/// The same cap the other cost measurements use, so the rows compare.
const COUNTER_CAP: i32 = 16;

fn conversations(default: &[i32]) -> Vec<i32> {
    match std::env::var("CONVERSATION") {
        Ok(named) => named.split(',').filter_map(|id| id.trim().parse().ok()).collect(),
        Err(_) => default.to_vec(),
    }
}

/// The entries one full crawl actually reaches, in the order it first reaches them.
///
/// WHY NOT STRUCTURAL DEPTH. The one-unseen measurement picks its quarry as the deepest
/// entry reachable by links alone, which is the right way to choose ONE hard question. It
/// is the wrong way to build a SERIES: entries that deep are largely ones the guards shut,
/// so seeding more of them changes nothing the crawl can find, and the first version of
/// this file produced a dead flat curve - every row exploring the same state space and
/// finding nothing - which measured the seeding rather than the falloff.
///
/// Reach order fixes both halves. Everything in the list is findable, because the crawl
/// just found it; and the position in the list is exactly how hard it is to find, because
/// it is how long this same search took to get there. Seeding the LAST-reached entries
/// makes the hardest question the group can pose, and each further entry is strictly
/// easier - which is what makes the series monotone.
fn in_reach_order(graph: &LookAheadGraph, start: DialogueNodeId, world: &dyn ILookAheadWorld)
    -> Vec<DialogueNodeId>
{
    let order = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::new(Mutex::new(HashSet::new()));
    let recording = Arc::clone(&order);
    let visited = Arc::clone(&seen);

    // Everything read, so nothing outranks the option and the crawl cannot stop early:
    // this has to walk the WHOLE space to report the order of all of it.
    LookAheadEngine::new(LookAheadOptions {
        state_budget: STATE_BUDGET,
        time_budget: std::time::Duration::from_secs(120),
        counter_cap: COUNTER_CAP,
        state_sample_interval: 1,
        on_state_reached: Some(Box::new(move |id, _, _| {
            if visited.lock().unwrap().insert(id) {
                recording.lock().unwrap().push(id);
            }
        })),
        ..Default::default()
    })
    .evaluate(graph, start, world, |_| Novelty::SeenThisGame);

    let found = order.lock().unwrap().clone();
    found
        .into_iter()
        .rev()
        .filter(|id| *id != start)
        // The game never writes a group's SimStatus, so every group reads as never
        // displayed; the crawl walks through one without scoring it, and seeding one as
        // unread would seed an entry that cannot end a search.
        .filter(|id| graph.get(*id).is_some_and(|node| !node.is_group))
        .collect()
}

#[test]
#[ignore = "a long measurement, not a test: run it with --ignored --release"]
fn how_crawl_cost_falls_as_more_entries_go_unread() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>7} {:>10} {:>9} {:>7}",
        "conv", "entries", "unseen", "states", "ms", "found"
    );

    // The per-row table below is what this measurement is FOR; the tally is the same runs
    // aggregated, and it exists so the shape of a cost report is exercised by something
    // rather than only tested. See de-i60.24.
    let mut cost = LookAheadStatistics::new();

    for conversation in conversations(&EXPENSIVE) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let ordered = in_reach_order(&graph, start, &world);
        if ordered.is_empty() {
            continue;
        }

        let mut previous: Option<usize> = None;

        for &count in &UNSEEN_COUNTS {
            if count > ordered.len() {
                break;
            }

            let unseen: HashSet<DialogueNodeId> = ordered[..count].iter().copied().collect();
            let novelty = move |id: DialogueNodeId| {
                if unseen.contains(&id) { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
            };

            let began = std::time::Instant::now();
            let result = LookAheadEngine::new(LookAheadOptions {
                state_budget: STATE_BUDGET,
                time_budget: std::time::Duration::from_secs(60),
                counter_cap: COUNTER_CAP,
                ..Default::default()
            })
            .evaluate(&graph, start, &world, novelty);
            let ms = began.elapsed().as_millis();

            cost.record(start, &result, ms as f64);

            let found = if result.best == Novelty::UnseenAnyGame {
                "yes"
            } else if result.budget_exhausted() {
                "gave up"
            } else {
                "no"
            };

            println!(
                "{conversation:>6} {:>8} {count:>7} {:>10} {ms:>9} {found:>7}",
                graph.count(),
                result.states_explored,
            );

            // MONOTONE BY CONSTRUCTION, and worth checking rather than assuming: each step
            // keeps every entry the last one had unread and adds one more, so the crawl
            // has strictly more ways to stop early. A row that cost MORE than the row above
            // it would mean the ordering is not doing what this file says it does.
            if let Some(before) = previous {
                assert!(
                    result.states_explored <= before,
                    "{conversation}: {count} unread cost {} states against {before} for fewer",
                    result.states_explored,
                );
            }
            previous = Some(result.states_explored);
        }

        println!();
    }

    println!("across every row above:");
    print!("{cost}");
}
