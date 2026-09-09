// SPDX-License-Identifier: MIT
//! Can ONE manager answer many REQUESTS, with a different world each time?
//!
//! ## The gate on de-2wtl, and the only thing left unmeasured about it
//!
//! That issue would keep the diagram manager alive across requests. Splitting the setup
//! (`repeat_question`) showed why that is the right thing to keep: the manager is nine or
//! ten milliseconds of the ten to fourteen, and `DataLayout::for_group` reads the world only
//! through `money()`, so it survives everything else the world does - where the compiler,
//! which bakes `is_seen`, does not and must be rebuilt per request at one to five.
//!
//! What nothing has measured is whether that is SAFE, and the issue's own description
//! flagged it before there was a reason to care: "the node capacity is allocated up front
//! and the manager is what fills up; with several searches accumulating in one manager,
//! 'out of nodes' stops being a fact about one search".
//!
//! Two ways it could go wrong, and they look different:
//!
//! - IT FILLS UP. Nodes from request one are never reclaimed, the store is exhausted after
//!   n requests, and a group that answered at the start of a session stops answering later.
//!   That would show as the node count climbing and the answers changing.
//! - IT DIES. de-fpax's fault is a SECOND MANAGER on a thread that has already built one,
//!   and this arrangement builds exactly one - so it should be the safe row of
//!   `search_residue`'s table. Should be; a run that dies takes the process with it and
//!   prints no closing line, so "no SURVIVED line" IS the failure.
//!
//! ## What is already known, and what it does not cover
//!
//! `search_residue` found a flat 344 MB of bounded retention at the group budget - a pool,
//! not a leak - and `menu_residue` ran 384 starts against one manager without trouble. Both
//! held the WORLD fixed. This varies it, because a workspace's whole point is surviving a
//! world that moved, and each new world compiles different formulas over the same variables.
//!
//! ## How the world is varied
//!
//! What the player has READ, which is the field that actually moves between menus and the
//! one the compiler bakes through `is_seen`. Each round marks one more of the group's
//! entries as seen, so every round compiles a genuinely different set of formulas - and the
//! money is held, since moving the money ceiling would change the LAYOUT and is the one
//! thing a workspace is allowed to rebuild for.
//!
//! ## What it said, 2026-09-07: SAFE, and worth more than the setup it saves
//!
//! Forty requests over conversation 28 at 256 MB, eight starts each, a different world every
//! round - four more entries marked seen each time, so every round compiles a different set
//! of formulas over the same variables.
//!
//! ```text
//!   round     seen   manager nodes    manager MB    menu ms
//!       1        4          146049           5.6         98
//!       2        8          146049           5.6         54
//!       3       12          146049           5.6         57
//!      ...
//!      40      160          146049           5.6         53
//! ```
//!
//! THE NODE COUNT DOES NOT MOVE. Not once, across forty requests and forty worlds: 146,049
//! nodes and 5.6 MB from the first round to the last. Whatever a request builds is reclaimed
//! when its formulas are dropped, so "out of nodes" stays a fact about one search and the
//! fear this measurement was written for does not happen. The answers are identical every
//! round too, and the process survived - which is the other half, since a death here would
//! print no closing line at all.
//!
//! ## AND THE FIRST ROUND COSTS NINETY-EIGHT MILLISECONDS WHERE THE REST COST FIFTY-THREE
//!
//! That is the finding worth having, and it is not what this set out to measure. The manager
//! is built OUTSIDE the timed region, so the 45 ms difference is not construction - it is the
//! first request paying to warm a cold manager: the apply cache is empty and the node store
//! has never been touched.
//!
//! So a persistent manager is worth MORE than the nine or ten milliseconds `repeat_question`
//! priced. It saves the construction AND the warm-up, and today every request pays both
//! because every request gets a fresh manager. Within one request the warmth is already
//! shared - a menu's second option meets a warm cache - which is why this shows up between
//! requests rather than inside one, and why nothing before now had a reason to see it.
//!
//! ## How to run it
//!
//! ```text
//! RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo manager-reuse -- \
//!   cargo run --release --example manager_reuse
//! ```

use std::collections::HashSet;
use std::time::Instant;

use lookahead_engine::bridge::{NodeRef, SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
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

/// One group, since the question is about a session in ONE conversation.
const CONVERSATION: i32 = 28;

/// The player's allowance.
const BUDGET_MB: usize = 256;

/// How many requests to put through the one manager.
const ROUNDS: usize = 40;

/// How many starts each request carries.
const STARTS: usize = 8;

/// How many entries are unseen at the deep end.
const UNSEEN: usize = 10;

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let conversation = from_env("CONVERSATION", CONVERSATION as usize) as i32;
    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);
    let rounds = from_env("ROUNDS", ROUNDS);
    let starts_wanted = from_env("STARTS", STARTS);
    let unseen_wanted = from_env("UNSEEN", UNSEEN);

    let Ok((graph, _)) = build_group_graph(&index, conversation) else {
        eprintln!("conversation {conversation}'s group does not build; skipping.");
        return;
    };
    let root = DialogueNodeId::new(conversation, 0);
    if graph.get(root).is_none() {
        eprintln!("no entry 0 in conversation {conversation}; skipping.");
        return;
    }
    let Some(profile) = MenuProfile::of(&graph, root, unseen_wanted, starts_wanted) else {
        eprintln!("conversation {conversation}: every start would be refused; skipping.");
        return;
    };

    println!(
        "conversation {conversation}: {} entries, {rounds} requests of {} starts each, \
         {} MB, ONE manager",
        graph.count(),
        profile.starts.len(),
        budget.memory() / (1024 * 1024),
    );
    println!(
        "\n{:>7}  {:>7}  {:>14}  {:>12}  {:>9}",
        "round", "seen", "manager nodes", "manager MB", "menu ms",
    );

    // FLUSHED AS IT GOES, because a run that dies takes the process with it and anything
    // buffered dies with it - the same reason menu_residue flushes.
    isolated::on_its_own_thread(|| {
        let symbols = graph.symbols().clone();

        // THE LAYOUT AND THE MANAGER, ONCE. The money is held across rounds precisely so
        // this stays valid - the money ceiling is the one world fact the layout reads.
        let held_money = SnapshotWorld::declaring(
            WorldSnapshot { day_minutes: 720, day_counter: 1, ..Default::default() },
            None,
        );
        let layout = DataLayout::for_group(&graph, &held_money, COUNTER_CAP);
        let Some(vars) = DataVars::try_new(&layout, &symbols, budget) else {
            eprintln!("no room for the manager; skipping.");
            return;
        };
        let shape = GroupShape::of(&graph);
        let search = portfolio::Budget::default();

        // Entries that will be marked seen, one more per round, in a fixed order so two
        // runs vary the world the same way.
        let mut walkable: Vec<DialogueNodeId> = graph
            .nodes()
            .map(|node| node.id)
            .filter(|id| !profile.unseen.contains(id))
            .collect();
        walkable.sort_unstable_by_key(|id| (id.conversation_id, id.entry_id));

        let mut answers_each_round: Vec<usize> = Vec::new();

        for round in 1..=rounds {
            // A DIFFERENT WORLD EVERY ROUND, and different in the field that actually moves
            // between two menus: what the player has read. Every round therefore compiles a
            // different set of formulas over the same variables.
            let seen: HashSet<NodeRef> = walkable
                .iter()
                .take(round.saturating_mul(4).min(walkable.len()))
                .map(|id| NodeRef::from(*id))
                .collect();
            let world = SnapshotWorld::declaring(
                WorldSnapshot {
                    day_minutes: 720,
                    day_counter: 1,
                    seen: seen.iter().copied().collect(),
                    ..Default::default()
                },
                None,
            );

            // REBUILT PER ROUND, which is what a workspace would do: one to five
            // milliseconds against the nine or ten the manager cost once.
            let mut compiler = GuardCompiler::new(&vars)
                .with_world(&world)
                .with_constant_clock(DataLayout::group_passes_time(&graph));
            let seed = seed_of(&graph, &world, &vars).expect("room for a seed");
            let novelty = profile.novelty();

            let mut found = 0;
            let began = Instant::now();
            for &start in &profile.starts {
                let Some(hunting) = graph.best_linked_class(start, &novelty) else { continue };
                if hunting <= Novelty::SeenThisGame {
                    continue;
                }
                let answer = portfolio::best_novelty(
                    &graph, start, StartBranch::Either, &seed, &mut compiler, &world,
                    COUNTER_CAP as u32, &novelty, hunting, &search, &shape,
                );
                if answer.best > Novelty::SeenThisGame {
                    found += 1;
                }
            }
            let took = began.elapsed();
            answers_each_round.push(found);

            println!(
                "{round:>7}  {:>7}  {:>14}  {:>12.1}  {:>9.0}",
                seen.len(),
                vars.node_count(),
                vars.memory_used() as f64 / (1024.0 * 1024.0),
                took.as_secs_f64() * 1000.0,
            );
            flush();
        }

        println!(
            "\nSURVIVED {rounds} requests on one manager, and the process is still here."
        );
        println!(
            "answers found per round: {:?}",
            answers_each_round,
        );
    });

    println!(
        "\nMANAGER NODES is the question. Climbing without bound means the store fills over \
         a\nsession and a group that answered early stops answering later, which is what \
         would\nmake a persistent manager the wrong thing to keep."
    );
}

fn flush() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

fn from_env(name: &str, fallback: usize) -> usize {
    std::env::var(name).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(fallback)
}
