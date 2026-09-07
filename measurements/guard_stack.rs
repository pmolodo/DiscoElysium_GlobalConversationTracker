// SPDX-License-Identifier: MIT
//! How deep a guard's tree can be before something that walks it overflows the stack.
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
//! ## What changed, and why this file no longer measures the parser
//!
//! It used to. The parser was recursive descent, so it was the shallowest thing in the
//! chain: it overflowed a megabyte at about 260 re-entries, five stack frames per level of
//! nesting. It is iterative now (de-bnjy.4) - nesting costs entries in a `Vec` and nothing
//! on the stack - so parsing is no longer what fails first, or at all.
//!
//! THE RISK DID NOT GO AWAY, IT MOVED. What the parser returns is a tree of `Box`es, and
//! everything that consumes one recurses over it: `evaluate`, `Display`, and the `Drop` that
//! frees it. So the number that matters now is theirs, and it is the number `MAX_DEPTH` in
//! src/parser/guard_parser.rs is chosen against.
//!
//! ## How
//!
//! Build a tree of a known depth - directly, in a loop, because the parser refuses anything
//! past its own limit and this has to go well past it - then USE it the way the engine does
//! and let it fall out of scope. On threads of known stack size, walking up until one dies.
//!
//! Run it deliberately: `cargo run --release --example guard_stack`. A thread
//! that overflows takes the process with it, which is why every step prints before it tries:
//! the last line printed is the answer.

use lookahead_engine::core::guard::{GuardExpression, IGuardContext};
use lookahead_engine::core::guard_value::GuardValue;

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

/// A tree `depth` levels deep, built without recursing.
fn nested(depth: usize) -> GuardExpression {
    let mut node = GuardExpression::Variable("x".into());
    for _ in 1..depth {
        node = GuardExpression::Not(Box::new(node));
    }
    node
}

/// Whether building, using and freeing a tree that deep survives on a `stack`-byte thread.
///
/// All three consumers in one pass, because an overflow ends the process and there is no
/// second run to try the next one in. The answer wanted is the shallowest depth at which ANY
/// of them fails, which is what one walk finds.
fn survives_here(depth: usize, stack: usize) -> bool {
    std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || {
            let tree = nested(depth);
            let _ = tree.evaluate(&Nothing);
            let _ = tree.to_string();
            drop(tree);
        })
        .expect("a thread")
        .join()
        .is_ok()
}

/// How deep is safe, on the stack size worth knowing about.
fn main() {
    // A megabyte is the default main-thread stack on Windows, which is the smallest place
    // this code could plausibly run.
    let stack = 1024 * 1024;

    // EVERYTHING A READER NEEDS IS PRINTED BEFORE THE WALK, because nothing after it runs.
    // These two lines used to be a closing summary, which could never appear: the walk ends
    // by taking the process down, so `deepest`, the bytes-a-level arithmetic and this note
    // were all dead code that nonetheless implied the run finishes normally - two
    // contradictory accounts of how to read the output, one of them false (de-wy8q).
    println!("walking up on a {} KB stack:", stack / 1024);
    println!("THE LAST 'trying N' LINE IS THE ANSWER: the process dies at that depth.");
    println!("  last recorded: 2,875 levels here, about 365 bytes a level");
    println!("  the deepest guard in the shipped database is 11 levels, of 26,210");
    println!("  MAX_DEPTH in src/parser/guard_parser.rs is 256, between those two\n");

    for depth in (25..40_000).step_by(25) {
        println!("  trying {depth}...");
        if !survives_here(depth, stack) {
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
