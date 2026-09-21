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
//! tools/run-logged.sh cargo cache-split -- \
//!   cargo run --release --example cache_split
//! ```
//!
//! `--conversation` picks the groups, `--split` the splits, `--budget-mb` the total, and
//! `--repeats` how many times each row is timed - the fastest is reported, since what is
//! wanted is what the work costs rather than what the machine was doing at the time.
//!
//! ONE THREAD PER SEARCH, through `symbolic::isolated`: a manager built on a thread that
//! has already built one is what makes the third search overflow the stack (de-fpax), and
//! this builds one per split per conversation per repeat.
//!
//! ## What it said, 2026-09-09: THE SPLIT DOES NOT MOVE ONE PASS AT ALL
//!
//! 512 MB, a 60-second cap, best of three, at the heaviest target in each group:
//!
//! ```text
//!   conv  entries  split      built ms  search ms   verdict   held nodes
//!     28     2186   1/64             3          2   settled           46
//!     28     2186    1/2            62          2   settled           46
//!    368     4724   1/64             5         54   settled      104,924
//!    368     4724   1/16            10         57   settled      104,924
//!    368     4724    1/8            21         57   settled      104,924
//!    368     4724    1/4            34         54   settled      104,924
//!    368     4724    1/2            62         55   settled      104,924
//!     14     3594   1/64             5         86   settled       18,601
//!     14     3594    1/2            66         97   settled       18,601
//!    631     4514   1/64             6          5   settled          379
//!    631     4514    1/2            64          5   settled          379
//! ```
//!
//! EVERY ROW SETTLES AND THE SEARCH COLUMN IS FLAT. Conversation 368 is 54 to 57 ms across a
//! twenty-fold change in cache size, and the differences are smaller than the run-to-run
//! noise. There is no no-room anywhere and no row reaches the cap.
//!
//! ## The workload is three orders of magnitude too small, and that is the finding
//!
//! One backward pass to the heaviest target in the group holds 105,000 diagram nodes at
//! worst. A cache sized at a sixty-fourth of the store is 239,674 entries - more than twice
//! the whole working set - so at every split on this sweep the cache is larger than the
//! problem, nothing is ever evicted, and there is nothing for the split to trade.
//!
//! WHAT THIS MEASURES NOW, therefore, is that A SINGLE PASS IS CACHE-INSENSITIVE, which is
//! worth knowing and is not what the file was written to ask. The only column that moves is
//! `built ms`, three to sixty-six milliseconds, because a wider cache costs more to
//! allocate - so on a workload this size a wide split is pure loss.
//!
//! ## Where the question actually lives now
//!
//! `performance/cache_split_menu.rs`, which sweeps the same split over a WHOLE MENU at the
//! player's own 256 MB. A menu is a dozen searches against one manager, so its working set
//! is the one that can outgrow a cache, and it is also the thing a player waits for. That
//! file is where the case for the shipped quarter rests: no split is best everywhere - a
//! sixty-fourth is bad on 1030, a half is bad on 362 - and a quarter is within noise of the
//! best on every group and worst on none, which is what a default should be.
//!
//! ## Whether this file still earns its place
//!
//! It answers a narrower question than it used to and should be read as answering that one:
//! how a single fixed point behaves when the cache cannot bind. Anyone changing the split
//! should read the menu sweep instead, and anyone who makes a single pass much larger -
//! a wider layout, a group the size of several - should come back here first to see whether
//! the cache has started to bind again.
//!

use std::time::{Duration, Instant};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::backward::{Backward, Budget, SettledPass};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;

use gct_measure::common;

use gct_measure::options;

/// The counter cap every symbolic measurement in this repository uses.
const COUNTER_CAP: i32 = 16;

/// The groups that cost something, which is the matrix's heavy list. `--conversation` moves it.
const CONVERSATIONS: [i32; 4] = [28, 368, 14, 631];

/// The splits to sweep, as nodes per cache entry - so a SMALLER number is a BIGGER cache.
///
/// `--split` moves it. Four is what everything ships with and is in the middle on purpose:
/// the question is which way to move, not whether to move.
const SPLITS: [usize; 5] = [64, 16, 8, 4, 2];

/// The total allowance every row is held to, in megabytes. `--budget-mb` moves it.
///
/// THE GROUP-SIZED 512 rather than a measurement's six gigabytes, because the trade is only
/// visible where the budget BINDS - and as of 2026-09-09 it does not bind here even at this,
/// since one pass's working set is a fraction of the narrowest cache on the sweep. Raising
/// it would make the curve flatter still; what would make it bind is a larger WORKLOAD, which
/// is `cache_split_menu`.
const BUDGET_MB: usize = 512;

/// How long one search may run before it is a gave-up.
///
/// Long enough that the cap is not what the rows are measuring, short enough that a sweep
/// of twenty of them fits in an afternoon. `--row-seconds` moves it.
const ROW_SECONDS: u64 = 120;

/// How many times each row is timed. `--repeats` moves it.
///
/// THE FASTEST IS REPORTED. What is wanted is what the work costs, and a slow run is a
/// machine doing something else - which is noise in one direction only, so a minimum is the
/// honest summary rather than a mean.
const REPEATS: usize = 3;

/// What this driver takes. With no group or split named it uses the lists above.
#[derive(clap::Parser)]
#[command(about = "What splitting the computed-table cache costs, group by group.")]
struct Options {
    #[command(flatten)]
    groups: options::Groups,
    #[command(flatten)]
    budget: options::Budget<BUDGET_MB>,
    /// Which cache splits to sweep; repeat the flag or comma-separate
    #[arg(long = "split", value_name = "N", value_delimiter = ',')]
    splits: Vec<usize>,
    /// How many times each group is timed; the fastest is reported
    #[arg(long, value_name = "N", default_value_t = REPEATS)]
    repeats: usize,
    /// How long one row may take, in seconds
    #[arg(long = "row-seconds", value_name = "S", default_value_t = ROW_SECONDS)]
    row_seconds: u64,
}

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
    let asked = <Options as clap::Parser>::parse();
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");
    let world = common::measurement_save();

    let conversations = asked.groups.or(&CONVERSATIONS);
    let splits: Vec<usize> = if asked.splits.is_empty() {
        SPLITS.to_vec()
    } else {
        asked.splits.clone()
    };
    let total = asked.budget.bytes();
    let repeats = asked.repeats.max(1);
    let cap = Duration::from_secs(asked.row_seconds);

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
        "conv",
        "entries",
        "split",
        "nodes",
        "cache",
        "built ms",
        "search ms",
        "verdict",
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

        // THE ENTRY THE MOST OTHERS CAN REACH, because that is exactly what a backward pass
        // visits: its fixed point spans the target's ancestors and nothing else, so the entry
        // with the most of them is the one that makes a search big enough for a cache to
        // matter. See common::heaviest_target for why depth is the wrong proxy.
        let target = common::heaviest_target(&graph, start);

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
                    let found = Backward::reaching_within(
                        &graph,
                        target,
                        &mut compiler,
                        &world,
                        COUNTER_CAP as u32,
                        &Budget {
                            time: cap,
                            ..Default::default()
                        },
                    );
                    // READ SO THE SEED IS NOT DEAD WEIGHT. Building it is half of what the
                    // "built ms" column measures, and a seed nothing asks about is a seed an
                    // optimiser may decline to build.
                    std::hint::black_box(found.reachable_from(start, &seed));

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
fn verdict(stats: &lookahead_engine::symbolic::backward::BackwardStats) -> &'static str {
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
