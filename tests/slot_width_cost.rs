// SPDX-License-Identifier: MIT
//! What the WIDTH of a crawl state costs, at a controlled number of states.
//!
//! The explicit crawl's cost is dominated by copying and comparing the slot vector - about
//! seventy per cent of the per-state cost on the widest group, measured in de-f9gt. Several
//! of the changes that epic is considering are all the same shape: make a state carry less,
//! or compare less of it. This is the measurement each of them has to be judged by.
//!
//! ## Why it caps the STATE COUNT and not the memory
//!
//! Because narrowing a state changes both halves of what a memory-bounded run does. The
//! matrix stops a crawl at a number of BYTES, so halving the width of a state doubles the
//! number of states the same crawl explores - and the two versions then do different
//! amounts of work, over different-sized hash sets, with different cache behaviour. Their
//! wall times are not a comparison of anything.
//!
//! Capping the state count instead makes both versions run the same search and stop in the
//! same place, so the difference in wall time is the difference in per-state cost, which is
//! what is actually being claimed. The memory saving is the other half of the answer and is
//! read straight off the slot count - it needs no timing.
//!
//! ## What it has said so far
//!
//! Dropping the slots no guard reads (de-f9gt.1), all-seen, 100,000 states:
//!
//! ```text
//!   conv   slots  us/state        slots  us/state
//!    631     518      3.32   ->     245      2.13
//!     14     436      3.15   ->     232      2.02
//! ```
//!
//! Thirty-six per cent off the per-state cost of both, against a prediction of a quarter to
//! a third - the trim went further than the prediction because dropping an action can drop
//! the `once:` slot that existed to stop it firing twice.
//!
//! Run it with `--ignored --release`.

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions};
use lookahead_engine::index::{build_group_graph, read_index};

mod common;

/// The two widest groups, which is where the slot vector's share of the cost is largest.
const MEASURED: [i32; 2] = [631, 14];

/// How many states each crawl explores before it is stopped.
///
/// The only budget in play: memory and time are switched off, so both a wide and a narrow
/// version of the same group do the identical search. Large enough that the fixed cost of
/// starting up disappears, small enough that the run is seconds rather than minutes.
const STATES: usize = 100_000;

/// The cap the other cost measurements use, so the rows can be read beside them.
const COUNTER_CAP: i32 = 16;

#[test]
#[ignore = "a measurement, not a test: run it with --ignored --release"]
fn what_a_states_width_costs_the_crawl() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!("{:>6} {:>7} {:>9} {:>7} {:>10}", "conv", "slots", "states", "ms", "us/state");

    for conversation in MEASURED {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        // ALL SEEN, so there is nothing to find and the crawl runs until the state cap
        // stops it. A row that found something would be timing the luck of the search
        // order rather than the cost of a state.
        let novelty = |_: DialogueNodeId| Novelty::SeenThisGame;

        let began = std::time::Instant::now();
        let result = LookAheadEngine::new(LookAheadOptions {
            state_budget: STATES,
            memory_budget: 0,
            time_budget: std::time::Duration::ZERO,
            counter_cap: COUNTER_CAP,
            ..Default::default()
        })
        .evaluate(&graph, start, &world, novelty);
        let millis = began.elapsed().as_millis();

        println!(
            "{conversation:>6} {:>7} {:>9} {millis:>7} {:>10.2}",
            graph.symbols().count(),
            result.states_explored,
            1000.0 * millis as f64 / result.states_explored.max(1) as f64,
        );
    }
}
