// SPDX-License-Identifier: MIT
//! How much smaller would the layout be if it were built from where a query STARTS?
//!
//! ## The question de-3x76.8 asks and could not answer
//!
//! `DataLayout::read_by` collects every name any guard ANYWHERE in the group reads, and
//! `keeping_only_read` then drops the slots nothing reads. But a look-ahead query enters at
//! one entry, and a slot read only by guards on entries that entry cannot structurally
//! reach is carried for nothing.
//!
//! The task said the saving "might be big on exactly the group that hurts" - conversation
//! 14 is 1,628 entries and its GROUP is 3,594 - and then said, correctly, that nothing
//! measures it. This does.
//!
//! ## What it costs if the answer is yes, which is why measuring came first
//!
//! The layout stops being a property of the GROUP and becomes a property of the QUERY.
//! Since de-2wtl that is not a small thing: `workspace::Workspace` keeps a diagram manager
//! alive between requests, keyed on the group, and `measurements/manager_reuse.rs` prices
//! that reuse at 45 ms a request (98 ms against 53 ms). A layout that differs per start
//! cannot share one manager across starts, so the saving has to beat that.
//!
//! ## THE UNIT IS A MENU, NOT A START, and that is the point most likely to be missed
//!
//! The plugin makes ONE call per response menu carrying all of that menu's options as
//! starts (`ResponseLookAheadPatch::AskAbout` groups by conversation and sends the list).
//! So a start-relative layout would in practice be relative to the UNION of a menu's
//! starts, not to one of them - and a menu whose options fan out across the group unions
//! back to nearly the whole thing.
//!
//! ## What it found, 2026-09-07
//!
//! ```text
//!  conv entries  whole   per start (mean)    per menu (mean)   per conversation
//!   362    1860    118 102 (14%), best 19 104 (12%), best 19 118 (0%), best 118
//!    28    2186    144  71 (51%), best 16  70 (51%), best 16 104 (28%), best 63
//!   368    4724    241 118 (51%), best 31 119 (51%), best 31 124 (49%), best 31
//!    14    3594    236 195 (17%), best 23 201 (15%), best 23 193 (18%), best 23
//!   631    4514    257 223 (13%), best 25 227 (12%), best 25 243 (5%), best 238
//!  1030    1476     73    67 (8%), best 0    67 (8%), best 1   73 (0%), best 73
//! ```
//!
//! The means are over samples, and the three columns sample different populations - starts,
//! menus, conversations - so a column being lower than the one beside it is not a like-for
//! -like comparison. The percentages against `whole` are what each column is for.
//!
//! THE UNION COSTS ALMOST NOTHING. Per-menu tracks per-start to within a point or two
//! everywhere, so the worry above - that a menu fanning out across the group unions back to
//! the whole thing - does not happen. The saving is real at the granularity that would
//! actually ship.
//!
//! AND THE PER-CONVERSATION COLUMN IS THE INTERESTING ONE. On 368, the largest group, it
//! keeps 49 of the 51 points; on 14 it is as good as per-menu or slightly better. It gets
//! nothing on 362 and 1030 and half the win on 28 and 631 - but those four all finish
//! already, so the two it does serve are the two that matter.
//!
//! IT DOES NOT UNBLOCK 14, which is the group this epic exists for: 236 variables to 193 is
//! not the difference between not finishing and finishing. That is the same answer
//! de-3x76.2 and de-3x76.5 reached, from different directions.
//!
//! ## How to run it
//!
//! ```text
//! DEGCT_RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo start-relative -- \
//!   cargo run --release --example start_relative_layout
//! ```
//!
//! Deterministic: it reads the graph and the layout and never runs a search.

use std::collections::{HashSet, VecDeque};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;

#[path = "../tests/common/mod.rs"]
mod common;

/// The groups the matrix measures, so the rows sit beside its numbers.
const GROUPS: [i32; 6] = [362, 28, 368, 14, 631, 1030];

/// The cap every symbolic measurement in this repository uses.
const COUNTER_CAP: i32 = 16;

/// How many menus to sample per group.
///
/// Every node with more than one link is a menu, and the big groups have thousands. The
/// sample is taken in id order rather than at random so two runs agree exactly.
const MENUS_SAMPLED: usize = 200;

/// How many starts to sample per group for the per-start column, for the same reason.
const STARTS_SAMPLED: usize = 200;

fn main() {
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the index reads");

    println!(
        "Variables in the shipped whole-group layout, against layouts built from what one \
         start, one MENU's starts, and one CONVERSATION's entries can structurally \
         reach.\n"
    );
    println!(
        "{:>5} {:>7} {:>6} {:>18} {:>18} {:>18}",
        "conv", "entries", "whole", "per start (mean)", "per menu (mean)",
        "per conversation"
    );

    for conversation in GROUPS {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            println!("{conversation:>5}  does not build");
            continue;
        };

        let symbols = graph.symbols().clone();
        let whole = vars_reading(&graph, &DataLayout::read_by(&graph));

        let ids: Vec<DialogueNodeId> = {
            let mut ids: Vec<DialogueNodeId> = graph.nodes().map(|node| node.id).collect();
            ids.sort_by_key(|id| (id.conversation_id, id.entry_id));
            ids
        };

        // ONE START AT A TIME: the best case, and not a shape anything actually asks for.
        let mut per_start = Sampler::new();
        for id in ids.iter().step_by(step(ids.len(), STARTS_SAMPLED)) {
            let reached = reachable_from(&graph, [*id]);
            per_start.add(vars_reading(&graph, &DataLayout::read_by_some(&graph, reached)));
        }

        // A WHOLE MENU AT ONCE, which is the shape the plugin sends: every option of one
        // response menu in a single request, so the layout has to serve all of them.
        let mut per_menu = Sampler::new();
        // SORTED BEFORE SAMPLING. `graph.nodes()` iterates a map, so its order is not
        // fixed - and a stride taken over an unordered list samples a different 200 menus
        // on every run, which showed up as this column moving by several points between
        // two runs of an otherwise deterministic measurement.
        let mut menus: Vec<(DialogueNodeId, &Vec<DialogueNodeId>)> = graph
            .nodes()
            .filter(|node| node.links.len() > 1)
            .map(|node| (node.id, &node.links))
            .collect();
        menus.sort_by_key(|(id, _)| (id.conversation_id, id.entry_id));
        for (_, menu) in menus.iter().step_by(step(menus.len(), MENUS_SAMPLED)) {
            let reached = reachable_from(&graph, menu.iter().copied());
            per_menu.add(vars_reading(&graph, &DataLayout::read_by_some(&graph, reached)));
        }

        // A WHOLE CONVERSATION AT ONCE - the middle ground, and the one that might be had
        // without giving up de-2wtl. A layout that is a property of the conversation the
        // player is standing in is STABLE ACROSS THE MENUS INSIDE IT, so the kept manager
        // survives from one menu to the next exactly where it earns its 45 ms; only walking
        // into a different conversation of the group rebuilds it, which is already when a
        // player pays for a scene change.
        let mut per_conversation = Sampler::new();
        let mut conversations: Vec<i32> =
            ids.iter().map(|id| id.conversation_id).collect();
        conversations.dedup();
        for member in conversations {
            let entries = ids.iter().copied().filter(|id| id.conversation_id == member);
            let reached = reachable_from(&graph, entries);
            per_conversation
                .add(vars_reading(&graph, &DataLayout::read_by_some(&graph, reached)));
        }

        println!(
            "{conversation:>5} {:>7} {whole:>6} {:>18} {:>18} {:>18}",
            graph.count(),
            per_start.report(whole),
            per_menu.report(whole),
            per_conversation.report(whole),
        );
        let _ = &symbols;
    }

    println!(
        "\nA per-start layout cannot be shared between the starts of one menu, and the \
         plugin asks about a whole menu in one call - so the per-menu column is what a \
         start-relative layout would really buy."
    );
    println!(
        "The per-conversation column is what could be had WITHOUT giving up the kept \
         manager: it does not move between the menus of one conversation, so a workspace \
         keyed on it survives them."
    );
    println!(
        "DETERMINISTIC: this reads the graph and the layout and never runs a search, so \
         two runs agree exactly."
    );
}

/// A stride that takes about `wanted` items out of `total`, and never zero.
fn step(total: usize, wanted: usize) -> usize {
    if total <= wanted { 1 } else { total / wanted }
}

/// How many variables a layout keeping only `reads` carries.
fn vars_reading(graph: &LookAheadGraph, reads: &HashSet<String>) -> u32 {
    DataLayout::for_graph(graph, COUNTER_CAP, None, false)
        .keeping_only_read(graph.symbols(), reads)
        .total_vars()
}

/// Every entry reachable from `starts` by following links, `starts` included.
///
/// STRUCTURAL ONLY - links followed, guards ignored - so it over-approximates what a real
/// search can walk and therefore cannot drop a slot a real path needs. That is the whole
/// safety argument for narrowing a layout this way, and it is why this must not learn to
/// read guards.
fn reachable_from(
    graph: &LookAheadGraph,
    starts: impl IntoIterator<Item = DialogueNodeId>,
) -> Vec<DialogueNodeId> {
    let mut seen = HashSet::new();
    let mut pending = VecDeque::new();
    for start in starts {
        if graph.contains(start) && seen.insert(start) {
            pending.push_back(start);
        }
    }

    while let Some(id) = pending.pop_front() {
        let Some(node) = graph.get(id) else { continue };
        for &next in &node.links {
            if graph.contains(next) && seen.insert(next) {
                pending.push_back(next);
            }
        }
    }

    seen.into_iter().collect()
}

/// Keeps the mean and the best case of a column, and prints them against the whole.
struct Sampler {
    total: u64,
    count: u64,
    best: u32,
}

impl Sampler {
    fn new() -> Self {
        Self { total: 0, count: 0, best: u32::MAX }
    }

    fn add(&mut self, vars: u32) {
        self.total += u64::from(vars);
        self.count += 1;
        self.best = self.best.min(vars);
    }

    /// "mean (saving%), best N" - or a dash where there was nothing to sample.
    fn report(&self, whole: u32) -> String {
        if self.count == 0 {
            return "-".to_string();
        }

        let mean = self.total as f64 / self.count as f64;
        let saved = if whole == 0 { 0.0 } else { 100.0 * (1.0 - mean / f64::from(whole)) };
        format!("{mean:.0} ({saved:.0}%), best {}", self.best)
    }
}
