// SPDX-License-Identifier: MIT
//! Every slot a group's layout carries, by name, with the variables it takes.
//!
//! ## What it answers
//!
//! "What is this search actually carrying?" - as a list rather than as a count.
//! `performance/layout_shape.rs` classifies the slots and totals each class, which is what
//! ranks a task; this prints them one per line, in variable order, so a particular name can be
//! found and its neighbours read off.
//!
//! The variable order is the layout's own, so the first line is the top of every diagram built
//! over it and the last is the bottom. Slots the layout dropped are listed separately at the
//! end, with why, since a name that is absent is otherwise indistinguishable from a name that
//! was never there.
//!
//! ## How to run it
//!
//! ```text
//! tools/run-logged.sh --kind analysis cargo layout-slots -- \
//!   cargo run --release --example layout_slots
//! ```
//!
//! The shipped variable order applies, so this also shows what an ordering did.

use std::collections::BTreeMap;

use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::{DataLayout, counters_from_onces};

use gct_measure::common;

use gct_measure::options;

/// What kind of state a slot holds, read off the prefix its name was interned with.
///
/// The prefixes are `core::state`'s and are how a slot's class is known at all - a slot is an
/// index into one table whatever it stands for.
fn class_of(name: &str) -> &'static str {
    for (prefix, class) in [
        ("once:", "once"),
        ("seen:", "seen"),
        ("item:", "item"),
        ("thought:", "thought"),
        ("damage:", "damage"),
        ("unequipped:", "unequipped"),
    ] {
        if name.starts_with(prefix) {
            return class;
        }
    }
    if name.starts_with("TASK.") {
        return "task";
    }
    "variable"
}

/// What this driver takes. A group is required: it lists one group's slots, so there is nothing
/// to fall back to.
#[derive(clap::Parser)]
#[command(about = "What one group's layout holds, slot by slot.")]
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
    let Some(conversation) = asked.groups.conversations.first().copied() else {
        eprintln!("name a group with --conversation.");
        return;
    };

    let Ok((graph, _)) = build_group_graph(&index, conversation) else {
        eprintln!("conversation {conversation}: no group builds from it");
        return;
    };
    let layout = DataLayout::for_group(&graph, &world, 16);
    let symbols = graph.symbols();
    let dropped = counters_from_onces(&graph);

    println!(
        "conversation {conversation}: {} variables over {} slot(s) the layout carries, \
              from a symbol table of {}",
        layout.total_vars(),
        (0..symbols.count())
            .filter(|slot| layout.slot(*slot).is_some())
            .count(),
        symbols.count(),
    );
    println!();
    println!(
        "{:>6}  {:>5}  {:>4}  {:>11}  {}",
        "vars", "slot", "bits", "class", "name"
    );

    // In VARIABLE order rather than slot order, which is the order the diagram branches in.
    let mut carried: Vec<(u32, usize, u8)> = (0..symbols.count())
        .filter_map(|slot| layout.slot(slot).map(|(at, bits)| (at, slot, bits)))
        .collect();
    carried.sort_unstable();

    let mut per_class: BTreeMap<&'static str, (usize, u32)> = BTreeMap::new();
    for (at, slot, bits) in &carried {
        let name = symbols.name_of(*slot).unwrap_or("?");
        let class = class_of(name);
        let counted = per_class.entry(class).or_insert((0, 0));
        counted.0 += 1;
        counted.1 += *bits as u32;

        let range = if *bits == 1 {
            format!("{at}")
        } else {
            format!("{at}..{}", at + *bits as u32 - 1)
        };
        println!("{range:>6}  {slot:>5}  {bits:>4}  {class:>11}  {name}");
    }

    println!();
    println!("{:>11}  {:>6}  {:>6}", "class", "slots", "vars");
    for (class, (slots, vars)) in &per_class {
        println!("{class:>11}  {slots:>6}  {vars:>6}");
    }

    // WHAT IS NOT THERE AND WHY, because a missing name otherwise reads as one the group never
    // mentions. Three things take a slot out: nothing reads it, nothing can have written it by
    // the time anything reads it, and its value being recoverable from the `once` slots that
    // determine it.
    println!();
    let reads = DataLayout::read_by(&graph);
    let mut gone: Vec<(usize, &str, &'static str)> = Vec::new();
    for slot in 0..symbols.count() {
        if layout.slot(slot).is_some() {
            continue;
        }
        let name = symbols.name_of(slot).unwrap_or("?");
        let why = match (dropped.contains_key(&slot), reads.contains(name)) {
            (true, _) => "counts once slots",
            (false, false) => "nothing reads it",
            // READ, AND STILL NOT CARRIED, which leaves one rule that can have taken it - see
            // `DataLayout::dropping_slots_written_too_late`.
            (false, true) => "no write reaches a read of it",
        };
        gone.push((slot, name, why));
    }
    println!("{} slot(s) the layout does not carry:", gone.len());
    for (slot, name, why) in gone
        .iter()
        .filter(|(_, _, why)| *why == "counts once slots")
    {
        let onces = dropped.get(slot).map(Vec::len).unwrap_or(0);
        println!("  slot {slot:>5}  {why} ({onces} of them)  {name}");
    }
    for (slot, name, why) in gone
        .iter()
        .filter(|(_, _, why)| *why == "no write reaches a read of it")
    {
        println!("  slot {slot:>5}  {why}  {name}");
    }
    println!(
        "  and {} more that nothing in the group reads",
        gone.iter()
            .filter(|(_, _, why)| *why == "nothing reads it")
            .count(),
    );
}
