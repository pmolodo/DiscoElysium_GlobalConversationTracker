// SPDX-License-Identifier: MIT
//! Does a budget cover the manager it buys?
//!
//! THE BUDGET HAS TO COVER EVERYTHING THE SEARCH SPENDS, and on the symbolic side that is
//! three allocations, not one: oxidd's node store, the unique table that finds a node
//! again, and the apply cache that remembers the result of an operation.
//!
//! TWO OF THE THREE ARE MADE WHEN THE MANAGER IS BUILT. This file used to say all three
//! were, and that none of them grew afterwards, and that was the mistake the whole thing
//! rested on: oxidd starts the unique table EMPTY and grows it as nodes are inserted, so an
//! empty manager has not paid for it and no measurement of construction could see it.
//!
//! So this checks the FLOOR - what construction alone costs, and that a budget covers it
//! and is mostly spent by it. What a node costs once the table has grown is the other half
//! of the question and cannot be answered by building empty managers; it is measured in
//! `measurements/manager_memory.rs`, and `DiagramBudget::BYTES_PER_NODE` is derived from
//! that rather than from anything here.

use lookahead_engine::symbolic::budget::DiagramBudget;

#[path = "common/counting_allocator.rs"]
mod counting_allocator;

use counting_allocator::{Counting, live};

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// What a manager built for this budget actually allocates.
fn cost_of(budget: usize) -> usize {
    let before = live();
    let manager = DiagramBudget::new(budget).manager();
    let after = live();
    drop(manager);
    after - before
}

/// The budgets a real run uses, from the smallest shipped to the largest measured.
const BUDGETS: [usize; 4] = [
    64 * 1024 * 1024,
    256 * 1024 * 1024,
    1024 * 1024 * 1024,
    6 * 1024 * 1024 * 1024,
];

/// ONE TEST, because the counter is the process's and tests run in parallel.
///
/// Two tests measuring against a global total interleave, and each reads the other's
/// allocations as its own: a first attempt at this split the two assertions below into two
/// tests and had them disagree about the same 6 GB manager by a factor of two, which looks
/// exactly like a real finding and is not one.
#[test]
fn a_budget_buys_a_manager_that_fits_inside_it_and_mostly_fills_it() {
    // Measured first and judged afterwards, so one run reports the whole table rather
    // than stopping at the first budget that disappoints.
    let measured: Vec<(usize, usize)> = BUDGETS
        .iter()
        .map(|&budget| (budget, cost_of(budget)))
        .collect();

    for &(budget, cost) in &measured {
        let percent = (cost as f64 / budget as f64) * 100.0;
        println!(
            "budget {:>5} MB -> {:>5} MB allocated ({percent:.1}%), {} nodes",
            budget / (1024 * 1024),
            cost / (1024 * 1024),
            DiagramBudget::new(budget).nodes(),
        );
    }

    for &(budget, cost) in &measured {
        assert!(
            cost <= budget,
            "a manager built for {budget} bytes allocated {cost} - the budget does not \
             cover what it buys, so a run held to it spends more than it was allowed"
        );

        // The other half of the same claim. A derivation that is too cautious buys a
        // manager far smaller than the budget allows, which shows up as a search giving up
        // while the allowance it was given sits unused - and that reads as "the algorithm
        // needs more memory" when it means "the arithmetic is wrong".
        //
        // Half rather than anything tighter because the apply cache is rounded up to a
        // power of two, so how much of a budget is spent depends on where the node count
        // falls against that rounding: 93 per cent at 6 GB, where it happens to land on a
        // power of two, and 65 at 64 MB, where it does not.
        assert!(
            cost * 2 >= budget,
            "a manager built for {budget} bytes allocated only {cost}, less than half of \
             it; the derivation is leaving most of the budget unspent"
        );
    }
}
