// SPDX-License-Identifier: MIT
//! What a SECOND question about the same group costs, which is what de-2wtl would save.
//!
//! ## The question
//!
//! `bridge::answer` keeps nothing between calls. Every request rebuilds the group graph
//! from the index - which parses the guard and the actions of every entry in it - then a
//! diagram manager, the compiled guards and the seed, and only then searches. A menu asks
//! one request carrying a dozen starts, so that setup is paid once per MENU; but the next
//! menu in the same conversation pays it all again, and a game session is hours of menus in
//! a handful of groups.
//!
//! de-2wtl is to give that work an owner that outlives a query. This is what the owner
//! would be worth, measured rather than assumed, because the answer decides whether it is
//! worth building at all: the setup is either most of what a menu costs or a rounding error
//! beside the search, and nobody had put a number on it.
//!
//! ## What it separates
//!
//! Three costs, and they have different fates under any design that shares work:
//!
//! - THE GRAPH. Reading the group out of the index and parsing every guard in it. Plain
//!   data, shareable across threads and across worlds, and the thing a cache would hold
//!   most easily.
//! - THE DIAGRAM SIDE. The manager, the compiled guards and the seed. Shareable only with
//!   the thread that built it - see [`lookahead_engine::symbolic::isolated`] - and only for
//!   ONE world snapshot, since the world is baked into the formulas.
//! - THE SEARCH. What is left, and the only part that has to happen per question.
//!
//! ## How to run it
//!
//! ```text
//! DEGCT_RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo repeat-question -- \
//!   cargo run --release --example repeat_question
//! ```
//!
//! `CONVERSATION` picks the groups, `BUDGET_MB` the allowance, `REPEATS` how many times
//! each is timed - the fastest is reported, since what is wanted is what the work costs
//! rather than what the machine was doing at the time.
//!
//! ## What it said, 2026-09-07, at the player's 256 MB
//!
//! ```text
//!   conv  entries  graph ms  diagram ms
//!     28     2186         4          16
//!    368     4724         8          18
//!     14     3594         8          17
//!    631     4514         9          18
//!    362     1860         3          15
//! ```
//!
//! ## And the diagram side splits again, which is what de-2wtl actually turns on
//!
//! A later run, 2026-09-07, with the manager timed apart from the compiler:
//!
//! ```text
//!   conv  entries  graph ms  diagram ms  of it mgr
//!     28     2186         5          10          9
//!    368     4724         8          14          9
//!     14     3594         7          13          9
//!    631     4514         8          14         10
//!    362     1860         3          11          9
//! ```
//!
//! THE MANAGER IS NEARLY ALL OF IT - nine or ten milliseconds of ten to fourteen - and the
//! compiler and seed are the remaining one to five.
//!
//! THAT IS THE OPPOSITE WAY ROUND FROM WHAT INVALIDATES. `DataLayout::for_group` reads the
//! world through `money()` alone - the money ceiling - so the layout, and the manager sized
//! from it, survives everything else the world does. The compiler and the seed do not:
//! `GuardCompiler::with_world` folds in the clock, the variables, the items, the tasks, the
//! thoughts and the world queries, and the seed carries what has been READ, which moves on
//! every line. (The compiler never asks `is_seen`; a seen-slot a guard reads is a tracked
//! variable whose starting value the seed supplies.)
//!
//! So a workspace keyed on the WORLD SNAPSHOT, which is what de-2wtl's design assumed,
//! would be thrown away almost every menu and buy nothing. A workspace that keeps the
//! MANAGER and rebuilds the compiler and seed per request keeps the nine or ten and pays
//! the one to five, and its invalidation rule is the money ceiling rather than everything.
//!
//! EIGHTEEN TO TWENTY-SEVEN MILLISECONDS, all in, on the five heaviest groups in the game.
//! That is the whole of what an owner outliving the query would stop paying.
//!
//! AND THE GRAPH IS THE CHEAP HALF, which is the opposite of what de-2wtl assumed. That
//! issue's case rests on `build_group_graph` parsing the guards and actions of every entry
//! in the group - "4,514 of them for conversation 631's, once per response menu" - and the
//! number is real but the cost is not: 631's whole group parses in NINE MILLISECONDS. The
//! diagram side is the larger half at fifteen to eighteen, and it is the half that is
//! hardest to share, since it belongs to one thread and one world snapshot.
//!
//! ### What that is against a real menu
//!
//! The search column here is capped at 250 ms and is a floor rather than a cost, so the
//! percentages it produces mean nothing. The figure to hold it against is
//! `menu_residue`'s: twenty-four starts over conversation 28, through `bridge::answer` at
//! the shipped budgets, in 0.8 seconds. So the setup is about THREE PER CENT of a menu, and
//! sharing it perfectly would save twenty-five milliseconds of eight hundred.
//!
//! ## So the reuse is not worth what it costs to own
//!
//! Not a measurement of the algorithm - a measurement of a proposal. de-2wtl would buy a
//! lifetime, a mailbox, an invalidation rule per world snapshot and per budget, and a
//! decision about what happens to a request for a group the owner does not hold. Twenty-five
//! milliseconds a menu does not pay for that, and the case for it should not be made again
//! without a number that contradicts this one.
//!
//! WHERE A NUMBER MIGHT STILL COME FROM, and this does not look for it: a group whose
//! layout is far larger than these, or a machine much slower than this one, or a menu
//! answered so often that a few milliseconds compound - none of which is conversation 631
//! on a desktop.

use std::time::{Duration, Instant};

use lookahead_engine::bridge::{SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::reachability::{Budget, Reachability, seed_of};
use lookahead_engine::symbolic::vars::DataVars;

#[path = "../tests/common/mod.rs"]
mod common;

/// The counter cap every symbolic measurement in this repository uses.
const COUNTER_CAP: i32 = 16;

/// The groups a player actually stands in for a while, which is the matrix's heavy list.
const CONVERSATIONS: [i32; 5] = [28, 368, 14, 631, 362];

/// What the manager is given, in megabytes.
///
/// THE PLAYER'S DEFAULT, not a measurement's six gigabytes, because the question is what a
/// MENU costs and a menu is answered under the shipped allowance.
const BUDGET_MB: usize = 256;

/// How long the search may run, so a group that never settles does not decide the run.
///
/// The search is not the subject here - the setup either side of it is - so this is short
/// on purpose and the search column should be read as "at least this", not as a cost.
const SEARCH_MS: u64 = 250;

/// How many times each group is timed; the fastest is reported.
const REPEATS: usize = 3;

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let conversations = numbers("CONVERSATION", &CONVERSATIONS);
    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);
    let repeats = from_env("REPEATS", REPEATS).max(1);
    let search = Duration::from_millis(from_env("SEARCH_MS", SEARCH_MS as usize) as u64);

    println!(
        "{} MB, search capped at {} ms, best of {repeats}\n",
        budget.memory() / (1024 * 1024),
        search.as_millis(),
    );
    println!(
        "{:>6}  {:>8}  {:>10}  {:>12}  {:>11}  {:>10}  {:>14}",
        "conv", "entries", "graph ms", "diagram ms", "of it mgr", "search ms", "setup share",
    );

    for conversation in conversations {
        let mut best: Option<(Duration, Duration, Duration, Duration, usize)> = None;

        for _ in 0..repeats {
            let began = Instant::now();
            let Ok((graph, _)) = build_group_graph(&index, conversation) else {
                eprintln!("conversation {conversation}'s group does not build; skipping.");
                break;
            };
            let graph_took = began.elapsed();

            let start = DialogueNodeId::new(conversation, 0);
            if graph.get(start).is_none() {
                eprintln!("no entry 0 in conversation {conversation}; skipping.");
                break;
            }

            // ONE THREAD, ONE MANAGER, as everything that builds one must - de-fpax.
            let (diagram_took, manager_took, search_took) = isolated::on_its_own_thread(|| {
                let symbols = graph.symbols().clone();
                let world = SnapshotWorld::declaring(
                    WorldSnapshot {
                        day_minutes: 720,
                        day_counter: 1,
                        ..Default::default()
                    },
                    None,
                );

                let building = Instant::now();
                // THE LAYOUT IS COUNTED WITH THE DIAGRAM, not with the graph. It is plain
                // data and could be shared like the graph, but it is derived per world -
                // `for_group` reads the world to decide what to track - so it belongs with
                // the things a changed save invalidates.
                let layout = DataLayout::for_group(&graph, &world, COUNTER_CAP);
                let vars = DataVars::new(&layout, &symbols, budget);
                // THE LAYOUT AND THE MANAGER SEPARATELY FROM THE REST, because they
                // invalidate on different things and de-2wtl turns on which. `for_group`
                // reads the world only through `money()` - the money ceiling - so the
                // layout, and therefore the manager sized from it, survives everything else
                // the world does. The compiler does not: `with_world` folds in the
                // variables, the items, the tasks, the queries, the check outcomes and
                // `is_seen`, and what the player has SEEN changes on every line they read.
                let manager = building.elapsed();
                let mut compiler = GuardCompiler::new(&vars)
                    .with_world(&world)
                    .with_constant_clock(DataLayout::group_passes_time(&graph));
                let seed = seed_of(&graph, &world, &vars).expect("room for a seed");
                let built = building.elapsed();

                let searching = Instant::now();
                let found = Reachability::explore_within(
                    &graph,
                    start,
                    &seed,
                    &mut compiler,
                    &world,
                    COUNTER_CAP as u32,
                    &Budget {
                        time: search,
                        ..Default::default()
                    },
                );
                std::hint::black_box(found.stats().entries_reached);

                (built, manager, searching.elapsed())
            });

            let row = (
                graph_took,
                diagram_took,
                manager_took,
                search_took,
                graph.count(),
            );
            best = match best {
                Some(had) if had.0 + had.1 <= row.0 + row.1 => Some(had),
                _ => Some(row),
            };
        }

        let Some((graph_took, diagram_took, manager_took, search_took, entries)) = best else {
            continue;
        };
        let setup = graph_took + diagram_took;
        let whole = setup + search_took;
        println!(
            "{conversation:>6}  {entries:>8}  {:>10}  {:>12}  {:>11}  {:>10}  {:>13.0}%",
            graph_took.as_millis(),
            diagram_took.as_millis(),
            manager_took.as_millis(),
            search_took.as_millis(),
            100.0 * setup.as_secs_f64() / whole.as_secs_f64().max(f64::EPSILON),
        );
    }

    println!(
        "\nSETUP IS WHAT A REPEAT QUESTION WOULD STOP PAYING. The graph is plain data and \
         shareable\nanywhere; the diagram side is shareable only with the thread that \
         built it and only for\none world snapshot. The search column is capped and is a \
         floor, not a cost."
    );
}

/// A number from the environment, or the default written down here.
fn from_env(name: &str, fallback: usize) -> usize {
    lookahead_engine::core::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(fallback)
}

/// A comma-separated list from the environment, or the default written down here.
fn numbers(name: &str, fallback: &[i32]) -> Vec<i32> {
    match lookahead_engine::core::env::var(name) {
        Ok(named) => named
            .split(',')
            .map(str::trim)
            .filter(|piece| !piece.is_empty())
            .map(|piece| {
                piece
                    .parse()
                    .unwrap_or_else(|_| panic!("{name}={piece:?} is not a number"))
            })
            .collect(),
        Err(_) => fallback.to_vec(),
    }
}
