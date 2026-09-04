// SPDX-License-Identifier: MIT
//! How many of the slots a crawl carries does anything ever READ?
//!
//! The explicit crawl's cost is dominated by copying and comparing the slot vector - about
//! seventy per cent of the per-state cost on the widest group, measured (de-f9gt). Every
//! slot in that vector is copied for every state, whether or not any decision depends on
//! it.
//!
//! The symbolic side already trims: `DataLayout::keeping_only_read` drops any variable no
//! guard mentions, keeping the engine's own seen- and once-bookkeeping. The explicit crawl
//! does no equivalent, so it may be carrying slots that are written, copied, hashed and
//! compared, and can never change an answer.
//!
//! This measures the size of that, because it decides whether the trim is worth building.
//!
//! Run it with `--ignored --release`.

use std::collections::HashSet;

use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::core::state::{ONCE_PREFIX, SEEN_PREFIX};
use lookahead_engine::symbolic::data_layout::DataLayout;

mod common;

const HEAVIEST: [i32; 6] = [362, 368, 631, 14, 28, 1030];

/// The prefixes the engine uses for its own bookkeeping.
///
/// No guard mentions these and every crawl needs them - a seen marker is what closes a
/// once-only check, and a once marker is what stops a purchase being charged twice - so
/// they are kept whatever the guards read. Same rule `keeping_only_read` applies.
///
/// IMPORTED, NOT COPIED. The first version of this file spelled them out and spelled them
/// wrong, so nothing matched and every bookkeeping slot was counted as droppable - a
/// measurement that overstated its own case, which is the worst kind.
const KEPT_PREFIXES: [&str; 2] = [SEEN_PREFIX, ONCE_PREFIX];

#[test]
#[ignore = "a measurement, not a test: run it with --ignored --release"]
fn how_many_slots_a_crawl_carries_that_nothing_reads() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");

    println!(
        "{:>6} {:>8} {:>7} {:>7} {:>9} {:>8}",
        "conv", "entries", "slots", "needed", "droppable", "saving"
    );

    for conversation in HEAVIEST {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let symbols = graph.symbols();
        let reads: HashSet<String> = DataLayout::read_by(&graph);

        let total = symbols.count();
        let mut needed = 0usize;

        for slot in 0..total {
            let Some(name) = symbols.name_of(slot) else {
                // Unnamed slots are the engine's, and it needs them.
                needed += 1;
                continue;
            };

            if KEPT_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
                || reads.contains(name)
            {
                needed += 1;
            }
        }

        let droppable = total - needed;
        println!(
            "{conversation:>6} {:>8} {total:>7} {needed:>7} {droppable:>9} {:>7.0}%",
            graph.count(),
            100.0 * droppable as f64 / total.max(1) as f64,
        );
    }
}
