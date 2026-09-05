// SPDX-License-Identifier: MIT
//! Does one symbolic search leave anything behind for the next one?
//!
//! ## The question this settles, and why it is first
//!
//! de-fpax: conversation 28 overflows the stack on the THIRD row of a matrix run, and the
//! same row alone finishes in 58 ms. So something about running several searches in one
//! process is different from running one - and "cumulative" was an observation about
//! behaviour, not a diagnosis. Two candidates, and they want opposite investigations:
//!
//! 1. A RECURSIVE DROP. Releasing a large reference-counted diagram walks its children, and
//!    that walk is as deep as the structure. A classic Rust overflow, the same shape as
//!    dropping a long linked list - and it fits the symptom, because the deepest recursion
//!    happens when a big structure is finally released, which is after the search that
//!    built it. The row that dies is not the row at fault.
//! 2. A MANAGER THAT IS NOT RELEASED when its handle goes out of scope, so later searches
//!    run against something bigger than they should. That one is a leak.
//!
//! Both predict "later searches are worse". Only (2) predicts growing live memory. So the
//! measurement below tells them apart, and it is the cheap thing to do before anything else:
//! run several searches in one process and watch what is held BETWEEN them.
//!
//! ## How to read it
//!
//! `cargo test --test search_residue -- --ignored --nocapture`.
//!
//! - Live bytes return to the baseline after each search -> NO LEAK. Look at the drop.
//! - Live bytes climb search over search -> a leak, and the manager is where to look.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated::on_its_own_thread;
use lookahead_engine::symbolic::reachability::{seed_of, Reachability};
use lookahead_engine::symbolic::vars::DataVars;

mod common;

/// An allocator that keeps a running total of what is held.
///
/// The same counting trick `tests/manager_memory.rs` uses for a different question - what a
/// manager asks for when it is built. This one watches what is still held after it is
/// dropped, which is the leak question.
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

fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// The counter cap every symbolic measurement in this repository uses.
const COUNTER_CAP: i32 = 16;

/// The conversation de-fpax dies on.
const CONVERSATION: i32 = 28;

/// How many searches to run in one process.
///
/// The matrix dies on the THIRD row, so three is the number that reproduces it and a couple
/// more is what says whether the trend continues or settles.
const SEARCHES: usize = 5;

#[test]
#[ignore = "a measurement; run it deliberately"]
fn what_one_search_leaves_behind_for_the_next() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");
    let Ok((graph, _)) = build_group_graph(&index, CONVERSATION) else {
        eprintln!("conversation {CONVERSATION}'s group does not build; skipping.");
        return;
    };

    let world = common::measurement_save();
    let start = DialogueNodeId::new(CONVERSATION, 0);
    if graph.get(start).is_none() {
        eprintln!("no entry 0 in conversation {CONVERSATION}; skipping.");
        return;
    }

    // Everything the searches share, allocated before the baseline so it is not counted as
    // residue: what is being measured is what a SEARCH leaves, not what the graph costs.
    let symbols = graph.symbols().clone();
    let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false)
        .keeping_only_read(&symbols, &DataLayout::read_by(&graph));

    let baseline = live();
    println!("baseline before any search: {:.1} MB", mb(baseline));
    println!("\n{:>6}  {:>12}  {:>12}  {:>12}", "search", "peak", "after", "residue");

    for run in 1..=SEARCHES {
        let peak;
        {
            // A SEARCH, in its own scope, so everything it owns is dropped before the
            // reading after it. The budget is the ordinary group-sized one rather than a
            // measurement's six gigabytes: the question is what is RETAINED, and that does
            // not need the search to be large.
            let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
            let mut compiler = GuardCompiler::new(&vars)
                .with_world(&world)
                .with_constant_clock(DataLayout::group_passes_time(&graph));

            let seed = seed_of(&graph, &world, &vars);
            let found = Reachability::explore(
                &graph, start, &seed, &mut compiler, &world, COUNTER_CAP as u32,
            );

            // Read something off it so nothing can be optimised away.
            let reached = found.entries().count();
            peak = live();
            assert!(reached > 0, "search {run} reached nothing at all");
        }

        let after = live();
        println!(
            "{run:>6}  {:>10.1} MB  {:>10.1} MB  {:>10.1} MB",
            mb(peak),
            mb(after),
            mb(after.saturating_sub(baseline)),
        );
    }

    let residue = live().saturating_sub(baseline);
    println!(
        "\nAFTER {SEARCHES} SEARCHES: {:.1} MB still held above the baseline",
        mb(residue),
    );
    println!(
        "  climbing per search -> a leak, and the manager is where to look\n  \
         back to about zero  -> no leak; the overflow is a recursive DROP"
    );
}
