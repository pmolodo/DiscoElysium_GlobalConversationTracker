// SPDX-License-Identifier: MIT
//! Every guard the shipped database contains still parses.
//!
//! The net under the guard parser. What the depth limit costs, and how deep real content
//! actually goes, is a number rather than a claim - it is measured in
//! `measurements/guard_depth.rs`, and this keeps that measurement's conclusion true
//! between runs of it.

use lookahead_engine::index::read_index;
use lookahead_engine::parser::guard_parser::parse_guard;

mod common;

/// EVERY guard in the database parses, which is what says the depth limit is not in the way.
///
/// ## Why this is a test and the measurement beside it is not
///
/// The parser refuses anything nested past `MAX_DEPTH` (de-fpax), because unbounded
/// recursion on content a game patch or another mod can change is a stack overflow, and an
/// overflow aborts the process rather than raising anything catchable. A limit is only
/// acceptable while it is comfortably above everything real - and "comfortably above" is a
/// claim about the database, which changes.
///
/// `measurements/guard_depth.rs` establishes the figure once, by hand. This keeps it true:
/// it is the same walk with an assertion on the end, so a database or a parser that stopped
/// accepting something real fails here instead of silently answering Unknown - which is
/// permissive, which is a marker that is wrong with nothing to say so.
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
