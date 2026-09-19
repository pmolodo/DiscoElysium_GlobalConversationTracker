// SPDX-License-Identifier: MIT
//! Which counter slots are a second copy of state the search already carries.
//!
//! ## The question
//!
//! A counter incremented only by `once` actions holds exactly how many of those actions have
//! fired, and the search carries a slot for each of them already. The counter is then redundant
//! state - see [`lookahead_engine::symbolic::data_layout::counters_from_onces`], which has the
//! conditions and why there is no dominance question to ask.
//!
//! WHAT IT WOULD BUY IS NOT THE BITS. A three-bit counter costs three decision-diagram
//! variables, which is small. What costs is that the diagram carries the RELATION between them
//! and the once slots, and no variable order makes that cheap: the counter's bits must interleave
//! with the once bits and any order is wrong for some. Dropping it deletes a constraint over six
//! variables rather than narrowing the state by three bits.
//!
//! ## What this reports
//!
//! Per group: every redundant counter, how many `once` slots determine it, and how wide the
//! layout carries it today. The width is what would come out of the state; the count is how many
//! variables the deleted constraint spanned.
//!
//! The summary also counts the ones this save holds ABOVE the sites it records as shown, which
//! is a counter some other conversation raises as well. Those are not lost - a guard rebases its
//! threshold by the difference - but they are the ones a group-local reading would have to see.
//!
//! ## How to run it
//!
//! ```text
//! tools/run-logged.sh --kind analysis cargo redundant -- \
//!   cargo run --release --example redundant_counters
//! ```
//!
//! With no conversation named it sweeps every group the index holds, which is how to learn
//! whether this shape is one conversation's quirk or a pattern worth building for.

use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::{DataLayout, counters_from_onces};

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "options.rs"]
mod options;

/// The layout `DataLayout::for_group` builds, minus the step that drops these counters.
///
/// THE ONLY WAY TO PRICE WHAT ONE COSTS, since the shipped layout drops them as it builds and
/// so cannot be asked what carrying one would take. These are the same three steps `for_group`
/// takes - every slot laid out, narrowed to what the group's guards read - stopping short of
/// the fourth.
fn carrying_counters(graph: &LookAheadGraph, cap: i32) -> DataLayout {
    DataLayout::for_graph(graph, cap, None, false)
        .keeping_only_read(graph.symbols(), &DataLayout::read_by(graph))
}

/// How many variables the relation between a counter and its `once` slots reaches across, and
/// how many the layout has in all.
///
/// END TO END RATHER THAN COUNTED. The diagram carries "this counter is how many of those slots
/// are set", and what that costs is governed by the distance between the first and last variable
/// it ties together, not by how many of them there are: a relation over five slots that sit
/// beside each other is local, and one over five spread across the layout is not.
fn relation_reach(
    layout: &DataLayout,
    slot: usize,
    onces: &[(lookahead_engine::core::types::DialogueNodeId, usize)],
) -> (u32, u32) {
    let Some((at, width)) = layout.slot(slot) else {
        return (0, layout.total_vars());
    };
    let positions: Vec<u32> = onces
        .iter()
        .filter_map(|(_, once)| layout.slot(*once).map(|(at, _)| at))
        .collect();
    let first = positions.iter().copied().min().unwrap_or(at).min(at);
    let last = positions
        .iter()
        .copied()
        .max()
        .unwrap_or(at)
        .max(at + width as u32 - 1);
    (last - first + 1, layout.total_vars())
}

/// What this driver takes. With no group named it sweeps every one in the game.
#[derive(clap::Parser)]
#[command(about = "Which counters a group carries that nothing reads back.")]
struct Options {
    #[command(flatten)]
    groups: options::Groups,
}

fn main() {
    let asked = <Options as clap::Parser>::parse();
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();
    let named = asked.groups.conversations.first().copied();

    let starts: Vec<i32> = match named {
        Some(one) => vec![one],
        None => {
            let mut all: Vec<i32> = index.keys().copied().collect();
            all.sort_unstable();
            all
        }
    };

    // `reach` IS THE COLUMN TO READ AGAINST WHAT A GROUP GAINED, and `spans` is not: `spans` is
    // `onces + bits` by construction and says nothing the two beside it do not. What a diagram
    // pays for is how far apart the ORDER puts the variables the relation ties together, since
    // the counter's bits must interleave with the once bits and no order is right for both. So
    // `reach` is that distance from end to end, and `of` is how many variables the group has -
    // a relation covering most of the layout is a different thing from one covering a tenth of
    // it. See de-0q6b, which asks where dropping stops paying.
    println!(
        "{:>7}  {:>38}  {:>5}  {:>5}  {:>6}  {:>6}  {:>6}",
        "conv", "slot", "onces", "bits", "spans", "reach", "of"
    );

    let mut groups = 0usize;
    let mut slots = 0usize;
    let mut bits = 0usize;
    let mut outside = 0usize;
    for conversation in starts {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let found = counters_from_onces(&graph);
        if found.is_empty() {
            continue;
        }
        let layout = carrying_counters(&graph, 16);
        // WHERE THE SAVE HOLDS MORE THAN THIS GROUP'S OWN SHOWN SITES, which is a counter some
        // other conversation also raises. The substitution still stands there - the guards
        // rebase by the difference - so this is a count of what a group-local reading would
        // have had to give up, not of anything wrong.
        let rebased = DataLayout::for_group(&graph, &world, 16);
        for slot in found.keys() {
            if let Some((_, offset)) = rebased.counter_onces(*slot)
                && offset != 0
            {
                outside += 1;
            }
        }
        groups += 1;
        let symbols = graph.symbols();
        for (slot, onces) in found {
            let width = layout.slot(slot).map(|(_, width)| width).unwrap_or(0);
            slots += 1;
            bits += width as usize;
            let name = symbols.name_of(slot).unwrap_or("?");
            let (reach, of) = relation_reach(&layout, slot, &onces);
            println!(
                "{:>7}  {:>38}  {:>5}  {:>5}  {:>6}  {:>6}  {:>6}",
                conversation,
                &name[name.len().saturating_sub(38)..],
                onces.len(),
                width,
                onces.len() + width as usize,
                reach,
                of,
            );
        }
    }

    println!();
    println!(
        "{slots} redundant counter(s) over {groups} group(s), {bits} bits of layout, \
         spanning constraints of up to that many variables each. {outside} of them hold more \
         than this save's shown sites account for, and rebase their guards by the difference."
    );

    // AND THAT THE BITS ACTUALLY COME OUT, which the table above only says they could. A slot
    // the layout still carries after being dropped is a bug, and so is a variable count that
    // does not fall by exactly the width of what was dropped.
    if let Some(one) = named {
        let Ok((graph, _)) = build_group_graph(&index, one) else {
            return;
        };
        let kept = carrying_counters(&graph, 16);
        let dropped = DataLayout::for_group(&graph, &world, 16);
        let went: u32 = counters_from_onces(&graph)
            .keys()
            .filter_map(|slot| kept.slot(*slot).map(|(_, width)| width as u32))
            .sum();
        println!();
        println!(
            "variables: {} carried, {} after dropping, {} fewer - and {went} were the dropped \
             slots' own width",
            kept.total_vars(),
            dropped.total_vars(),
            kept.total_vars() - dropped.total_vars(),
        );

        // HOW FAR APART THE ORDER PUTS THEM, which is the reason to expect more than the bits.
        // Variables are laid out end to end in SLOT order and slot order is interning order:
        // a counter's name is interned while its script is parsed, and the `once:` slots are
        // interned later, in `LookAheadGraph::new`. So the relation the diagram has to carry -
        // this counter is how many of those slots are set - spans whatever lies between them.
        for (slot, onces) in counters_from_onces(&graph) {
            let Some((at, width)) = kept.slot(slot) else {
                continue;
            };
            let positions: Vec<u32> = onces
                .iter()
                .filter_map(|(_, once)| kept.slot(*once).map(|(at, _)| at))
                .collect();
            let (first, last) = (
                positions.iter().copied().min().unwrap_or(at),
                positions.iter().copied().max().unwrap_or(at),
            );
            let span = first.min(at)..=last.max(at + width as u32 - 1);
            println!(
                "  slot {slot}: counter at variables {at}..{}, its onces at {first}..{last} \
                 - the relation spans {} of {} variables",
                at + width as u32 - 1,
                span.end() - span.start() + 1,
                kept.total_vars(),
            );
            println!(
                "    after dropping: {:?}, rebasing guards by {:?}",
                dropped.slot(slot),
                dropped.counter_onces(slot).map(|(_, offset)| offset),
            );
        }
    }
}
