// SPDX-License-Identifier: MIT
//! What a menu pays PER OPTION for work that depends on the graph alone.
//!
//! ## The question
//!
//! `repeat_question` priced the setup a REQUEST pays once - the group graph, then the
//! diagram side - and found eighteen to twenty-seven milliseconds. It did not look inside
//! the request, where a second kind of setup is paid once per START:
//!
//! - `IterationOrder::of(graph)`, a Tarjan pass over the whole group, built inside
//!   `Reachability::explore_branch_within` - which is what `portfolio::forwards_for` calls.
//! - `IterationOrder::of_from(graph, start)`, the same Tarjan again plus a distance walk,
//!   inside `Known::of_from` in `portfolio::best_novelty`.
//! - The parent map in `Known::of`, every node's incoming edges, identical for every start
//!   in the group.
//!
//! A response menu is a dozen options and a rolled check is TWO starts, so twenty-four is
//! an ordinary menu - and none of that work differs between them. `Backward` already
//! refuses to rebuild the parent map per CANDIDATE ("a driver that asks about forty
//! candidates walked the whole graph forty times to build the same map"); this asks what
//! the same argument is worth one level up, across the starts of one menu.
//!
//! ## How to run it
//!
//! ```text
//! RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo per-start-setup -- \
//!   cargo run --release --example per_start_setup
//! ```
//!
//! `CONVERSATION` picks the groups, `STARTS` how many options the menu has, `REPEATS` how
//! many times each is timed - the fastest is reported, since what is wanted is what the
//! work costs rather than what the machine was doing at the time.
//!
//! ## What it said, 2026-09-07, for a menu of 24 starts
//!
//! ```text
//!   conv  entries  alone ms  per start ms  apart ms  together ms   saving
//!     28     2186      2.67          0.38      64.2         10.7     6.0x
//!    368     4724      5.52          0.32     132.4         10.5    12.6x
//!     14     3594      4.55          0.53     109.2         15.0     7.3x
//!    631     4514      5.73          0.76     137.4         21.0     6.5x
//!    362     1860      2.25          0.33      54.0          9.4     5.8x
//! ```
//!
//! For scale: `menu_residue` answers twenty-four starts over conversation 28 in 0.8
//! seconds, and `repeat_question` put the whole per-REQUEST setup - the group graph and the
//! diagram side, which is all de-2wtl would save - at eighteen to twenty-seven
//! milliseconds. This was the larger waste, and it needs nothing that outlives the query.
//!
//! ### The first run of this was worse still, and the difference is a bug it found
//!
//! Before `IterationOrder::distanced_from` existed, the APART column read 98.0, 305.9,
//! 187.7, 246.1 and 86.4 - roughly twice what it reads now. `Known::of_from` was running
//! Tarjan TWICE: once inside `Known::of` and again inside `IterationOrder::of_from`, which
//! threw the first away. Every caller had been paying for that, not just a menu. So the
//! saving against the code this measurement was written to price is 246 ms to 21 on
//! conversation 631, about twelve times over.
//!
//! ## What it prices, and what it does not
//!
//! ONLY THE GRAPH-ONLY WORK. `Where::of` is called three times per start - by
//! `bridge::scored`, by `portfolio::best_novelty` and by `novelty_search::best_novelty` -
//! and is the same waste in the same place, but it needs a compiled world to run and so
//! belongs with the diagram side rather than here. The columns above are a LOWER BOUND on
//! what a menu spends rebuilding what it already had.

use std::time::{Duration, Instant};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::known::{GroupShape, Known};
use lookahead_engine::symbolic::order::IterationOrder;

#[path = "../tests/common/mod.rs"]
mod common;

/// The groups a player actually stands in for a while, which is the matrix's heavy list.
const CONVERSATIONS: [i32; 5] = [28, 368, 14, 631, 362];

/// How many starts a menu asks about.
///
/// TWENTY-FOUR, the same number `menu_residue` uses: a dozen options, every one of them a
/// rolled check, which de-fes makes two starts apiece. That is the top of the ordinary
/// range rather than a worst case.
const STARTS: usize = 24;

/// How many times each group is timed; the fastest is reported.
const REPEATS: usize = 5;

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let conversations = numbers("CONVERSATION", &CONVERSATIONS);
    let starts = from_env("STARTS", STARTS).max(1);
    let repeats = from_env("REPEATS", REPEATS).max(1);

    println!("a menu of {starts} starts, best of {repeats}\n");
    println!(
        "{:>6}  {:>8}  {:>10}  {:>12}  {:>10}  {:>12}  {:>9}",
        "conv", "entries", "alone ms", "per start ms", "apart ms", "together ms", "saving",
    );

    for conversation in conversations {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            eprintln!("conversation {conversation}'s group does not build; skipping.");
            continue;
        };

        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            eprintln!("no entry 0 in conversation {conversation}; skipping.");
            continue;
        }

        let mut best: Option<(Duration, Duration, Duration, Duration)> = None;
        for _ in 0..repeats {
            // ALONE: what one start pays when it works everything out for itself. The
            // forward slice's own order, plus `Known::of_from` for the backward half.
            let timing = Instant::now();
            let order = IterationOrder::of(&graph);
            let alone_order = timing.elapsed();
            std::hint::black_box(&order);

            let timing = Instant::now();
            let known = Known::of_from(&graph, start);
            let alone_known = timing.elapsed();
            std::hint::black_box(&known);

            // SHARED: the shape once for the whole menu, and then per start only the BFS
            // that measures distances from where THIS search begins.
            let timing = Instant::now();
            let shape = GroupShape::of(&graph);
            let shared_once = timing.elapsed();

            let timing = Instant::now();
            let from_shape = shape.known_from(&graph, start);
            let shared_each = timing.elapsed();
            std::hint::black_box(&from_shape);

            let row = (alone_order, alone_known, shared_once, shared_each);
            best = match best {
                Some(had) if had.0 + had.1 <= row.0 + row.1 => Some(had),
                _ => Some(row),
            };
        }

        let Some((alone_order, alone_known, shared_once, shared_each)) = best else { continue };

        // WHAT A MENU PAYS EITHER WAY, which is the number this exists for.
        let apart = (alone_order + alone_known) * starts as u32;
        let together = shared_once + shared_each * starts as u32;

        println!(
            "{conversation:>6}  {:>8}  {:>10.2}  {:>12.2}  {:>10.1}  {:>12.1}  {:>8.1}x",
            graph.count(),
            ms(alone_order + alone_known),
            ms(shared_each),
            ms(apart),
            ms(together),
            apart.as_secs_f64() / together.as_secs_f64().max(f64::MIN_POSITIVE),
        );
    }

    println!(
        "\nAPART is every start working the group's shape out for itself; TOGETHER is one \
         `GroupShape`\nfor the menu and a distance walk per start. The difference is the \
         same answer recomputed."
    );
}

fn ms(took: Duration) -> f64 {
    took.as_secs_f64() * 1000.0
}

fn from_env(name: &str, fallback: usize) -> usize {
    std::env::var(name).ok().and_then(|text| text.parse().ok()).unwrap_or(fallback)
}

fn numbers(name: &str, fallback: &[i32]) -> Vec<i32> {
    match std::env::var(name) {
        Ok(text) => text.split(',').filter_map(|part| part.trim().parse().ok()).collect(),
        Err(_) => fallback.to_vec(),
    }
}
