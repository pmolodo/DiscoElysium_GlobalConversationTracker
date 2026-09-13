// SPDX-License-Identifier: MIT
//! How deep a guard can be before the one thing that still walks it recursively overflows.
//!
//! ## Why this is worth measuring rather than guessing
//!
//! A guard comes out of a dialogue database that a game patch or another mod can change, so
//! "this string is not a guard" has to be an error and never a crash. Making that true needs
//! a depth limit, and a depth limit needs a number: high enough to accept everything real,
//! low enough to be safe on the smallest stack this code can find itself on.
//!
//! The deepest guard in the shipped database is ELEVEN levels, of 26,210
//! (measurements/guard_depth.rs). What is not known without measuring is the other end -
//! and it cannot be taken from a plain `cargo test` run, because a test thread's stack is
//! generous. This code runs inside the game, on whatever thread the dialogue system calls
//! it from.
//!
//! ## What is left to overflow, which is one thing rather than five
//!
//! It used to be five. The parser was recursive descent and gave out at about 260
//! re-entries; it is iterative now (de-bnjy.4). What it returned was a tree of `Box`es, and
//! `evaluate`, `Display` and the derived `Drop` all walked that; a guard is a flat table now
//! (de-eyk8.2), so building, freeing, evaluating and rendering one are sweeps in index order
//! that cannot overflow at any depth.
//!
//! `GuardCompiler::compile_node` still descends, and by choice: a comparison answers from
//! its operands' SHAPE without compiling either, so a bottom-up sweep would build a decision
//! diagram for every operand a comparison never looks at. Demand-driven is the cheaper walk,
//! and its depth is what `MAX_DEPTH` in src/parser/guard_parser.rs is chosen against.
//!
//! ## How
//!
//! Two phases, because the two halves now answer differently.
//!
//! FIRST, the flat consumers, at a depth two orders past the old cliff. Building, using and
//! freeing a 40,000-level guard on a one-megabyte stack either survives or it does not, and
//! it is one line of output rather than a walk.
//!
//! SECOND, the compiler, walking up until a thread dies. Every step prints before it tries,
//! because an overflow takes the process with it: the last line printed is the answer.
//!
//! Run it deliberately: `cargo run --release --example guard_stack`.

use lookahead_engine::core::guard::{Guard, IGuardContext};
use lookahead_engine::core::guard_value::GuardValue;
use lookahead_engine::core::state::StateSymbols;
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::graph::LookAheadGraph;
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

/// Far enough past the old cliff of 2,875 levels that surviving it means something.
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

    // EVERYTHING A READER NEEDS IS PRINTED BEFORE THE WALK, because nothing after it runs.
    // A closing summary could never appear: the walk ends by taking the process down, so
    // `deepest` and its arithmetic would be dead code that nonetheless implied the run
    // finishes normally - two contradictory accounts of the output, one false (de-wy8q).
    println!("\n  walking up on GuardCompiler::compile_node, the one walk that still descends:");
    println!("  THE LAST 'trying N' LINE IS THE ANSWER: the process dies at that depth.");
    println!("  the deepest guard in the shipped database is 11 levels, of 26,210");
    println!("  MAX_DEPTH in src/parser/guard_parser.rs is 256, between those two\n");

    for depth in (25..40_000).step_by(25) {
        println!("  trying {depth}...");
        if !compiling_survives(depth) {
            // NOT THE OVERFLOW, which never reaches here - a stack overflow on Windows is
            // STATUS_STACK_OVERFLOW rather than a panic, so the guard-page handler aborts
            // the process and `join` never returns at all. This catches an ordinary panic
            // inside the thread, which would otherwise look like the walk running out of
            // range.
            println!("  the thread at {depth} failed WITHOUT overflowing; that is a bug.");
            break;
        }
    }
}
