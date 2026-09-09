// SPDX-License-Identifier: MIT
//! What each counter slot would cost under each of the three encodings.
//!
//! ## The question
//!
//! de-12wr.4 asks for the most compact counter encoding available, from three candidates:
//!
//! 1. ONE BIT PER WRITER - which increment sites have fired. n sites, n bits, and it does not
//!    care what the amounts are, so it is the compact one where they vary wildly.
//! 2. ONE BIT PER COMPARISON CLASS - the ranges the guards actually test against. THIS ONE IS
//!    ALREADY SHIPPED, in its saturating form: `DataLayout::narrow_to_thresholds` caps a slot
//!    at the largest constant compared against it, plus one. Saturating rather than classing
//!    is what makes its transitions deterministic without extra states.
//! 3. MAX POSSIBLE ADDITIONAL VALUE - every increment in the group fired and summed, divided
//!    by the greatest common divisor of the amounts, with the arriving value folded into the
//!    guard rather than carried in the slot.
//!
//! ALL THREE STAY CANDIDATES even where one of them wins nothing today. From the user,
//! 2026-09-09: this codebase may be ported to an entirely different dialogue set, and the
//! writer encoding is the one that does not care what the amounts ARE - so it is the one that
//! pays where they vary wildly, which is exactly the case this game does not contain. The
//! choice is made per slot from the numbers below, so a corpus that needs it gets it without
//! anybody revisiting the decision.
//!
//! ## What it reports, per counter slot
//!
//! The increment sites and their amounts, the GCD, the summed range, the width each encoding
//! would give, and the width the slot HAS today. A slot where the shipped width is already the
//! smallest of the four is a slot neither new encoding can improve.
//!
//! ## What it cannot answer, and says so rather than guessing
//!
//! WHETHER A SITE SITS INSIDE A CYCLE. Encoding 1's bound holds only if each site fires at
//! most once on a path, which the group's LINK STRUCTURE decides rather than its action list.
//! This reports the site count as an upper bound on what that encoding could buy and marks the
//! slot, rather than pretending the question is settled. de-3x76.2 rejected the site-count
//! bound partly for needing an SCC pass, and that objection stands until someone does one.
//!
//! ## How to run it
//!
//! ```text
//! DEGCT_RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo counter-widths -- \
//!   cargo run --release --example counter_widths
//! ```
//!
//! `DEGCT_CONVERSATION=631,368` picks the groups.

use std::collections::BTreeMap;

use lookahead_engine::core::action::DialogueActionKind;
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::world::world::ILookAheadWorld;

#[path = "../tests/common/mod.rs"]
mod common;

/// The six heaviest groups, which is where a wide slot would live if one does.
const GROUPS: [i32; 6] = [362, 368, 631, 14, 28, 1030];

/// The counter cap every layout in this repository is built with.
const COUNTER_CAP: i32 = 16;

/// How many bits hold `max`, which is `DataLayout`'s own rule.
fn bits_for(max: u32) -> u8 {
    if max == 0 { 1 } else { (u32::BITS - max.leading_zeros()) as u8 }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// What one slot's increments look like.
#[derive(Default)]
struct Counter {
    /// One entry per increment site, holding the amount it adds.
    amounts: Vec<i32>,
    /// Whether anything ASSIGNS this slot, which puts a floor under it no delta encoding can
    /// lower - `ActionImage::assign` writes the number directly.
    assigned: Vec<i32>,
}

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "per counter slot: the three encodings against the width the slot has today\n\
         \n\
         sites   how many actions increment it\n\
         amounts the distinct amounts they add\n\
         gcd     the greatest common divisor of those amounts\n\
         range   every increment fired and summed\n\
         writer  ceil(log2(sites + 1)) - one state per number of sites fired, the floor for\n\
         \x20       encoding 1; the true width is up to `sites` bits where the amounts differ\n\
         value   ceil(log2(range / gcd + 1)) - encoding 3\n\
         today   what DataLayout gives it now, which is encoding 2 in its saturating form\n"
    );

    let mut groups_with_counters = 0;
    let mut slots_seen = 0;
    let mut slots_a_new_encoding_could_narrow = 0;
    // THE WHOLE POINT OF TAKING A MINIMUM: bits today against bits under a rule that picks
    // the narrowest legal encoding per slot, INCLUDING the one that already ships. A rule
    // shaped that way cannot cost a slot anything, so the only question is what it buys.
    let mut bits_today = 0u32;
    let mut bits_best = 0u32;
    // Whether the writer encoding ever wins outright, which decides whether it is worth
    // being one of the candidates at all.
    let mut writer_wins = 0;

    for conversation in numbers("CONVERSATION", &GROUPS) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        // WHAT THE ENGINE ACTUALLY BUILDS, rather than a layout of this file's own - the drift
        // `DataLayout::for_group`'s doc warns about, which caught `layout_shape` once.
        let layout = DataLayout::for_group(&graph, &world, COUNTER_CAP);
        let symbols = graph.symbols();

        let mut counters: BTreeMap<usize, Counter> = BTreeMap::new();
        for node in graph.nodes() {
            for action in &node.actions {
                let slot = action.slot();
                if slot < 0 || slot as usize >= symbols.count() {
                    continue;
                }
                let slot = slot as usize;
                match action.kind() {
                    DialogueActionKind::Increment => {
                        counters.entry(slot).or_default().amounts.push(action.value());
                    }
                    DialogueActionKind::Assign => {
                        counters.entry(slot).or_default().assigned.push(action.value());
                    }
                    _ => {}
                }
            }
        }

        // A SLOT NOTHING INCREMENTS IS NOT A COUNTER. One that is only assigned is a different
        // question and none of the three encodings is about it.
        counters.retain(|_, counter| !counter.amounts.is_empty());
        if counters.is_empty() {
            println!("conversation {conversation}: no counter slot at all\n");
            continue;
        }
        groups_with_counters += 1;

        println!(
            "conversation {conversation}: {} counter slot(s) of {} slots\n\
             {:>34}  {:>5}  {:>18}  {:>5}  {:>7}  {:>6}  {:>5}  {:>5}  {:>4}  {}",
            counters.len(),
            symbols.count(),
            "slot",
            "sites",
            "amounts",
            "gcd",
            "range",
            "writer",
            "value",
            "today",
            "best",
            "win",
        );

        for (slot, counter) in &counters {
            slots_seen += 1;

            let mut distinct: Vec<i32> = counter.amounts.clone();
            distinct.sort_unstable();
            distinct.dedup();

            let unit = distinct
                .iter()
                .map(|amount| amount.unsigned_abs())
                .filter(|amount| *amount > 0)
                .reduce(gcd)
                .unwrap_or(1)
                .max(1);
            let range: u32 = counter.amounts.iter().map(|a| a.max(&0).unsigned_abs()).sum();

            let writer_bits = bits_for(counter.amounts.len() as u32);
            let value_bits = bits_for(range / unit);
            let today = layout.slot(*slot).map(|(_, width)| width).unwrap_or(0);

            // AN ASSIGN PUTS A FLOOR UNDER THE SLOT that no delta encoding can lower, so a
            // slot the group assigns is marked rather than counted as narrowable.
            let assigned = counter.assigned.iter().copied().max().unwrap_or(0);
            let best_new = writer_bits.min(value_bits);
            if best_new < today && assigned == 0 {
                slots_a_new_encoding_could_narrow += 1;
            }

            // THE MINIMUM INCLUDING TODAY'S, which is what "the most compact encoding
            // available" has to mean if it is never to cost a slot anything. A slot the
            // group ASSIGNS keeps what it has: an assign writes the number directly, so
            // neither delta encoding is legal there whatever its width would be.
            let best = if assigned > 0 { today } else { today.min(best_new) };
            bits_today += today as u32;
            bits_best += best as u32;
            if assigned == 0 && writer_bits < value_bits && writer_bits < today {
                writer_wins += 1;
            }

            // WHICH ENCODING THIS SLOT WOULD TAKE, named rather than left to a reader
            // comparing three columns. On a different dialogue set this is the column that
            // answers the question the other five only supply the evidence for.
            let win = if assigned > 0 {
                "ceiling (assigned)"
            } else if today <= writer_bits && today <= value_bits {
                "ceiling"
            } else if value_bits <= writer_bits {
                "value"
            } else {
                "writer"
            };

            let name = symbols.name_of(*slot).unwrap_or("?");
            let shown: Vec<String> = distinct.iter().take(4).map(|a| a.to_string()).collect();
            println!(
                "{:>34}  {:>5}  {:>18}  {:>5}  {:>7}  {:>6}  {:>5}  {:>5}  {:>4}  {}",
                &name[name.len().saturating_sub(34)..],
                counter.amounts.len(),
                shown.join(","),
                unit,
                range,
                writer_bits,
                value_bits,
                today,
                best,
                win,
            );
        }
        println!();
    }

    println!(
        "{groups_with_counters} group(s) have a counter at all; {slots_seen} counter slot(s) \
         in total,\nof which {slots_a_new_encoding_could_narrow} could be narrowed by one of \
         the two new encodings."
    );
    println!(
        "\nTAKING THE NARROWEST OF THE THREE, per slot, including the one that ships:\n\
         \x20 {bits_today} bits today -> {bits_best} bits, a saving of {} across every counter \
         in the groups measured. A rule shaped as a minimum cannot cost a slot \
         anything, so this is\n  the whole of what the standardisation is worth.",
        bits_today - bits_best,
    );
    money_report(&index, &world);

    println!(
        "\nTHE WRITER ENCODING WINS OUTRIGHT ON {writer_wins} SLOT(S) IN THIS DIALOGUE SET, and \
         is kept\n  as a candidate anyway. It is the encoding that does not care what the \
         amounts ARE, so\n  it is the one that pays where they vary wildly - an option costing \
         50 beside one costing\n  0.5 - and every amount in THIS game is an integer with a GCD \
         of 1, which is precisely\n  the case it cannot beat. A different dialogue set is the \
         reason it stays: the choice is\n  made per slot from the numbers above, so a corpus \
         that needs it gets it without anybody\n  revisiting this decision. See the `win` \
         column for which encoding each slot would take."
    );
}

/// The same three encodings applied to MONEY, which is where the wildly varying amounts are.
///
/// ## Why money and not a counter slot
///
/// From the user, 2026-09-09: the sneakers-and-speakers case - one option costing 50 and
/// another 0.5 - is MONEY, and money in this game is integer centimes. So it never was a
/// fractional counter; it is 5,000 centimes beside 50, and it lives in its own slot rather
/// than among the counters above.
///
/// THAT IS THE SLOT WHERE THE SCALING PAYS. A counter here spans one to five bits. Money is
/// sized by `DataLayout::money_ceiling` - the starting purse plus everything the group can
/// gain - as an ABSOLUTE centime value, so it is the widest thing in the layout by a long way,
/// and every amount in it is a multiple of something.
///
/// ## What the two columns mean
///
/// `today` is `bits_for(ceiling)`. `scaled` is what the same slot costs holding the DELTA from
/// the arriving purse, in units of the greatest common divisor of the amounts the group can
/// gain: `bits_for(gained / gcd)`. The delta is a property of the group's actions and the gcd
/// of its amounts, so neither depends on how much the player happens to be carrying - which is
/// what keeps the layout world-independent and the workspace's key intact.
fn money_report(index: &lookahead_engine::index::Index, world: &dyn ILookAheadWorld) {
    println!("\nMONEY, which is the slot the sneakers-and-speakers case actually lives in\n");
    println!(
        "{:>6}  {:>10}  {:>10}  {:>8}  {:>6}  {:>6}  {:>6}",
        "conv", "ceiling", "gained", "gcd", "today", "scaled", "saved",
    );

    let mut today_total = 0u32;
    let mut scaled_total = 0u32;
    for conversation in numbers("CONVERSATION", &GROUPS) {
        let Ok((graph, _)) = build_group_graph(index, conversation) else { continue };
        let Some(ceiling) = DataLayout::money_ceiling(&graph, world.money()) else {
            println!("{conversation:>6}  {:>10}", "not read");
            continue;
        };

        let amounts: Vec<u32> = graph
            .nodes()
            .flat_map(|node| &node.actions)
            .filter(|action| {
                matches!(
                    action.kind(),
                    DialogueActionKind::GainMoney | DialogueActionKind::LoseMoney
                )
            })
            .map(|action| action.value().unsigned_abs())
            .filter(|value| *value > 0)
            .collect();

        // THE COSTS COUNT TOO, not only the gains. A cost is what a guard compares against,
        // so a scaling that did not divide the costs would be dividing half the arithmetic.
        let costs: Vec<u32> = graph
            .nodes()
            .filter(|node| node.is_cost_option())
            .map(|node| node.cost.unsigned_abs())
            .filter(|value| *value > 0)
            .collect();

        let unit = amounts
            .iter()
            .chain(costs.iter())
            .copied()
            .reduce(gcd)
            .unwrap_or(1)
            .max(1);
        let gained: u32 = amounts.iter().sum();

        let today = bits_for(ceiling);
        let scaled = bits_for(gained / unit);
        today_total += today as u32;
        scaled_total += scaled as u32;

        println!(
            "{conversation:>6}  {ceiling:>10}  {gained:>10}  {unit:>8}  {today:>6}  \
             {scaled:>6}  {:>6}",
            today as i32 - scaled as i32,
        );
    }

    println!(
        "\n  {today_total} bits of money today -> {scaled_total} scaled, a saving of {} over \
         the groups measured.",
        today_total as i32 - scaled_total as i32,
    );
}

/// A comma-separated list from the environment, or the default written down here.
fn numbers(name: &str, fallback: &[i32]) -> Vec<i32> {
    match lookahead_engine::core::env::var(name) {
        Ok(named) => named
            .split(',')
            .map(str::trim)
            .filter(|piece| !piece.is_empty())
            .map(|piece| {
                piece.parse().unwrap_or_else(|_| panic!("{name}={piece:?} is not a number"))
            })
            .collect(),
        Err(_) => fallback.to_vec(),
    }
}
