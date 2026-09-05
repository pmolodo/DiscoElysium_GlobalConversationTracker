// SPDX-License-Identifier: MIT
//! Does conversation 14 reach MORE STATES, or the same states in more nodes?
//!
//! ## The one measurement that decides whether de-3x76 is alive
//!
//! Three measurements have failed to explain why 14 never reaches a fixed point while 631
//! and 368 do:
//!
//! - de-3x76.1: variable COUNT does not separate them. 631 carries 257 and finishes; 14
//!   carries 236 and does not.
//! - de-3x76.3's measurement: step COUNT does not either, and in the wrong direction - in
//!   equal time the groups that FINISH did three times more steps.
//! - de-3x76.10: the variable ORDER matters by about a factor of two, and the order already
//!   in use is the best of five tried.
//!
//! What is left is that 14's diagrams are simply bigger - 535,000 nodes against 631's
//! 237,000 at equal steps, largest set 3,640 against 1,772. Two readings, opposite
//! consequences:
//!
//! - BIGGER SET. 14 genuinely reaches far more data states. Then no encoding change fixes
//!   it, de-3x76 is a bag of optimisations rather than an answer, and the response is
//!   de-a1wb: never send a group like 14 to the backward engine.
//! - WORSE ENCODING. 14 reaches a similar number of states in far more nodes. Then the
//!   encoding is at fault and the epic is pointed at the right thing.
//!
//! Nodes cannot tell these apart. Counting the SATISFYING ASSIGNMENTS can.
//!
//! ## Counting them needs a float, and why
//!
//! oxidd's own example counts into `Saturating<u64>`, which is right for small domains and
//! useless here: these layouts have over two hundred variables, so a set of any size at all
//! is far past 2^64 and every row would read u64::MAX. The count type below is an f64, and
//! oxidd's counting algorithm has a floating-point path for exactly this - it scales the
//! terminal value down by the exponent range and back up at the end, so the precision goes
//! where it is needed.
//!
//! Run it with `cargo test --test reachable_states -- --ignored --nocapture`.

use oxidd::util::{IsFloatingPoint, SatCountCache};
use oxidd::{BooleanFunction, Function};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::reachability::{seed_of, Budget, Reachability};
use lookahead_engine::symbolic::vars::DataVars;

mod common;

const COUNTER_CAP: i32 = 16;

/// The group that fails, and two that finish, so the failing one has a reference.
const GROUPS: [i32; 3] = [14, 631, 368];

/// The same fixed work for every group, so the sets are what differ rather than the effort.
const STEPS: usize = 4_000;

/// A model count that does not saturate.
///
/// Deliberately NOT implementing `ShlAssign<i32>`: oxidd decides whether a count type is
/// floating point through a blanket implementation over that trait, so a type that has it
/// is treated as an integer and loses the scaling that makes large domains countable.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
struct Count(f64);

impl From<u32> for Count {
    fn from(value: u32) -> Self {
        Count(f64::from(value))
    }
}

impl std::ops::Add for Count {
    type Output = Count;
    fn add(self, other: Count) -> Count {
        Count(self.0 + other.0)
    }
}

impl std::ops::Shl<u32> for Count {
    type Output = Count;
    fn shl(self, by: u32) -> Count {
        Count(self.0 * 2f64.powi(by as i32))
    }
}

impl std::ops::Shr<u32> for Count {
    type Output = Count;
    fn shr(self, by: u32) -> Count {
        Count(self.0 / 2f64.powi(by as i32))
    }
}

impl IsFloatingPoint for Count {
    const FLOATING_POINT: bool = true;
    const MIN_EXP: i32 = f64::MIN_EXP;
}

#[test]
#[ignore = "a measurement; run it deliberately"]
fn whether_the_failing_group_reaches_more_states_or_just_holds_them_worse() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");
    let world = common::measurement_save();

    println!(
        "{:>5}  {:>5}  {:>10}  {:>12}  {:>13}  {:>13}",
        "conv", "vars", "entries", "held nodes", "largest set", "states in it"
    );

    for conversation in GROUPS {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            println!("{conversation:>5}  does not build");
            continue;
        };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            println!("{conversation:>5}  no entry 0");
            continue;
        }

        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false)
            .keeping_only_read(&symbols, &DataLayout::read_by(&graph));
        let total_vars = layout.total_vars();

        // A fat stack for the same reason tests/order_sensitivity.rs uses one: releasing a
        // large diagram walks it recursively (de-fpax).
        let row = std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(512 * 1024 * 1024)
                .spawn_scoped(scope, || {
                    let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
                    let mut compiler = GuardCompiler::new(&vars)
                        .with_world(&world)
                        .with_constant_clock(DataLayout::group_passes_time(&graph));
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
                        &graph, start, &seed, &mut compiler, &world, COUNTER_CAP as u32,
                        &budget,
                    );
                    let stats = found.stats();

                    // THE LARGEST SINGLE SET, which is the one the file's own guidance says
                    // to watch: a total over entries climbs merely because more entries have
                    // a set, where the largest says whether the representation is failing.
                    let mut cache: SatCountCache<Count, std::collections::hash_map::RandomState> =
                        SatCountCache::default();
                    let biggest = found
                        .entries()
                        .filter_map(|id| found.states_at(id))
                        .max_by_key(|set| set.node_count())
                        .map(|set| set.sat_count(total_vars, &mut cache).0)
                        .unwrap_or(0.0);

                    (
                        stats.entries_reached,
                        vars.node_count(),
                        stats.largest_set,
                        biggest,
                    )
                })
                .expect("a measurement thread")
                .join()
                .expect("the measurement thread")
        });

        let (entries, held, largest, states) = row;
        println!(
            "{conversation:>5}  {total_vars:>5}  {entries:>10}  {held:>12}  {largest:>13}  \
             {states:>13.3e}"
        );
    }

    println!(
        "\nSTATES SIMILAR AND NODES FAR APART -> the encoding is at fault and de-3x76 is \
         alive.\nSTATES FAR APART -> 14's set is genuinely bigger, no encoding change fixes \
         it, and the answer is de-a1wb."
    );
}
