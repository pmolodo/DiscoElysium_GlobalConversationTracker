// SPDX-License-Identifier: MIT
//! How deeply nested is the deepest guard the shipped database actually contains?
//!
//! The number behind a claim that was only ever a comment. `tests/properties.rs` says of its
//! 300-level nesting case that "the deepest guard in the shipped database is nothing like
//! this", and what rests on that being true is where `guard_parser::MAX_DEPTH` is set. So
//! measure it.
//!
//! DEPTH IS WHAT BOUNDS THE ONE REMAINING RECURSION. A guard is a flat table, so building,
//! freeing, evaluating and rendering one are all sweeps that cannot overflow whatever the
//! nesting. `GuardCompiler::compile_node` still descends, because it is demand-driven and a
//! sweep would compile operands that comparisons never read - and that descent is what this
//! figure bounds.
//!
//! THE CHECK THAT KEEPS THIS TRUE IS ELSEWHERE. `tests/guard_depth.rs` walks the same
//! guards with an assertion on the end, so a database or a parser that stopped accepting
//! something real fails the suite rather than waiting for somebody to run this. The two
//! were one file until de-18bo.4; the doc comment there says why they are not.
//!
//! Run it with `cargo run --release --example guard_depth`.

use std::collections::BTreeMap;

use lookahead_engine::index::read_index;
use lookahead_engine::parser::guard_parser::parse_guard;

#[path = "../tests/common/mod.rs"]
mod common;

fn main() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");

    let mut histogram: BTreeMap<usize, usize> = BTreeMap::new();
    let mut deepest = (0usize, String::new(), 0i32, 0i32);
    let mut guards = 0usize;
    let mut unparsed = 0usize;

    for (id, conversation) in &index {
        for entry in &conversation.entries {
            if entry.guard.trim().is_empty() {
                continue;
            }
            guards += 1;

            let Ok(parsed) = parse_guard(&entry.guard) else {
                unparsed += 1;
                continue;
            };

            let depth = parsed.depth();
            *histogram.entry(depth).or_insert(0) += 1;
            if depth > deepest.0 {
                deepest = (depth, entry.guard.clone(), *id, entry.id);
            }
        }
    }

    println!("{guards} non-empty guards, {unparsed} of them unparsed");
    println!("\n{:>6}  {:>7}", "depth", "guards");
    for (depth, count) in &histogram {
        println!("{depth:>6}  {count:>7}");
    }

    println!(
        "\nDEEPEST: {} levels, at {}:{}",
        deepest.0, deepest.2, deepest.3
    );
    let text = deepest.1.trim();
    println!("{}", &text[..text.len().min(600)]);
}
