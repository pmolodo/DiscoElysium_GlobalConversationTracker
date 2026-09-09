// SPDX-License-Identifier: MIT
//! What the apply cache is worth, so the split stops being a convention.
//!
//! ## The question
//!
//! A diagram manager preallocates two things out of one allowance: the node store, and the
//! APPLY CACHE that memoises diagram operations. `DiagramBudget` divides them at one cache
//! entry per four nodes, and that number has never been anything but a convention - de-1e8l.
//!
//! The cache CHANGES NO ANSWER. A smaller one recomputes results it could have remembered;
//! a larger one takes bytes the node store could have had. So this is a curve rather than a
//! correctness check, and it has two ends that can both go wrong:
//!
//! - too small, and the search recomputes and is slow;
//! - too large, and the store is short of nodes, so a search that could have finished runs
//!   out of room instead. A `found` becomes a `no-room`.
//!
//! ## What it does
//!
//! Fixes a total allowance and sweeps the split over it - a sixty-fourth, a sixteenth, an
//! eighth, a quarter, a half - on each of the heavy groups, reporting how long the search
//! took and whether it answered at all. The interesting output is where the curve flattens,
//! and whether it flattens in the same place for a group that fits easily and one that does
//! not.
//!
//! THE TOTAL IS HELD, which is the only thing that makes the rows comparable: a bigger
//! cache buys fewer nodes out of the same bytes rather than being added on top of them, so
//! `DiagramBudget::with_cache_split` moves what a node costs as well as what the cache
//! holds. Two rows that differed in total memory would be measuring two budgets rather than
//! one trade.
//!
//! ## How to run it
//!
//! ```text
//! RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo cache-split -- \
//!   cargo run --release --example cache_split
//! ```
//!
//! `CONVERSATION` picks the groups, `SPLITS` the splits, `BUDGET_MB` the total, and
//! `REPEATS` how many times each row is timed - the fastest is reported, since what is
//! wanted is what the work costs rather than what the machine was doing at the time.
//!
//! ONE THREAD PER SEARCH, through `symbolic::isolated`: a manager built on a thread that
//! has already built one is what makes the third search overflow the stack (de-fpax), and
//! this builds one per split per conversation per repeat.
//!
//! ## What it said, 2026-09-07, at 512 MB and a 60-second cap
//!
//! ```text
//!   conv  entries  split         nodes       cache  built  search   verdict   held nodes
//!     28     2186   1/64      15,339,168     239,674    2ms    54ms   settled      227,036
//!     28     2186   1/16      14,913,080     932,067    6ms    64ms   settled      227,036
//!     28     2186    1/8      14,510,024   1,813,753   12ms    67ms   settled      227,036
//!     28     2186    1/4      13,421,772   3,355,443   20ms    55ms   settled      227,036
//!     28     2186    1/2      11,930,464   5,965,232   39ms    57ms   settled      227,036
//!    368     4724   1/64      15,339,168     239,674    6ms  6805ms   settled    4,955,128
//!    368     4724   1/16      14,913,080     932,067    9ms  4682ms   settled    4,955,128
//!    368     4724    1/8      14,510,024   1,813,753   15ms  4625ms   settled    4,955,128
//!    368     4724    1/4      13,421,772   3,355,443   25ms  4493ms   settled    4,955,128
//!    368     4724    1/2      11,930,464   5,965,232   43ms  4709ms   settled    4,955,128
//!     14     3594   1/64      15,339,168     239,674    5ms    cap    gave-up   34,258,351
//!     14     3594   1/16      14,913,080     932,067    9ms    cap    gave-up   38,320,857
//!     14     3594    1/8      14,510,024   1,813,753   15ms    cap    gave-up   41,498,710
//!     14     3594    1/4      13,421,772   3,355,443   24ms    cap    gave-up   42,419,612
//!     14     3594    1/2      11,930,464   5,965,232   43ms    cap    gave-up   41,484,448
//!    631     4514   1/64      15,339,168     239,674    7ms 17682ms   NO-ROOM    9,507,000
//!    631     4514   1/16      14,913,080     932,067   10ms    cap    gave-up   22,770,543
//!    631     4514    1/8      14,510,024   1,813,753   15ms    cap    gave-up   27,254,207
//!    631     4514    1/4      13,421,772   3,355,443   25ms    cap    gave-up   28,824,872
//!    631     4514    1/2      11,930,464   5,965,232   44ms    cap    gave-up   28,531,927
//! ```
//!
//! HOW TO READ THE TWO KINDS OF ROW. A group that SETTLES is read on search time. A group
//! that does not is capped at sixty seconds whatever the split, so its time says nothing
//! and the column that matters is HELD NODES - how far the same search got in the same
//! time. More is better there.
//!
//! ### A QUARTER IS RIGHT, and this is the first thing behind it
//!
//! Every group with a signal peaks at 1/4 and is flat or worse either side:
//!
//! - 368 settles fastest at 1/4 (4493 ms), and the curve is already flat by 1/16 (4682) -
//!   but 1/64 costs half as long again (6805).
//! - 14 gets furthest at 1/4 (42.4M nodes) and 631 gets furthest at 1/4 (28.8M).
//! - Going wider to 1/2 buys nothing anywhere: 368 gets slower, 14 and 631 get less far,
//!   and construction doubles (25 ms to 44 ms).
//! - 28 is flat across the whole sweep, because it settles in fifty milliseconds and there
//!   is nothing for a cache to save.
//!
//! So the convention was well placed, and the reason to keep it is now a measurement rather
//! than the absence of one.
//!
//! ### THE ISSUE EXPECTED THE OPPOSITE FAILURE FROM THE ONE THAT HAPPENED
//!
//! de-1e8l predicted that a split too GENEROUS would starve the node store and turn a found
//! into a no-room. The only no-room in the table is at 1/64 - the STINGIEST cache, and the
//! row that bought the MOST nodes of any. 631 ran out of room at 17.7 seconds holding 9.5M
//! where every wider cache survived the full minute and got to 22-28M.
//!
//! That is worth understanding rather than filing as a curiosity: an apply cache is what
//! stops a subproblem being recomputed, and a recomputation ALLOCATES NODES. Starve the
//! cache and the same intermediate diagrams are built again and again, each time out of the
//! store the small cache was supposed to be protecting. So the two are not simply traded off
//! against each other at the bottom end - a cache too small costs time AND room.
//!
//! It also means the shape of this curve is not symmetric, and the safe direction to be
//! wrong in is WIDE. Too wide costs construction time and some nodes, and degrades
//! smoothly; too narrow can end a search outright.
//!
//! ### WHAT WAS NOT MEASURED
//!
//! THE PLAYER'S END. Every row here is at 512 MB, where the budget binds on the heavy
//! groups. At the shipped default of 256 MB the whole manager is half this and a quarter of
//! not-much may behave differently - `BUDGET_MB` is there to ask, and nobody has.
//!
//! And these are single searches over a group. What a MENU costs is several of them against
//! one manager, where the cache is warm for the second option onwards - which is the
//! arrangement `bridge::answer` actually uses, and a cache that pays for itself across
//! options may want to be wider than one measured on a single search.

use std::time::{Duration, Instant};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::reachability::{seed_of, Budget, Reachability};
use lookahead_engine::symbolic::vars::DataVars;

#[path = "../tests/common/mod.rs"]
mod common;

/// The counter cap every symbolic measurement in this repository uses.
const COUNTER_CAP: i32 = 16;

/// The groups that cost something, which is the matrix's heavy list. `CONVERSATION` moves it.
const CONVERSATIONS: [i32; 4] = [28, 368, 14, 631];

/// The splits to sweep, as nodes per cache entry - so a SMALLER number is a BIGGER cache.
///
/// `SPLITS` moves it. Four is what everything ships with and is in the middle on purpose:
/// the question is which way to move, not whether to move.
const SPLITS: [usize; 5] = [64, 16, 8, 4, 2];

/// The total allowance every row is held to, in megabytes. `BUDGET_MB` moves it.
///
/// THE GROUP-SIZED 512 rather than a measurement's six gigabytes, because the trade is only
/// visible where the budget BINDS. At six gigabytes the heavy groups have room to spare at
/// every split, so every row would finish and the curve would be flat for a reason that has
/// nothing to do with the cache.
const BUDGET_MB: usize = 512;

/// How long one search may run before it is a gave-up.
///
/// Long enough that the cap is not what the rows are measuring, short enough that a sweep
/// of twenty of them fits in an afternoon. `ROW_SECONDS` moves it.
const ROW_SECONDS: u64 = 120;

/// How many times each row is timed. `REPEATS` moves it.
///
/// THE FASTEST IS REPORTED. What is wanted is what the work costs, and a slow run is a
/// machine doing something else - which is noise in one direction only, so a minimum is the
/// honest summary rather than a mean.
const REPEATS: usize = 3;

/// What one row did.
///
/// BUILDING AND SEARCHING ARE TIMED APART, and the first cut of this did not do that. A
/// wider cache is a bigger preallocation, so the manager takes longer to construct - and on
/// a group whose search is fifty milliseconds that construction is most of the wall time.
/// Reporting one number made a wider cache look uniformly worse when what it had actually
/// done was cost more UP FRONT, which is a different claim: construction happens once per
/// request and the search happens once per option.
struct Row {
    verdict: &'static str,
    built: Duration,
    searched: Duration,
    /// Diagram nodes across every entry's set, which is what the search actually held.
    nodes: usize,
}

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");
    let world = common::measurement_save();

    let conversations = numbers("CONVERSATION", &CONVERSATIONS);
    let splits = numbers("SPLITS", &SPLITS.map(|s| s as i32))
        .into_iter()
        .map(|s| s as usize)
        .collect::<Vec<_>>();
    let total = from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024;
    let repeats = from_env("REPEATS", REPEATS).max(1);
    let cap = Duration::from_secs(from_env("ROW_SECONDS", ROW_SECONDS as usize) as u64);

    println!(
        "{} MB total, {} s a row, best of {repeats}",
        total / (1024 * 1024),
        cap.as_secs(),
    );
    println!(
        "a SMALLER split is a BIGGER cache: {} means one cache entry per {} nodes\n",
        splits.first().copied().unwrap_or(0),
        splits.first().copied().unwrap_or(0),
    );

    println!(
        "{:>6}  {:>8}  {:>6}  {:>12}  {:>12}  {:>9}  {:>9}  {:>10}  {:>12}",
        "conv", "entries", "split", "nodes", "cache", "built ms", "search ms", "verdict",
        "held nodes",
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

        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false)
            .keeping_only_read(&symbols, &DataLayout::read_by(&graph));

        for split in &splits {
            let budget = DiagramBudget::new(total).with_cache_split(*split);
            let mut best: Option<Row> = None;

            for _ in 0..repeats {
                // ONE THREAD, ONE MANAGER - see the module note and de-fpax. The manager,
                // the compiled guards and the sets are all built and dropped inside.
                let row = isolated::on_its_own_thread(|| {
                    let began = Instant::now();
                    let vars = DataVars::new(&layout, &symbols, budget);
                    let mut compiler = GuardCompiler::new(&vars)
                        .with_world(&world)
                        .with_constant_clock(DataLayout::group_passes_time(&graph));
                    let seed = seed_of(&graph, &world, &vars).expect("room for a seed");
                    let built = began.elapsed();

                    let searching = Instant::now();
                    let found = Reachability::explore_within(
                        &graph,
                        start,
                        &seed,
                        &mut compiler,
                        &world,
                        COUNTER_CAP as u32,
                        &Budget { time: cap, ..Default::default() },
                    );

                    // READ BEFORE THE MANAGER GOES, and read at all so nothing can be
                    // optimised away. `out_of_memory` is what tells a search that ran out
                    // of room from one that ran out of clock, which is the whole
                    // right-hand end of this curve.
                    let searched = searching.elapsed();
                    let stats = found.stats();
                    Row {
                        verdict: verdict(stats),
                        built,
                        searched,
                        nodes: stats.diagram_nodes,
                    }
                });

                // BEST BY SEARCH TIME, since that is the column the question is about.
                best = match best {
                    Some(had) if had.searched <= row.searched => Some(had),
                    _ => Some(row),
                };
            }

            let row = best.expect("at least one repeat");
            println!(
                "{conversation:>6}  {:>8}  {:>6}  {:>12}  {:>12}  {:>9}  {:>9}  {:>10}  {:>12}",
                graph.count(),
                format!("1/{split}"),
                budget.nodes(),
                budget.cache_entries(),
                row.built.as_millis(),
                row.searched.as_millis(),
                row.verdict,
                row.nodes,
            );
            flush();
        }
    }

    println!(
        "\nA ROW IS THE SAME SEARCH EVERY TIME and the cache changes no answer, so a \
         verdict that\nmoves across a line is the split running the store out of nodes - \
         not a different result."
    );
}

/// What a finished search is, in the matrix's vocabulary.
///
/// THE ORDER MATTERS. A search that ran out of nodes also failed to reach a fixed point, so
/// asking about the fixed point first would report every no-room as a gave-up - and those
/// are the two ends of this curve, told apart.
fn verdict(stats: &lookahead_engine::symbolic::reachability::ReachabilityStats) -> &'static str {
    if stats.out_of_memory {
        "no-room"
    } else if stats.reached_fixed_point {
        "settled"
    } else {
        "gave-up"
    }
}

fn flush() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

/// A number from the environment, or the default written down here.
fn from_env(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(fallback)
}

/// A comma-separated list from the environment, or the default written down here.
fn numbers(name: &str, fallback: &[i32]) -> Vec<i32> {
    match std::env::var(name) {
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
