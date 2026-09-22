// SPDX-License-Identifier: MIT
//! How many of the ENGINE's own groups carry a clock, and how wide.
//!
//! Two Python surveys of this disagreed with each other and with the engine. One partitioned
//! conversations into weakly connected components; the other followed links forwards. Neither
//! is what `build_group_graph` does, and a group is whatever that builds - so this asks it,
//! over every start in the index.

use std::collections::{BTreeMap, BTreeSet};

use lookahead_engine::index::{build_group_graph, discover_group, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;

use gct_measure::common;

fn main() {
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index, and nothing can build one here");
        return;
    };
    let index = read_index(&path).expect("the index reads");

    // ONE GROUP PER SET OF CONVERSATIONS, since a group of six answers the same six times.
    let mut groups: BTreeMap<BTreeSet<i32>, i32> = BTreeMap::new();
    for conversation in index.keys() {
        let reach: BTreeSet<i32> = discover_group(&index, *conversation).into_iter().collect();
        groups.entry(reach).or_insert(*conversation);
    }

    let mut passes = 0;
    let mut reads = 0;
    let mut widths: BTreeMap<u8, usize> = BTreeMap::new();
    let mut unbounded = 0;

    for start in groups.values() {
        let Ok((graph, _)) = build_group_graph(&index, *start) else {
            continue;
        };
        if !DataLayout::group_passes_time(&graph) {
            continue;
        }
        passes += 1;
        if !DataLayout::group_reads_the_clock(&graph) {
            continue;
        }
        reads += 1;

        if graph.minutes_passable().is_none() {
            unbounded += 1;
        }
        let bits = DataLayout::for_graph(&graph, 16, None, true)
            .clock()
            .map_or(0, |(_, bits)| bits);
        *widths.entry(bits).or_default() += 1;
    }

    println!("groups the engine builds:            {}", groups.len());
    println!("of those, groups that pass time:     {passes}");
    println!("of those, groups that read the clock:{reads:>5}");
    println!("  so groups carrying no clock:       {}", passes - reads);
    println!("of the readers, unbounded (a cycle): {unbounded}");
    println!();
    println!("what the carriers pay, in bits:");
    for (bits, count) in &widths {
        println!("  {bits} bit(s): {count} group(s)");
    }
    let total: usize = widths
        .iter()
        .map(|(bits, count)| *bits as usize * count)
        .sum();
    println!(
        "  {total} variables over {reads} group(s), against {} at eleven bits each",
        reads * 11
    );
}
