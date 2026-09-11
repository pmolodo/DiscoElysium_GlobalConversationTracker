// SPDX-License-Identifier: MIT
//! How much of the structural bound's slack is guards that NO data state can satisfy.
//!
//! The branch and bound in `symbolic::menu` skips a target when its bound cannot beat the
//! best distance proven this round, and on conversation 761 it skips nothing: the bound
//! reads 8 where the layered pass proves 23 to 25. de-0jsf.13 measured that gap and
//! de-0jsf.15 asks whether closing it is a matter of walking a smaller graph.
//!
//! `novelty_search::choice_bounds` consults no guard at all. It walks `node.links`,
//! respecting only the cut set, so every route the dialogue refuses for reasons that do not
//! depend on the data state is still in it.
//!
//! THE BOUND MUST STAY A LOWER BOUND, so only a node NO state can enter may be refused -
//! never one that is merely shut in the state the search starts in, since a route may open
//! it on the way. Three such refusals, all of which the symbolic passes already make and
//! the structural walk does not:
//!
//! - a compiled guard whose `may_be_true` is unsatisfiable, which is what a guard on a
//!   world constant the world reports false compiles to, and what a contradiction compiles
//!   to;
//! - a passive check the world fails, which is `reachability::never_displays`;
//! - a hidden `Test`, for which `Backward::pre_enter` returns the empty set unconditionally.
//!
//! ## What it prints
//!
//! One block per group. How many nodes each refusal catches, and then, per menu option, the
//! bound the walk gives now against the bound it gives over the smaller graph. The row that
//! matters is the per-target one: the ten unread candidates of 761 against the 8 and 9 the
//! bound reads today.
//!
//! ## What it said, 2026-09-10: NOTHING, on every heavy group
//!
//! The seven heavy groups, eight options, ten unread, the player's 256 MB. The refusal set
//! is tiny and it lies nowhere near the short routes, so every bound is unmoved:
//!
//! ```text
//!   conv  entries  refused   bounds now   bounds refusing
//!    761     2263       23         8, 9              8, 9
//!    631     2857       22       10, 11            10, 11
//!    640     2445        8            -                 -
//!    368     3192       48            -                 -
//!     14     2252        7            -                 -
//!   1030     1003       10            -                 -
//!     16     2332       21            -                 -
//! ```
//!
//! NOT ONE TARGET MOVED BY ONE CHOICE, on any group. Twenty-three refusals out of 2,263
//! entries on 761, and the ten unread candidates read 8 and 9 either way against a true
//! distance of 23 to 25.
//!
//! WHAT THAT SETTLES. The bound's slack is not routes the world has already shut. It is
//! routes shut by guards on variables the dialogue itself writes, and those cannot be
//! refused here: a route may set what it reads on the way, so removing the edge would make
//! the walk report a distance ABOVE the truth and the branch and bound would cut a target
//! that should have won. 761 is the shape at its worst - content that sits eight choices
//! away through links and twenty-four away in fact, because a variable has to be set first.
//!
//! Tightening the bound therefore needs an analysis that reasons about ORDER - what must be
//! set before an entry opens, and what setting it costs - rather than about which entries
//! are shut from the start. See de-0jsf.15.
//!
//! ## How to run it
//!
//! ```text
//! DEGCT_CONVERSATION=761 \
//!   tools/run-logged.sh cargo bound-slack -- cargo run --release --example bound_slack
//! ```
//!
//! `DEGCT_STARTS`, `DEGCT_UNSEEN` and `DEGCT_BUDGET_MB` mean what they mean in
//! `menu_matrix`, so a reading here lines up with a row there.

use std::collections::HashSet;

use lookahead_engine::bridge::{SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::{DialogueCheckKind, DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::novelty_search::{Where, choice_bounds};
use lookahead_engine::symbolic::reachability::{never_displays, seed_of};
use lookahead_engine::symbolic::vars::DataVars;
use oxidd::BooleanFunction;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

const CONVERSATIONS: [i32; 7] = [761, 631, 640, 368, 14, 1030, 16];
const BUDGET_MB: usize = 256;
const STARTS: usize = 8;
const UNSEEN: usize = 10;
const COUNTER_CAP: i32 = 16;

/// Why a node can never be entered, in the order the cheapest test comes first.
#[derive(Default)]
struct Refused {
    hidden_test: usize,
    passive_fails: usize,
    guard_empty: usize,
}

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");
    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);
    let starts_wanted = from_env("STARTS", STARTS);
    let unseen_wanted = from_env("UNSEEN", UNSEEN);

    for conversation in numbers("CONVERSATION", &CONVERSATIONS) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            eprintln!("conversation {conversation}: no group builds from it; skipping.");
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            eprintln!("conversation {conversation}: no entry 0; skipping.");
            continue;
        }
        let Some(profile) = MenuProfile::of(&graph, root, unseen_wanted, starts_wanted) else {
            println!("conversation {conversation}: no menu");
            continue;
        };
        isolated::on_its_own_thread(|| {
            report(conversation, &graph, &profile, budget);
            Some(())
        });
    }
}

fn report(conversation: i32, graph: &LookAheadGraph, profile: &MenuProfile, budget: DiagramBudget) {
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
    let Some(vars) = DataVars::try_new(&layout, &symbols, budget) else {
        eprintln!("conversation {conversation}: no room for the variables; skipping.");
        return;
    };
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(&world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, &world, &vars).expect("room for a seed");

    let mut refused = Refused::default();
    let mut shut = HashSet::new();
    for node in graph.nodes() {
        if node.is_group {
            continue;
        }
        if node.kind == DialogueCheckKind::Test {
            refused.hidden_test += 1;
            shut.insert(node.id);
            continue;
        }
        if never_displays(node, &world) {
            refused.passive_fails += 1;
            shut.insert(node.id);
            continue;
        }
        // ONLY AN EMPTY MAY-BE-TRUE, which says no data state at all admits this node. A
        // guard merely false in the seed is not this and must not be refused: a route can
        // set what it reads on the way.
        let compiled = compiler.compile_for(node.id, &node.guard);
        if !compiled.may_be_true.satisfiable() {
            refused.guard_empty += 1;
            shut.insert(node.id);
        }
    }

    let entries = graph.nodes().filter(|n| !n.is_group).count();
    println!(
        "\n== conversation {conversation}: {entries} entries, {} refused",
        shut.len()
    );
    println!(
        "   hidden tests {}   passive the world fails {}   guard empty in every state {}",
        refused.hidden_test, refused.passive_fails, refused.guard_empty
    );

    let novelty = profile.novelty();
    let mut targets: Vec<_> = graph
        .nodes()
        .filter(|n| !n.is_group && novelty(n.id) > Novelty::SeenThisGame)
        .map(|n| n.id)
        .collect();
    targets.sort_by_key(|id| (id.conversation_id, id.entry_id));

    let empty = HashSet::new();
    let mut now = std::collections::HashMap::<DialogueNodeId, usize>::new();
    let mut tighter = std::collections::HashMap::<DialogueNodeId, usize>::new();
    for &start in &profile.starts {
        let position = Where::of(
            graph,
            start,
            StartBranch::Either,
            &seed,
            &mut compiler,
            &world,
            COUNTER_CAP as u32,
        )
        .position(start);
        for (id, distance) in choice_bounds(graph, &position, &empty) {
            now.entry(id)
                .and_modify(|d| *d = (*d).min(distance))
                .or_insert(distance);
        }
        for (id, distance) in choice_bounds(graph, &position, &shut) {
            tighter
                .entry(id)
                .and_modify(|d| *d = (*d).min(distance))
                .or_insert(distance);
        }
    }

    println!("   {:>14}  {:>6}  {:>8}", "target", "bound", "refusing");
    for id in targets {
        let before = now.get(&id).map(|d| d.to_string()).unwrap_or("-".into());
        let after = tighter
            .get(&id)
            .map(|d| d.to_string())
            .unwrap_or("-".into());
        println!(
            "   {:>9}:{:<4}  {:>6}  {:>8}",
            id.conversation_id, id.entry_id, before, after
        );
    }
}

fn from_env(name: &str, fallback: usize) -> usize {
    lookahead_engine::core::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(fallback)
}

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
