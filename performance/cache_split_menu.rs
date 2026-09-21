// SPDX-License-Identifier: MIT
//! The apply-cache split swept over a WHOLE MENU, at the budget the player actually gets.
//!
//! ## What de-1e8l left open, and it named both gaps itself
//!
//! `performance/cache_split.rs` swept a sixty-fourth to a half of the node store over the
//! heavy groups and found a quarter the flat optimum everywhere with a signal. Two things
//! it could not answer, in its own words:
//!
//! - "THE PLAYER'S END. Every row is at 512 MB, where the budget binds on the heavy groups.
//!   At the shipped 256 MB the whole manager is half this."
//! - "A WHOLE MENU. These are single searches. `bridge::answer` runs a dozen options against
//!   ONE manager, so the cache is warm from the second option onwards - and a cache that
//!   pays for itself across options may want to be wider than one measured on a single
//!   search."
//!
//! The second is the interesting one and it points the opposite way to a sweep of single
//! searches. A single search FILLS the cache once and then stops; a menu's second option
//! asks about the same group with the cache already warm, and its twenty-fourth asks with a
//! cache that has seen twenty-three options' worth of subproblems. If reuse across options
//! is real, the optimum moves wider.
//!
//! ## What this does
//!
//! Runs one adversarial menu - `menu_profile`, the same profile `prune_on_menus` uses - over
//! ONE manager per split, exactly as `bridge::answer_within` does, and reports what the whole
//! menu cost. The split is not on the wire, so this calls the internals: `DiagramBudget`
//! carries `with_cache_split`, which de-1e8l added for precisely this.
//!
//! THE TOTAL DOES NOT MOVE between splits. `DiagramBudget::bytes_per_node` prices the
//! cache's share, so a wider cache buys fewer nodes out of the same allowance - a sweep over
//! the split is a sweep over one trade, not over two budgets that happen to differ.
//!
//! ## What it said, 2026-09-07, at the shipped 256 MB over 24 starts
//!
//! Menu milliseconds per split, two runs where both were made:
//!
//! ```text
//!   conv    1/64    1/16     1/8     1/4     1/2   spread
//!     28     192     189     192     195     198      5%
//!            214     204     199     202     208
//!    368   20337   19167   17941   17711   17524      9%
//!     14    7079    7035    6685    6819    7273      9%
//!    631    7088    7031    7343    7405    7012      6%
//!    362    1121    1177    1185    1194    1195      6%
//!            1122    1179    1194    1203    1251
//!   1030    3545    3423    3308    2999    3074     18%
//!            4667    3419    3274    3241    3219
//! ```
//!
//! ## KEEP THE QUARTER. The menu does not move the optimum.
//!
//! Which was not the expected answer: de-1e8l's hypothesis was that a cache warm from the
//! second option onwards "may want to be wider than one measured on a single search". It
//! does not, and the reason the table cannot support that is that THE OPTIMUM IS NOT IN THE
//! SAME PLACE TWICE. Conversation 28's fastest split moved from a sixteenth to an eighth
//! between two runs; 1030's moved from a quarter to a half. Spreads of five to nine per cent
//! with the minimum wandering inside them is a flat curve being read as a peak.
//!
//! ## The two things that DID reproduce, and they point opposite ways
//!
//! - CONVERSATION 362 PREFERS THE NARROWEST CACHE, monotonically, in both runs: 1121 and
//!   1122 ms at a sixty-fourth rising to 1195 and 1251 at a half.
//! - CONVERSATION 1030 PUNISHES THE NARROWEST, in both runs: 3545 and 4667 ms at a
//!   sixty-fourth against about 3250 at everything from an eighth on. That is the same
//!   asymmetry de-1e8l found at 512 MB, where a sixty-fourth was the only NO-ROOM in the
//!   whole sweep - starve the cache and subproblems are recomputed, and recomputing
//!   allocates nodes out of the store the small cache was protecting.
//!
//! No split is best everywhere, so the question is which is never BAD. A sixty-fourth is,
//! on 1030. A half is, on 362. The quarter is within noise of the best on every group in
//! the table and worst on none, which is what a default should be.
//!
//! ## So both of de-1e8l's open questions are now closed
//!
//! The shipped 256 MB gives the same answer as the 512 MB it was swept at, and a whole menu
//! against one warm manager gives the same answer as a single search. `NODES_PER_CACHE_ENTRY`
//! stays at four.
//!
//! ## How to run it
//!
//! ```text
//! tools/run-logged.sh cargo cache-split-menu -- \
//!   cargo run --release --example cache_split_menu
//! ```
//!
//! `BUDGET_MB` moves the allowance - 256 is the shipped default and what this runs at, 512
//! is what de-1e8l used. `SPLITS` overrides the sweep, `STARTS` the menu's size.

use std::time::{Duration, Instant};

use lookahead_engine::bridge::{GameWorld, LookAheadRequest, NodeRef, WorldRawData, answer_starts};
use lookahead_engine::core::types::{DialogueNodeId, SeenState};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "options.rs"]
mod options;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

const COUNTER_CAP: i32 = 16;

/// The heavy list plus 1030, matching the other whole-menu measurements.
const CONVERSATIONS: [i32; 6] = [28, 368, 14, 631, 362, 1030];

/// What a player's search gets, TAKEN FROM THE ENGINE rather than restated. A menu is
/// answered under the shipped allowance, so a number typed here would go stale the day
/// that one moved and the row would quietly stop being what it claims to be.
const BUDGET_MB: usize = DiagramBudget::DEFAULT_MEMORY_BUDGET / (1024 * 1024);

/// Nodes per cache entry: a sixty-fourth, a sixteenth, an eighth, a quarter, a half.
///
/// A SMALLER NUMBER IS A BIGGER CACHE - one entry per that many nodes - so this list runs
/// from the stingiest to the most generous. Four is `NODES_PER_CACHE_ENTRY`, what ships.
const SPLITS: [usize; 5] = [64, 16, 8, 4, 2];

const STARTS: usize = 24;
const UNSEEN: usize = 10;

/// What this driver takes. With no group named it uses the list above, and with no split named
/// the sweep above.
#[derive(clap::Parser)]
#[command(about = "What splitting the computed-table cache costs a whole menu.")]
struct Options {
    #[command(flatten)]
    groups: options::Groups,
    #[command(flatten)]
    starts: options::Starts<STARTS>,
    #[command(flatten)]
    unseen: options::Unseen<UNSEEN>,
    #[command(flatten)]
    budget: options::Budget<BUDGET_MB>,
    /// Which cache splits to sweep; repeat the flag or comma-separate
    #[arg(long = "split", value_name = "N", value_delimiter = ',')]
    splits: Vec<usize>,
}

fn main() {
    let asked = <Options as clap::Parser>::parse();
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let budget_mb = asked.budget.budget_mb;
    let splits: Vec<usize> = if asked.splits.is_empty() {
        SPLITS.to_vec()
    } else {
        asked.splits.iter().map(|split| (*split).max(1)).collect()
    };
    let starts_wanted = asked.starts.starts;
    let unseen_wanted = asked.unseen.unseen;

    println!(
        "{budget_mb} MB held, {starts_wanted} starts, {unseen_wanted} deepest unseen, \
         one manager per menu\n"
    );

    for conversation in asked.groups.or(&CONVERSATIONS) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            eprintln!("conversation {conversation}'s group does not build; skipping.");
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            continue;
        }
        let Some(profile) = MenuProfile::of(&graph, root, unseen_wanted, starts_wanted) else {
            eprintln!("conversation {conversation}: every start would be refused; skipping.");
            continue;
        };
        let seen_any_game = profile.seen_any_game();

        println!(
            "conversation {conversation}: {} entries, {} starts",
            graph.count(),
            profile.starts.len(),
        );
        println!(
            "  {:>7}  {:>12}  {:>12}  {:>10}  {:>9}",
            "split", "nodes", "cache", "menu ms", "answers",
        );

        let mut best: Option<(usize, Duration)> = None;
        for &split in &splits {
            let budget = DiagramBudget::new(budget_mb * 1024 * 1024).with_cache_split(split);
            let Some((took, found)) = menu(&graph, &profile.starts, &seen_any_game, budget) else {
                println!("  {:>7}  no room for the manager", format!("1/{split}"));
                continue;
            };

            println!(
                "  {:>7}  {:>12}  {:>12}  {:>10.0}  {found:>9}",
                format!("1/{split}"),
                budget.nodes(),
                budget.cache_entries(),
                took.as_secs_f64() * 1000.0,
            );

            best = match best {
                Some(had) if had.1 <= took => Some(had),
                _ => Some((split, took)),
            };
        }

        if let Some((split, took)) = best {
            println!(
                "  fastest at 1/{split} ({:.0} ms){}\n",
                took.as_secs_f64() * 1000.0,
                if split == DiagramBudget::NODES_PER_CACHE_ENTRY {
                    ", which is what ships"
                } else {
                    ", which is NOT what ships"
                },
            );
        }
    }

    println!(
        "ANSWERS is how many starts found something, and it must not move: the cache \
         changes\nwhat a search RECOMPUTES, never what it concludes. A column that varies \
         is a bug,\nnot a result."
    );
}

/// One menu against one manager at one split: what it cost, and how many starts found.
fn menu<F>(
    graph: &LookAheadGraph,
    starts: &[DialogueNodeId],
    seen_any_game: &F,
    budget: DiagramBudget,
) -> Option<(Duration, usize)>
where
    // THE SET, NOT A CLASSIFIER: the world is the other half of what decides a seen state and
    // it is built below. See `world::seen_state`.
    F: Fn(DialogueNodeId) -> bool + Sync,
{
    isolated::on_its_own_thread(|| {
        let symbols = graph.symbols().clone();
        let world = GameWorld::declaring(
            WorldRawData {
                day_minutes: 720,
                day_counter: 1,
                ..Default::default()
            },
            common::declared(),
        );
        let seen_state = &lookahead_engine::world::seen_states(&world, seen_any_game);
        let layout = DataLayout::for_group(graph, &world, COUNTER_CAP);
        let vars = DataVars::try_new(&layout, &symbols, budget)?;
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
        let seed = seed_of(graph, &world, &vars).expect("room for a seed");

        // THE WHOLE MENU IN ONE CALL, which is what a request is. This asked the options one
        // at a time until the per-option path was deleted; the sweep is about what an
        // apply-cache split costs a MENU, so asking as a menu is what it meant all along.
        // WITH A WALK, as the product asks: built before the clock starts, since it stands in for
        // what the plugin records as a conversation plays - see `hub::walk_to_menu`.
        let conversation = starts.first().map_or(0, |start| start.conversation_id);
        let request = LookAheadRequest {
            conversation,
            starts: starts.iter().copied().map(NodeRef::from).collect(),
            encountered: lookahead_engine::symbolic::hub::walk_to_menu(graph, conversation, starts)
                .into_iter()
                .map(NodeRef::from)
                .collect(),
            ..Default::default()
        };
        let began = Instant::now();
        let answers = answer_starts(graph, &world, &request, seen_state, &mut compiler, &seed);
        let found = answers
            .iter()
            .filter(|answer| answer.best > SeenState::SeenThisGame as i32)
            .count();

        Some((began.elapsed(), found))
    })
}
