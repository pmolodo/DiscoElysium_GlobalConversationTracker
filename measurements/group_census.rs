// SPDX-License-Identifier: MIT
//! How many DISTINCT groups the game has, which decides the size of everything shipped per
//! group.
//!
//! ## Why this comes first
//!
//! de-eyk8.1 is to build what is a pure function of the index at index time and ship it -
//! the group graph, and the parent map and SCC decomposition beside it. de-t7wr is to ship
//! a verdict per group. Both are priced per group and both name the same unknown, which
//! de-t7wr calls "a cheap pre-measurement worth doing first": there are 1,501
//! conversations, but `discover_group` follows links between them, so a group is usually
//! several conversations and many conversations plausibly share one.
//!
//! If a thousand conversations resolve to a few hundred groups then the artefact is a few
//! hundred entries and the disk question answers itself. If they resolve to fourteen
//! hundred, it does not.
//!
//! ## What a group IS here, and the subtlety that decides the cache key
//!
//! `discover_group(index, start)` is the FORWARD closure of `start` over
//! `entry.to_conversation` - so it is not an equivalence relation, and two conversations in
//! the same cycle share a group while two that merely lead into a third do not.
//!
//! BUT THE GRAPH DEPENDS ONLY ON THE SET. `build_group_graph` walks the group in ascending
//! conversation order - `discover_group` sorts before returning - and that order is what
//! fixes the slot numbering. So two starts whose closures are the same SET produce an
//! identical graph, whichever member they started from, and the set is the right key.
//!
//! That is what this counts: distinct closures, not distinct starts.
//!
//! ## How to run it
//!
//! ```text
//! RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo group-census -- \
//!   cargo run --release --example group_census
//! ```
//!
//! ## What it said, 2026-09-07, and it is not the hoped-for answer
//!
//! ```text
//! conversations in the index       1501
//! distinct groups                  1422
//! groups only one start reaches    1400
//! conversations per group          1.06 on average
//!
//! entries in the index           112962
//! entries over all groups        130826, which is 1.2x the index
//! ```
//!
//! THERE IS ALMOST NO SHARING. de-t7wr hoped "a thousand conversations share a few hundred
//! groups"; the ratio is 1.06 to one and fourteen hundred of the groups are reached by
//! exactly one start. So a per-group artefact is fourteen hundred entries, not a few
//! hundred, and shipping the graph for all of them is 130,826 entries of parse against an
//! index of 112,962 - more than the index it would sit beside.
//!
//! ## But the split by SIZE is the useful answer, and it is lopsided the helpful way
//!
//! ```text
//! one conversation       1372 groups,   58487 entries,    43 entries each
//! several                  50 groups,   72339 entries,  1447 entries each
//! ```
//!
//! FIFTY GROUPS CARRY FIFTY-FIVE PER CENT OF THE ENTRIES. The other 1,372 average
//! forty-three entries apiece - `repeat_question` puts a 2,186-entry group at 4 ms, so
//! forty-three entries is microseconds, and shipping those buys nothing while costing half
//! the artefact.
//!
//! So the thing worth shipping is the FIFTY spanning groups: 3.5 per cent of the groups,
//! all of the cost, and the whole heavy list the measurements already use is inside it -
//! 368 at 4,724 entries over five conversations, 631 at 4,514 over six, 14 at 3,594 over
//! six.
//!
//! The tail by conversation count is worth seeing too: one group spans 49 conversations and
//! holds only 115 entries, and two more span 23 and 25 conversations for about 2,700 each.
//! Conversation count does not predict entry count, so a rule for "which groups to ship"
//! should be keyed on entries.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use lookahead_engine::index::{discover_group, read_index};

#[path = "../tests/common/mod.rs"]
mod common;

/// How many of the largest groups to name, for a sense of the tail.
const SHOW_LARGEST: usize = 10;

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let mut conversations: Vec<i32> = index.keys().copied().collect();
    conversations.sort_unstable();

    // The closure of every conversation, keyed by the SET it resolves to. The value is how
    // many conversations share it, which is the whole answer.
    let mut groups: HashMap<BTreeSet<i32>, usize> = HashMap::new();
    let began = std::time::Instant::now();
    for &conversation in &conversations {
        let group: BTreeSet<i32> = discover_group(&index, conversation).into_iter().collect();
        *groups.entry(group).or_default() += 1;
    }
    let took = began.elapsed();

    let alone = groups.values().filter(|shared| **shared == 1).count();
    let entries: usize = groups
        .keys()
        .map(|group| {
            group.iter().map(|id| index[id].entries.len()).sum::<usize>()
        })
        .sum();
    let all_entries: usize = conversations
        .iter()
        .map(|id| index[id].entries.len())
        .sum();

    println!("conversations in the index      {}", conversations.len());
    println!("distinct groups                 {}", groups.len());
    println!("groups only one start reaches   {alone}");
    println!(
        "conversations per group         {:.2} on average",
        conversations.len() as f64 / groups.len().max(1) as f64,
    );
    println!();
    println!("entries in the index            {all_entries}");
    println!(
        "entries over all distinct groups {entries}, which is {:.1}x the index",
        entries as f64 / all_entries.max(1) as f64,
    );
    println!("\ndiscovering all {} closures took {:.1?}", conversations.len(), took);

    // THE SPLIT THAT DECIDES WHAT IS WORTH SHIPPING. A group of one conversation is a few
    // dozen entries and parses in microseconds; the groups that cost milliseconds are the
    // ones that span several. If the second kind is a small set carrying most of the
    // entries, a per-group artefact can be restricted to it and stay small.
    let mut lone_groups = 0;
    let mut lone_entries = 0;
    let mut spanning_groups = 0;
    let mut spanning_entries = 0;
    for group in groups.keys() {
        let entries: usize = group.iter().map(|id| index[id].entries.len()).sum();
        if group.len() == 1 {
            lone_groups += 1;
            lone_entries += entries;
        } else {
            spanning_groups += 1;
            spanning_entries += entries;
        }
    }

    println!();
    println!(
        "one conversation      {lone_groups:>5} groups, {lone_entries:>7} entries, \
         {:>5.0} entries each",
        lone_entries as f64 / lone_groups.max(1) as f64,
    );
    println!(
        "several               {spanning_groups:>5} groups, {spanning_entries:>7} entries, \
         {:>5.0} entries each",
        spanning_entries as f64 / spanning_groups.max(1) as f64,
    );

    // THE TAIL, because an average hides it: if a handful of groups carry most of the
    // entries then what is shipped is dominated by those, however many groups there are.
    let mut by_size: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for group in groups.keys() {
        by_size.entry(group.len()).or_default().push(
            group.iter().map(|id| index[id].entries.len()).sum(),
        );
    }

    println!("\n{:>14}  {:>8}  {:>12}", "conversations", "groups", "entries");
    for (size, sizes) in by_size.iter() {
        println!(
            "{size:>14}  {:>8}  {:>12}",
            sizes.len(),
            sizes.iter().sum::<usize>(),
        );
    }

    let mut largest: Vec<(usize, usize)> = groups
        .keys()
        .map(|group| {
            (group.iter().map(|id| index[id].entries.len()).sum::<usize>(), group.len())
        })
        .collect();
    largest.sort_unstable_by(|a, b| b.cmp(a));
    println!("\nthe {SHOW_LARGEST} largest groups, by entries:");
    for (entries, conversations) in largest.iter().take(SHOW_LARGEST) {
        println!("  {entries:>8} entries over {conversations} conversations");
    }
}
