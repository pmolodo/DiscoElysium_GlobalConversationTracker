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
//! `cargo run --release --example search_residue`.
//!
//! - Live bytes return to the baseline after each search -> NO LEAK. Look at the drop.
//! - Live bytes climb search over search -> a leak, and the manager is where to look.
//!
//! ## What it answered, and the second question that left behind
//!
//! NOT A LEAK. Five searches over conversation 28: 33.6 MB baseline, 344.1 MB of residue
//! after EVERY one of them - identical to a tenth of a megabyte - and identical peaks of
//! 377.8 MB. A leak climbs; this jumps once and stays flat, which is a pool being reused
//! rather than anything escaping.
//!
//! AND AT FIRST IT DID NOT OVERFLOW, where the matrix does. The difference was one
//! CONDITION rather than anything subtler: the probe ran at the group-sized 512 MB budget
//! where a matrix row runs at six gigabytes. `BUDGET_MB` and `SEARCHES` move both, and at
//! the matrix's budget this reproduces the matrix exactly - 0xc00000fd on the THIRD search,
//! never earlier, never later, in four runs out of ten:
//!
//! ```text
//! DEGCT_BUDGET_MB=6144 DEGCT_SEARCHES=5 cargo run --release --example search_residue
//! ```
//!
//! A run that dies takes the process with it - a stack overflow is not a panic - so the LAST
//! ROW PRINTED is how far it got, and a run with no closing line did not survive. It is
//! intermittent, so no single run says anything: every number quoted here is out of at
//! least twenty.
//!
//! ## What the arrangement decides, which is the finding
//!
//! `THREAD` runs the same five searches four ways, and the table is in
//! [`lookahead_engine::symbolic::isolated`]. The short form: it is not the number of
//! searches and it is not the stack - it is BUILDING A SECOND MANAGER ON A THREAD THAT HAS
//! ALREADY BUILT ONE. One manager will take as many searches as it is given and never fail;
//! a second one on the same thread makes the third overflow.

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::backward::{Backward, SettledPass};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;

#[path = "../tests/common/mod.rs"]
mod common;

/// The shared counting allocator, asked a different question.
///
/// `tests/manager_memory.rs` uses it to ask what a manager takes when it is BUILT. This
/// asks what is still held after one is dropped, which is the leak question - same counter,
/// read at a different moment.
#[path = "../tests/common/counting_allocator.rs"]
mod counting_allocator;

use counting_allocator::{Counting, live};

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// The counter cap every symbolic measurement in this repository uses.
const COUNTER_CAP: i32 = 16;

/// The conversation de-fpax dies on.
const CONVERSATION: i32 = 28;

/// How many searches to run in one process. `SEARCHES` moves it.
///
/// The matrix dies on the THIRD row, so three is the number that reproduces it and a couple
/// more is what says whether the trend continues or settles.
const SEARCHES: usize = 5;

/// What each search's diagram manager is given, in megabytes. `BUDGET_MB` moves it.
///
/// THE GROUP-SIZED 512 BY DEFAULT, because the question this file was written for is what a
/// search RETAINS and that does not need the search to be large. It is a knob because the
/// OTHER question - why the matrix overflows where this does not - is a question about the
/// conditions, and six gigabytes is the first of them: `DiagramBudget::measurement`.
const BUDGET_MB: usize = 512;

/// A number from the environment, or the default written down above.
fn from_env(name: &str, fallback: usize) -> usize {
    lookahead_engine::core::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(fallback)
}

fn main() {
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
    // THE ENTRY THE MOST OTHERS CAN REACH, so a search is as big as the group allows: what
    // is being measured is what a large search leaves behind, and a small one leaves little
    // whatever the arrangement. See common::heaviest_target for why depth is the wrong proxy.
    let target = common::heaviest_target(&graph, start);

    // Everything the searches share, allocated before the baseline so it is not counted as
    // residue: what is being measured is what a SEARCH leaves, not what the graph costs.
    let symbols = graph.symbols().clone();
    let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false)
        .keeping_only_read(&symbols, &DataLayout::read_by(&graph));

    let searches = from_env("SEARCHES", SEARCHES);
    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);

    let baseline = live();
    println!("baseline before any search: {:.1} MB", mb(baseline));
    println!(
        "{searches} searches over conversation {CONVERSATION}, {} MB each",
        budget.memory() / (1024 * 1024),
    );
    // WHERE THE SEARCHES RUN, which is the control this file exists to be able to make.
    //
    // de-8hh2.13 established that the accumulation is PER THREAD and that stack size is not
    // the cause - one big spawned thread died just the same as the main one. These three
    // arms are that experiment, kept runnable rather than remembered, because the reason it
    // matters has moved: `bridge::answer` takes ONE thread per request and runs every start
    // of a menu on it, so "many searches, one big thread" is now the shipped arrangement
    // rather than a control.
    let arm = lookahead_engine::core::env::var("THREAD").unwrap_or_else(|_| "main".to_string());
    println!("thread: {arm}");
    println!(
        "\n{:>6}  {:>12}  {:>12}  {:>12}",
        "search", "peak", "after", "residue"
    );

    let one_search = |run: usize| {
        let peak;
        {
            // A SEARCH, in its own scope, so everything it owns is dropped before the
            // reading after it. The budget defaults to the ordinary group-sized one rather
            // than a measurement's six gigabytes, because the question this was written for
            // is what a search RETAINS and that does not need the search to be large - see
            // `BUDGET_MB` for the second question it can be turned up to ask.
            let vars = DataVars::new(&layout, &symbols, budget);
            let mut compiler = GuardCompiler::new(&vars)
                .with_world(&world)
                .with_constant_clock(DataLayout::group_passes_time(&graph));

            let seed = seed_of(&graph, &world, &vars).expect("room for a seed");
            let found =
                Backward::reaching(&graph, target, &mut compiler, &world, COUNTER_CAP as u32);

            // Read something off it so nothing can be optimised away.
            let reached = found.entries().count();
            std::hint::black_box(found.reachable_from(start, &seed));
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

        // FLUSHED, because the next search may not return: a stack overflow is not a panic
        // and nothing after it runs, so a buffered row would be lost with the process and
        // the log would not say which search it got to.
        use std::io::Write;
        let _ = std::io::stdout().flush();
    };

    match arm.as_str() {
        // THE MAIN THREAD, which is where this measurement has always run and where the
        // matrix runs too - see de-w0rw.
        "main" => (1..=searches).for_each(one_search),
        // ONE BIG THREAD FOR ALL OF THEM: what `bridge::answer` gives a menu, and what
        // de-8hh2.13 found does NOT help. 512 MB of stack, from `isolated::STACK`.
        "one" => isolated::on_its_own_thread(|| (1..=searches).for_each(&one_search)),
        // A THREAD EACH: the arrangement de-8hh2.13 verified over twelve searches, and what
        // every converted measurement in this repository now does.
        "each" => (1..=searches).for_each(|run| isolated::on_its_own_thread(|| one_search(run))),
        // ONE MANAGER FOR ALL OF THEM, on one thread: what `bridge::answer` actually does
        // with a menu, and the arm that separates "per search" from "per manager".
        //
        // Every other arm here builds a manager per search. This one builds it once and runs
        // every search against it, which is why it reports no residue: nothing is dropped
        // between the readings, so the column that matters in this arm is whether the run
        // finishes at all.
        "one-manager" => isolated::on_its_own_thread(|| {
            let vars = DataVars::new(&layout, &symbols, budget);
            let mut compiler = GuardCompiler::new(&vars)
                .with_world(&world)
                .with_constant_clock(DataLayout::group_passes_time(&graph));
            let seed = seed_of(&graph, &world, &vars).expect("room for a seed");

            for run in 1..=searches {
                let found =
                    Backward::reaching(&graph, target, &mut compiler, &world, COUNTER_CAP as u32);
                let reached = found.entries().count();
                std::hint::black_box(found.reachable_from(start, &seed));
                assert!(reached > 0, "search {run} reached nothing at all");

                println!(
                    "{run:>6}  {:>10.1} MB  {:>12}  {:>12}",
                    mb(live()),
                    "-",
                    "-",
                );
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
        }),
        other => {
            eprintln!(
                "{}={other:?} is not one of main, one, each, one-manager",
                lookahead_engine::core::env::qualified("THREAD")
            );
            std::process::exit(2);
        }
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
