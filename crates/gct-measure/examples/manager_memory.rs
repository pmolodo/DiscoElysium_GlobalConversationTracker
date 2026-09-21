// SPDX-License-Identifier: MIT
//! What a diagram manager's node really costs, once its unique table has grown.
//!
//! THE FIGURE `DiagramBudget::BYTES_PER_NODE` IS DERIVED FROM. A budget divides by that
//! constant to decide how many nodes it buys, and a search enforces its allowance with
//! `node_count() * BYTES_PER_NODE` - which is circular, and reports what the constant
//! claims rather than what a node costs. If it undercounts, every search quietly overshoots
//! the budget it was given and nothing notices.
//!
//! What that cost once: a manager built for 1 GB spent 1020 of its 1024 MB while holding
//! 23.3 million of the 33.5 million nodes the budget said it had bought, so a search that
//! spent its allowance was already past it.
//!
//! IT CANNOT BE ANSWERED BY BUILDING EMPTY MANAGERS, which is why this is not beside the
//! test. oxidd starts the unique table empty and grows it as nodes are inserted (de-mnrb),
//! so construction has not paid for it yet. This FILLS one manager and reads the allocator
//! as it goes; the marginal cost between two readings is the table's share.
//!
//! `tests/manager_memory.rs` checks the other half - that a budget covers the manager it
//! buys at all, and is mostly spent by it - and runs in the suite.
//!
//! Run it with `cargo run --release --example manager_memory`.

use oxidd::bdd::{BDDFunction, BDDManagerRef};
use oxidd::{BooleanFunction, Manager, ManagerRef};

use lookahead_engine::symbolic::budget::DiagramBudget;

use gct_measure::counting_allocator;

use counting_allocator::{Counting, live};

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// How many variables the synthetic layout declares.
///
/// About what a real group carries after the read trim - 362 keeps 102 slots and 14 keeps
/// 232 - so the diagrams are the shape of real ones without needing a real one.
const SYNTHETIC_VARS: usize = 160;

/// The budget the growth is measured inside.
///
/// Large enough that the fill never approaches capacity, because approaching it measures
/// something else: a manager cannot hold its stated number of LIVE nodes - every operation
/// builds intermediates in the same store - so a fill taken to the limit reports the
/// refusal point rather than the cost of a node.
const GROWTH_BUDGET: usize = 1024 * 1024 * 1024;

/// Node counts to stop and read the allocator at.
///
/// CARRIED CLOSE TO CAPACITY on purpose. A hash table doubles, so the unique table's growth
/// may not be finished halfway - a marginal cost measured at half the store can miss a
/// doubling that only happens near the end, and near the end is where a budget is spent.
const SAMPLES: [usize; 9] = [
    2_000_000, 5_000_000, 9_000_000, 13_000_000, 17_000_000, 21_000_000, 25_000_000, 28_000_000,
    31_000_000,
];

/// Fills a manager toward `target` nodes and returns how many it ended up holding.
///
/// ## Why this builds diagrams rather than running a search
///
/// The first version of this filled the manager by running real reachability over a
/// conversation group, and that was a mistake twice over: it measured the SEARCH, which is
/// minutes, and it dragged the search's own sets and queue into a figure that is supposed to
/// be per NODE. What a node costs is a property of the manager's data structures - the store
/// entry, the unique table slot, the apply cache - and has nothing to do with what the node
/// means. So the fill can be anything that makes nodes, and the cheapest thing that makes
/// them in bulk is a union of random cubes.
///
/// Each cube conjoins one polarity of every variable, which is a chain as deep as the
/// variable count; unioning them into an accumulator keeps most of those nodes alive. A
/// deterministic pseudo-random sequence, so a run can be repeated.
fn grow_to(
    accumulated: &mut BDDFunction,
    seed: &mut u64,
    vars: &[BDDFunction],
    manager: &BDDManagerRef,
    target: usize,
) -> usize {
    // POLLED EVERY FEW CUBES, not every one: num_inner_nodes appears to walk the store, so
    // asking after each cube made the fill quadratic and a three-budget run took four
    // minutes. The overshoot past `target` this allows is a few cubes' worth, which against
    // hundreds of thousands of nodes is noise.
    const CUBES_BETWEEN_READINGS: usize = 512;
    let mut until_reading = CUBES_BETWEEN_READINGS;

    // STOPS WHEN THE MANAGER REFUSES, not only when the target is met. A manager cannot
    // actually hold its stated capacity in LIVE nodes: every operation builds intermediates
    // in the same store, so it runs out somewhere below the number the budget named. That
    // gap is part of what is being measured, so a refusal ends the fill rather than failing
    // the test.
    loop {
        let mut cube = manager.with_manager_shared(BDDFunction::t);
        for var in vars {
            // xorshift64*, so the fill is reproducible between runs.
            *seed ^= *seed << 13;
            *seed ^= *seed >> 7;
            *seed ^= *seed << 17;
            let literal = match if *seed & 1 == 0 {
                Ok(var.clone())
            } else {
                var.not()
            } {
                Ok(literal) => literal,
                Err(_) => return manager.with_manager_shared(|m| m.num_inner_nodes()),
            };
            cube = match cube.and(&literal) {
                Ok(next) => next,
                Err(_) => return manager.with_manager_shared(|m| m.num_inner_nodes()),
            };
        }
        *accumulated = match accumulated.or(&cube) {
            Ok(next) => next,
            Err(_) => return manager.with_manager_shared(|m| m.num_inner_nodes()),
        };

        until_reading -= 1;
        if until_reading == 0 {
            until_reading = CUBES_BETWEEN_READINGS;
            let held = manager.with_manager_shared(|m| m.num_inner_nodes());
            if held >= target {
                return held;
            }
        }
    }
}

/// What a node costs ONCE THE TABLE HAS GROWN, which is the figure a budget needs.
///
/// ## The question, and why the test above cannot answer it
///
/// [`DiagramBudget::BYTES_PER_NODE`] decides how many nodes a budget buys, and a search
/// enforces its allowance with `DataVars::memory_used`, which is `node_count() *
/// BYTES_PER_NODE`. That is CIRCULAR: it reports what the constant claims a node costs, not
/// what one costs. If the constant undercounts, every search quietly overshoots the budget
/// it was given, and nothing notices.
///
/// The test above counts an EMPTY manager. oxidd starts the unique table empty and grows it
/// as nodes are inserted (de-mnrb), so an empty manager has not paid for it yet - and this
/// file's own header, "all three are made when the manager is built and none of them grows
/// afterwards", is the claim in question.
///
/// ## How it measures, and why it is shaped this way
///
/// ONE manager, filled once, with the allocator read at increasing node counts. The
/// MARGINAL cost between two readings is what a node adds, which is the unique table's
/// share and the thing nobody has measured. The preallocated share is the empty figure
/// divided by capacity, and the two together are what `BYTES_PER_NODE` has to cover.
///
/// Filling several managers to their stated capacities was tried first and is a worse
/// measurement in two ways: it reports the point at which the manager refuses rather than
/// the cost of a node, and its per-budget baselines came out inconsistent - a 128 MB
/// manager reading "empty" at 18 MB against a 64 MB manager's 42.
fn main() {
    let allowance = DiagramBudget::new(GROWTH_BUDGET);
    let capacity = allowance.nodes();

    let before = live();
    let manager = allowance.manager();
    let vars: Vec<BDDFunction> = manager.with_manager_exclusive(|m| {
        m.add_vars(SYNTHETIC_VARS as u32)
            .map(|v| BDDFunction::var(m, v).expect("a freshly added variable"))
            .collect()
    });
    let empty = live() - before;

    println!(
        "a {} MB budget preallocates {:.1} MB for {capacity} nodes: {:.1} bytes a node \
         before anything is stored\n",
        GROWTH_BUDGET / (1024 * 1024),
        empty as f64 / (1024.0 * 1024.0),
        empty as f64 / capacity as f64,
    );
    println!(
        "{:>12}  {:>10}  {:>12}  {:>14}",
        "nodes", "held MB", "grown MB", "marginal B/node"
    );

    let mut accumulated = manager.with_manager_shared(BDDFunction::f);
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut last: Option<(usize, usize)> = None;
    // EVERY reading, so the figure taken at the end is the MEDIAN rather than the last one.
    // The last is not safe to use: once the store is nearly full the manager starts
    // refusing and collecting, the node count goes DOWN between samples, and the marginal
    // computed across that is meaningless - one run read 8.8 and then 15.1 where every
    // earlier sample agreed on 15.7.
    let mut marginals: Vec<f64> = Vec::new();

    for target in SAMPLES {
        let held = grow_to(&mut accumulated, &mut seed, &vars, &manager, target);
        let total = live() - before;
        let grown = total - empty;

        let marginal = match last {
            Some((was_held, was_total)) if held > was_held => {
                let step = (total - was_total) as f64 / (held - was_held) as f64;
                marginals.push(step);
                step
            }
            _ => grown as f64 / held.max(1) as f64,
        };
        println!(
            "{held:>12}  {:>10.1}  {:>12.1}  {marginal:>14.1}",
            total as f64 / (1024.0 * 1024.0),
            grown as f64 / (1024.0 * 1024.0),
        );
        let previous = last;
        last = Some((held, total));

        // The manager refuses somewhere below its stated capacity, because operations build
        // intermediates in the same store. When it does, grow_to stops making progress and
        // there is nothing further to read.
        if previous.is_some_and(|(was_held, _)| held <= was_held) {
            println!("(stopping: the manager will not take any more)");
            break;
        }
    }

    assert!(
        !marginals.is_empty(),
        "no sample grew the store, so nothing was measured"
    );
    marginals.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
    let marginal = marginals[marginals.len() / 2];

    let preallocated = empty as f64 / capacity as f64;
    let real = preallocated + marginal;
    println!(
        "\n{preallocated:.1} preallocated + {marginal:.1} grown = {real:.1} bytes a node, \
         against BYTES_PER_NODE of {}",
        DiagramBudget::BYTES_PER_NODE,
    );

    assert!(
        real <= DiagramBudget::BYTES_PER_NODE as f64,
        "a node really costs {real:.1} bytes - {preallocated:.1} preallocated plus \
         {marginal:.1} as the table grows - against the {} the budget divides by. The \
         budget therefore buys more nodes than it can pay for, and every search that \
         spends its allowance overshoots.",
        DiagramBudget::BYTES_PER_NODE,
    );
}
