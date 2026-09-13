// SPDX-License-Identifier: MIT
//! Where a matrix row's floor time actually goes, part by part.
//!
//! ## The question, and why a subtraction was not enough
//!
//! Most of a whole-game matrix run IS the floor: 4,130 of the 4,382 rows measured on
//! 2026-09-07 did no searching worth the name, and the 1,372-group tail still to come is
//! almost entirely floor. de-thlz.5 asks what to do about that, and the answer sorts
//! differently depending on one number: how much of the ~300 ms an engine spends before it
//! searches is the diagram manager PREALLOCATING its node store, against the layout, the
//! variables, the compiled guards and the seed.
//!
//! The matrix's own `setup` column is the four of them added together. That was enough to
//! establish the floor per row and not enough to pick a fix: the in-game columns report 16
//! ms of setup where the measurement columns report 350 to 460, and the only thing that
//! differs between them is the manager's size - which POINTS at preallocation without
//! measuring it. The ~500 ms the issue calls unattributed is a subtraction too. This times
//! each part instead.
//!
//! ## What it reports
//!
//! ONCE PER PROCESS - the index read, which every row of a one-row-per-process run pays
//! again.
//!
//! ONCE PER GROUP - `build_group_graph` and `candidates`, which every row of a group pays
//! again because the script pins one profile per process.
//!
//! ONCE PER ENGINE SETUP, split four ways: `DataLayout::for_group`, `DataVars::try_new`,
//! `GuardCompiler::new` and `seed_of`. Reported at BOTH allowances - the six-gigabyte
//! measurement budget a matrix row runs under, and the 256 MB a player's menu runs under -
//! because if preallocation is the cost then the two differ by the ratio of the budgets and
//! nothing else does.
//!
//! ## What it deliberately does not do
//!
//! No searching. The point is the part of a row that is paid whether or not there is
//! anything to find, so a row that searches would bury it. And no verdicts: this measures
//! cost, and the matrix is where answers come from.
//!
//! ## How to run it
//!
//! ```text
//! tools/run-logged.sh cargo row-overhead -- \
//!   cargo run --release --example row_overhead
//! ```
//!
//! `DEGCT_CONVERSATION=631,368` picks the groups; `DEGCT_ROUNDS=3` says how many times each part is
//! timed, and the row reports the FASTEST of them, since what is wanted is the cost of the
//! work rather than of the machine's worst moment.

use std::time::{Duration, Instant};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::world::ILookAheadWorld;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "seen_profile.rs"]
mod seen_profile;
use seen_profile::candidates;

/// The six heaviest groups, which is what the matrix's serial prefix measures.
const GROUPS: [i32; 6] = [362, 368, 631, 14, 28, 1030];

/// The counter cap a matrix row's layout is built with, so a setup here is that setup.
const COUNTER_CAP: i32 = 16;

/// How many times each part is timed.
const ROUNDS: usize = 3;

/// The allowances a setup is timed under, and what each one is.
const ALLOWANCES: [(&str, usize); 2] = [
    // What a matrix row gets. DiagramBudget::measurement().
    ("6 GB (matrix)", 6 * 1024 * 1024 * 1024),
    // What a player's response menu gets. LookAheadMemoryBudgetMb's default.
    ("256 MB (menu)", 256 * 1024 * 1024),
];

/// One setup, timed part by part.
struct Setup {
    layout: Duration,
    vars: Duration,
    compiler: Duration,
    seed: Duration,
}

impl Setup {
    fn total(&self) -> Duration {
        self.layout + self.vars + self.compiler + self.seed
    }

    /// The fastest of `rounds` attempts, part by part.
    ///
    /// PART BY PART RATHER THAN WHOLE-RUN, because the parts are what is being compared and
    /// a slow moment in one of them should not be attributed to the others. `None` where the
    /// allowance could not supply a manager, which is a fact about the budget rather than a
    /// timing.
    fn timed(
        graph: &LookAheadGraph,
        world: &dyn ILookAheadWorld,
        budget: DiagramBudget,
        rounds: usize,
    ) -> Option<Self> {
        let mut best: Option<Setup> = None;
        for _ in 0..rounds {
            let began = Instant::now();
            let layout = DataLayout::for_group(graph, world, COUNTER_CAP);
            let after_layout = began.elapsed();

            let began = Instant::now();
            let vars = DataVars::try_new(&layout, graph.symbols(), budget)?;
            let after_vars = began.elapsed();

            let began = Instant::now();
            let mut compiler = GuardCompiler::new(&vars)
                .with_world(world)
                .with_constant_clock(DataLayout::group_passes_time(graph));
            let after_compiler = began.elapsed();

            let began = Instant::now();
            let seed = seed_of(graph, world, &vars)?;
            let after_seed = began.elapsed();

            // THE COMPILER AND THE SEED ARE KEPT ALIVE UNTIL HERE. Dropping either before
            // the clock is read would put its teardown inside the next part's timing, and
            // the manager's teardown is not small.
            std::hint::black_box((&mut compiler, &seed));

            let round = Setup {
                layout: after_layout,
                vars: after_vars,
                compiler: after_compiler,
                seed: after_seed,
            };
            best = Some(match best {
                None => round,
                Some(kept) => Setup {
                    layout: kept.layout.min(round.layout),
                    vars: kept.vars.min(round.vars),
                    compiler: kept.compiler.min(round.compiler),
                    seed: kept.seed.min(round.seed),
                },
            });
        }
        best
    }
}

fn main() {
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index; skipping.");
        return;
    };

    let rounds = from_env("ROUNDS", ROUNDS);

    // ONCE PER PROCESS, and the first thing measured because it is the first thing paid.
    let began = Instant::now();
    let index = read_index(&path).expect("the index reads");
    let read = began.elapsed();

    let began = Instant::now();
    let world = common::measurement_save();
    let save = began.elapsed();

    // THE TRIMMED INDEX BESIDE IT, because it is the obvious question a reader of the row
    // above asks. It is the same records with everything no crawl reads removed - 15 MB
    // against 50 - and a matrix row does nothing but crawl. Timed rather than argued about;
    // whether the rows come out the same is a separate question this does not answer.
    let trimmed = common::shipped_index().map(|path| {
        let began = Instant::now();
        let index = read_index(&path).expect("the shipped index reads");
        let took = began.elapsed();
        std::hint::black_box(index);
        took
    });

    // THE FULL INDEX IS WHAT A MATRIX ROW READS - `performance_matrix::main` opens
    // `common::conversation_index()`. The figure the issue carried, 173 to 244 ms, is
    // `repeat_question`'s, and that one reads the trimmed copy.
    println!("once per process:");
    println!("  read the full index    {:>6.0} ms", ms(read));
    if let Some(took) = trimmed {
        println!("  read the trimmed index {:>6.0} ms", ms(took));
    }
    println!("  build the world        {:>6.0} ms\n", ms(save));

    println!("per group, paid again by every row of a one-row-per-process run\n");
    println!(
        "{:>6}  {:>10}  {:>12}  {:>12}",
        "conv", "entries", "graph ms", "candidates ms"
    );

    let groups = numbers("CONVERSATION", &GROUPS);
    let mut built: Vec<(i32, LookAheadGraph, DialogueNodeId)> = Vec::new();
    for conversation in groups {
        let began = Instant::now();
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let graph_ms = began.elapsed();

        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let began = Instant::now();
        let reachable = candidates(&graph, start);
        let candidates_ms = began.elapsed();
        if reachable.is_empty() {
            continue;
        }

        println!(
            "{conversation:>6}  {:>10}  {:>12.0}  {:>13.0}",
            graph.nodes().count(),
            ms(graph_ms),
            ms(candidates_ms),
        );
        built.push((conversation, graph, start));
    }

    if built.is_empty() {
        eprintln!("no group built; nothing measured.");
        return;
    }

    for (allowance, memory) in ALLOWANCES {
        let budget = DiagramBudget::new(memory);
        println!(
            "\nper engine setup at {allowance}, fastest of {rounds}\n\n\
             {:>6}  {:>10}  {:>10}  {:>10}  {:>8}  {:>9}  {:>9}",
            "conv", "layout ms", "vars ms", "guards ms", "seed ms", "total ms", "vars %",
        );

        for (conversation, graph, _) in &built {
            let Some(setup) = Setup::timed(graph, &world, budget, rounds) else {
                println!("{conversation:>6}  {:>10}", "no room");
                continue;
            };
            let total = setup.total();
            println!(
                "{conversation:>6}  {:>10.0}  {:>10.0}  {:>10.0}  {:>8.0}  {:>9.0}  {:>8.0}%",
                ms(setup.layout),
                ms(setup.vars),
                ms(setup.compiler),
                ms(setup.seed),
                ms(total),
                100.0 * ms(setup.vars) / ms(total).max(f64::MIN_POSITIVE),
            );
        }
    }

    // THE COMPARISON THE ISSUE TURNS ON, stated rather than left to a reader with two
    // tables. If the vars column is most of the total at six gigabytes and a small part of
    // it at 256 MB, the cost is the node store being preallocated and scales with the
    // allowance - which means de-thlz.4's budget division already speeds the tail up, and
    // the fixes aimed at parsing or compiling are aimed at the small half.
    println!(
        "\nRead the two vars columns against each other: the allowances differ by a factor \
         of {}, and\nanything that scales with that factor is the node store being \
         preallocated rather than\nwork done on the group.",
        ALLOWANCES[0].1 / ALLOWANCES[1].1,
    );
}

fn ms(took: Duration) -> f64 {
    took.as_secs_f64() * 1000.0
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
