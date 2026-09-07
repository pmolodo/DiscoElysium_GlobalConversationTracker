// SPDX-License-Identifier: MIT
//! Is a group's PARSED graph bigger or smaller than the source it was parsed from?
//!
//! ## The one number de-eyk8.1 turns on
//!
//! That issue would ship what is a pure function of the index - the parsed group graph -
//! so the engine does not rebuild it. `measurements/group_census.rs` sized the job: fifty
//! groups spanning several conversations carry 72,339 of the game's 130,826 group-entries
//! and are where all the cost is.
//!
//! But shipping it AS AN ADDITION does not pay, and the measurement that says so is not
//! about the parse at all. Reading the 14.4 MB `conversation_index.trimmed.jsonl` takes
//! 173 to 244 ms at engine-host startup, against 3 to 9 ms to parse one group on the first
//! menu in it. A player meets a handful of the heavy groups in a session, so an artefact
//! that has to be READ at startup to save tens of milliseconds later is the wrong way
//! round.
//!
//! WHICH LEAVES ONE DESIGN THAT COULD PAY: ship the parsed form INSTEAD OF the source
//! strings for those groups, so the index does not grow, the parse is skipped, and startup
//! may get faster. That is worth building if and only if the parsed form is no larger than
//! the `guard` and `script` strings it replaces - which is what this weighs.
//!
//! ## What it compares
//!
//! Per group, over the entries in it:
//!
//! - SOURCE: the bytes of `entry.guard` and `entry.script`, which are what a replacement
//!   would delete from the index. This is the honest baseline - the rest of an entry (id,
//!   links, fields) stays whatever happens.
//! - WHOLE: the entire `LookAheadGraph` through `bincode`, which is the format a shipped
//!   artefact would sensibly use and is already a dependency here. It carries more than the
//!   two strings - the symbol table, the links, the slot assignments, the kind and cost
//!   fields - so it OVERSTATES what a replacement would cost.
//! - PARSED: just each entry's parsed guard and parsed actions, which is exactly what the
//!   two source strings become. The tight comparison, and the one the ratio reports.
//!
//! ## What it said, 2026-09-07, and the answer is NO
//!
//! ```text
//!   conv   entries   source kB   whole kB   parsed kB   ratio
//!     28      2186        56.6      255.3       111.3   1.97x
//!    368      4724        87.6      520.1       218.8   2.50x
//!     14      3594       100.6      411.9       175.4   1.74x
//!    631      4514        86.2      492.3       200.0   2.32x
//!    362      1860        33.2      211.2        91.7   2.76x
//!
//!    all     72339      1630.3     8143.4      3447.6   2.11x
//! ```
//!
//! THE PARSED FORM IS TWICE THE SOURCE, on the most generous reading available: 3.4 MB of
//! guard trees and action lists against 1.6 MB of the strings they came from, over the
//! fifty groups worth shipping at all. The whole graph is five times.
//!
//! So the replacement does not replace anything - it ADDS about 1.8 MB to a 14.4 MB index
//! that already takes 173 to 244 ms to read at startup. Call it twenty to thirty
//! milliseconds more, every session, to save the 3 to 9 ms of parse for each heavy group
//! the player actually walks into. Four such groups in a session is roughly a wash, and the
//! price of the wash is a new shipped artefact, an index format bump, and a staleness rule.
//!
//! WHY IT COMES OUT THIS WAY is worth keeping, because the intuition says otherwise: a
//! guard's source is a terse expression - a few dozen characters - and its parsed form is a
//! tree of tagged nodes, each with a discriminant and its own length-prefixed fields. Text
//! is a good encoding of a small expression. The parse is not saving space, it is saving
//! TIME, and 3 to 9 ms a group turns out not to be enough of it.
//!
//! ## How to run it
//!
//! ```text
//! RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo parsed-vs-source -- \
//!   cargo run --release --example parsed_vs_source
//! ```

use std::collections::BTreeSet;

use lookahead_engine::index::{build_group_graph, discover_group, read_index, Index};

#[path = "../tests/common/mod.rs"]
mod common;

/// The heavy groups every other measurement here uses, named for comparability.
const NAMED: [i32; 5] = [28, 368, 14, 631, 362];

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    println!(
        "{:>6}  {:>8}  {:>12}  {:>14}  {:>12}  {:>8}",
        "conv", "entries", "source kB", "whole kB", "parsed kB", "ratio",
    );

    let mut named_source = 0usize;
    let mut named_parsed = 0usize;
    for conversation in NAMED {
        let Some((source, whole, parsed, entries)) = weigh(&index, conversation) else {
            continue;
        };
        named_source += source;
        named_parsed += parsed;
        row(conversation.to_string(), entries, source, whole, parsed);
    }

    println!();
    println!(
        "the five named groups: {:.0} kB of source against {:.0} kB of parsed guards and \
         actions, {:.2}x",
        named_source as f64 / 1024.0,
        named_parsed as f64 / 1024.0,
        named_parsed as f64 / named_source.max(1) as f64,
    );

    // AND THE WHOLE JOB, which is what would actually be shipped: every group that spans
    // more than one conversation, deduplicated by the SET it resolves to - see
    // `group_census` for why the set is the key.
    let mut seen: BTreeSet<Vec<i32>> = BTreeSet::new();
    let mut conversations: Vec<i32> = index.keys().copied().collect();
    conversations.sort_unstable();

    let mut groups = 0usize;
    let (mut source, mut whole, mut parsed, mut entries) = (0usize, 0usize, 0usize, 0usize);
    for conversation in conversations {
        let group = discover_group(&index, conversation);
        if group.len() < 2 || !seen.insert(group.clone()) {
            continue;
        }
        let Some((one_source, one_whole, one_parsed, one_entries)) = weigh(&index, conversation)
        else {
            continue;
        };
        groups += 1;
        source += one_source;
        whole += one_whole;
        parsed += one_parsed;
        entries += one_entries;
    }

    println!();
    row("all".to_string(), entries, source, whole, parsed);
    println!(
        "\n{groups} groups spanning several conversations, {entries} entries.\n\
         The parsed guards and actions are {:.2}x the source strings they would replace; \
         the whole graph is {:.2}x.",
        parsed as f64 / source.max(1) as f64,
        whole as f64 / source.max(1) as f64,
    );
}

/// One group's source bytes, its serialised sizes, and how many entries it holds.
fn weigh(index: &Index, conversation: i32) -> Option<(usize, usize, usize, usize)> {
    let (graph, group) = build_group_graph(index, conversation).ok()?;

    let source: usize = group
        .iter()
        .flat_map(|id| index[id].entries.iter())
        .map(|entry| entry.guard.len() + entry.script.len())
        .sum();

    let binary = match bincode::serialize(&graph) {
        Ok(bytes) => bytes.len(),
        Err(error) => {
            eprintln!("conversation {conversation}: bincode said {error}");
            return None;
        }
    };

    // THE TIGHTEST FAIR COMPARISON, and the column that actually decides: just the two
    // parsed things against just the two strings they came from. The whole-graph column
    // above carries the symbol table, the links and the slot assignments as well, none of
    // which a replacement would delete from the index, so it overstates the cost.
    let just_parsed: Vec<(&_, &_)> = graph
        .nodes()
        .map(|node| (&node.guard, &node.actions))
        .collect();
    let parsed = bincode::serialize(&just_parsed).map(|bytes| bytes.len()).unwrap_or(0);

    Some((source, binary, parsed, graph.count()))
}

/// The ratio reported is PARSED against SOURCE - the tight comparison - and not the whole
/// graph, which is shown beside it for scale rather than as the thing being decided.
fn row(name: String, entries: usize, source: usize, whole: usize, parsed: usize) {
    println!(
        "{name:>6}  {entries:>8}  {:>12.1}  {:>14.1}  {:>12.1}  {:>7.2}x",
        source as f64 / 1024.0,
        whole as f64 / 1024.0,
        parsed as f64 / 1024.0,
        parsed as f64 / source.max(1) as f64,
    );
}
