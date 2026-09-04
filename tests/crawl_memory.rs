// SPDX-License-Identifier: MIT
//! What does a crawl actually COST IN MEMORY, as against in states?
//!
//! The state budget counts states, and a state is not a fixed size: it carries one slot per
//! tracked variable in the group, so the same 200,000 states is a different amount of
//! memory in every conversation. This measures the difference, because a budget that says
//! "200,000" tells a player nothing about whether their machine can afford it - and because
//! comparing the forward crawl against the backward one by counting states against counting
//! diagram nodes compares two things that are not alike (de-e23q).
//!
//! ## What it found, 2026-09-04
//!
//! ```text
//!   conv  entries  slots     states  bytes/state   total MB
//!    631     4514    518     200000         2386      455.1
//!     14     3594    436     200000         2017      384.7
//!    368     4724    337     200001         1571      299.6
//!     28     2186    256     200001         1207      230.2
//!    362     1860    146     200001          712      135.8
//!   1030     1476    137        410          671        0.3
//! ```
//!
//! THE SAME BUDGET COSTS BETWEEN 136 AND 455 MEGABYTES - a spread of 3.3x across the groups
//! it is meant to protect. A budget of "200,000" therefore says nothing about what a menu
//! will cost the machine it runs on, which is the case for measuring it in memory instead.
//!
//! The spread is slots, not entries. 362 is the largest conversation in the game and the
//! CHEAPEST per state, because it tracks 146 variables; 631 has a third fewer entries and
//! tracks 518, so each of its states is three and a half times the size. Entry count
//! predicts memory about as badly as it predicts time.
//!
//! 455 MB of transient allocation inside a running game is also simply a lot, and nothing
//! was previously reporting it in a unit anyone would recognise as large.
//!
//! WHAT THE DEFAULT DOES WITH THIS, now that it is a memory budget of 256 MB: the three
//! groups above that figure are held to it, and the two below it are unaffected.
//!
//! ```text
//!   conv   was     now
//!    631   455.1   256.0
//!     14   384.7   256.0
//!    368   299.6   256.0
//!     28   230.2   230.2
//!    362   135.8   135.8
//! ```
//!
//! So the worst case falls from 455 MB to 256, and every group is allowed the same amount
//! rather than an amount decided by how many variables its guards happen to read.
//!
//! 1030 is the control: it exhausts its whole reachable space in 410 states, so it never
//! approaches any budget and costs a third of a megabyte.
//!
//! Run it with `--ignored --release`.

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions};
use lookahead_engine::index::{build_group_graph, read_index};

mod common;

const EXPENSIVE: [i32; 6] = [362, 368, 631, 14, 28, 1030];

const STATE_BUDGET: usize = 200_000;
const COUNTER_CAP: i32 = 16;

fn conversations(default: &[i32]) -> Vec<i32> {
    match std::env::var("CONVERSATION") {
        Ok(named) => named.split(',').filter_map(|id| id.trim().parse().ok()).collect(),
        Err(_) => default.to_vec(),
    }
}

#[test]
#[ignore = "a long measurement, not a test: run it with --ignored --release"]
fn what_a_full_state_budget_costs_in_memory() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>6} {:>10} {:>12} {:>10} {:>9}",
        "conv", "entries", "slots", "states", "bytes/state", "total MB", "ms"
    );

    for conversation in conversations(&EXPENSIVE) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        // Everything read, so nothing outranks the option and the crawl cannot stop early.
        // That is the shape that fills the budget, which is the one worth costing.
        let began = std::time::Instant::now();
        let result = LookAheadEngine::new(LookAheadOptions {
            state_budget: STATE_BUDGET,
            // NO MEMORY LIMIT, which is the whole point of this measurement: it asks what a
            // full STATE budget costs, and the memory budget it produced would cap the
            // answer at itself and report that back as a discovery.
            memory_budget: 0,
            time_budget: std::time::Duration::from_secs(120),
            counter_cap: COUNTER_CAP,
            ..Default::default()
        })
        .evaluate(&graph, start, &world, |_| Novelty::SeenThisGame);
        let ms = began.elapsed().as_millis();

        // THE SEARCH FRONTIER IS WHAT COSTS. Every state the crawl has seen is kept, keyed
        // by the entry it was at, so the walk can tell a state it has already explored from
        // a new one. That set is the memory: one entry id and one state per element.
        //
        // A state is its slots on the heap plus its money, clock and cached hash inline.
        // The set itself adds a control byte per element and runs at around 7/8 load, which
        // is where the eighth comes from - an estimate rather than a measurement, and near
        // enough for a budget to be set in terms a person can hold in their head.
        let slots = graph.symbols().count();
        let per_state = std::mem::size_of::<DialogueNodeId>()
            + std::mem::size_of::<lookahead_engine::core::state::LookAheadState>()
            + slots * std::mem::size_of::<i32>();
        let with_overhead = per_state + per_state / 8 + 1;
        let total = result.states_explored * with_overhead;

        println!(
            "{conversation:>6} {:>8} {slots:>6} {:>10} {with_overhead:>12} {:>10.1} {ms:>9}",
            graph.count(),
            result.states_explored,
            total as f64 / (1024.0 * 1024.0),
        );
    }
}
