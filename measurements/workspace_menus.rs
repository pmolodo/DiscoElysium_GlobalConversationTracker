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
//! serve: `FRESH=1` sends every request for a DIFFERENT group in rotation, so the workspace
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
//! ## How to run it
//!
//! ```text
//! RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo workspace-menus -- \
//!   cargo run --release --example workspace_menus
//! ```

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
    let fresh = std::env::var("FRESH").map(|on| on.trim() != "0").unwrap_or(false);

    // The starts and quarry for each group, worked out once so neither arm pays for it.
    //
    // THROUGH `MenuProfile`, and that is not a detail. Taking the first entries by id gives
    // starts with nothing better beyond them, `bridge::class_worth_hunting` refuses every
    // one before a diagram is touched, and the whole measurement reads one millisecond a
    // request while measuring nothing at all. That is exactly the trap the first cut of
    // `menu_residue` fell into and the reason the profile is shared.
    let mut menus: Vec<(i32, Vec<NodeRef>, Vec<NodeRef>)> = Vec::new();
    for conversation in CONVERSATIONS {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            continue;
        }
        let Some(profile) = MenuProfile::of(&graph, root, 10, starts_wanted) else { continue };
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
    println!("{:>7}  {:>6}  {:>10}", "round", "conv", "request ms");

    let mut took_each: Vec<Duration> = Vec::new();
    for round in 0..rounds {
        // ONE GROUP THROUGHOUT for the kept arm, which is a player standing in a
        // conversation; rotating for the fresh arm, which is what defeats the workspace.
        let (conversation, starts, unseen) =
            &menus[if fresh { round % menus.len() } else { 0 }];

        // A DIFFERENT WORLD EVERY ROUND, in the field that actually moves between menus.
        let seen: HashSet<NodeRef> = starts.iter().take(round % starts.len()).copied().collect();
        let request = LookAheadRequest {
            conversation: *conversation,
            starts: starts.clone(),
            unseen_any_game: unseen.iter().copied().collect(),
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
        let response = service.look_ahead(&json).expect("a well-formed request is answered");
        let took = began.elapsed();
        std::hint::black_box(&response);

        println!("{:>7}  {conversation:>6}  {:>10.0}", round + 1, took.as_secs_f64() * 1000.0);
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

fn from_env(name: &str, fallback: usize) -> usize {
    std::env::var(name).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(fallback)
}
