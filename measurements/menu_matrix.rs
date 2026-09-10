// SPDX-License-Identifier: MIT
//! What a whole MENU costs, over every group in the game.
//!
//! ## Why a menu and not a row of the option matrix
//!
//! `measurements/performance_matrix.rs` measures one search from one start. A request is a
//! whole response menu: `bridge::answer_starts` runs every option against ONE manager and
//! ONE compiler, three options in the ordinary case and twenty-four when every option is a
//! rolled check. So a menu is not the sum of its options - the second option answers against
//! a store the first one filled, and the twenty-fourth against one that has seen
//! twenty-three options' worth of subproblems.
//!
//! THAT IS WHAT A PLAYER WAITS FOR. An option-level total can move while a menu's does not,
//! and the other way round, so the two readings are separate baselines rather than one
//! reading at two scales. de-dt75.1 turned on exactly this distinction and said the per-menu
//! number overrides the per-option one where they disagree.
//!
//! ## What it does
//!
//! One group per row. Builds the adversarial profile - `menu_profile`, shared with
//! `menu_residue` and `menu_wall` - takes its starts as the menu, and answers every one of
//! them through `answer::best_novelty` against one manager, exactly as the bridge does.
//!
//! THE GATE THE GAME APPLIES IS APPLIED HERE, so an option the mod would refuse before
//! touching a diagram is refused here too: `graph.best_linked_class` decides, which is what
//! `bridge::class_worth_hunting` calls. Counting those as searches would fill a row with
//! free options and make a heavy group read as a light one.
//!
//! ONE MANAGER PER MENU, ON A THREAD OF ITS OWN. Building a second manager on a thread that
//! has already built one is what overflows a stack (de-fpax), and the menu's own warmth is
//! the thing this measurement exists to capture, so the manager is built inside and dropped
//! with the thread.
//!
//! ## What the columns say
//!
//! `menu ms` is the whole thing, setup included, because that is what a request costs.
//! `setup ms` is how much of it was building the layout, the manager, the compiled guards
//! and the seed rather than searching, so the searching is the difference.
//!
//! `options` is how many of the profile's starts were actually searched, out of how many the
//! profile offered; the difference is what the gate refused. `asked` is candidates asked
//! about across the menu, one fixed point each.
//!
//! `settled`, `at start` and `partly` split the options by how they were answered - see
//! `answer::Answered`. A menu that is mostly `partly` is one where the budget bound, and its
//! milliseconds are a floor rather than a cost.
//!
//! `nodes` is what the manager holds when the menu ends, which is the number a parallel
//! split has to clear a group against.
//!
//! ## What it said, 2026-09-09: the whole game, and two menus that are slow
//!
//! 395 menus of eight options, at the player's own 256 MB and the shipped budget, one group
//! per process on a quiet machine. 126 of the 521 measurable groups have no menu at all -
//! no start of theirs has anything worth hunting beyond it.
//!
//! ```text
//!   median 14 ms   p90 48   p99 345   max 3084
//!   total 16.2 s over 395 menus, mean 41 ms
//!
//!   over   250 ms:   5 of 395  (1.3%)
//!   over   500 ms:   3 of 395  (0.8%)
//!   over  1000 ms:   2 of 395  (0.5%)
//! ```
//!
//! 2,970 options and 5,377 candidates asked, and EVERY OPTION SETTLED: none was answered at
//! the start without searching, and none came back a bound. So no menu here is a menu whose
//! budget bound, and every millisecond below is work actually done.
//!
//! ### The two menus over a second, which is what this measurement is for
//!
//! ```text
//!     conv  entries  options  menu ms  setup  asked        nodes
//!      761     3975        8     3084     33     26    1,198,484
//!      368     4724        8     2277     26     56      224,750
//!       14     3594        8      819     27     48       98,158
//! ```
//!
//! `LookAheadTimeBudgetMs` BOUNDS ONE OPTION AND A REQUEST IS A WHOLE MENU. Every option in
//! those rows finished inside its own budget - that is what a `partly` of zero says - so
//! nothing here is the wall failing to hold. Eight options that each behave are still eight
//! options, and on 761 they add to three seconds.
//!
//! That is the question `measurements/menu_wall.rs` asks, and this is the first whole-game
//! answer to it: TWO GROUPS, NAMED, out of 395. A per-option reading cannot produce that
//! list - 761's worst option is well inside its budget - which is the whole reason this
//! measurement exists beside the option matrix rather than instead of it.
//!
//! 761 is also the only row anywhere near the manager, at 1.2 million diagram nodes on the
//! player's 256 MB.
//!
//! ### What a menu costs is mostly not setup
//!
//! 5.6 of the 16.2 seconds, so about a third - and on the slow rows far less, 33 ms of
//! 3,084. That is the opposite way round from the option matrix, where setup is eighty-nine
//! per cent of the time, and the difference is the point: a menu amortises one setup over
//! eight options where a matrix row pays it for one.
//!
//! ## How to run it
//!
//! One group per process, because a manager that runs out of nodes takes the process with
//! it and a crash on one group should not cost the rest:
//!
//! ```text
//! DEGCT_CONVERSATION=631 \
//!   tools/run-logged.sh cargo menu-matrix -- cargo run --release --example menu_matrix
//! ```
//!
//! `DEGCT_HEADER=1` prints the column names and measures nothing, which is how a driver
//! writing one file out of many processes gets a header without parsing a row.
//!
//! `DEGCT_STARTS` sets the menu's width, `DEGCT_UNSEEN` how many of the deepest entries are
//! unread, and `DEGCT_BUDGET_MB` what the manager is given.

use std::time::{Duration, Instant};

use lookahead_engine::bridge::{SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::answer;
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::known::GroupShape;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

/// The columns, written down here and nowhere else.
///
/// A DRIVER ASKS FOR THESE rather than parsing them off a row, so that a file assembled from
/// many processes cannot get a header that disagrees with its rows.
const COLUMNS: [&str; 11] = [
    "conv", "entries", "options", "offered", "menu_ms", "setup_ms", "asked", "settled", "at_start",
    "partly", "nodes",
];

/// The groups to measure when nothing is named: the heavy list the matrix has always meant.
const CONVERSATIONS: [i32; 6] = [362, 368, 631, 14, 28, 1030];

/// What a player's response menu is allowed.
///
/// THE PLAYER'S OWN 256 MB, not a measurement's six gigabytes, because the question is what a
/// menu costs and a menu is answered under the shipped allowance.
const BUDGET_MB: usize = 256;

/// How many options the menu asks about.
///
/// EIGHT, which is what `workspace_menus` uses, so a figure here is comparable with one
/// there. A wider menu is measurable with `DEGCT_STARTS`; twenty-four is what a menu of
/// rolled checks costs, since de-fes makes each outcome its own start.
const STARTS: usize = 8;

/// How many of the group's deepest entries are unread.
const UNSEEN: usize = 10;

const COUNTER_CAP: i32 = 16;

/// A verdict for a row that could not be measured, which is not a slow row.
const NOT_MEASURED: &str = "NOT-MEASURED";

/// A verdict for a group with no menu to ask about, which is not an empty one.
const NO_MENU: &str = "NO-MENU";

/// What one menu cost.
#[derive(Default)]
struct Menu {
    took: Duration,
    setup: Duration,
    options: usize,
    asked: usize,
    settled: usize,
    at_start: usize,
    partly: usize,
    nodes: usize,
}

fn main() {
    if lookahead_engine::core::env::is_set("HEADER") {
        println!("{}", COLUMNS.join("\t"));
        return;
    }

    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);
    let starts_wanted = from_env("STARTS", STARTS);
    let unseen_wanted = from_env("UNSEEN", UNSEEN);

    for conversation in numbers("CONVERSATION", &CONVERSATIONS) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            eprintln!("conversation {conversation}: no group builds from it; skipping.");
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            eprintln!("conversation {conversation}: no entry 0; skipping.");
            continue;
        }

        // THROUGH `MenuProfile`, for the reason it exists: a menu whose starts have nothing
        // better beyond them is refused before a diagram is touched, and the whole row reads
        // as a fast engine while measuring nothing.
        let Some(profile) = MenuProfile::of(&graph, root, unseen_wanted, starts_wanted) else {
            row(conversation, &graph, 0, NO_MENU, None);
            continue;
        };

        let novelty = profile.novelty();
        match menu(&graph, &profile.starts, &novelty, budget) {
            Some(measured) => row(
                conversation,
                &graph,
                profile.starts.len(),
                "",
                Some(&measured),
            ),
            // THE MACHINE COULD NOT SUPPLY THE BUDGET, which is not a finding about the
            // menu. Loud, and a different word from a slow row, so a folder holding one is
            // not read as a measurement.
            None => row(
                conversation,
                &graph,
                profile.starts.len(),
                NOT_MEASURED,
                None,
            ),
        }
    }
}

/// One menu: every option answered against one manager, warmed by the menu itself.
fn menu<F>(
    graph: &LookAheadGraph,
    starts: &[DialogueNodeId],
    novelty: &F,
    budget: DiagramBudget,
) -> Option<Menu>
where
    // SYNC, because the search runs on a thread of its own - de-fpax - and the closure
    // carried over is a shared reference to this one.
    F: Fn(DialogueNodeId) -> Novelty + Sync,
{
    isolated::on_its_own_thread(|| {
        let began = Instant::now();
        let symbols = graph.symbols().clone();
        let world = SnapshotWorld::declaring(
            WorldSnapshot {
                day_minutes: 720,
                day_counter: 1,
                ..Default::default()
            },
            None,
        );
        let layout = DataLayout::for_group(graph, &world, COUNTER_CAP);
        let vars = DataVars::try_new(&layout, &symbols, budget)?;
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
        let seed = seed_of(graph, &world, &vars).expect("room for a seed");
        // WORKED OUT ONCE FOR THE MENU, which is what the bridge does: the parent map and
        // the order are facts about the links, so a menu of eight options builds them once
        // rather than eight times. See `GroupShape::of`.
        let shape = GroupShape::of(graph);
        let setup = began.elapsed();

        // WHAT THE PLUGIN ASKS FOR, taken from the product rather than restated here, so a
        // change to the shipped budget moves this row with it.
        let search = lookahead_engine::bridge::LookAheadRequest {
            memory_budget_mb: budget.memory() / (1024 * 1024),
            ..Default::default()
        }
        .search_budget();

        let mut counted = Menu {
            setup,
            ..Default::default()
        };

        for &start in starts {
            // THE GATE THE GAME APPLIES, so an option the mod would not search is not
            // searched here either - `bridge::class_worth_hunting` refuses when nothing
            // link-reachable beats where the option already lands.
            let Some(hunting) = graph.best_linked_class(start, novelty) else {
                continue;
            };
            if hunting <= Novelty::SeenThisGame {
                continue;
            }

            let found = answer::best_novelty(
                graph,
                start,
                StartBranch::Either,
                &seed,
                &mut compiler,
                &world,
                COUNTER_CAP as u32,
                novelty,
                hunting,
                &search,
                &shape,
                None,
            );

            counted.options += 1;
            counted.asked += found.targets_asked;
            match found.by {
                answer::Answered::Backwards => counted.settled += 1,
                answer::Answered::AtTheStart => counted.at_start += 1,
                answer::Answered::Partly => counted.partly += 1,
            }
        }

        counted.took = began.elapsed();
        counted.nodes = vars.node_count();
        Some(counted)
    })
}

/// One row, as a tab-separated line.
///
/// `why` is a word for a row that was not measured, and empty for one that was. It goes in
/// the `menu_ms` column rather than in a column of its own, so a row that says nothing says
/// so where a reader is already looking.
fn row(
    conversation: i32,
    graph: &LookAheadGraph,
    offered: usize,
    why: &str,
    measured: Option<&Menu>,
) {
    let cells: Vec<String> = match measured {
        Some(m) => vec![
            conversation.to_string(),
            graph.count().to_string(),
            m.options.to_string(),
            offered.to_string(),
            format!("{:.0}", ms(m.took)),
            format!("{:.0}", ms(m.setup)),
            m.asked.to_string(),
            m.settled.to_string(),
            m.at_start.to_string(),
            m.partly.to_string(),
            m.nodes.to_string(),
        ],
        None => {
            let mut cells = vec![
                conversation.to_string(),
                graph.count().to_string(),
                "0".to_string(),
                offered.to_string(),
                why.to_string(),
            ];
            cells.extend(COLUMNS.iter().skip(5).map(|_| "?".to_string()));
            cells
        }
    };
    println!("{}", cells.join("\t"));
}

fn ms(took: Duration) -> f64 {
    took.as_secs_f64() * 1000.0
}

fn from_env(name: &str, fallback: usize) -> usize {
    lookahead_engine::core::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(fallback)
}

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
