// SPDX-License-Identifier: MIT
//! What carrying the clock costs, on the groups that could ever want one.
//!
//! Eleven more variables and magnitude comparisons over them is the classic way to make a
//! decision diagram explode, and that is the stated reason the clock was held constant. This
//! prices it: every guard in a group compiled twice, against a layout carrying a clock and
//! against the same layout without one, with the world and everything else held.
//!
//! WHAT IT PRICES IS THE GUARDS, not a whole search. The sets a search carries would hold the
//! eleven extra variables too, and nothing here walks one - so a cheap result means the
//! compile does not blow up, and says nothing yet about the walk.

use std::time::Instant;

use lookahead_engine::core::guard::GuardExpression;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::vars::DataVars;

use gct_measure::common;

const COUNTER_CAP: i32 = 16;

/// The groups that pass time AND ask the hour, which are the only ones a clock could help,
/// then two that pass no time at all as a control.
const GROUPS: [(i32, &str); 10] = [
    (14, "passes time, 66 hour questions"),
    (17, "passes time, 4 hour questions"),
    (381, "passes time, 2 hour questions"),
    (566, "passes time, 12 hour questions"),
    (631, "passes time, 14 hour questions"),
    (827, "passes time, 8 hour questions"),
    (1030, "passes time, 4 hour questions"),
    (1260, "passes time, 20 hour questions"),
    (368, "control: no PassTime"),
    (28, "control: no PassTime"),
];

/// What compiling a group's guards over one layout cost.
struct Cost {
    variables: u32,
    fallbacks: usize,
    compiled: usize,
    nodes: usize,
    millis: u128,
}

fn measure(graph: &lookahead_engine::graph::LookAheadGraph, carried: bool) -> Cost {
    let layout = DataLayout::for_graph(graph, COUNTER_CAP, None, carried);
    let symbols = graph.symbols().clone();
    let world = common::measurement_save();
    let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(&world)
        .with_constant_clock(DataLayout::group_passes_time(graph));

    let started = Instant::now();
    for node in graph.nodes() {
        if matches!(node.guard.expression(), GuardExpression::Literal(_)) {
            continue;
        }
        let _ = compiler.compile(&node.guard);
    }
    let millis = started.elapsed().as_millis();

    Cost {
        variables: layout.total_vars(),
        fallbacks: compiler.fallbacks(),
        compiled: compiler.compiled(),
        nodes: vars.node_count(),
        millis,
    }
}

fn main() {
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index, and nothing can build one here");
        return;
    };
    let index = read_index(&path).expect("the index reads");

    println!(
        "{:>6} {:>5} {:>9} {:>9} {:>10} {:>7}  {}",
        "conv", "arm", "variables", "compiled", "nodes", "ms", "what it is"
    );

    for (conversation, what) in GROUPS {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            eprintln!("{conversation}: no group builds from it");
            continue;
        };

        for (arm, carried) in [("held", false), ("clock", true)] {
            let cost = measure(&graph, carried);
            println!(
                "{conversation:>6} {arm:>5} {:>9} {:>9} {:>10} {:>7}  {what}",
                cost.variables,
                format!("{}/{}", cost.compiled, cost.compiled + cost.fallbacks),
                cost.nodes,
                cost.millis,
            );
        }
    }
}
