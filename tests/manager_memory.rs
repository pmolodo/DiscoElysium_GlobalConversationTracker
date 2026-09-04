// SPDX-License-Identifier: MIT
//! What a diagram manager actually allocates, and whether a budget covers it.
//!
//! THE BUDGET HAS TO COVER EVERYTHING THE SEARCH SPENDS, and on the symbolic side that is
//! three allocations, not one: oxidd's node store, the unique table that finds a node
//! again, and the apply cache that remembers the result of an operation. All three are
//! made when the manager is built and none of them grows afterwards.
//!
//! The arithmetic that sizes them therefore has to be measured rather than guessed. It was
//! guessed - 32 bytes a node, covering the node and its table slot and ignoring the cache
//! entirely - so a budget bought more memory than it said. This measures the real figure
//! and holds the derivation to it.
//!
//! HOW IT MEASURES. A counting allocator, rather than the process's resident size: the
//! question is how many bytes this library asks for, which is exactly what a global
//! allocator sees, and it is unaffected by what the operating system decides to keep
//! resident or when a page is first touched.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use lookahead_engine::symbolic::budget::DiagramBudget;

/// An allocator that keeps a running total of what has been asked for.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(new_size, Ordering::Relaxed);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// What is currently allocated, in bytes.
fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

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
    let measured: Vec<(usize, usize)> =
        BUDGETS.iter().map(|&budget| (budget, cost_of(budget))).collect();

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
