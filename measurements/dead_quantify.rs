// SPDX-License-Identifier: MIT
//! What forgetting each entry's dead slots is worth, on the set and on the menu.
//!
//! `measurements/live_ranges.rs` counted the OPPORTUNITY - half to three quarters of a
//! layout is dead at the average entry - and stopped there, because counting dead variables
//! says nothing about what quantifying them away costs. This applies the abstraction and
//! prices it.
//!
//! ## Two arms, picked by argument, because they can disagree
//!
//! - `sets` (the default) runs a forward search to a FIXED POINT over each group, both ways.
//!   It is the direct question: does the abstraction make the representation smaller, and
//!   does it pay for its own quantifiers.
//! - `menu` runs the shipped call - a forward slice, then the backward driver told what it
//!   found - over an adversarial menu, both ways. It is the question a player's wait is
//!   made of, and it is NOT the same question.
//!
//! THE MENU ARM CAN MOVE THE OTHER WAY, which is the reason it is here rather than assumed
//! from the first. The shipped slice is fifty milliseconds of a two-second budget and it
//! usually halts long before that, so the abstraction has little to do; where it matters is
//! whether a slice SETTLES, because only a settled run may narrow the backward passes
//! (`Known::restricted`). Abstraction pulls both ways there. Smaller sets settle sooner,
//! which is more pruning. An abstracted set is a LARGER set, which prunes less once it has
//! settled.
//!
//! ## What the abstraction is and why it cannot lose an answer
//!
//! See `reachability::Budget::forget_dead`. `tests/dead_quantify.rs` holds it to the claim
//! over twelve real groups rather than to the argument.
//!
//! ## How to run it
//!
//! ```text
//! RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo dead-quantify -- \
//!   cargo run --release --example dead_quantify
//! RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo dead-quantify-menu -- \
//!   cargo run --release --example dead_quantify menu
//! ```
//!
//! `CONVERSATION` narrows the groups, `BUDGET_MB` the manager, `STARTS` and `UNSEEN` the
//! menu.

use std::sync::Arc;
use std::time::{Duration, Instant};

use lookahead_engine::bridge::{SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::known::GroupShape;
use lookahead_engine::symbolic::live_slots::LiveSlots;
use lookahead_engine::symbolic::portfolio;
use lookahead_engine::symbolic::reachability::{self, Reachability, seed_of};
use lookahead_engine::symbolic::vars::DataVars;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

const COUNTER_CAP: i32 = 16;

/// The heavy list the other symbolic measurements use, so the rows read against theirs.
const CONVERSATIONS: [i32; 6] = [28, 368, 14, 631, 362, 1030];

/// The player's allowance, because the question is what a player would see.
const BUDGET_MB: usize = 256;

/// How long a fixed point may take in the `sets` arm.
///
/// GENEROUS AND STILL A LIMIT. 14 and 631 are known not to settle at all, and the arm is
/// worth running on them anyway - what a search reached in a fixed time is comparable
/// between the arms even when neither finished, as long as both got the same time.
const SETTLE_SECONDS: u64 = 120;

/// How many starts a menu asks about, and how many entries are unseen at the deep end.
const STARTS: usize = 24;
const UNSEEN: usize = 10;

fn main() {
    let arm = std::env::args().nth(1).unwrap_or_else(|| "sets".to_string());
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");
    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);

    match arm.as_str() {
        "sets" => sets_arm(&index, budget),
        "menu" => menu_arm(&index, budget),
        other => eprintln!("no arm called {other}; try `sets` or `menu`."),
    }
}

/// The fixed point over each group, held exactly and then abstracted.
fn sets_arm(index: &lookahead_engine::index::Index, budget: DiagramBudget) {
    println!(
        "{} MB, {SETTLE_SECONDS}s per search, forward to a fixed point\n",
        budget.memory() / (1024 * 1024),
    );
    println!(
        "{:>6}  {:>7}  {:>6}  {:>15}  {:>15}  {:>13}  {:>13}",
        "conv", "entries", "dead%", "exact", "forgetful", "nodes", "ms",
    );

    for conversation in numbers("CONVERSATION", &CONVERSATIONS) {
        let Some((graph, start)) = group(index, conversation) else { continue };
        let live = Arc::new(LiveSlots::of(&graph));
        let dead = dead_share(&graph, &live);

        let exact = fixed_point(&graph, start, budget, None);
        let forgetful = fixed_point(&graph, start, budget, Some(live));

        let (Some(exact), Some(forgetful)) = (exact, forgetful) else {
            eprintln!("conversation {conversation}: no room for the manager; skipping.");
            continue;
        };

        println!(
            "{conversation:>6}  {:>7}  {:>5.0}%  {:>15}  {:>15}  {:>13}  {:>13}",
            graph.count(),
            dead * 100.0,
            exact.describe(),
            forgetful.describe(),
            ratio(exact.nodes as f64, forgetful.nodes as f64),
            ratio(exact.took.as_secs_f64(), forgetful.took.as_secs_f64()),
        );
    }

    println!(
        "\nEXACT and FORGETFUL are `entries/nodes/stopped`, and the two ratio columns are \
         exact over\nforgetful - above 1.00 the abstraction won.\n\nREAD `stopped` BEFORE \
         THE ms COLUMN. `full` is a search that filled the manager and gave\nup; `clock` is \
         one that was still working when its time ran out. Those are not the same\nkind of \
         stop, and a `full` run against a `clock` run makes the abstraction look slow\n\
         precisely where it is keeping the search alive longest - compare their ENTRIES and \
         NODES\ninstead. Only two `settled` rows compare on ms.\n\nTWO SETTLED SEARCHES THAT \
         REACHED DIFFERENT ENTRIES would be a bug rather than a result,\nand \
         tests/dead_quantify.rs is what would have caught it."
    );
}

/// The shipped call over an adversarial menu, both ways.
fn menu_arm(index: &lookahead_engine::index::Index, budget: DiagramBudget) {
    let starts_wanted = from_env("STARTS", STARTS);
    let unseen_wanted = from_env("UNSEEN", UNSEEN);

    println!(
        "{} MB, {starts_wanted} starts, {unseen_wanted} deepest unseen, forward slice {} ms\n",
        budget.memory() / (1024 * 1024),
        portfolio::Budget::default().forwards.as_millis(),
    );
    println!(
        "{:>6}  {:>7}  {:>6}  {:>10}  {:>10}  {:>9}  {:>11}  {:>11}",
        "conv", "entries", "dead%", "off ms", "on ms", "saving", "off backward", "on backward",
    );

    for conversation in numbers("CONVERSATION", &CONVERSATIONS) {
        let Some((graph, start)) = group(index, conversation) else { continue };
        let Some(profile) = MenuProfile::of(&graph, start, unseen_wanted, starts_wanted) else {
            eprintln!("conversation {conversation}: every start would be refused; skipping.");
            continue;
        };
        let live = Arc::new(LiveSlots::of(&graph));
        let dead = dead_share(&graph, &live);
        let novelty = profile.novelty();

        let off = menu(&graph, &profile.starts, &novelty, budget, None);
        let on = menu(&graph, &profile.starts, &novelty, budget, Some(live));

        let (Some(off), Some(on)) = (off, on) else {
            eprintln!("conversation {conversation}: no room for the manager; skipping.");
            continue;
        };

        println!(
            "{conversation:>6}  {:>7}  {:>5.0}%  {:>10.0}  {:>10.0}  {:>8.2}x  {:>11}  {:>11}",
            graph.count(),
            dead * 100.0,
            off.0.as_secs_f64() * 1000.0,
            on.0.as_secs_f64() * 1000.0,
            off.0.as_secs_f64() / on.0.as_secs_f64().max(f64::MIN_POSITIVE),
            format!("{} of {}", off.1, profile.starts.len()),
            format!("{} of {}", on.1, profile.starts.len()),
        );
    }

    println!(
        "\nBACKWARD is how many starts reached the backward driver at all - a forward slice \
         that\nHALTS answers outright, and the abstraction cannot change WHICH starts do \
         that, only how\nfast. So a difference in those two columns is the interesting one: \
         it means the\nabstraction changed whether a slice SETTLED, which is the only thing \
         that lets a backward\npass be narrowed. Both arms are the shipped call at the \
         shipped budget."
    );
}

/// What one search cost, and what stopped it.
struct Cost {
    entries: usize,
    nodes: usize,
    stopped: &'static str,
    took: Duration,
}

impl Cost {
    fn describe(&self) -> String {
        format!("{}/{}/{}", self.entries, self.nodes, self.stopped)
    }
}

/// Why a run ended, which the ms column CANNOT be read without.
///
/// A search that filled the manager gave up early and one that spent the clock did not, so
/// two unsettled rows can differ by a factor of two in ms while the slower one is the one
/// that was allowed to keep working. Without this the abstraction looks like a slowdown on
/// exactly the groups where it is holding the search up longest.
fn stopped_by(stats: &reachability::ReachabilityStats) -> &'static str {
    if stats.reached_fixed_point {
        "settled"
    } else if stats.out_of_memory || stats.out_of_system_memory {
        "full"
    } else {
        "clock"
    }
}

/// One group's forward fixed point, on a thread of its own.
fn fixed_point(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    budget: DiagramBudget,
    forget_dead: Option<Arc<LiveSlots>>,
) -> Option<Cost> {
    isolated::on_its_own_thread(|| {
        let symbols = graph.symbols().clone();
        let world = measuring_world();
        let layout = DataLayout::for_group(graph, &world, COUNTER_CAP);
        let vars = DataVars::try_new(&layout, &symbols, budget)?;
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
        let seed = seed_of(graph, &world, &vars).expect("room for a seed");

        // THE CLOCK STARTS AFTER THE SETUP, unlike the performance matrix's, because the
        // two arms build the identical apparatus and folding it into both would only dilute
        // the difference this measurement is about. The analysis itself is built by the
        // CALLER and shared, so neither arm pays for it here either - it is a fact about the
        // group, computed once, exactly as `GroupShape` is.
        let began = Instant::now();
        let run = Reachability::explore_within(
            graph,
            start,
            &seed,
            &mut compiler,
            &world,
            COUNTER_CAP as u32,
            &reachability::Budget {
                time: Duration::from_secs(SETTLE_SECONDS),
                memory: budget.memory(),
                forget_dead,
                ..Default::default()
            },
        );
        let took = began.elapsed();

        Some(Cost {
            entries: run.stats().entries_reached,
            nodes: run.stats().diagram_nodes,
            stopped: stopped_by(run.stats()),
            took,
        })
    })
}

/// One menu's worth of shipped calls, and how many of them reached the backward driver.
fn menu<F>(
    graph: &LookAheadGraph,
    starts: &[DialogueNodeId],
    novelty: &F,
    budget: DiagramBudget,
    forget_dead: Option<Arc<LiveSlots>>,
) -> Option<(Duration, usize)>
where
    // SYNC, because the search runs on a thread of its own - de-fpax - and the closure
    // carried over is a shared reference to this one.
    F: Fn(DialogueNodeId) -> Novelty + Sync,
{
    isolated::on_its_own_thread(|| {
        let symbols = graph.symbols().clone();
        let world = measuring_world();
        let layout = DataLayout::for_group(graph, &world, COUNTER_CAP);
        let vars = DataVars::try_new(&layout, &symbols, budget)?;
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
        let seed = seed_of(graph, &world, &vars).expect("room for a seed");
        let shape = GroupShape::of(graph);
        let search = portfolio::Budget { forget_dead, ..Default::default() };

        let mut backward = 0;
        let began = Instant::now();
        for &start in starts {
            let Some(hunting) = graph.best_linked_class(start, novelty) else { continue };
            if hunting <= Novelty::SeenThisGame {
                continue;
            }
            let answer = portfolio::best_novelty(
                graph, start, StartBranch::Either, &seed, &mut compiler, &world,
                COUNTER_CAP as u32, novelty, hunting, &search, &shape,
            );
            if answer.by != portfolio::Answered::Forwards {
                backward += 1;
            }
            std::hint::black_box(&answer);
        }

        Some((began.elapsed(), backward))
    })
}

/// The group and its canonical start, or `None` where there is nothing to measure.
fn group(
    index: &lookahead_engine::index::Index,
    conversation: i32,
) -> Option<(LookAheadGraph, DialogueNodeId)> {
    let Ok((graph, _)) = build_group_graph(index, conversation) else {
        eprintln!("conversation {conversation}'s group does not build; skipping.");
        return None;
    };
    let start = DialogueNodeId::new(conversation, 0);
    graph.get(start)?;
    Some((graph, start))
}

/// The share of the layout's variables that are dead at the average entry.
///
/// The same figure `live_ranges` reports, restated here so a row carries the opportunity
/// beside what the opportunity was worth. A reader should not have to hold two tables
/// side by side to tell a group with nothing to gain from one that had plenty and gained
/// nothing anyway.
fn dead_share(graph: &LookAheadGraph, live: &LiveSlots) -> f64 {
    let world = measuring_world();
    let layout = DataLayout::for_group(graph, &world, COUNTER_CAP);

    let width = |slot: usize| layout.slot(slot).map(|(_, bits)| bits as usize).unwrap_or(0);
    let carried: usize = (0..layout.slot_count()).map(width).sum();
    if carried == 0 {
        return 0.0;
    }

    let mut dead = 0usize;
    let mut entries = 0usize;
    for node in graph.nodes() {
        dead += live.dead_out(node.id, &layout).into_iter().map(width).sum::<usize>();
        entries += 1;
    }

    dead as f64 / (carried * entries.max(1)) as f64
}

fn measuring_world() -> SnapshotWorld {
    SnapshotWorld::declaring(
        WorldSnapshot { day_minutes: 720, day_counter: 1, ..Default::default() },
        None,
    )
}

/// `exact` over `forgetful`, as a ratio a reader can scan - above one, the abstraction won.
fn ratio(exact: f64, forgetful: f64) -> String {
    format!("{:.2}x", exact / forgetful.max(f64::MIN_POSITIVE))
}

fn from_env(name: &str, fallback: usize) -> usize {
    std::env::var(name).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(fallback)
}

fn numbers(name: &str, fallback: &[i32]) -> Vec<i32> {
    match std::env::var(name) {
        Ok(text) => text.split(',').filter_map(|part| part.trim().parse().ok()).collect(),
        Err(_) => fallback.to_vec(),
    }
}
