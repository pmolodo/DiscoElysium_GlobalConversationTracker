// SPDX-License-Identifier: MIT
//! How often does the forward run SETTLE inside the budget the portfolio already gives it?
//!
//! ## The question, and why the answer splits de-bnjy.9 in two
//!
//! de-fawk built pruning - a settled forward run bounds what can arrive at an entry, so a
//! backward pass may narrow every pre-image by it, and on conversation 28 the backward half
//! falls from 19 ms to 2. It then left it OFF, because `portfolio::Budget::default` gives
//! the forward slice fifty milliseconds and 28 needs about fifty-seven to settle, so on the
//! shipped path there would be nothing settled to prune with.
//!
//! THAT ARGUMENT WAS MADE FROM ONE GROUP. Conversation 28 is the largest thing the heavy
//! list has that settles at all, and the game is 1,422 groups of which 1,372 hold about
//! forty-three entries each - see `group_census`. A group that size settles long before
//! fifty milliseconds, and pruning is off for every one of them today.
//!
//! So there are two different changes hiding under "turn settling on":
//!
//! - `Known::pruning(true)`, which is FREE and self-guarding. `Known` refuses to narrow
//!   anything unless `forward_settled`, so switching it on changes nothing whatsoever for a
//!   group that does not settle, and helps every group that already does. It costs no
//!   budget anywhere.
//! - RAISING THE FORWARD BUDGET, which is not free. It is spent per START, and a menu is a
//!   dozen starts - twenty-four when they are rolled checks - so ten milliseconds more buys
//!   28 its settle and costs 631 and 14 a quarter of a second per menu for nothing, since
//!   they do not settle in over a minute.
//!
//! This measures which groups fall on which side, so the first change can be made on
//! evidence and the second argued about separately.
//!
//! ## What it does
//!
//! For each group, builds the diagram side exactly as `bridge::answer_within` does and runs
//! one forward pass from the group's entry 0 under the shipped forward budget, reporting
//! whether the sets stopped moving.
//!
//! NO `halt_on`, deliberately. The portfolio's slice halts the moment it finds an entry of
//! the class it is hunting, and a halted run answers outright - there is no backward pass
//! and nothing to prune. The case this is about is the run that finds nothing, and that run
//! either settles or spends its budget.
//!
//! ONE MANAGER PER THREAD, per de-fpax, so each group gets its own.
//!
//! ## How to run it
//!
//! ```text
//! RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo settles-within -- \
//!   cargo run --release --example settles_within
//! ```
//!
//! `SPANNING` caps how many multi-conversation groups are tried, `LONE` how many
//! single-conversation ones, `FORWARD_MS` moves the budget being tested.

use std::collections::BTreeSet;
use std::time::Duration;

use lookahead_engine::bridge::{SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, discover_group, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::reachability::{seed_of, Budget, Reachability};
use lookahead_engine::symbolic::vars::DataVars;

#[path = "../tests/common/mod.rs"]
mod common;

const COUNTER_CAP: i32 = 16;

/// The player's allowance, because that is what the question is asked under.
const BUDGET_MB: usize = 256;

/// The forward slice's budget, from `portfolio::Budget::default`.
const FORWARD_MS: u64 = 50;

/// How many groups of each kind to try. The spanning ones are the fifty that matter; the
/// lone ones are sampled because there are 1,372 and they are all the same shape.
const SPANNING: usize = 50;
const LONE: usize = 120;

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);
    let forward = Duration::from_millis(from_env("FORWARD_MS", FORWARD_MS as usize) as u64);
    let spanning_wanted = from_env("SPANNING", SPANNING);
    let lone_wanted = from_env("LONE", LONE);

    // One representative per distinct group, split by whether it spans conversations -
    // `group_census` establishes that the set is the key and that the split by size is the
    // one that matters.
    let mut seen: BTreeSet<Vec<i32>> = BTreeSet::new();
    let mut spanning: Vec<i32> = Vec::new();
    let mut lone: Vec<i32> = Vec::new();
    let mut conversations: Vec<i32> = index.keys().copied().collect();
    conversations.sort_unstable();
    for conversation in conversations {
        let group = discover_group(&index, conversation);
        if !seen.insert(group.clone()) {
            continue;
        }
        if group.len() > 1 {
            spanning.push(conversation);
        } else {
            lone.push(conversation);
        }
    }
    spanning.truncate(spanning_wanted);
    lone.truncate(lone_wanted);

    println!(
        "{} MB, forward budget {} ms, {} spanning groups and {} lone ones\n",
        budget.memory() / (1024 * 1024),
        forward.as_millis(),
        spanning.len(),
        lone.len(),
    );

    for (name, sample) in [("spanning", &spanning), ("lone", &lone)] {
        let mut settled = 0usize;
        let mut spent = 0usize;
        let mut skipped = 0usize;
        let mut worst_settled = Duration::ZERO;
        let mut entries_settled = 0usize;
        let mut entries_spent = 0usize;

        for &conversation in sample.iter() {
            match settles(&index, conversation, budget, forward) {
                Some((true, took, entries)) => {
                    settled += 1;
                    entries_settled += entries;
                    worst_settled = worst_settled.max(took);
                }
                Some((false, _, entries)) => {
                    spent += 1;
                    entries_spent += entries;
                }
                None => skipped += 1,
            }
        }

        let tried = settled + spent;
        println!(
            "{name:>10}: {settled} of {tried} settled inside {} ms ({:.0}%), \
             {skipped} skipped",
            forward.as_millis(),
            100.0 * settled as f64 / tried.max(1) as f64,
        );
        println!(
            "{:>10}  settled groups average {:.0} entries, the slowest took {:.1?}",
            "",
            entries_settled as f64 / settled.max(1) as f64,
            worst_settled,
        );
        println!(
            "{:>10}  groups that spent the budget average {:.0} entries\n",
            "",
            entries_spent as f64 / spent.max(1) as f64,
        );
    }

    println!(
        "A group that SETTLES can be pruned with today, at no cost and under the budget \
         already\nshipped - Known refuses to narrow without forward_settled, so the switch \
         is self-guarding.\nRaising the budget is the separate question, and it is spent \
         per start whether or not it\nis claimed."
    );
}

/// Whether one group's forward run settles, how long it took, and how big it is.
fn settles(
    index: &lookahead_engine::index::Index,
    conversation: i32,
    budget: DiagramBudget,
    forward: Duration,
) -> Option<(bool, Duration, usize)> {
    let (graph, _) = build_group_graph(index, conversation).ok()?;
    let start = DialogueNodeId::new(conversation, 0);
    graph.get(start)?;
    let entries = graph.count();

    isolated::on_its_own_thread(|| {
        let symbols = graph.symbols().clone();
        let world = SnapshotWorld::declaring(
            WorldSnapshot { day_minutes: 720, day_counter: 1, ..Default::default() },
            None,
        );
        let layout = DataLayout::for_group(&graph, &world, COUNTER_CAP);
        let vars = DataVars::try_new(&layout, &symbols, budget)?;
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(&graph));
        let seed = seed_of(&graph, &world, &vars);

        let began = std::time::Instant::now();
        let found = Reachability::explore_within(
            &graph,
            start,
            &seed,
            &mut compiler,
            &world,
            COUNTER_CAP as u32,
            &Budget { time: forward, ..Default::default() },
        );

        Some((found.stats().reached_fixed_point, began.elapsed(), entries))
    })
}

fn from_env(name: &str, fallback: usize) -> usize {
    std::env::var(name).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(fallback)
}
