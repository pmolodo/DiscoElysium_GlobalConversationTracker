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
//! Run it with `--ignored --release`.

use std::collections::BTreeMap;

use lookahead_engine::core::guard::GuardExpression;
use lookahead_engine::index::read_index;
use lookahead_engine::parser::guard_parser::parse_guard;

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

#[test]
#[ignore = "a measurement, not a test: run it with --ignored --release"]
fn how_deep_the_deepest_guard_in_the_database_is() {
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

/// EVERY guard in the database parses, which is what says the depth limit is not in the way.
///
/// ## Why this is a test and the one above is a measurement
///
/// The parser refuses anything nested past `MAX_DEPTH` (de-fpax), because unbounded
/// recursion on content a game patch or another mod can change is a stack overflow, and an
/// overflow aborts the process rather than raising anything catchable. A limit is only
/// acceptable while it is comfortably above everything real - and "comfortably above" is a
/// claim about the database, which changes.
///
/// The measurement above establishes it once, by hand. This keeps it true: it is the same
/// walk with an assertion on the end, so a database or a parser that stopped accepting
/// something real fails here instead of silently answering Unknown - which is permissive,
/// which is a marker that is wrong with nothing to say so.
///
/// WHAT IT DOES NOT CATCH, checked rather than assumed: a depth limit set far too low.
/// Dropping MAX_DEPTH from 64 to 4 leaves this passing, because real guards barely recurse
/// at all - `a and b and c` chains in a while loop and costs no recursion, and the nesting
/// that does cost it, `not` and parentheses, is rare and shallow in this database. So this
/// guards the PARSER against losing real content, which is what it is for and what makes it
/// the net under a rewrite; it is not a check on the limit.
///
/// ## What it does when there is no corpus
///
/// Skips, loudly. The index is extracted game content and a checkout without the game
/// cannot produce it; `common::conversation_index()` rebuilds it where it can and returns
/// None where it cannot.
#[test]
fn every_guard_in_the_database_parses() {
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the index reads");

    let mut guards = 0usize;
    let mut refused: Vec<String> = Vec::new();

    for (id, conversation) in &index {
        for entry in &conversation.entries {
            if entry.guard.trim().is_empty() {
                continue;
            }

            guards += 1;
            if let Err(why) = parse_guard(&entry.guard) {
                // The first few are what a person needs; a list of thousands is not.
                if refused.len() < 5 {
                    refused.push(format!("{id}:{} - {why}", entry.id));
                }
            }
        }
    }

    assert!(guards > 0, "the index carried no guards at all, so this checked nothing");
    assert!(
        refused.is_empty(),
        "{} of {guards} guards in the database no longer parse:\n  {}",
        refused.len(),
        refused.join("\n  "),
    );

    eprintln!("{guards} guards, all of them parsed");
}
