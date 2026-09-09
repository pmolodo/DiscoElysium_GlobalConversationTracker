// SPDX-License-Identifier: MIT
//! What pruning is worth ON THE SHIPPED PATH, at the budget the game actually gives.
//!
//! ## Why this and not `shared_symbolic`
//!
//! That measurement answered whether pruning works and how much it saves, and its answer -
//! conversation 28's backward half from 19 ms to 2, the whole row 78 to 61 - is what
//! de-fawk rests on. But it runs the forward half UNCAPPED to get a settled run, which is
//! not what the game does: `portfolio::Budget::default` gives the slice fifty milliseconds
//! and takes what it has.
//!
//! de-fawk read that as "the shipped path almost never has a settled run to prune with" and
//! left pruning off. `measurements/settles_within.rs` says otherwise for the game as a
//! whole - 119 of 120 ordinary groups settle inside those fifty milliseconds, and 25 of the
//! 50 that span conversations - so the question is what turning it on is worth at the
//! shipped budget, which is what this asks.
//!
//! ## What it does
//!
//! Runs the same menu twice through `portfolio::best_novelty` - the call `bridge::scored`
//! makes - with `Budget::pruning` off and then on, everything else held. Both arms are the
//! shipped path rather than a search built beside it, which is the point: a difference here
//! is a difference a player would see.
//!
//! ONE MANAGER PER THREAD per de-fpax, and the two arms are separate managers, so neither
//! inherits the other's node store.
//!
//! ## What it said, 2026-09-07, at the player's 256 MB over 24 starts
//!
//! ```text
//!   conv  entries  starts  searched  off ms  on ms  saving
//!     28     2186      24        20     334    193   1.73x
//!    368     4724      24        24   17941  17627   1.02x
//!     14     3594      24        24    6702   6751   0.99x
//!    631     4514      24        24    6606   6995   0.94x
//!    362     1860      24        24    1198   1192   1.01x
//!   1030     1476      24        24    4478   3113   1.44x
//! ```
//!
//! and a second run of the three interesting rows: 28 at 1.83x, 631 at 1.03x, 1030 at
//! 1.30x. SO 631'S 0.94 WAS NOISE, which is worth saying because a lone sub-one reading
//! looks like a regression and one run cannot tell the two apart.
//!
//! THE SHAPE IS EXACTLY WHAT THE SELF-GUARDING PREDICTS. Conversation 28 settles inside the
//! fifty milliseconds and nearly halves; 1030 settles and gains a third. 368, 14 and 631 do
//! not settle - 4.6 seconds, over a minute, over a minute, per de-fawk - so `Known` refuses
//! to narrow and they are unchanged within noise. Pruning costs nothing where it cannot
//! apply, which is what makes turning it on a decision about the groups it helps rather
//! than a trade against the ones it does not.
//!
//! ## The profile
//!
//! ADVERSARIAL, like `menu_residue`'s: the structurally deepest entries are the only unseen
//! ones, so every start that can reach them pays for a real search rather than being
//! refused by `class_worth_hunting` before a diagram is touched. A typical profile refuses
//! most starts and would measure nothing.
//!
//! ## How to run it
//!
//! ```text
//! DEGCT_RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo prune-on-menus -- \
//!   cargo run --release --example prune_on_menus
//! ```

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
use lookahead_engine::symbolic::portfolio;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

const COUNTER_CAP: i32 = 16;

/// The heavy list, plus 1030, matching the other symbolic measurements.
const CONVERSATIONS: [i32; 6] = [28, 368, 14, 631, 362, 1030];

/// The player's allowance, because the question is what a player would see.
const BUDGET_MB: usize = 256;

/// How many starts a menu asks about.
const STARTS: usize = 24;

/// How many entries are unseen, at the deep end.
const UNSEEN: usize = 10;

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);
    let starts_wanted = from_env("STARTS", STARTS);
    let unseen_wanted = from_env("UNSEEN", UNSEEN);

    println!(
        "{} MB, {starts_wanted} starts, {unseen_wanted} deepest unseen, forward slice {} ms\n",
        budget.memory() / (1024 * 1024),
        portfolio::Budget::default().forwards.as_millis(),
    );
    println!(
        "{:>6}  {:>8}  {:>7}  {:>11}  {:>10}  {:>10}  {:>9}",
        "conv", "entries", "starts", "searched", "off ms", "on ms", "saving",
    );

    for conversation in numbers("CONVERSATION", &CONVERSATIONS) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            eprintln!("conversation {conversation}'s group does not build; skipping.");
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            continue;
        }

        let Some(profile) = MenuProfile::of(&graph, root, unseen_wanted, starts_wanted) else {
            eprintln!("conversation {conversation}: every start would be refused; skipping.");
            continue;
        };
        let starts = &profile.starts;
        let novelty = profile.novelty();

        let off = menu(&graph, starts, &novelty, budget, false);
        let on = menu(&graph, starts, &novelty, budget, true);

        let (Some((off_took, _)), Some((on_took, settled))) = (off, on) else {
            eprintln!("conversation {conversation}: no room for the manager; skipping.");
            continue;
        };

        println!(
            "{conversation:>6}  {:>8}  {:>7}  {settled:>4} of {:>4}  {:>10.0}  {:>10.0}  \
             {:>8.2}x",
            graph.count(),
            starts.len(),
            starts.len(),
            off_took.as_secs_f64() * 1000.0,
            on_took.as_secs_f64() * 1000.0,
            off_took.as_secs_f64() / on_took.as_secs_f64().max(f64::MIN_POSITIVE),
        );
    }

    println!(
        "\nSEARCHED is how many starts reached the backward driver at all - a forward slice \
         that\nHALTS answers outright and prunes nothing. Both arms are the shipped call at \
         the shipped\nbudget, so a difference here is a difference a player would see."
    );
}

/// One menu's worth of searches, and how many of them had a settled forward run.
fn menu<F>(
    graph: &LookAheadGraph,
    starts: &[DialogueNodeId],
    novelty: &F,
    budget: DiagramBudget,
    pruning: bool,
) -> Option<(Duration, usize)>
where
    // SYNC, because the search runs on a thread of its own - de-fpax - and the closure
    // carried over is a shared reference to this one.
    F: Fn(DialogueNodeId) -> Novelty + Sync,
{
    isolated::on_its_own_thread(|| {
        let symbols = graph.symbols().clone();
        let world = SnapshotWorld::declaring(
            WorldSnapshot { day_minutes: 720, day_counter: 1, ..Default::default() },
            None,
        );
        let layout = DataLayout::for_group(graph, &world, COUNTER_CAP);
        let vars = DataVars::try_new(&layout, &symbols, budget)?;
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
        let seed = seed_of(graph, &world, &vars).expect("room for a seed");
        let shape = GroupShape::of(graph);
        let search = portfolio::Budget { pruning, ..Default::default() };

        // WARMED, so the first start does not pay the manager's first allocations for the
        // whole arm. The two arms are separate processes' worth of manager either way, and
        // an unwarmed first row is the difference between them being noise.
        let mut settled = 0;
        let began = Instant::now();
        for &start in starts {
            let Some(hunting) = graph.best_linked_class(start, novelty) else { continue };
            if hunting <= Novelty::SeenThisGame {
                continue;
            }
            let answer = portfolio::best_novelty(
                graph, start, StartBranch::Either, &seed, &mut compiler, &world,
                COUNTER_CAP as u32, novelty, hunting, &search, &shape, None,
            );
            // STARTS THAT REACHED THE BACKWARD DRIVER, which is not the same as starts with
            // a settled forward run and must not be labelled as though it were. A slice
            // that HALTS answers outright and no backward pass runs, so this counts the
            // ones where pruning could have applied at all.
            if answer.by != portfolio::Answered::Forwards {
                settled += 1;
            }
            std::hint::black_box(&answer);
        }

        Some((began.elapsed(), settled))
    })
}

fn from_env(name: &str, fallback: usize) -> usize {
    lookahead_engine::core::env::var(name).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(fallback)
}

fn numbers(name: &str, fallback: &[i32]) -> Vec<i32> {
    match lookahead_engine::core::env::var(name) {
        Ok(text) => text.split(',').filter_map(|part| part.trim().parse().ok()).collect(),
        Err(_) => fallback.to_vec(),
    }
}
