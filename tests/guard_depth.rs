// SPDX-License-Identifier: MIT
//! Every guard the shipped database contains still parses.
//!
//! The net under the guard parser. How deep real content actually goes is a number rather
//! than a claim - it is measured in `performance/guard_depth.rs` - and this checks the
//! thing the figure is only a description of: that all of that content is still read.

use lookahead_engine::index::read_index;
use lookahead_engine::parser::guard_parser::parse_guard;

use gct_measure::common;

/// EVERY guard in the database parses.
///
/// ## Why this is a test and the measurement beside it is not
///
/// A guard the parser refuses is not an error anybody sees: it evaluates as Unknown, which
/// is permissive, which is a marker that is wrong with nothing to say so. Whether real content
/// parses is a claim about the database, which a game patch or another mod can change, and
/// about the parser, which is rewritten from time to time.
///
/// `performance/guard_depth.rs` walks the same guards once, by hand, to describe them. This
/// is the same walk with an assertion on the end, so a database or a parser that stopped
/// accepting something real fails here instead of silently answering Unknown.
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

    assert!(
        guards > 0,
        "the index carried no guards at all, so this checked nothing"
    );
    assert!(
        refused.is_empty(),
        "{} of {guards} guards in the database no longer parse:\n  {}",
        refused.len(),
        refused.join("\n  "),
    );

    eprintln!("{guards} guards, all of them parsed");
}
