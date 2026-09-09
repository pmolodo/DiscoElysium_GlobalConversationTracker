// SPDX-License-Identifier: MIT
//! Does a forward run still settle when the seen-slots are left FREE?
//!
//! ## The gate on de-bnjy.11
//!
//! That issue would settle a forward run once per group and let every menu in the group
//! prune with it. The obstacle was that a settled run is baked to a world, and the world
//! moves between menus - so a run settled for one menu is not sound for the next.
//!
//! WHICH HALF OF THE WORLD MOVES IS THE POINT, and reading the code says: usually only the
//! seed. `GuardCompiler::with_world` takes the clock, the variables, the items, the tasks,
//! the thoughts and the world queries - things that move when the player ACTS. What has
//! been READ reaches only the seed, through `core::state` seeding each node's seen-slot
//! from `world.is_seen`. The compiler never asks `is_seen` at all.
//!
//! So: SETTLE FROM A SEED WITH THE SEEN-SLOTS EXISTENTIALLY QUANTIFIED AWAY. Forward sets
//! only grow, and starting from more states can only reach more, so such a run
//! over-approximates the run from any real seed that differs only in what has been read -
//! and pruning needs exactly an over-approximation (`Known::restricted` intersects against
//! it). One settled run would then serve every menu in the conversation.
//!
//! ## The two things that could kill it, and this measures the first
//!
//! 1. IT MIGHT NOT SETTLE. A freer seed has more to explore. Conversation 28 settles in
//!    about 57 ms from a real seed; from a relaxed one it may not settle at all, and then
//!    there is nothing to prune with.
//! 2. THE BOUND MIGHT BE WORTHLESS. An over-approximation that admits nearly everything
//!    narrows nearly nothing. That is `prune_on_menus`'s question, not this one.
//!
//! ## What it said, 2026-09-07, and the answer is no - for a reason that is NOT the relaxation
//!
//! ```text
//!   conv  entries  freed   real ms     real  relaxed ms  relaxed
//!     28     2186     16      5443    spent        7468    spent
//!    368     4724     31      5359    spent        6771    spent
//!     14     3594     23      5463    spent        6890    spent
//!    631     4514     25      5462    spent        6959    spent
//!    362     1860     19      5381    spent        6658    spent
//!   1030     1476      0        34  settled          10  settled
//! ```
//!
//! THE REAL RUN DOES NOT SETTLE EITHER. Five seconds at the player's 256 MB, from the
//! group's own entry 0, and not one of the five heavy groups finishes - with the seed
//! untouched. So the relaxation is not what kills it, and this measurement answers a
//! different question from the one it was written for.
//!
//! ## Why that does not contradict `settles_within` or `prune_on_menus`
//!
//! Because THEY DO NOT START HERE. The portfolio's forward slice runs
//! `Reachability::explore_branch_within` from ONE OPTION - an entry partway down the group,
//! with only what lies beyond it left to explore. That is what settles inside fifty
//! milliseconds for most groups, and it is what pruning uses today.
//!
//! A run from the group's ROOT has the whole group in front of it, and on these five that is
//! more than five seconds' worth. Conversation 1030 settles because it has no seen or once
//! slots at all (`freed` is 0) and is the smallest group here.
//!
//! ## What that means for de-bnjy.11
//!
//! The cheap version of the idea - settle ONE run per group and let every menu prune with it
//! - has no run to settle. A single shared run has to start somewhere all the starts can be
//! measured from, and the only such place is the root, which does not finish.
//!
//! So a shared settled run would have to be per START and not per group, which is what the
//! portfolio already computes and already throws away at the end of each request. KEEPING
//! THOSE - a settled run per start, cached on the workspace and reused while the world's
//! non-seen half holds - is the version of this issue that the numbers still allow. It is a
//! smaller idea than "settle the group once", and it wants its own measurement: how often
//! does the same start get asked about twice in one conversation?
//!
//! ## How to run it
//!
//! ```text
//! DEGCT_RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo relaxed-settle -- \
//!   cargo run --release --example relaxed_settle
//! ```

use std::time::{Duration, Instant};

use oxidd::{BooleanFunction, BooleanFunctionQuant};

use lookahead_engine::bridge::{COUNTER_CAP, SnapshotWorld, WorldSnapshot};
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

const CONVERSATIONS: [i32; 6] = [28, 368, 14, 631, 362, 1030];

/// The player's allowance, since this would run inside a player's workspace.
const BUDGET_MB: usize = 256;

/// How long a settle is allowed. GENEROUS: this runs off the menu's critical path, so
/// seconds are affordable in a way milliseconds-per-start never were.
const SETTLE_MS: u64 = 5_000;

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);
    let settle = Duration::from_millis(from_env("SETTLE_MS", SETTLE_MS as usize) as u64);

    println!(
        "{} MB, {} ms to settle\n",
        budget.memory() / (1024 * 1024),
        settle.as_millis(),
    );
    println!(
        "{:>6}  {:>8}  {:>7}  {:>12}  {:>10}  {:>12}  {:>10}",
        "conv", "entries", "freed", "real ms", "real", "relaxed ms", "relaxed",
    );

    for conversation in numbers("CONVERSATION", &CONVERSATIONS) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let Some((freed, real, relaxed)) =
            isolated::on_its_own_thread(|| run(&graph, start, budget, settle))
        else {
            println!("{conversation:>6}  no room for the manager");
            continue;
        };

        println!(
            "{conversation:>6}  {:>8}  {freed:>7}  {:>12.0}  {:>10}  {:>12.0}  {:>10}",
            graph.count(),
            real.0.as_secs_f64() * 1000.0,
            if real.1 { "settled" } else { "spent" },
            relaxed.0.as_secs_f64() * 1000.0,
            if relaxed.1 { "settled" } else { "spent" },
        );
    }

    println!(
        "\nRELAXED is the run de-bnjy.11 would keep: the seen-slots quantified away, so it \
         over-\napproximates every world that differs only in what the player has read. A \
         row that says\n'spent' has nothing to keep."
    );
}

/// Both runs on one thread and one manager, which is the arrangement de-fpax requires.
#[allow(clippy::type_complexity)]
fn run(
    graph: &lookahead_engine::graph::graph::LookAheadGraph,
    start: DialogueNodeId,
    budget: DiagramBudget,
    settle: Duration,
) -> Option<(usize, (Duration, bool), (Duration, bool))> {
    let symbols = graph.symbols().clone();
    let world = SnapshotWorld::declaring(
        WorldSnapshot {
            day_minutes: 720,
            day_counter: 1,
            ..Default::default()
        },
        None,
    );
    let layout = DataLayout::for_group(graph, &world, COUNTER_CAP);
    let vars = DataVars::try_new(&layout, &symbols, budget)?;
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(&world)
        .with_constant_clock(DataLayout::group_passes_time(graph));

    let seed = seed_of(graph, &world, &vars).expect("room for a seed");

    // THE SEEN AND ONCE SLOTS, which are exactly what `is_seen` decides the starting value
    // of. Quantifying their variables away leaves a seed that says nothing about what has
    // been read, and therefore covers every save that has read anything.
    let mut cube = vars.top();
    let mut freed = 0;
    for node in graph.nodes() {
        for slot in [node.seen_slot, node.once_slot] {
            if slot < 0 {
                continue;
            }
            if let Some(one) = vars.slot_cube(slot as usize) {
                if let Ok(wider) = cube.and(&one) {
                    cube = wider;
                    freed += 1;
                }
            }
        }
    }
    let relaxed = seed.exists(&cube).unwrap_or_else(|_| vars.top());

    let measure = |from: &oxidd::bdd::BDDFunction, compiler: &mut GuardCompiler| {
        let began = Instant::now();
        let found = Reachability::explore_within(
            graph,
            start,
            from,
            compiler,
            &world,
            COUNTER_CAP as u32,
            &Budget {
                time: settle,
                ..Default::default()
            },
        );
        (began.elapsed(), found.stats().reached_fixed_point)
    };

    let real = measure(&seed, &mut compiler);
    let relaxed = measure(&relaxed, &mut compiler);
    Some((freed, real, relaxed))
}

fn from_env(name: &str, fallback: usize) -> usize {
    lookahead_engine::core::env::var(name)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(fallback)
}

fn numbers(name: &str, fallback: &[i32]) -> Vec<i32> {
    match lookahead_engine::core::env::var(name) {
        Ok(text) => text
            .split(',')
            .filter_map(|part| part.trim().parse().ok())
            .collect(),
        Err(_) => fallback.to_vec(),
    }
}
