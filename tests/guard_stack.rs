// SPDX-License-Identifier: MIT
//! How much STACK one level of guard nesting costs, and therefore how deep is safe.
//!
//! ## Why this is worth measuring rather than guessing
//!
//! The guard parser is recursive descent, and a guard comes out of a dialogue database that
//! a game patch or another mod can change - so "this string is not a guard" has to be an
//! error and never a crash. Making that true needs a depth limit, and a depth limit needs a
//! number: high enough to accept everything real, low enough to be safe on the smallest
//! stack the parser can find itself on.
//!
//! The deepest guard in the shipped database is ELEVEN levels, of 26,210 (tests/
//! guard_depth.rs). What is not known without measuring is the other end - and it cannot be
//! taken from a plain `cargo test` run, because a test thread's stack is generous. The
//! parser runs inside the game, on whatever thread the dialogue system calls it from.
//!
//! ## How
//!
//! Parse at increasing depths on threads of known stack size, and find where each one
//! stops. `stack / deepest` is what a level costs, and it is what a limit has to be chosen
//! against.
//!
//! Run it deliberately: `cargo test --test guard_stack -- --ignored --nocapture`. Each
//! failure is a thread that overflows, which Rust reports and aborts - so this spawns them
//! one at a time and reads the exit rather than catching anything.

use lookahead_engine::parser::guard_parser::parse_guard;

/// A guard nested `depth` deep: `not (not (... x ...))`.
fn nested(depth: usize) -> String {
    format!(
        "{}Variable[\"x\"]{}",
        "not (".repeat(depth),
        ")".repeat(depth),
    )
}

/// Whether parsing at `depth` survives on a thread with `stack` bytes.
///
/// A thread that overflows takes the PROCESS down, so this cannot simply be looped over in
/// one binary. It is run as a child process instead - see the test below.
fn survives_here(depth: usize, stack: usize) -> bool {
    std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || {
            let _ = parse_guard(&nested(depth));
        })
        .expect("a thread")
        .join()
        .is_ok()
}

/// What one level costs, on the stack sizes worth knowing about.
///
/// ONE DEPTH PER PROCESS would be the careful way and is far too slow; instead this walks
/// up from a depth known to be safe and stops at the first that is not. When it overflows
/// it takes the run with it, and the last line printed is the answer - which is why every
/// step prints before it tries.
#[test]
#[ignore = "a measurement; it ends by overflowing a thread on purpose"]
fn what_one_level_of_nesting_costs() {
    // A megabyte is the default main-thread stack on Windows, which is the smallest place
    // this code could plausibly run.
    let stack = 1024 * 1024;

    println!("walking up on a {} KB stack:", stack / 1024);
    let mut deepest = 0;
    for depth in (5..2000).step_by(5) {
        println!("  trying {depth}...");
        if !survives_here(depth, stack) {
            break;
        }
        deepest = depth;
    }

    println!(
        "\nDEEPEST at {} KB: {deepest} levels, about {} bytes a level",
        stack / 1024,
        if deepest > 0 { stack / deepest } else { 0 },
    );
    println!("the deepest guard in the shipped database is 11 levels");
}
