// SPDX-License-Identifier: MIT
//! Does an option reach unread content WITHOUT coming back through the menu?
//!
//! The marking asks how far away the nearest unread line is, and proving an exact distance is
//! what costs: twenty-four layers and 64 million diagram nodes on conversation 761, against a
//! worklist pass that meets in under half a second on the same group. de-0jsf.17 asks whether
//! a coarser question is worth as much to a player and costs almost nothing.
//!
//! THE QUESTION HERE IS BINARY. Cut every other option of the menu, then ask plain
//! reachability: can this option still reach anything unread? A route that had to return
//! through a sibling option cannot survive the cut, so a yes means the option leads ONWARD
//! and a no means it only ever gets there by looping back through the menu first.
//!
//! That is the distinction de-0jsf.17 says a player would actually notice - 761's ten
//! candidates sit at distances 23 to 25, so the exact answer is separating options nobody
//! could tell apart - and it is asked with the fixed point this engine is fastest at.
//!
//! ## What it prints
//!
//! One row per option: whether it reaches unread content with its siblings cut, whether it
//! reaches any with them left in, and what each cost. The `onward` column is the proposed
//! marker; `at all` is what a reachability-only marking would have said.
//!
//! THE ROW THAT DECIDES THE IDEA is the count at the bottom. A marker that says the same
//! thing about every option in a menu is worth nothing - that is what the marking before
//! de-0jsf.7 did, and why the nearest rule was introduced at all.
//!
//! ## What it said, 2026-09-11: it separates, and it does it at the player's budget
//!
//! The seven heavy groups, eight options, ten unread, THE PLAYER'S OWN 256 MB:
//!
//! ```text
//!   conv   lead onward   reach anything   ms per option
//!    761        4 of 8           5 of 8       97 - 1,028
//!    631        3 of 8           7 of 8          23 - 57
//!    640        5 of 8           7 of 8          19 - 43
//!     16        3 of 8           7 of 8          10 - 23
//!    368        0 of 8           0 of 8          33 - 88
//!     14        0 of 8           0 of 8          19 - 33
//!   1030        0 of 8           0 of 8          12 - 50
//! ```
//!
//! 761's EIGHT OPTIONS TOGETHER COST ABOUT 1.9 SECONDS AT 256 MB, where the exact distance
//! cannot be answered at 256 MB at all and costs 10.8 seconds at six gigabytes.
//!
//! AND IT CARRIES REAL INFORMATION over plain reachability, which is the other half of being
//! worth having. On 631 a reachability marker lights seven options of eight - the
//! uselessness the nearest rule was introduced to fix - where this lights three. On 16 it is
//! seven against three, on 640 seven against five. The three groups that mark nothing mark
//! nothing either way, which matches their zero rounds under the exact search.
//!
//! ## Against the exact marking: 22 of 32 agree, and the other 10 are not mistakes
//!
//! ```text
//!   conv   agree   marked but not onward   onward but not marked
//!    761     6/8                       1                       1
//!    631     5/8                       3                       0
//!    640     6/8                       2                       0
//!     16     5/8                       1                       2
//! ```
//!
//! THE TWO ANSWER DIFFERENT QUESTIONS AND ARE MEANT TO. "Marked" is competitive and relative:
//! the greedy marks at most one option a round, so an option is marked because no rival
//! reached that line sooner. "Onward" is absolute: this option gets there without returning
//! through the menu. Neither implies the other, and both directions actually occur.
//!
//! 16:918 IS MARKED AT DISTANCE 9 AND IS NOT ONWARD - it is the nearest way to a line, and it
//! still has to loop back to get there. 761:848 is onward and unmarked - it leads somewhere
//! new directly, and some rival happened to reach that line first. Which of those a player
//! wants told is the whole question de-0jsf.17 was opened on, and the answer there is the
//! second: eliminate the options that make you come back when others do not.
//!
//! ONWARD IS THE MORE SELECTIVE OF THE TWO on the groups where it matters - 3 against 6 on
//! 631, 5 against 7 on 640 - and it never marked nothing where the exact marking marked
//! something. Both of those are the right way round for a marker meant to point somewhere
//! rather than to light up.
//!
//! ## How to run it
//!
//! `DEGCT_COMPARE=1` also runs the exact marking in the same manager and prints which options
//! it marked, which is the check that says whether the cheap answer is the RIGHT answer: a
//! marker that separates the options but separates the wrong ones is worse than none.
//!
//! ```text
//! DEGCT_CONVERSATION=761 \
//!   tools/run-logged.sh cargo onward -- cargo run --release --example onward_or_back
//! ```
//!
//! `DEGCT_STARTS`, `DEGCT_UNSEEN` and `DEGCT_BUDGET_MB` mean what they mean in `menu_matrix`.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use lookahead_engine::bridge::{SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::backward::{Backward, Budget as PassBudget};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::known::GroupShape;
use lookahead_engine::symbolic::menu;
use lookahead_engine::symbolic::novelty_search::Where;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

const CONVERSATIONS: [i32; 7] = [761, 631, 640, 368, 14, 1030, 16];
const BUDGET_MB: usize = 256;
const STARTS: usize = 8;
const UNSEEN: usize = 10;
const COUNTER_CAP: i32 = 16;
const EACH_MS: u64 = 60_000;

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");
    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);
    for conversation in numbers("CONVERSATION", &CONVERSATIONS) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            eprintln!("conversation {conversation}: no group builds from it; skipping.");
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        let Some(profile) = MenuProfile::of(
            &graph,
            root,
            from_env("UNSEEN", UNSEEN),
            from_env("STARTS", STARTS),
        ) else {
            println!("conversation {conversation}: no menu");
            continue;
        };
        isolated::on_its_own_thread(|| {
            ask(conversation, &graph, &profile, budget);
            Some(())
        });
    }
}

fn ask(conversation: i32, graph: &LookAheadGraph, profile: &MenuProfile, budget: DiagramBudget) {
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
    let Some(vars) = DataVars::try_new(&layout, &symbols, budget) else {
        eprintln!("conversation {conversation}: no room for the variables.");
        return;
    };
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(&world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, &world, &vars).expect("room for a seed");
    let shape = GroupShape::of(graph);
    let novelty = profile.novelty();

    let options: HashSet<_> = profile.starts.iter().copied().collect();
    let targets: Vec<_> = graph
        .nodes()
        .filter(|n| {
            !n.is_group && novelty(n.id) > Novelty::SeenThisGame && !options.contains(&n.id)
        })
        .map(|n| n.id)
        .collect();

    println!(
        "\n== conversation {conversation}: {} options, {} unread",
        profile.starts.len(),
        targets.len()
    );
    println!(
        "   {:>10}  {:>7}  {:>8}  {:>7}  {:>8}",
        "option", "onward", "ms", "at all", "ms"
    );

    let mut onward = 0usize;
    let mut at_all = 0usize;
    for &start in &profile.starts {
        // SIBLINGS CUT. A route that had to return through another option of this menu
        // cannot survive it, so what is left is what this option reaches on its own.
        let siblings: HashSet<_> = options.iter().copied().filter(|id| *id != start).collect();
        let (cut_yes, cut_ms) = reaches(
            graph,
            &targets,
            &siblings,
            &mut compiler,
            &world,
            &seed,
            &shape,
            start,
        );
        let (open_yes, open_ms) = reaches(
            graph,
            &targets,
            &HashSet::new(),
            &mut compiler,
            &world,
            &seed,
            &shape,
            start,
        );
        onward += usize::from(cut_yes);
        at_all += usize::from(open_yes);
        println!(
            "   {:>5}:{:<4}  {:>7}  {:>8}  {:>7}  {:>8}",
            start.conversation_id,
            start.entry_id,
            if cut_yes { "YES" } else { "no" },
            cut_ms,
            if open_yes { "YES" } else { "no" },
            open_ms
        );
    }
    println!(
        "   SEPARATES: {onward} of {} lead onward, {at_all} reach something at all",
        profile.starts.len()
    );

    if !lookahead_engine::core::env::is_set("COMPARE") {
        return;
    }
    // AGAINST THE EXACT MARKING, which is the only thing that says whether the cheap answer
    // is the RIGHT answer. A marker that separates the options but separates the wrong ones
    // is worse than no marker at all.
    let contestants: Vec<_> = profile
        .starts
        .iter()
        .map(|&start| menu::Contestant {
            position: Where::of(
                graph,
                start,
                StartBranch::Either,
                &seed,
                &mut compiler,
                &world,
                COUNTER_CAP as u32,
            )
            .position(start),
            baseline: novelty(start),
        })
        .collect();
    let began = Instant::now();
    let found = menu::mark_menu(
        graph,
        &mut compiler,
        &world,
        COUNTER_CAP as u32,
        &novelty,
        &contestants,
        &menu::Budget {
            wall: Duration::from_secs(1200),
            each: Duration::from_secs(600),
        },
        &shape,
    );
    println!("   exact marking in {} ms:", began.elapsed().as_millis());
    for (start, mark) in profile.starts.iter().zip(&found.marks) {
        println!(
            "   {:>5}:{:<4}  marked {:>5}  distance {:>5}  settled {}",
            start.conversation_id,
            start.entry_id,
            if mark.round.is_some() { "YES" } else { "no" },
            mark.distance.map_or("-".into(), |d| d.to_string()),
            mark.complete
        );
    }
}

/// Whether anything in `targets` is reachable from `start` with `cut` refused.
fn reaches(
    graph: &LookAheadGraph,
    targets: &[DialogueNodeId],
    cut: &HashSet<DialogueNodeId>,
    compiler: &mut GuardCompiler<'_>,
    world: &SnapshotWorld,
    seed: &oxidd::bdd::BDDFunction,
    shape: &GroupShape,
    start: DialogueNodeId,
) -> (bool, u128) {
    let began = Instant::now();
    let position = Where::of(
        graph,
        start,
        StartBranch::Either,
        seed,
        compiler,
        world,
        COUNTER_CAP as u32,
    )
    .position(start);
    let mut known = shape.known_from(graph, start);
    for &entry in &position.entries {
        known = known.from(entry, &position.holding);
    }
    let pass = Backward::reaching_any_knowing(
        graph,
        targets,
        cut,
        compiler,
        world,
        COUNTER_CAP as u32,
        &PassBudget {
            time: Duration::from_millis(from_env("EACH_MS", EACH_MS as usize) as u64),
            steps: usize::MAX,
            ..Default::default()
        },
        Some(&known),
    );
    let met = pass.stats().met_at.is_some();
    (met, began.elapsed().as_millis())
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
            .map(|piece| piece.parse().expect("a number"))
            .collect(),
        Err(_) => fallback.to_vec(),
    }
}
