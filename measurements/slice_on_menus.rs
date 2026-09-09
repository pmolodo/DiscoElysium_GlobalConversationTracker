// SPDX-License-Identifier: MIT
//! What the forward slice is worth over a whole MENU, rather than over one start.
//!
//! ## Why the matrix cannot answer this
//!
//! de-dt75.1, part three, and the part its own notes say OVERRIDES the other two. A matrix row
//! is one search from one start. A request is a whole response menu: `bridge::answer_starts`
//! runs every option against ONE manager and ONE compiler, three options in the ordinary case
//! and twenty-four when every option is a rolled check. Turning the slice off changes what
//! every option costs, and the options are not independent - they share a manager, so what one
//! leaves in the node store the next inherits.
//!
//! So a matrix that says the slice is worth nothing per row can be right and still be the
//! wrong basis for the switch. This measures the menu.
//!
//! ## What the two arms are
//!
//! ONE FIELD APART, exactly as `prune_on_menus` does it: `portfolio::Budget::forwards` at its
//! shipped fifty milliseconds, and the same budget with it at zero. Everything else - the
//! manager, the layout, the compiled guards, the seed, the group shape, the world - is built
//! the same way in both, and each arm gets its own so that neither inherits the other's warm
//! store.
//!
//! ONE MANAGER PER ARM, WARMED BY THE MENU ITSELF. That is the whole point: the second option
//! of a menu answers against a store the first one filled, which is the thing a per-row
//! measurement cannot show.
//!
//! ## What to read
//!
//! `answered` is how many of the menu's options the slice answered outright, which is the
//! benefit it is there for. `on ms` against `off ms` is what that cost or saved over the whole
//! menu. A group where the slice answers most of the menu and still reads slower is a group
//! where it is not paying its way.
//!
//! ## How to run it
//!
//! ```text
//! DEGCT_RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo slice-on-menus -- \
//!   cargo run --release --example slice_on_menus
//! ```
//!
//! `DEGCT_CONVERSATION=631,368` picks the groups, `DEGCT_STARTS=24` the menu's width and
//! `DEGCT_UNSEEN=10` how many of the deepest entries are unread.

use std::time::{Duration, Instant};

use lookahead_engine::bridge::{SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::known::GroupShape;
use lookahead_engine::symbolic::portfolio;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

/// The heaviest groups, which is where a menu costs anything at all.
const CONVERSATIONS: [i32; 6] = [362, 368, 631, 14, 28, 1030];

/// What a player's response menu is allowed.
const BUDGET_MB: usize = 256;

/// How many options the menu asks about.
///
/// EIGHT, which is what `workspace_menus` uses, so a figure here is comparable with one there.
/// A wider menu is measurable with DEGCT_STARTS.
const STARTS: usize = 8;

/// How many of the group's deepest entries are unread.
const UNSEEN: usize = 10;

const COUNTER_CAP: i32 = 16;

/// What one arm of one menu cost, and how much of it the slice answered.
struct Arm {
    took: Duration,
    answered: usize,
    asked: usize,
}

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);
    let starts_wanted = from_env("STARTS", STARTS);
    let unseen_wanted = from_env("UNSEEN", UNSEEN);

    println!(
        "{} MB, {starts_wanted} starts a menu, {unseen_wanted} deepest unseen, \
         slice {} ms when on\n",
        budget.memory() / (1024 * 1024),
        portfolio::Budget::default().forwards.as_millis(),
    );
    println!(
        "{:>6}  {:>8}  {:>7}  {:>9}  {:>9}  {:>9}  {:>10}  {:>10}",
        "conv", "entries", "starts", "on ms", "off ms", "saving", "answered", "asked on/off",
    );

    let mut total_on = Duration::ZERO;
    let mut total_off = Duration::ZERO;
    let mut menus = 0;

    for conversation in numbers("CONVERSATION", &CONVERSATIONS) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            continue;
        }

        // THROUGH `MenuProfile`, for the reason it exists: a menu whose starts have nothing
        // better beyond them is refused before a diagram is touched, and the whole thing reads
        // as a fast engine while measuring nothing.
        let Some(profile) = MenuProfile::of(&graph, root, unseen_wanted, starts_wanted) else {
            eprintln!("conversation {conversation}: every start would be refused; skipping.");
            continue;
        };
        let starts = &profile.starts;
        let novelty = profile.novelty();

        let on = menu(&graph, starts, &novelty, budget, true);
        let off = menu(&graph, starts, &novelty, budget, false);

        let (Some(on), Some(off)) = (on, off) else {
            eprintln!("conversation {conversation}: no room for the manager; skipping.");
            continue;
        };

        total_on += on.took;
        total_off += off.took;
        menus += 1;

        println!(
            "{conversation:>6}  {:>8}  {:>7}  {:>9.0}  {:>9.0}  {:>9.0}  {:>7}/{:<2}  {:>4}/{:<5}",
            graph.count(),
            starts.len(),
            ms(on.took),
            ms(off.took),
            ms(off.took) - ms(on.took),
            on.answered,
            starts.len(),
            on.asked,
            off.asked,
        );
    }

    if menus == 0 {
        eprintln!("no menu measured.");
        return;
    }

    println!(
        "\n{menus} menu(s): {:.0} ms with the slice, {:.0} ms without, a {} of {:.0} ms.",
        ms(total_on),
        ms(total_off),
        if total_off > total_on {
            "saving"
        } else {
            "cost"
        },
        (ms(total_off) - ms(total_on)).abs(),
    );
    println!(
        "\nREAD THIS AGAINST THE PER-ROW ANSWER, which is what tools/matrix-slice-worth.py \
         gives\nfrom a matrix folder. de-dt75.1 says the per-MENU number overrides the per-row \
         one where\nthey disagree: a player waits for a menu, not for a start."
    );
}

/// One arm of one menu: every option answered against one manager, warmed by the menu itself.
fn menu<F>(
    graph: &LookAheadGraph,
    starts: &[DialogueNodeId],
    novelty: &F,
    budget: DiagramBudget,
    slice_on: bool,
) -> Option<Arm>
where
    // SYNC, because the search runs on a thread of its own - de-fpax - and the closure
    // carried over is a shared reference to this one.
    F: Fn(DialogueNodeId) -> Novelty + Sync,
{
    isolated::on_its_own_thread(|| {
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
        let shape = GroupShape::of(graph);

        // THE ONE FIELD THAT DIFFERS. Zero is `Engine::BackwardInGame`'s own setting in the
        // matrix, so the two arms here are the two columns there - measured over a menu
        // instead of over a start.
        let search = portfolio::Budget {
            forwards: if slice_on {
                portfolio::Budget::default().forwards
            } else {
                Duration::ZERO
            },
            ..Default::default()
        };

        let mut answered = 0;
        let mut asked = 0;
        let began = Instant::now();
        for &start in starts {
            // THE GATE THE GAME APPLIES, so an option the mod would not search is not searched
            // here either - `bridge::scored` refuses when nothing link-reachable beats the
            // baseline. Counting those would put the same free rows in both arms and dilute
            // the difference.
            let Some(hunting) = graph.best_linked_class(start, novelty) else {
                continue;
            };
            if hunting <= Novelty::SeenThisGame {
                continue;
            }
            let answer = portfolio::best_novelty(
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
            if answer.by == portfolio::Answered::Forwards {
                answered += 1;
            }
            asked += answer.targets_asked;
        }

        Some(Arm {
            took: began.elapsed(),
            answered,
            asked,
        })
    })
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
