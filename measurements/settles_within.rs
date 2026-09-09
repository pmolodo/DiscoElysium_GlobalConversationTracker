// SPDX-License-Identifier: MIT
//! How often does the forward run SETTLE inside the budget the portfolio already gives it?
//!
//! ## The question, and why the answer splits de-bnjy.9 in two
//!
//! de-fawk built pruning - a settled forward run bounds what can arrive at an entry, so a
//! backward pass may narrow every pre-image by it, and on conversation 28 the backward half
//! falls from 19 ms to 2. It then left it OFF, because `portfolio::Budget::default` gives
//! the forward slice fifty milliseconds and 28 needs about fifty-seven to settle, so on the
//! shipped path there would be nothing settled to prune with.
//!
//! THAT ARGUMENT WAS MADE FROM ONE GROUP. Conversation 28 is the largest thing the heavy
//! list has that settles at all, and the game is 1,422 groups of which 1,372 hold about
//! forty-three entries each - see `group_census`. A group that size settles long before
//! fifty milliseconds, and pruning is off for every one of them today.
//!
//! So there are two different changes hiding under "turn settling on":
//!
//! - `Known::pruning(true)`, which is FREE and self-guarding. `Known` refuses to narrow
//!   anything unless `forward_settled`, so switching it on changes nothing whatsoever for a
//!   group that does not settle, and helps every group that already does. It costs no
//!   budget anywhere.
//! - RAISING THE FORWARD BUDGET, which is not free. It is spent per START, and a menu is a
//!   dozen starts - twenty-four when they are rolled checks - so ten milliseconds more buys
//!   28 its settle and costs 631 and 14 a quarter of a second per menu for nothing, since
//!   they do not settle in over a minute.
//!
//! This measures which groups fall on which side, so the first change can be made on
//! evidence and the second argued about separately.
//!
//! ## The budget sweep, 2026-09-07, which settles the SECOND change (de-wncd.1)
//!
//! `FORWARD_MS` moves the budget, so the question "how many groups settle just past what
//! they are allowed" is answerable by running this four times:
//!
//! ```text
//!   forward budget    spanning (of 50)    lone (of 120)
//!              50 ms        25                  119
//!             100 ms        26                  119
//!             250 ms        28                  119
//!            1000 ms        29                  120
//! ```
//!
//! TWENTY TIMES THE BUDGET BUYS FOUR GROUPS. And the spanning row is not a sample - there
//! are exactly 50 multi-conversation groups in the game (`group_census`), so that column is
//! the whole population: 25 of them settle at the shipped budget and 29 ever settle at all
//! within a second.
//!
//! The band de-wncd.1 was written about - groups like conversation 28, which needs about 57
//! ms against the 50 it is given - contains ONE further group between 50 and 100 ms. And
//! the budget is spent PER START, so buying it costs a 24-start menu 1.2 extra seconds.
//!
//! So a per-group rule for raising the forward budget has nothing to win: the groups it
//! would newly settle are a handful, they are the FAST groups already, and the slow ones
//! (368, 631, 14) settle at no budget a menu could afford. What the sweep does confirm is
//! that the FIRST change was right - most of the game settles at the budget it already has.
//!
//! ## THE SAME SWEEP WITH THE DEAD-SLOT ABSTRACTION ON, 2026-09-08 (de-rn59.6)
//!
//! Fifty milliseconds was chosen against the EXACT search, and de-rn59.3 then built an
//! abstraction that quantifies away the slots nothing onward reads. The `sweep` arm runs both
//! arms at every budget, one budget per process:
//!
//! ```text
//!   forward budget    spanning off   spanning on    lone off    lone on
//!             50 ms      25 of 50      27 of 50    119 of 120  120 of 120
//!            100 ms      27 of 50      28 of 50    119 of 120  120 of 120
//!            250 ms      28 of 50      28 of 50    119 of 120  120 of 120
//!           1000 ms      29 of 50      31 of 50         -           -
//!           2000 ms      29 of 50      33 of 50         -           -
//! ```
//!
//! The lone column saturates at the shipped budget, so the two heavy rows are the spanning
//! groups alone - which is the whole population of fifty rather than a sample.
//!
//! THE EXACT SEARCH PLATEAUS AND THE ABSTRACTED ONE DOES NOT. Twenty times the budget buys
//! the exact slice four groups and then nothing: 29 of 50 at a second is also 29 at two.
//! The abstraction is still climbing at the same point, 31 and then 33, so what it changes
//! is the SHAPE of the curve rather than its level - a group the exact search will never
//! settle at any budget a machine would give it does settle abstracted.
//!
//! AND NONE OF THAT IS REACHABLE FROM A MENU, which is what decides the issue. The budget is
//! spent PER START and a menu is a dozen starts, twenty-four when they are rolled checks, so
//! two seconds a start is forty-eight seconds of a two-second answer. In the band a menu can
//! actually afford the abstraction is worth two spanning groups and one lone one out of 170,
//! and by 250 ms it is worth nothing at all - the exact search has caught up.
//!
//! So the answer to "what should the slice cost, given the abstraction" is FIFTY
//! MILLISECONDS, the same as before, and the abstraction stays off by default. The gains it
//! does have live at budgets that are not a slice.
//!
//! WHAT WAS NOT MEASURED, and did not need to be: whether a settled abstracted run narrows
//! the backward passes as well as a settled exact one does. That comparison decides whether
//! the extra settled groups are worth having, and it only matters if something is going to
//! be turned on. Nothing is.
//!
//! ## What it does
//!
//! For each group, builds the diagram side exactly as `bridge::answer_within` does and runs
//! one forward pass from the group's entry 0 under the shipped forward budget, reporting
//! whether the sets stopped moving.
//!
//! NO `halt_on`, deliberately. The portfolio's slice halts the moment it finds an entry of
//! the class it is hunting, and a halted run answers outright - there is no backward pass
//! and nothing to prune. The case this is about is the run that finds nothing, and that run
//! either settles or spends its budget.
//!
//! ONE MANAGER PER THREAD, per de-fpax, so each group gets its own.
//!
//! ## How to run it
//!
//! ```text
//! RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo settles-within -- \
//!   cargo run --release --example settles_within
//! ```
//!
//! `SPANNING` caps how many multi-conversation groups are tried, `LONE` how many
//! single-conversation ones, `FORWARD_MS` moves the budget being tested.
//!
//! The `sweep` arm runs both arms of the abstraction over a list of budgets:
//!
//! ```text
//! RUN_LOG_DIR=measurements/logs SWEEP_MS=50 tools/run-logged.sh cargo settles-sweep-50 -- \
//!   cargo run --release --example settles_within sweep
//! ```
//!
//! ONE BUDGET PER PROCESS, which `SWEEP_MS` is for. Every group builds a manager sized to
//! the whole allowance and the sweep builds one per group per arm per budget, so the default
//! list in one process runs a machine out of memory part way through and loses the rows it
//! had already printed. `LONE=0` drops the lone column for the heavy budgets, where it has
//! nothing left to say.

use std::collections::BTreeSet;
use std::time::Duration;

use lookahead_engine::bridge::{SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, discover_group, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::live_slots::LiveSlots;
use lookahead_engine::symbolic::reachability::{seed_of, Budget, Reachability};
use lookahead_engine::symbolic::vars::DataVars;

#[path = "../tests/common/mod.rs"]
mod common;

const COUNTER_CAP: i32 = 16;

/// The player's allowance, because that is what the question is asked under.
const BUDGET_MB: usize = 256;

/// The forward slice's budget, from `portfolio::Budget::default`.
const FORWARD_MS: u64 = 50;

/// The budgets the sweep arm tries, in milliseconds.
///
/// The four the 2026-09-07 sweep used, so its table and the abstraction's can be read
/// against each other row for row, and one beyond them: the question de-rn59.6 asks is what
/// the slice SHOULD cost given the abstraction, and an answer that stops at the old ceiling
/// could only ever say "the same".
const SWEEP_MS: [u64; 5] = [50, 100, 250, 1000, 2000];

/// How many groups of each kind to try. The spanning ones are the fifty that matter; the
/// lone ones are sampled because there are 1,372 and they are all the same shape.
const SPANNING: usize = 50;
const LONE: usize = 120;

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    if std::env::args().nth(1).as_deref() == Some("sweep") {
        sweep(&index);
        return;
    }

    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);
    let forward = Duration::from_millis(from_env("FORWARD_MS", FORWARD_MS as usize) as u64);
    let spanning_wanted = from_env("SPANNING", SPANNING);
    let lone_wanted = from_env("LONE", LONE);

    let (spanning, lone) = samples(&index, spanning_wanted, lone_wanted);

    println!(
        "{} MB, forward budget {} ms, {} spanning groups and {} lone ones\n",
        budget.memory() / (1024 * 1024),
        forward.as_millis(),
        spanning.len(),
        lone.len(),
    );

    for (name, sample) in [("spanning", &spanning), ("lone", &lone)] {
        let mut settled = 0usize;
        let mut spent = 0usize;
        let mut skipped = 0usize;
        let mut worst_settled = Duration::ZERO;
        let mut entries_settled = 0usize;
        let mut entries_spent = 0usize;

        for &conversation in sample.iter() {
            match settles(&index, conversation, budget, forward) {
                Some((true, took, entries)) => {
                    settled += 1;
                    entries_settled += entries;
                    worst_settled = worst_settled.max(took);
                }
                Some((false, _, entries)) => {
                    spent += 1;
                    entries_spent += entries;
                }
                None => skipped += 1,
            }
        }

        let tried = settled + spent;
        println!(
            "{name:>10}: {settled} of {tried} settled inside {} ms ({:.0}%), \
             {skipped} skipped",
            forward.as_millis(),
            100.0 * settled as f64 / tried.max(1) as f64,
        );
        println!(
            "{:>10}  settled groups average {:.0} entries, the slowest took {:.1?}",
            "",
            entries_settled as f64 / settled.max(1) as f64,
            worst_settled,
        );
        println!(
            "{:>10}  groups that spent the budget average {:.0} entries\n",
            "",
            entries_spent as f64 / spent.max(1) as f64,
        );
    }

    println!(
        "A group that SETTLES can be pruned with today, at no cost and under the budget \
         already\nshipped - Known refuses to narrow without forward_settled, so the switch \
         is self-guarding.\nRaising the budget is the separate question, and it is spent \
         per start whether or not it\nis claimed."
    );
}

/// The same groups over a sweep of slice budgets, with the dead-slot abstraction off and on.
///
/// ## The question this arm exists for
///
/// de-rn59.6. The fifty milliseconds the slice gets was chosen against the EXACT search, and
/// de-rn59.3 then built an abstraction that changes what a slice can do in that time - the
/// set stored at each entry is quantified over the slots nothing onward reads. On a
/// whole-group fixed point that is a large win: conversation 28 settles abstracted and does
/// not settle exact, at 51x fewer nodes, and 368 settles twenty times faster.
///
/// WHETHER ANY OF THAT REACHES A SLICE is what this asks. Settling is the only mechanism by
/// which a cheaper forward run pays a menu, because only a settled run may narrow the
/// backward passes after it - so the number that matters is not how fast a slice runs but
/// how many groups it finishes.
///
/// ## Why both arms at every budget rather than one arm twice
///
/// Because the two questions the issue asks are the same table read two ways: how many more
/// groups settle at the SHIPPED fifty with the abstraction on, and what budget would be
/// needed to settle the rest. Running the arms apart would make the first a subtraction
/// across two runs on two machine states.
fn sweep(index: &lookahead_engine::index::Index) {
    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);
    let spanning_wanted = from_env("SPANNING", SPANNING);
    let lone_wanted = from_env("LONE", LONE);
    let (spanning, lone) = samples(index, spanning_wanted, lone_wanted);

    println!(
        "{} MB, {} spanning groups and {} lone ones, the dead-slot abstraction off and on\n",
        budget.memory() / (1024 * 1024),
        spanning.len(),
        lone.len(),
    );
    println!(
        "SETTLING IS THE POINT, not speed: only a settled forward run may narrow the \
         backward\npasses after it, so a slice that spends its budget without settling buys \
         nothing but the\nsets it leaves to be met.\n"
    );
    println!(
        "{:>10}  {:>12}  {:>12}  {:>12}  {:>12}",
        "budget", "spanning off", "spanning on", "lone off", "lone on",
    );

    // ONE BUDGET PER PROCESS IS THE WAY TO RUN THIS, and `SWEEP_MS` is how. Each group builds
    // a manager sized to the whole allowance, and the sweep builds one per group per arm per
    // budget - seventeen hundred of them in a default run, which is enough for a machine to
    // run out of memory part way through and lose the rows already printed. A run that has to
    // finish should ask for one budget at a time and keep the logs.
    let wanted: Vec<u64> = match std::env::var("SWEEP_MS") {
        Ok(value) => value.split(',').filter_map(|ms| ms.trim().parse().ok()).collect(),
        Err(_) => SWEEP_MS.to_vec(),
    };

    for milliseconds in wanted {
        let forward = Duration::from_millis(milliseconds);
        let mut cells = Vec::new();
        for sample in [&spanning, &lone] {
            for forget in [false, true] {
                let settled = sample
                    .iter()
                    .filter_map(|&conversation| {
                        settles_forgetting(index, conversation, budget, forward, forget)
                    })
                    .filter(|(settled, _, _)| *settled)
                    .count();
                cells.push((settled, sample.len()));
            }
        }
        // AN EMPTY SAMPLE IS A DASH, not "0 of 0". The heavy budgets are worth running over
        // the spanning groups alone - the lone column saturates at the shipped fifty and has
        // nothing left to say - and a zero there would read as a row where nothing settled.
        let cell = |(settled, tried): (usize, usize)| match tried {
            0 => "-".to_string(),
            _ => format!("{settled} of {tried}"),
        };
        println!(
            "{:>8}ms  {:>12}  {:>12}  {:>12}  {:>12}",
            milliseconds,
            cell(cells[0]),
            cell(cells[1]),
            cell(cells[2]),
            cell(cells[3]),
        );
    }

    println!(
        "\nA GROUP THAT SETTLES ABSTRACTED IS NARROWED LESS THAN ONE THAT SETTLES EXACT, \
         since the\nabstracted set is the larger set. So a column that gains groups has not \
         yet shown a\nsaving: what the narrowing is worth on the groups it newly settles is \
         the comparison\nthis arm does not make.\n"
    );
}

/// One representative per distinct group, split by whether it spans conversations.
///
/// `group_census` establishes that the set is the key and that the split by size is the one
/// that matters. The spanning ones are the fifty in the game; the lone ones are sampled,
/// there being 1,372 of the same shape.
fn samples(
    index: &lookahead_engine::index::Index,
    spanning_wanted: usize,
    lone_wanted: usize,
) -> (Vec<i32>, Vec<i32>) {
    let mut seen: BTreeSet<Vec<i32>> = BTreeSet::new();
    let mut spanning: Vec<i32> = Vec::new();
    let mut lone: Vec<i32> = Vec::new();
    let mut conversations: Vec<i32> = index.keys().copied().collect();
    conversations.sort_unstable();
    for conversation in conversations {
        let group = discover_group(index, conversation);
        if !seen.insert(group.clone()) {
            continue;
        }
        if group.len() > 1 {
            spanning.push(conversation);
        } else {
            lone.push(conversation);
        }
    }
    spanning.truncate(spanning_wanted);
    lone.truncate(lone_wanted);
    (spanning, lone)
}

/// Whether one group's forward run settles, how long it took, and how big it is.
fn settles(
    index: &lookahead_engine::index::Index,
    conversation: i32,
    budget: DiagramBudget,
    forward: Duration,
) -> Option<(bool, Duration, usize)> {
    settles_forgetting(index, conversation, budget, forward, false)
}

/// The same, with the dead-slot abstraction on or off.
fn settles_forgetting(
    index: &lookahead_engine::index::Index,
    conversation: i32,
    budget: DiagramBudget,
    forward: Duration,
    forget: bool,
) -> Option<(bool, Duration, usize)> {
    let (graph, _) = build_group_graph(index, conversation).ok()?;
    let start = DialogueNodeId::new(conversation, 0);
    graph.get(start)?;
    let entries = graph.count();

    isolated::on_its_own_thread(|| {
        let symbols = graph.symbols().clone();
        let world = SnapshotWorld::declaring(
            WorldSnapshot { day_minutes: 720, day_counter: 1, ..Default::default() },
            None,
        );
        let layout = DataLayout::for_group(&graph, &world, COUNTER_CAP);
        let vars = DataVars::try_new(&layout, &symbols, budget)?;
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(&graph));
        let seed = seed_of(&graph, &world, &vars).expect("room for a seed");

        // BUILT INSIDE THE THREAD, like everything else here: the liveness is a fact about
        // the group rather than about a world or a manager, but building it out here would
        // put its cost in one arm's timings and not the other's.
        let forget_dead = forget.then(|| std::sync::Arc::new(LiveSlots::of(&graph)));

        let began = std::time::Instant::now();
        let found = Reachability::explore_within(
            &graph,
            start,
            &seed,
            &mut compiler,
            &world,
            COUNTER_CAP as u32,
            &Budget { time: forward, forget_dead, ..Default::default() },
        );

        Some((found.stats().reached_fixed_point, began.elapsed(), entries))
    })
}

fn from_env(name: &str, fallback: usize) -> usize {
    std::env::var(name).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(fallback)
}
