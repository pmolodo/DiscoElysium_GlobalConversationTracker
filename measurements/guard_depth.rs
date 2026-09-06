// SPDX-License-Identifier: MIT
//! How deeply nested is the deepest guard the shipped database actually contains?
//!
//! The number behind a claim that was only ever a comment. `tests/properties.rs` says of its
//! 300-level nesting case that "the deepest guard in the shipped database is nothing like
//! this", and everything that follows from that - whether a recursive-descent parser is
//! safe, whether `GuardExpression`'s recursive drop can ever matter, whether the flattened
//! arena of de-eyk8.2 is worth its cost - rests on it being true. So measure it.
//!
//! DEPTH IS WHAT BOUNDS THE RECURSION, in both directions. The parser recurses as it builds,
//! several frames per level; `evaluate`, `read_by_guard`, the guard compiler and `Display`
//! recurse as they walk; and the derived `Drop` recurses as it frees, because the children
//! are `Box`ed. All four are bounded by this one figure.
//!
//! THE CHECK THAT KEEPS THIS TRUE IS ELSEWHERE. `tests/guard_depth.rs` walks the same
//! guards with an assertion on the end, so a database or a parser that stopped accepting
//! something real fails the suite rather than waiting for somebody to run this. The two
//! were one file until de-18bo.4; the doc comment there says why they are not.
//!
//! Run it with `cargo run --release --example guard_depth`.

use std::collections::BTreeMap;

use lookahead_engine::core::guard::GuardExpression;
use lookahead_engine::index::read_index;
use lookahead_engine::parser::guard_parser::parse_guard;

#[path = "../tests/common/mod.rs"]
mod common;

/// How many levels of nesting an expression has; a leaf is 1.
fn depth_of(guard: &GuardExpression) -> usize {
    match guard {
        GuardExpression::Literal(_) | GuardExpression::Variable(_) => 1,
        GuardExpression::Not(inner) => 1 + depth_of(inner),
        GuardExpression::And(a, b)
        | GuardExpression::Or(a, b)
        | GuardExpression::Comparison(_, a, b) => 1 + depth_of(a).max(depth_of(b)),
        GuardExpression::Call(_, args) => {
            1 + args.iter().map(depth_of).max().unwrap_or(0)
        }
    }
}

fn main() {
    let Some(path) = common::conversation_index() else { return };
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

            let depth = depth_of(&parsed);
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

    println!("\nDEEPEST: {} levels, at {}:{}", deepest.0, deepest.2, deepest.3);
    let text = deepest.1.trim();
    println!("{}", &text[..text.len().min(600)]);
}
