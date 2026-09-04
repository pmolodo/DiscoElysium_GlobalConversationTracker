// SPDX-License-Identifier: MIT
//! Writes what the parser makes of every guard in the database, for comparing runs.
//!
//! ## What this is for
//!
//! Rewriting a parser (de-bnjy.4, recursive descent to an explicit stack) needs a stronger
//! check than "everything still parses" - `tests/guard_depth.rs` says that, and it would
//! also say it about a parser that got the precedence backwards. What is wanted is that the
//! new one produces the SAME TREES as the old one on real content.
//!
//! So: dump one line per guard, printed through `Display`, which is the tree written out.
//! Run it on each side of the change and diff the two files. Identical output over 26,210
//! guards is the equivalence claim; a diff is the list of guards that changed meaning.
//!
//! ## How
//!
//! `GUARD_SNAPSHOT=<path> cargo test --release --test guard_snapshot -- --ignored`
//!
//! It is kept rather than deleted because the next person to touch the parser wants it, and
//! it costs nothing while it is ignored.

use std::fmt::Write as _;
use std::fs;

use lookahead_engine::index::read_index;
use lookahead_engine::parser::guard_parser::parse_guard;

mod common;

#[test]
#[ignore = "a snapshot; run it deliberately, on both sides of a parser change"]
fn what_the_parser_makes_of_every_guard() {
    let out = std::env::var("GUARD_SNAPSHOT")
        .expect("set GUARD_SNAPSHOT to the file to write");
    let Some(path) = common::conversation_index() else {
        panic!("no conversation index, so there is nothing to snapshot");
    };
    let index = read_index(&path).expect("the index reads");

    // Sorted, so two runs line up regardless of how the index iterates.
    let mut ids: Vec<_> = index.keys().copied().collect();
    ids.sort();

    let mut text = String::new();
    let mut guards = 0usize;
    for id in ids {
        let conversation = &index[&id];
        for entry in &conversation.entries {
            if entry.guard.trim().is_empty() {
                continue;
            }
            guards += 1;
            match parse_guard(&entry.guard) {
                Ok(tree) => writeln!(text, "{id}:{} = {tree}", entry.id).unwrap(),
                Err(why) => writeln!(text, "{id}:{} ! {why}", entry.id).unwrap(),
            }
        }
    }

    // The corner cases the corpus does not contain, which is exactly where a rewrite goes
    // wrong: precedence between the four operators, the shapes that are errors on purpose,
    // and the two the tokeniser has to be talked out of (a minus before a number, a name
    // that is a call only because a parenthesis follows it).
    text.push_str("\n-- shapes the database has none of --\n");
    for guard in TRICKY {
        match parse_guard(guard) {
            Ok(tree) => writeln!(text, "{guard} = {tree}").unwrap(),
            Err(why) => writeln!(text, "{guard} ! {why}").unwrap(),
        }
    }

    fs::write(&out, text).expect("the snapshot writes");
    eprintln!("{guards} guards and {} shapes written to {out}", TRICKY.len());
}

/// Hand-written guards that pin down the grammar's corners.
const TRICKY: &[&str] = &[
    // Precedence, in both directions and against `not`.
    "a and b or c",
    "a or b and c",
    "a or b or c",
    "a and b and c",
    "not a and b",
    "not a == b",
    "not not a",
    "not (a and b)",
    "(a or b) and c",
    "a == b and c == d",
    // Comparison does not chain, and never did.
    "a == b == c",
    "1 < 2 < 3",
    // Calls: none, one, several, nested, trailing comma, and a call as an operand.
    "f()",
    "f(a)",
    "f(a, b)",
    "f(a,)",
    "f(g(a), b or c)",
    "f(a) and g(b)",
    "not f(a)",
    // Values.
    "x > -1",
    "-1 < x",
    "x ~= nil",
    "true and false",
    "Variable[\"x\"] == \"text\"",
    "Variable [ \"spaced\" ]",
    // And the ones that are errors, whose messages should not drift either.
    "(a",
    "a)",
    "f(a",
    "f(,a)",
    "and a",
    "a and",
    "()",
    "",
];
