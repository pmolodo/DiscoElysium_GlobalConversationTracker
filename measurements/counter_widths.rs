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
//! ## The cycle test, which decides whether either delta encoding is legal
//!
//! BOTH new encodings bound the slot by what a path can ADD, and both assume each site fires
//! at most once. A site inside a dialogue loop breaks that: encoding 3 undercounts the sum,
//! and encoding 1 records a SET of writers, so a site firing twice decodes to one helping of
//! its amount rather than two. A slot with a repeatable site therefore keeps the shipped
//! ceiling whatever the delta arithmetic says.
//!
//! A site is repeatable when its entry sits on a cycle AND the action is not marked `once` -
//! a `once` action fires at most once by construction, loop or no loop. `on_a_cycle` answers
//! the first half with Tarjan; `DialogueAction::once` answers the second.
//!
//! A DECREMENT disqualifies a slot for the same kind of reason: a distance goes negative and
//! the slot is unsigned. `reputation.kim` is the only one in this dialogue set.
//!
//! ## What this reports once the rule ships
//!
//! `DataLayout::narrow_to_deltas` applies the rule, and the `today` column reads the layout
//! the engine actually builds. So the saving printed at the end is what is still LEFT on the
//! table, and a run where that is zero is a run where the layout takes everything the three
//! encodings offer.
//!
//! ## How to run it
//!
//! ```text
//! DEGCT_RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo counter-widths -- \
//!   cargo run --release --example counter_widths
//! ```
//!
//! `DEGCT_CONVERSATION=631,368` picks the groups.

use std::collections::{BTreeMap, HashMap, HashSet};

use lookahead_engine::core::action::DialogueActionKind;
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::graph::LookAheadGraph;
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
    if max == 0 {
        1
    } else {
        (u32::BITS - max.leading_zeros()) as u8
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Every entry that sits on a cycle, by strongly connected component.
///
/// ## Why this decides whether the delta encoding is legal at all
///
/// The delta bound is "every increment site fired once, summed". That holds only if each site
/// fires at most once on a path - and a site inside a loop can fire again and again, so the
/// delta is bounded by the counter cap rather than by the sum. Saturating at the sum would
/// then be UNSOUND: a guard whose rebased threshold sits above it folds to "always false"
/// when it should sometimes be true.
///
/// A node is on a cycle if its strongly connected component holds more than one entry, or if
/// it links to itself. de-3x76.2 rejected the site-count bound partly for needing this pass;
/// the pass is thirty lines and the group graph is a few thousand entries, so what it needed
/// was doing rather than avoiding.
fn on_a_cycle(graph: &LookAheadGraph) -> HashSet<DialogueNodeId> {
    // ITERATIVE TARJAN, because the recursive one is a stack overflow waiting on conversation
    // 631's 4,514 entries - the same fault de-fpax spent a week on in the diagram code.
    let mut index_of: HashMap<DialogueNodeId, u32> = HashMap::new();
    let mut low: HashMap<DialogueNodeId, u32> = HashMap::new();
    let mut on_stack: HashSet<DialogueNodeId> = HashSet::new();
    let mut stack: Vec<DialogueNodeId> = Vec::new();
    let mut next_index = 0u32;
    let mut cyclic: HashSet<DialogueNodeId> = HashSet::new();

    // (node, how many of its links have been walked)
    let mut work: Vec<(DialogueNodeId, usize)> = Vec::new();

    for node in graph.nodes() {
        if index_of.contains_key(&node.id) {
            continue;
        }
        work.push((node.id, 0));

        while let Some((id, child)) = work.pop() {
            if child == 0 {
                index_of.insert(id, next_index);
                low.insert(id, next_index);
                next_index += 1;
                stack.push(id);
                on_stack.insert(id);
            }

            let links: &[DialogueNodeId] = graph
                .get(id)
                .map(|node| node.links.as_slice())
                .unwrap_or(&[]);

            // A SELF LINK IS A CYCLE OF ONE, and Tarjan puts it in a component by itself, so
            // it has to be caught here rather than by the size test below.
            if child == 0 && links.contains(&id) {
                cyclic.insert(id);
            }

            let mut descended = false;
            for (at, &next) in links.iter().enumerate().skip(child) {
                if graph.get(next).is_none() {
                    continue;
                }
                if !index_of.contains_key(&next) {
                    work.push((id, at + 1));
                    work.push((next, 0));
                    descended = true;
                    break;
                }
                if on_stack.contains(&next) {
                    let seen = index_of[&next];
                    let mine = low[&id];
                    low.insert(id, mine.min(seen));
                }
            }
            if descended {
                continue;
            }

            // EVERY LINK WALKED, so this entry's component is decided. Fold its low link into
            // its parent's before the parent is looked at again.
            if low[&id] == index_of[&id] {
                let mut component = Vec::new();
                while let Some(member) = stack.pop() {
                    on_stack.remove(&member);
                    component.push(member);
                    if member == id {
                        break;
                    }
                }
                if component.len() > 1 {
                    cyclic.extend(component);
                }
            }
            if let Some(&(parent, _)) = work.last() {
                let mine = low[&id];
                let theirs = low[&parent];
                low.insert(parent, theirs.min(mine));
            }
        }
    }

    cyclic
}

/// What one slot's increments look like.
#[derive(Default)]
struct Counter {
    /// One entry per increment site, holding the amount it adds.
    amounts: Vec<i32>,
    /// Whether anything ASSIGNS this slot, which puts a floor under it no delta encoding can
    /// lower - `ActionImage::assign` writes the number directly.
    assigned: Vec<i32>,
    /// Whether any increment site can fire more than once: on a cycle, and not `once`.
    looped: bool,
    /// Whether anything DECREMENTS the slot, which no delta encoding can express.
    ///
    /// A distance from the arriving value goes negative as soon as the group can subtract,
    /// and the slot is an unsigned run of bits. It is also order-dependent in a way a sum is
    /// not: the absolute encoding clamps at zero after EACH step, so `+1 -2 +1` from a
    /// starting value of zero ends at one, and no summed distance says so.
    ///
    /// This is what `reputation.kim` is, and it is the whole of what the delta encoding
    /// leaves on the table in this dialogue set - four slots and seven bits.
    signed: bool,
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
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        // WHAT THE ENGINE ACTUALLY BUILDS, rather than a layout of this file's own - the drift
        // `DataLayout::for_group`'s doc warns about, which caught `layout_shape` once.
        let layout = DataLayout::for_group(&graph, &world, COUNTER_CAP);
        let symbols = graph.symbols();
        let cyclic = on_a_cycle(&graph);

        let mut counters: BTreeMap<usize, Counter> = BTreeMap::new();
        for node in graph.nodes() {
            // A SITE THAT CAN FIRE TWICE, which is what disqualifies a slot from either delta
            // encoding. `once` survives a loop; anything else on one does not.
            let repeatable = cyclic.contains(&node.id);
            for action in &node.actions {
                let slot = action.slot();
                if slot < 0 || slot as usize >= symbols.count() {
                    continue;
                }
                let slot = slot as usize;
                match action.kind() {
                    DialogueActionKind::Increment => {
                        let counter = counters.entry(slot).or_default();
                        counter.amounts.push(action.value());
                        counter.looped |= repeatable && !action.once();
                        counter.signed |= action.value() < 0;
                    }
                    DialogueActionKind::Assign => {
                        counters
                            .entry(slot)
                            .or_default()
                            .assigned
                            .push(action.value());
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
            let range: u32 = counter
                .amounts
                .iter()
                .map(|a| a.max(&0).unsigned_abs())
                .sum();

            let writer_bits = bits_for(counter.amounts.len() as u32);
            let value_bits = bits_for(range / unit);
            let today = layout.slot(*slot).map(|(_, width)| width).unwrap_or(0);

            // AN ASSIGN PUTS A FLOOR UNDER THE SLOT that no delta encoding can lower, so a
            // slot the group assigns is marked rather than counted as narrowable. A
            // repeatable site does the same for the reason the cycle test exists.
            let assigned = counter.assigned.iter().copied().max().unwrap_or(0);
            let delta_legal = assigned == 0 && !counter.looped && !counter.signed;
            let best_new = writer_bits.min(value_bits);
            if best_new < today && delta_legal {
                slots_a_new_encoding_could_narrow += 1;
            }

            // THE MINIMUM INCLUDING TODAY'S, which is what "the most compact encoding
            // available" has to mean if it is never to cost a slot anything. A slot the
            // group ASSIGNS keeps what it has: an assign writes the number directly, so
            // neither delta encoding is legal there whatever its width would be.
            let best = if delta_legal {
                today.min(best_new)
            } else {
                today
            };
            bits_today += today as u32;
            bits_best += best as u32;
            if delta_legal && writer_bits < value_bits && writer_bits < today {
                writer_wins += 1;
            }

            // WHICH ENCODING THIS SLOT WOULD TAKE, named rather than left to a reader
            // comparing three columns. On a different dialogue set this is the column that
            // answers the question the other five only supply the evidence for.
            let win = if assigned > 0 {
                "ceiling (assigned)"
            } else if counter.looped {
                "ceiling (looped)"
            } else if counter.signed {
                "ceiling (signed)"
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
        "{:>6}  {:>10}  {:>10}  {:>8}  {:>6}  {:>6}  {:>6}  {:>6}",
        "conv", "ceiling", "gained", "gcd", "today", "scaled", "saved", "looped",
    );

    let mut today_total = 0u32;
    let mut scaled_total = 0u32;
    for conversation in numbers("CONVERSATION", &GROUPS) {
        let Ok((graph, _)) = build_group_graph(index, conversation) else {
            continue;
        };
        let Some(ceiling) = DataLayout::money_ceiling(&graph, world.money()) else {
            println!("{conversation:>6}  {:>10}", "not read");
            continue;
        };

        // THE SAME CYCLE TEST THE COUNTERS GET. A gain the player can walk back round to is a
        // gain the summed delta undercounts, so the scaling is unsound on that group however
        // well the gcd divides.
        let cyclic = on_a_cycle(&graph);
        let mut looped = false;
        let mut amounts: Vec<u32> = Vec::new();
        for node in graph.nodes() {
            for action in &node.actions {
                if !matches!(
                    action.kind(),
                    DialogueActionKind::GainMoney | DialogueActionKind::LoseMoney
                ) {
                    continue;
                }
                if action.value() != 0 {
                    amounts.push(action.value().unsigned_abs());
                }
                looped |= cyclic.contains(&node.id) && !action.once();
            }
            // A COST IS CHARGED EVERY TIME THE OPTION IS TAKEN unless it is `cost_once`, so a
            // priced option on a loop drains the purse repeatedly.
            if node.is_cost_option() && node.cost != 0 && !node.cost_once {
                looped |= cyclic.contains(&node.id);
            }
        }

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
        let scaled = if looped {
            today
        } else {
            bits_for(gained / unit).min(today)
        };
        today_total += today as u32;
        scaled_total += scaled as u32;

        println!(
            "{conversation:>6}  {ceiling:>10}  {gained:>10}  {unit:>8}  {today:>6}  \
             {scaled:>6}  {:>6}  {:>6}",
            today as i32 - scaled as i32,
            if looped { "yes" } else { "" },
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
                piece
                    .parse()
                    .unwrap_or_else(|_| panic!("{name}={piece:?} is not a number"))
            })
            .collect(),
        Err(_) => fallback.to_vec(),
    }
}
