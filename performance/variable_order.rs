// SPDX-License-Identifier: MIT
//! How far a group's constraints reach under each variable ordering.
//!
//! ## What it measures
//!
//! SPAN, summed over the group's guards: for each guard that names more than one slot, the
//! distance from the first of its slots to the last in the order the layout lays them out in.
//! A guard whose slots sit next to each other spans two; one whose ends are at opposite ends
//! of the layout spans the whole layout.
//!
//! Span bounds what the diagram has to carry - Meijer and van de Pol (arXiv:1511.08678) show
//! it is within twice the bandwidth of the dependency matrix - which is why bandwidth
//! reduction is the standard answer to variable ordering.
//!
//! ON THIS DIALOGUE SET IT MISLEADS, and the whole reason to keep this tool is to say so.
//! FORCE cuts 761's span by 92 per cent and costs 59 per cent more time and 42 per cent more
//! nodes. Interning order already groups slots by where the dialogue decides them, and that
//! locality is worth more than short guards. Read a span as a reason to measure a candidate
//! ordering, never as a verdict on one.
//!
//! ## How to run it
//!
//! ```text
//! DEGCT_CONVERSATION=761 tools/run-logged.sh --kind analysis cargo var-order -- \
//!   cargo run --release --example variable_order
//! ```
//!
//! With no conversation named it sweeps every group, which is how to tell whether an ordering
//! helps generally or only where somebody went looking.

use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::var_order::{Ordering, span};

#[path = "../tests/common/mod.rs"]
mod common;

/// The orderings to report, in the column order they are printed in.
const ORDERINGS: [(&str, Ordering); 4] = [
    ("slot", Ordering::Slot),
    ("force", Ordering::Force),
    ("force-entries", Ordering::ForceEntries),
    ("dialogue", Ordering::Dialogue),
];

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();
    let named = lookahead_engine::core::env::var("CONVERSATION")
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok());

    let starts: Vec<i32> = match named {
        Some(one) => vec![one],
        None => {
            let mut all: Vec<i32> = index.keys().copied().collect();
            all.sort_unstable();
            all
        }
    };

    println!(
        "{:>7}  {:>6}  {:>10}  {:>10}  {:>13}  {:>10}  {:>8}",
        "conv", "vars", "slot", "force", "force-entries", "dialogue", "change"
    );

    let mut total_slot = 0usize;
    let mut total_force = 0usize;
    let mut total_entries = 0usize;
    let mut total_dialogue = 0usize;
    let mut better = 0usize;
    let mut worse = 0usize;
    for conversation in starts {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        // THE LAYOUT THAT SHIPS, so the slot count and what is in it are the ones a search
        // would carry rather than a count of every name the group mentions.
        let layout = DataLayout::for_group(&graph, &world, 16);
        let slots = graph.symbols().count();
        if layout.total_vars() == 0 {
            continue;
        }

        let spans: Vec<usize> = ORDERINGS
            .iter()
            .map(|(_, ordering)| span(&graph, slots, &ordering.of(&graph, slots)))
            .collect();
        let (slot, force, entries, dialogue) = (spans[0], spans[1], spans[2], spans[3]);
        if slot == 0 {
            continue;
        }
        total_slot += slot;
        total_force += force;
        total_entries += entries;
        total_dialogue += dialogue;
        match force.cmp(&slot) {
            std::cmp::Ordering::Less => better += 1,
            std::cmp::Ordering::Greater => worse += 1,
            std::cmp::Ordering::Equal => {}
        }

        println!(
            "{:>7}  {:>6}  {:>10}  {:>10}  {:>13}  {:>10}  {:>7.1}%",
            conversation,
            layout.total_vars(),
            slot,
            force,
            entries,
            dialogue,
            (force as f64 / slot as f64 - 1.0) * 100.0,
        );
    }

    println!();
    let against = |total: usize| (total as f64 / total_slot.max(1) as f64 - 1.0) * 100.0;
    println!(
        "total span: slot {total_slot}, force {total_force} ({:+.1}%), \
         force-entries {total_entries} ({:+.1}%), dialogue {total_dialogue} ({:+.1}%) - \
         force is shorter on {better} group(s) and longer on {worse}",
        against(total_force),
        against(total_entries),
        against(total_dialogue),
    );
    println!();
    println!(
        "SPAN IS A PROXY AND ON 761 IT MISLED: force cut the span 92 per cent and measured \
         15,313 ms against slot order's 9,735, on 9,154,253 nodes against 6,444,119. Read a \
         span as a reason to measure an ordering, never as a verdict on one."
    );
}
