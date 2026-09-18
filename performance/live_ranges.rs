// SPDX-License-Identifier: MIT
//! How much of the state is DEAD at the average entry?
//!
//! ## Why this is the question worth asking now
//!
//! Every measurement in de-3x76 has said the same thing from a different angle: conversation
//! 14's problem is the SIZE of its reachable sets, and nothing about the encoding makes them
//! smaller. Variable count does not separate it from the groups that finish (de-3x76.1),
//! step count does not (de-3x76.3), the variable order buys a factor of two and the order in
//! use is already the best of five (de-3x76.10), and counting the states themselves showed
//! 14 holding under four thousand of them in 3,640 nodes - about one node per state, a
//! diagram compressing nothing.
//!
//! LIVE RANGES ATTACK THE SET RATHER THAN THE ENCODING. A variable that no path onward from
//! an entry will read before overwriting it cannot affect any answer from that entry on. The
//! set held there does not need to distinguish its values, so it can be quantified away -
//! and every pair of states differing only in dead variables collapses into one.
//!
//! That is the standard move in symbolic model checking and it is the only idea on the list
//! that makes the SET smaller rather than holding the same set more tightly.
//!
//! ## What this measures, and what it does not
//!
//! It measures the OPPORTUNITY: what fraction of the layout is dead at each entry. It does
//! not quantify anything. If most variables are live nearly everywhere there is no win here
//! and the idea should be dropped cheaply; if the average entry has most of its state dead,
//! it is worth building.
//!
//! ## The analysis
//!
//! Ordinary backward dataflow to a fixed point:
//!
//! ```text
//!   live_out(n) = union of live_in(s) over successors s
//!   live_in(n)  = reads(n) union (live_out(n) minus kills(n))
//! ```
//!
//! An `Assign` KILLS a variable - it overwrites it with a constant, so whatever it held
//! before cannot matter. An `Increment` does NOT: it reads the old value to produce the new
//! one, so it keeps the variable live. That asymmetry is the whole reason a counter behaves
//! differently from a flag here.
//!
//! Reads come from `DataLayout::read_by_nodes` over one node, which is the same routine the
//! layout uses, so this cannot drift from what the engine actually consults - and it already
//! includes a rolled check's own pass and fail flags.
//!
//! ## What it said, 2026-09-08: the opportunity is large, and smallest where it is needed
//!
//! ```text
//!   conv  entries   vars   live median   live mean   dead mean
//!    362     1860    109            51        47.9   61.1 (56%)
//!     28     2186    138            52        41.2   96.8 (70%)
//!    368     4724    225            44        51.8  173.2 (77%)
//!     14     3594    233           135       114.9  118.1 (51%)
//!    631     4514    249           127       117.5  131.5 (53%)
//!   1030     1476     73            49        48.1   24.9 (34%)
//! ```
//!
//! HALF TO THREE QUARTERS OF THE LAYOUT IS DEAD at the average entry, so the idea is not
//! refuted the cheap way - there really is state no path onward will read.
//!
//! BUT IT IS INVERSELY ORDERED AGAINST THE PROBLEM. Conversation 368, which settles, has
//! the most to gain at 77 per cent dead; conversation 14, the group that never settles,
//! has the least of the heavy groups at 51 per cent, and 631, the other one that does not
//! settle, is next at 53. The two groups this was written to rescue are the two with the
//! least dead state to quantify away.
//!
//! WHY THAT IS ENOUGH TO CLOSE IT FOR 14 rather than merely discouraging: collapsing
//! states that differ only in dead variables can only help where there ARE states to
//! collapse, and 14 holds 3,927 of them in 3,640 nodes - one node per state, a diagram
//! compressing nothing (de-3x76.11). Halving the variables that vary cannot compress a set
//! that small and that structureless. 368 has fifteen times the states and already
//! finishes.
//!
//! So this measures a real opportunity for a DIFFERENT question - what an average menu
//! costs, rather than what makes 14 fail - and de-3x76 was the wrong epic for it.
//!
//! Run it with `cargo run --release --example live_ranges`.

use std::collections::{HashMap, HashSet};

use lookahead_engine::core::action::DialogueActionKind;
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;

#[path = "../tests/common/mod.rs"]
mod common;

const COUNTER_CAP: i32 = 16;

/// The group that fails, and the ones that finish, to read it against.
const GROUPS: [i32; 6] = [362, 28, 368, 14, 631, 1030];

fn main() {
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the index reads");

    println!(
        "{:>5}  {:>7}  {:>5}  {:>11}  {:>11}  {:>11}",
        "conv", "entries", "vars", "live median", "live mean", "dead mean"
    );

    for conversation in GROUPS {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            println!("{conversation:>5}  does not build");
            continue;
        };

        let symbols = graph.symbols().clone();
        let reads_names = DataLayout::read_by(&graph);
        let passes_time = DataLayout::group_passes_time(&graph);
        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, passes_time)
            .keeping_only_read(&symbols, &reads_names);

        // Only slots the layout actually carries; a width of zero is one already trimmed.
        let carried: Vec<usize> = (0..symbols.count())
            .filter(|slot| {
                layout
                    .slot(*slot)
                    .map(|(_, bits)| bits > 0)
                    .unwrap_or(false)
            })
            .collect();
        let width: HashMap<usize, usize> = carried
            .iter()
            .map(|slot| {
                (
                    *slot,
                    layout
                        .slot(*slot)
                        .map(|(_, bits)| bits as usize)
                        .unwrap_or(0),
                )
            })
            .collect();
        let total_vars: usize = width.values().sum();

        // Per entry: what it reads, and what it kills.
        let mut reads: HashMap<DialogueNodeId, HashSet<usize>> = HashMap::new();
        let mut kills: HashMap<DialogueNodeId, HashSet<usize>> = HashMap::new();
        for node in graph.nodes() {
            let names = DataLayout::read_by_nodes(std::iter::once(node), &symbols);
            let mut read_here = HashSet::new();
            for name in &names {
                if let Some(slot) = symbols.find(name)
                    && width.contains_key(&slot)
                {
                    read_here.insert(slot);
                }
            }

            let mut killed = HashSet::new();
            for action in &node.actions {
                let Ok(slot) = usize::try_from(action.slot()) else {
                    continue;
                };
                if !width.contains_key(&slot) {
                    continue;
                }
                match action.kind() {
                    // Overwrites with a constant, so what it held cannot matter.
                    DialogueActionKind::Assign => {
                        killed.insert(slot);
                    }
                    // READS ITS OWN VALUE to produce the new one, so it keeps the variable
                    // live rather than killing it.
                    DialogueActionKind::Increment => {
                        read_here.insert(slot);
                    }
                    _ => {}
                }
            }

            // The engine's own writes are assignments of 1.
            for slot in [
                node.flag_slot,
                node.failed_flag_slot,
                node.seen_slot,
                node.once_slot,
            ] {
                if let Ok(slot) = usize::try_from(slot)
                    && width.contains_key(&slot)
                {
                    killed.insert(slot);
                }
            }

            reads.insert(node.id, read_here);
            kills.insert(node.id, killed);
        }

        // Backward to a fixed point. The graph is cyclic, so this iterates rather than
        // walking a topological order; sets only grow, so it terminates.
        let mut live: HashMap<DialogueNodeId, HashSet<usize>> = graph
            .nodes()
            .map(|node| (node.id, HashSet::new()))
            .collect();
        let mut changed = true;
        let mut rounds = 0usize;
        while changed {
            changed = false;
            rounds += 1;
            for node in graph.nodes() {
                let mut out: HashSet<usize> = HashSet::new();
                for to in &node.links {
                    if let Some(theirs) = live.get(to) {
                        out.extend(theirs.iter().copied());
                    }
                }
                let killed = &kills[&node.id];
                let mut mine = reads[&node.id].clone();
                mine.extend(out.iter().copied().filter(|slot| !killed.contains(slot)));

                if mine.len() != live[&node.id].len() {
                    live.insert(node.id, mine);
                    changed = true;
                }
            }
        }

        // In VARIABLES, not slots, because that is what a diagram pays for.
        let mut live_vars: Vec<usize> = live
            .values()
            .map(|slots| slots.iter().map(|slot| width[slot]).sum())
            .collect();
        live_vars.sort_unstable();

        let median = live_vars.get(live_vars.len() / 2).copied().unwrap_or(0);
        let mean: f64 = live_vars.iter().sum::<usize>() as f64 / live_vars.len().max(1) as f64;

        println!(
            "{conversation:>5}  {:>7}  {total_vars:>5}  {median:>11}  {mean:>11.1}  \
             {:>10.1} ({:.0}%)   [{rounds} rounds]",
            graph.count(),
            total_vars as f64 - mean,
            (total_vars as f64 - mean) / total_vars.max(1) as f64 * 100.0,
        );
    }

    println!(
        "\nA LARGE DEAD FRACTION means quantifying dead variables out of each entry's set \
         would collapse states that differ only in them, and is worth building.\nA small one \
         means the state really is live everywhere and this idea is not the answer."
    );
}
