// SPDX-License-Identifier: MIT
//! Whether anything that handles a guard still costs stack in proportion to its depth.
//!
//! ## Why this is worth measuring rather than guessing
//!
//! A guard comes out of a dialogue database that a game patch or another mod can change, so
//! no string may bring the process down. The parser accepts a guard of any depth, which is
//! only safe while nothing that walks the result overflows the smallest stack this code can
//! find itself on - and a stack overflow aborts the process rather than panicking.
//!
//! The deepest guard in the shipped database is ELEVEN levels, of 26,210 - what
//! `guard_depth` counted, and `performance/removed_tools.md` says how to count it again.
//! What is not known without measuring is the other end -
//! and it cannot be taken from a plain `cargo test` run, because a test thread's stack is
//! generous. This code runs inside the game, on whatever thread the dialogue system calls
//! it from.
//!
//! ## What could overflow, and why nothing should
//!
//! Six things walk a guard. The parser holds its own stacks. A guard is a flat table, so
//! building, freeing, evaluating and rendering one are sweeps in index order. And
//! `GuardCompiler::compile_node`, which has to walk DOWN - a comparison answers from its
//! operands' shape without compiling either, so a sweep would build diagrams nobody reads -
//! does so on a work stack of its own rather than on the thread's.
//!
//! So none of them costs stack in proportion to depth, and this is the check that that is
//! still true rather than a search for where it stops being.
//!
//! ## How
//!
//! Two lines, one per half: build, use and free a guard two orders past the old cliff on a
//! one-megabyte stack, then compile one. Each survives or it does not. A death is the
//! process ending, so each prints what it is about to try before it tries it.
//!
//! Run it deliberately: `cargo run --release --example guard_stack`.

use lookahead_engine::core::guard::{Guard, IGuardContext};
use lookahead_engine::core::guard_value::GuardValue;
use lookahead_engine::core::state::StateSymbols;
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::graph::node::LookAheadNode;
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::vars::DataVars;

/// A world that has heard of nothing, so evaluation walks the whole tree.
///
/// Unknown rather than false on purpose: a definite answer lets `And` and `Or` short-circuit
/// and stop descending, which would measure something shallower than the worst case.
struct Nothing;

impl IGuardContext for Nothing {
    fn get_variable(&self, _name: &str) -> GuardValue {
        GuardValue::unknown()
    }

    fn query(&self, _name: &str, _arguments: &[GuardValue]) -> GuardValue {
        GuardValue::unknown()
    }
}

/// A megabyte: the default main-thread stack on Windows, and the smallest place this code
/// could plausibly run.
const STACK: usize = 1024 * 1024;

/// Far enough past where a recursive walk overflows a one-megabyte stack - 2,875 levels in a
/// release build - that surviving it means something.
const WELL_PAST: usize = 40_000;

/// A guard `depth` levels deep, built without recursing.
fn nested(depth: usize) -> Guard {
    let mut node = Guard::variable("x");
    for _ in 1..depth {
        node = Guard::not(node);
    }
    node
}

/// Whether building, using and freeing a guard that deep survives on a `STACK`-byte thread.
///
/// All three in one pass, because an overflow ends the process and there is no second run to
/// try the next one in.
fn flat_consumers_survive(depth: usize) -> bool {
    on_a_small_thread(move || {
        let guard = nested(depth);
        let _ = guard.evaluate(&Nothing);
        let _ = guard.to_string();
        drop(guard);
    })
}

/// Whether compiling a guard that deep survives on a `STACK`-byte thread.
///
/// The manager is built INSIDE, which is the one-manager-per-thread invariant
/// `symbolic::isolated` records - each of these threads sees exactly one.
fn compiling_survives(depth: usize) -> bool {
    on_a_small_thread(move || {
        let symbols = StateSymbols::new();
        let node = LookAheadNode {
            ..LookAheadNode::new(DialogueNodeId::new(1, 0))
        };
        let graph = LookAheadGraph::new(vec![node], symbols).expect("a one-entry graph");
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        let vars = DataVars::new(&layout, graph.symbols(), DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars);

        let guard = nested(depth);
        let _ = compiler.compile(&guard);
    })
}

/// Runs `work` on a thread with the smallest stack worth worrying about.
fn on_a_small_thread(work: impl FnOnce() + Send + 'static) -> bool {
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(work)
        .expect("a thread")
        .join()
        .is_ok()
}

fn main() {
    println!("on a {} KB stack:\n", STACK / 1024);

    // THE FLAT HALF, and one line is the whole of it. A sweep in index order costs the same
    // stack at any depth, so there is nothing to walk up towards.
    println!("  building, evaluating, rendering and freeing {WELL_PAST} levels...");
    println!(
        "  {}",
        match flat_consumers_survive(WELL_PAST) {
            true => "survived - the flat consumers do not overflow at any depth",
            false => "DIED, which means something about a guard recurses again",
        }
    );

    // THE COMPILER, the same way. What is printed first is what was being tried if the
    // process ends here: an overflow on Windows is STATUS_STACK_OVERFLOW rather than a panic,
    // so the guard-page handler aborts the process and nothing after this line runs.
    println!("\n  compiling {WELL_PAST} levels...");
    println!(
        "  {}",
        match compiling_survives(WELL_PAST) {
            true => "survived - the compiler does not overflow at any depth",
            // An ordinary panic, not an overflow - which would never have reached here.
            false => "the thread failed WITHOUT overflowing, which is a bug",
        }
    );
}
