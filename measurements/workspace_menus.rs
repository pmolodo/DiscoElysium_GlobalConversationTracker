// SPDX-License-Identifier: MIT
//! What the kept manager is worth, end to end, through the call the game actually makes.
//!
//! ## Why this and not `manager_reuse`
//!
//! That one established the MECHANISM - that a manager survives forty worlds without
//! growing, and that a cold one costs ninety-eight milliseconds where a warm one costs
//! fifty-three - by driving the portfolio directly. This drives `Service::look_ahead`, which
//! is what the engine host calls when the plugin sends a request, so what it reports is what
//! a player's menu costs.
//!
//! Both arms are that same call. The difference is only whether the workspace is allowed to
//! serve: `DEGCT_FRESH=1` sends every request for a DIFFERENT group in rotation, so the workspace
//! is replaced each time and every request pays what it always paid.
//!
//! ## What it said, 2026-09-07: about twice as fast per menu
//!
//! Twelve requests of eight starts, adversarial profile, the shipped budget.
//!
//! ```text
//!   KEPT (same group throughout)     FRESH (rotating, so never served)
//!   round  conv  request ms          round  conv  request ms
//!       1    28         118              1    28         176
//!       2    28          54              4    28         116
//!       3    28          54              7    28         126
//!      ...                              10    28         119
//!      12    28          53
//! ```
//!
//! READ THE CONVERSATION 28 ROWS AGAINST EACH OTHER, since those are the same work either
//! way. Served, a request costs 53 to 60 milliseconds; unserved, 116 to 126. About sixty-four
//! milliseconds a menu, and roughly a halving, on every menu after the first in a group.
//!
//! THAT IS FAR MORE THAN THE SETUP IT SAVES. `repeat_question` puts the graph and diagram
//! setup at eighteen to twenty-seven milliseconds together; the rest is the warm-up
//! `manager_reuse` found - an empty apply cache and an untouched node store, which the first
//! request against a manager pays and every later one does not. Before this, every request
//! got a fresh manager and so every request paid it.
//!
//! ## What the FRESH arm is not
//!
//! A controlled comparison of the whole table. It rotates groups to defeat the workspace,
//! so its 368 and 631 rows are different work and are not comparable with anything - they
//! are there to force the replacement. Only the conversation 28 rows compare, and they
//! follow a replacement rather than each other, which is the honest version of "the
//! workspace did not serve this".
//!
//! ## What the manager holds, which is the other question this answers
//!
//! de-dt75.2 asked whether the default memory budget should move, since one manager now
//! serves every menu of a conversation and could in principle accumulate. The `held` column
//! says it does not. Forty-menu sessions at the shipped 256 MB, which is about 6.7 million
//! nodes:
//!
//! ```text
//!   conv       held   of cap
//!     28      2,464     0.0%
//!    368     10,294     0.2%
//!    631     78,895     1.2%
//!     14     93,833     1.4%
//!    761  1,198,484    17.9%
//! ```
//!
//! FLAT, NOT CLIMBING. The store is filled by the first request and the next thirty-nine add
//! two tenths of a per cent. Over the forty heaviest groups in the game, 761 is the only one
//! above two per cent and the next highest is 1030 at 1.5%.
//!
//! THE SHRINKING QUARRY IS WHAT MAKES THAT A MEASUREMENT. Held fixed, every round after the
//! first asks the same question, the memo answers it without running a pass, and the manager
//! allocates nothing - so the column reads perfectly constant and means nothing at all. A
//! session's unseen set shrinks as the player reads, and this mirrors that.
//!
//! ## How to run it
//!
//! ```text
//! DEGCT_RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo workspace-menus -- \
//!   cargo run --release --example workspace_menus
//! ```
//!
//! `DEGCT_CONVERSATION=14,368,631` picks the groups, the FIRST of which is the one a kept
//! session stands in. `DEGCT_ROUNDS` is how many menus, and `DEGCT_STARTS` how wide each is.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use lookahead_engine::bridge::{LookAheadRequest, NodeRef, WorldSnapshot};
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::service::Service;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

/// The group a session stands in, plus others to rotate through for the FRESH arm.
const CONVERSATIONS: [i32; 3] = [28, 368, 631];

/// How many requests a session makes.
const ROUNDS: usize = 12;

/// How many starts each request carries.
const STARTS: usize = 8;

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");
    let service = Service::open(&path, None).expect("the engine opens");

    let rounds = from_env("ROUNDS", ROUNDS);
    let starts_wanted = from_env("STARTS", STARTS);
    let fresh = lookahead_engine::core::env::var("FRESH")
        .map(|on| on.trim() != "0")
        .unwrap_or(false);

    // The starts and quarry for each group, worked out once so neither arm pays for it.
    //
    // THROUGH `MenuProfile`, and that is not a detail. Taking the first entries by id gives
    // starts with nothing better beyond them, `bridge::class_worth_hunting` refuses every
    // one before a diagram is touched, and the whole measurement reads one millisecond a
    // request while measuring nothing at all. That is exactly the trap the first cut of
    // `menu_residue` fell into and the reason the profile is shared.
    let mut menus: Vec<(i32, Vec<NodeRef>, Vec<NodeRef>)> = Vec::new();
    // THE FIRST ONE IS THE SESSION, since the kept arm stands in `menus[0]` throughout and
    // the rest are only there to defeat the workspace in the fresh arm. So
    // `DEGCT_CONVERSATION=14,368,631` measures a session in conversation 14.
    for conversation in conversations() {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            continue;
        }
        let Some(profile) = MenuProfile::of(&graph, root, 10, starts_wanted) else {
            continue;
        };
        menus.push((
            conversation,
            profile.starts.iter().map(|id| NodeRef::from(*id)).collect(),
            profile.unseen.iter().map(|id| NodeRef::from(*id)).collect(),
        ));
    }

    if menus.is_empty() {
        eprintln!("no group big enough; skipping.");
        return;
    }

    println!(
        "{rounds} requests of {starts_wanted} starts, arm: {}\n",
        if fresh {
            "FRESH - a different group each time, so the workspace never serves"
        } else {
            "KEPT - the same group each time, so the workspace serves after the first"
        },
    );
    println!(
        "{:>7}  {:>6}  {:>10}  {:>12}  {:>8}",
        "round", "conv", "request ms", "held", "of cap"
    );

    // WHAT THE MANAGER HOLDS, ROUND BY ROUND, which is the question de-dt75.2 asks and the
    // one the per-request path could never raise: a manager now serves every menu of a
    // conversation, so the store grows ACROSS menus rather than starting empty each time.
    //
    // IT IS THE STORE'S OCCUPANCY, not what is still referenced. Every search dropped its
    // sets as it went, so a rising number says how far the store grew and not how much any
    // one menu needs. That is the right currency for "does the budget fill" and the wrong
    // one for "how much does a menu cost".
    //
    // WHAT TO READ IT FOR IS THE SHAPE. A curve that flattens is a session that has reached
    // whatever it is going to hold, and the cap is then a question about the plateau. One
    // still climbing at the last round has not been run long enough to say anything, and
    // DEGCT_ROUNDS is how to run it longer.
    let cap = lookahead_engine::symbolic::budget::DiagramBudget::new(
        lookahead_engine::symbolic::budget::DiagramBudget::DEFAULT_MEMORY_BUDGET,
    )
    .nodes();

    let mut took_each: Vec<Duration> = Vec::new();
    for round in 0..rounds {
        // ONE GROUP THROUGHOUT for the kept arm, which is a player standing in a
        // conversation; rotating for the fresh arm, which is what defeats the workspace.
        let (conversation, starts, unseen) = &menus[if fresh { round % menus.len() } else { 0 }];

        // A DIFFERENT WORLD EVERY ROUND, in the field that actually moves between menus.
        let seen: HashSet<NodeRef> = starts.iter().take(round % starts.len()).copied().collect();

        // AND A SHRINKING QUARRY, which is what a session IS: the player reads lines, so
        // entries leave `unseen_any_game` as the conversation goes on. Holding it fixed
        // makes every round after the first the same question, which the memo answers
        // without running a pass - so the manager never allocates and a measurement of what
        // it accumulates measures nothing. de-dt75.2.
        let read = (round * unseen.len()) / rounds.max(1);
        let hunting = unseen.iter().skip(read).copied().collect();
        let request = LookAheadRequest {
            conversation: *conversation,
            starts: starts.clone(),
            unseen_any_game: hunting,
            world: WorldSnapshot {
                day_minutes: 720,
                day_counter: 1,
                seen: seen.into_iter().collect(),
                ..Default::default()
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&request).expect("a request serialises");

        let began = Instant::now();
        let response = service
            .look_ahead(&json)
            .expect("a well-formed request is answered");
        let took = began.elapsed();
        std::hint::black_box(&response);

        // AFTER THE REQUEST, so what is reported is the store as the menu left it. The
        // query queues behind the request on the owner thread, so it cannot race it.
        let held = service.workspace_held();
        println!(
            "{:>7}  {conversation:>6}  {:>10.0}  {:>12}  {:>8}",
            round + 1,
            took.as_secs_f64() * 1000.0,
            held.map(|n| n.to_string())
                .unwrap_or_else(|| "no workspace".to_string()),
            held.map(|n| format!("{:.1}%", 100.0 * n as f64 / cap as f64))
                .unwrap_or_default(),
        );
        took_each.push(took);
    }

    let after_first: Duration = took_each.iter().skip(1).sum();
    println!(
        "\nfirst {:.0} ms; the other {} averaged {:.0} ms",
        took_each[0].as_secs_f64() * 1000.0,
        took_each.len() - 1,
        after_first.as_secs_f64() * 1000.0 / (took_each.len() - 1).max(1) as f64,
    );
}

/// The groups to walk, the first of which is the one a kept session stands in.
fn conversations() -> Vec<i32> {
    match lookahead_engine::core::env::var("CONVERSATION") {
        Ok(named) => named
            .split(',')
            .map(str::trim)
            .filter(|piece| !piece.is_empty())
            .map(|piece| {
                piece.parse().unwrap_or_else(|_| {
                    panic!(
                        "{}={piece:?} is not a conversation id",
                        lookahead_engine::core::env::qualified("CONVERSATION")
                    )
                })
            })
            .collect(),
        Err(_) => CONVERSATIONS.to_vec(),
    }
}

fn from_env(name: &str, fallback: usize) -> usize {
    lookahead_engine::core::env::var(name)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(fallback)
}
