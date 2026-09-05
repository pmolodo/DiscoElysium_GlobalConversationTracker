// SPDX-License-Identifier: MIT
//! Does the VARIABLE ORDER change what the fixed point costs?
//!
//! ## Why this is the question left standing
//!
//! Two measurements have now failed to explain why conversation 14 never reaches a fixed
//! point while 631 and 368 do:
//!
//! - de-3x76.1: variable COUNT does not separate them. 631 carries 257 variables and
//!   finishes; 14 carries 236 and never does.
//! - de-3x76.3's measurement: step COUNT does not separate them either, and not in the
//!   direction that task assumed - in the same ninety seconds the groups that finish did
//!   THREE TIMES MORE steps than the one that does not. 14 is limited by what a step costs.
//!
//! Both point at the same remaining explanation: the SHAPE of the diagrams rather than their
//! number of variables. For a BDD, shape is largely the variable order, and size is
//! exponential in it in the bad cases.
//!
//! ## What was tested before, and why it did not settle this
//!
//! tests/symbolic_compression.rs compares the natural order against its exact REVERSE and
//! finds little difference. That is a weak probe of a space of n! orders: reversal mirrors
//! the order, it does not rearrange it, so adjacent variables stay adjacent. It also
//! measures a different object - a union of whole crawl states, entry included - where the
//! fixed point keeps one set per entry with no entry variable at all.
//!
//! ## What this does instead
//!
//! Runs the same bounded search under several orders and compares what the diagrams cost.
//! RANDOM PERMUTATIONS rather than a clever heuristic, because the question here is not
//! "which order is best" but "does the order matter at all" - and if several random orders
//! land within a few per cent of each other, no heuristic is going to rescue this and the
//! epic needs a different idea entirely.
//!
//! A spread says the opposite: that a better order exists and is worth looking for.
//!
//! Run it with `cargo test --test order_sensitivity -- --ignored --nocapture`.

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::reachability::{seed_of, Budget, Reachability};
use lookahead_engine::symbolic::vars::DataVars;

mod common;

const COUNTER_CAP: i32 = 16;

/// The group that fails, and one that succeeds to read it against.
const GROUPS: [i32; 2] = [14, 631];

/// How many random orders to try beyond the natural one.
const SHUFFLES: usize = 4;

/// A FIXED NUMBER OF STEPS, not a time limit.
///
/// The comparison has to be of equal work, and a time limit gives each order a different
/// number of steps - so the one that got furthest would look worst, which is backwards. With
/// the step count fixed, the diagram sizes are what differ and they are what is being asked
/// about.
const STEPS: usize = 4_000;

/// Lays out `count` slots in a deterministic shuffle.
fn shuffled(count: usize, seed: u64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..count).collect();
    let mut state = seed | 1;
    // Fisher-Yates with xorshift64*, so a run repeats exactly.
    for i in (1..order.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let j = (state % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    order
}

#[test]
#[ignore = "a measurement; run it deliberately"]
fn whether_the_variable_order_changes_what_a_search_costs() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");
    let world = common::measurement_save();

    for conversation in GROUPS {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            println!("{conversation}: does not build");
            continue;
        };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            println!("{conversation}: no entry 0");
            continue;
        }

        let symbols = graph.symbols().clone();
        let reads = DataLayout::read_by(&graph);
        let passes_time = DataLayout::group_passes_time(&graph);
        let natural =
            DataLayout::for_graph(&graph, COUNTER_CAP, None, passes_time)
                .keeping_only_read(&symbols, &reads);
        let slots = symbols.count();

        println!("\nconversation {conversation}, {STEPS} steps each:");
        println!(
            "  {:>10}  {:>12}  {:>12}  {:>10}",
            "order", "held nodes", "largest set", "entries"
        );

        let mut sizes: Vec<usize> = Vec::new();

        for attempt in 0..=SHUFFLES {
            let layout = if attempt == 0 {
                natural.clone()
            } else {
                natural.clone().in_slot_order(&shuffled(slots, attempt as u64 * 0x9E37_79B9))
            };

            // ONE THREAD PER ORDER, WITH A FAT STACK, and it is the DROP that needs it
            // rather than the search. Releasing a large diagram walks it recursively
            // (de-fpax), so five managers built and released in sequence on an ordinary
            // stack overflowed on the second - after the first had already printed its row,
            // which is exactly how that failure looks: a measurement that dies once it has
            // said something plausible.
            //
            // Scoped, so the graph and the world can be borrowed rather than cloned per
            // order, and joined immediately so only one manager is ever alive.
            let (held, largest, entries) = std::thread::scope(|scope| {
                std::thread::Builder::new()
                    .stack_size(512 * 1024 * 1024)
                    .spawn_scoped(scope, || {
                        let vars =
                            DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
                        let mut compiler = GuardCompiler::new(&vars)
                            .with_world(&world)
                            .with_constant_clock(passes_time);
                        let seed = seed_of(&graph, &world, &vars);

                        let budget = Budget {
                            steps: STEPS,
                            time: std::time::Duration::from_secs(600),
                            memory: DiagramBudget::over_a_group().memory(),
                            report_every: 20_000,
                            report_gap: std::time::Duration::ZERO,
                            check_gap: std::time::Duration::ZERO,
                            on_progress: None,
                            on_step: None,
                            system_reserve: 0.0,
                            halt_on: None,
                        };

                        let found = Reachability::explore_within(
                            &graph,
                            start,
                            &seed,
                            &mut compiler,
                            &world,
                            COUNTER_CAP as u32,
                            &budget,
                        );
                        let stats = found.stats();
                        (vars.node_count(), stats.largest_set, stats.entries_reached)
                    })
                    .expect("a measurement thread")
                    .join()
                    .expect("the measurement thread")
            });

            println!(
                "  {:>10}  {held:>12}  {largest:>12}  {entries:>10}",
                if attempt == 0 { "natural".to_string() } else { format!("shuffle {attempt}") },
            );
            sizes.push(held);
        }

        let smallest = sizes.iter().copied().min().unwrap_or(0);
        let biggest = sizes.iter().copied().max().unwrap_or(0);
        println!(
            "  spread: {smallest} to {biggest}, a factor of {:.2}",
            if smallest > 0 { biggest as f64 / smallest as f64 } else { 0.0 },
        );
    }

    println!(
        "\nA FACTOR NEAR ONE says the order does not matter for these diagrams, and de-3x76 \
         needs an idea that is not about the encoding.\nA LARGE SPREAD says a better order \
         exists and is worth searching for."
    );
}
