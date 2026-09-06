// SPDX-License-Identifier: MIT
//! How often the fixed point steps the SAME entry, which decides whether SCC ordering pays.
//!
//! ## The question, and why the totals cannot answer it
//!
//! Conversation 14 ran for forty minutes at eleven steps a minute and reached 962 entries.
//! That is about fifteen steps per entry on average, and de-3x76.3 proposes replacing the
//! FIFO worklist with a weak topological order to cut the redundant ones. But an average of
//! fifteen is consistent with two completely different situations:
//!
//! - A FEW ENTRIES RE-PROPAGATED THOUSANDS OF TIMES while most are stepped once or twice.
//!   That is redundant work, it is what a FIFO worklist does to a cyclic graph, and SCC
//!   ordering is the standard fix.
//! - EVERY ENTRY STEPPED A DOZEN TIMES, each step expensive because the sets are enormous.
//!   Ordering changes nothing there; the cost is per step and the steps are all needed.
//!
//! The totals look identical. The distribution does not, and rewriting a solver on the
//! strength of an average would be building the fix for whichever one it happens to be.
//!
//! ## How to read it
//!
//! `cargo run --release --example requeue_shape`
//!
//! A long tail - a maximum far above the median - says redundant re-propagation, and
//! de-3x76.3 is worth building. A flat distribution says the steps are all real work, and
//! the effort belongs somewhere else in de-3x76.

use std::collections::HashMap;
use std::cell::RefCell;
use std::rc::Rc;

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated::on_its_own_thread;
use lookahead_engine::symbolic::reachability::{seed_of, Budget, Reachability};
use lookahead_engine::symbolic::vars::DataVars;

#[path = "../tests/common/mod.rs"]
mod common;

/// The cap every symbolic measurement in this repository uses.
const COUNTER_CAP: i32 = 16;

/// Groups worth asking about: one that fails, two that finish, one that is trivial.
///
/// 14 is the group de-3x76 exists for. 631 and 368 both reach a fixed point, so their
/// distributions are what a HEALTHY one looks like and are the comparison that makes 14's
/// readable. 1030 finishes in milliseconds and is the control.
const GROUPS: [i32; 4] = [14, 631, 368, 1030];

/// Long enough to see the shape, short enough to run over four groups.
///
/// 14 will not finish in any time available, and does not need to: the distribution of a
/// search stopped part way still says whether a few entries are dominating, which is the
/// whole question.
const CAP: std::time::Duration = std::time::Duration::from_secs(90);

fn main() {
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
        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false)
            .keeping_only_read(&symbols, &DataLayout::read_by(&graph));
        // A THREAD FOR THE SEARCH, with the manager built inside it - de-fpax. The `Rc`
        // below is not `Send`, which is exactly right: it is created, used and dropped in
        // here, and only the counted steps come back out.
        let (steps, settled) = on_its_own_thread(|| {
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(&graph));
        let seed = seed_of(&graph, &world, &vars);

        // Rc<RefCell<..>> because the hook is an ordinary Fn and the counts have to outlive
        // it. Not shared across threads, so there is nothing to lock.
        let counts: Rc<RefCell<HashMap<DialogueNodeId, usize>>> = Rc::default();
        let recording = Rc::clone(&counts);

        let budget = Budget {
            steps: usize::MAX,
            time: CAP,
            memory: DiagramBudget::over_a_group().memory(),
            report_every: 20_000,
            report_gap: std::time::Duration::ZERO,
            check_gap: std::time::Duration::ZERO,
            on_progress: None,
            on_step: Some(Box::new(move |id| {
                *recording.borrow_mut().entry(id).or_default() += 1;
            })),
            // Off: a machine-dependent stop would make the distribution depend on what else
            // was running.
            system_reserve: 0.0,
            halt_on: None,
        };

        let found = Reachability::explore_within(
            &graph, start, &seed, &mut compiler, &world, COUNTER_CAP as u32, &budget,
        );
        let settled = found.stats().reached_fixed_point;

        let counts = counts.borrow();
        let mut steps: Vec<usize> = counts.values().copied().collect();
        steps.sort_unstable();
        (steps, settled)
        });

        let total: usize = steps.iter().sum();
        let median = steps.get(steps.len() / 2).copied().unwrap_or(0);
        let p90 = steps.get(steps.len() * 9 / 10).copied().unwrap_or(0);
        let max = steps.last().copied().unwrap_or(0);
        // What share of all the work went to the busiest tenth of entries. A FIFO worklist
        // re-propagating a cycle concentrates here; real work does not.
        let busiest_tenth: usize = steps.iter().rev().take(steps.len() / 10).sum();

        println!(
            "{conversation:>5}  {} entries stepped, {total} steps  median {median}, p90 {p90}, \
             max {max}  busiest tenth took {:.0}% of the work  ({})",
            steps.len(),
            if total > 0 { busiest_tenth as f64 / total as f64 * 100.0 } else { 0.0 },
            if settled { "finished" } else { "stopped early" },
        );
    }

    println!(
        "\nA MAX FAR ABOVE THE MEDIAN, and a busiest tenth taking most of the work, says \
         redundant re-propagation and de-3x76.3 is worth building.\nA flat distribution says \
         the steps are all real and ordering will not help."
    );
}
