// SPDX-License-Identifier: MIT
//! Does a remembered backward pass answer what a fresh one answers?
//!
//! ## What is at risk
//!
//! de-znov.3 keeps a settled backward fixed point's SETS past the request that computed
//! them, and reads a verdict off them against whatever seed the next request brings. Two
//! ways that could be wrong, and they fail differently:
//!
//! - THE PASS WAS NOT A WHOLE ANSWER. A pass that ran out of budget, that ran out of room,
//!   or that met what the search holds where it began, holds a SUBSET of the states the
//!   target is reachable from - so reading it back later would refuse something genuinely
//!   reachable, and a marker the player should have seen would go missing.
//!   `symbolic::memo` refuses to keep all three, and this is what checks that the refusal is
//!   complete rather than plausible.
//! - THE VERDICT WAS KEPT INSTEAD OF THE SETS. A verdict is the sets met with ONE seed, and
//!   the seed carries what has been read - so a memo that kept verdicts would answer this
//!   menu's question with the last menu's reading.
//!
//! ## How it is checked
//!
//! The same starts asked twice over: once through a `Memo` that has been accumulating
//! passes, and once with no memo at all, which is the shipped search as it was. Every answer
//! must match.
//!
//! AND THE SEEN SET MOVES BETWEEN ROUNDS, which is the whole point. What has been read is
//! exactly what a kept pass must not depend on, so a memo that had baked a seed in would
//! disagree from the second round onwards - and a test that asked one world repeatedly would
//! pass with a memo that had.
//!
//! IT ALSO CHECKS THE MEMO IS USED. A feature that silently never fires would pass every
//! agreement check there is, so the hit count has to be positive for the test to mean
//! anything.

use std::collections::HashSet;

use lookahead_engine::bridge::{COUNTER_CAP, NodeRef, SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::answer;
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::known::GroupShape;
use lookahead_engine::symbolic::memo::{self, Memo};
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;

mod common;

/// The groups asked about, and they are two different jobs.
///
/// 1123, 484 and 1066 are `workspace_agreement`'s three: small enough to answer exhaustively,
/// so a disagreement is a disagreement rather than two budgets running out at different
/// moments. 362 is one of the heavy nine the measurements share, and it is here because the
/// small ones cannot produce a KEPT pass at all - see [`snapshot`].
const GROUPS: [i32; 4] = [1123, 484, 1066, 362];

/// How many worlds to ask each group about. Each round leaves one fewer entry unread.
const ROUNDS: usize = 4;

/// How many starts to ask about in one round.
const STARTS: usize = 12;

/// How many entries the first round leaves unread.
///
/// SMALL, because that is the regime a memo is for and the only one that produces anything
/// to keep - see [`snapshot`].
const UNREAD: usize = 6;

#[test]
fn a_remembered_pass_answers_what_a_fresh_one_answers() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");

    let mut compared = 0;
    let mut memos = lookahead_engine::symbolic::memo::MemoStats::default();
    for conversation in GROUPS {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let mut entries: Vec<NodeRef> = graph
            .nodes()
            .filter(|node| !node.is_group)
            .map(|node| NodeRef::from(node.id))
            .collect();
        // Ordered, so the rounds mark the same entries on every machine.
        entries.sort_unstable_by_key(|node| (node.conversation, node.entry));
        if entries.len() < UNREAD {
            continue;
        }

        // ONE MANAGER, MANY SEARCHES, ONE THREAD - de-fpax, and it is also what a memo
        // requires: every set it holds is a formula in this manager.
        let Some((asked, stats)) = one_group(&graph, &entries) else {
            continue;
        };
        compared += asked;
        memos.hits += stats.hits;
        memos.misses += stats.misses;
        memos.kept += stats.kept;
        memos.unsettled += stats.unsettled;
        memos.met += stats.met;
        memos.out_of_room += stats.out_of_room;
        memos.evicted += stats.evicted;
    }

    assert!(
        compared > 0,
        "no starts were compared, so nothing was checked"
    );
    assert!(
        memos.hits > 0,
        "{compared} answers agreed and the memo never once answered one - a feature that \
         never fires agrees with everything. It kept {} and refused {:?}",
        memos.kept,
        memos,
    );
}

/// One group: every round asked twice, and what the memo did.
fn one_group(
    graph: &lookahead_engine::graph::graph::LookAheadGraph,
    entries: &[NodeRef],
) -> Option<(usize, lookahead_engine::symbolic::memo::MemoStats)> {
    isolated::on_its_own_thread(|| {
        let symbols = graph.symbols().clone();
        let world = snapshot(entries, 0);
        let opening = SnapshotWorld::declaring(world.clone(), None);
        let layout = DataLayout::for_group(graph, &opening, COUNTER_CAP);
        let vars = DataVars::try_new(&layout, &symbols, DiagramBudget::over_a_group())?;
        let shape = GroupShape::of(graph);
        let memo = Memo::new(
            memo::key_of(&world),
            DiagramBudget::over_a_group().nodes() / 4,
        );

        // ORDERED BEFORE TAKING, and this is not tidiness. `LookAheadGraph::nodes` yields
        // whatever the hash map does, which Rust seeds afresh per process - so taking the
        // first dozen gave a DIFFERENT dozen starts every run. Two runs in eight then asked
        // only about starts whose candidates were all reachable, every pass met, nothing was
        // keepable, and the test failed for reasons that were nobody's fault.
        let mut starts: Vec<DialogueNodeId> = graph
            .nodes()
            .filter(|node| !node.is_group)
            .map(|node| node.id)
            .collect();
        starts.sort_unstable_by_key(|id| (id.conversation_id, id.entry_id));
        starts.truncate(STARTS);

        let mut compared = 0;
        for round in 0..ROUNDS {
            let snapshot = snapshot(entries, round);
            // THE KEY MUST NOT MOVE, because only what has been READ is changing - and if it
            // does, the memo empties every round and this test checks nothing.
            assert!(
                memo.keyed_on(memo::key_of(&snapshot)),
                "marking an entry as read moved the memo's key",
            );

            let world = SnapshotWorld::declaring(snapshot.clone(), None);
            // A COMPILER PER ROUND, as a request gets, over the manager built once above.
            let mut compiler = GuardCompiler::new(&vars)
                .with_world(&world)
                .with_constant_clock(DataLayout::group_passes_time(graph));
            let seed = seed_of(graph, &world, &vars)?;
            let read: HashSet<NodeRef> = snapshot.seen.iter().copied().collect();
            let novelty = |id: DialogueNodeId| {
                if read.contains(&NodeRef::from(id)) {
                    Novelty::SeenThisGame
                } else {
                    Novelty::UnseenAnyGame
                }
            };

            for &start in &starts {
                let Some(hunting) = graph.best_linked_class(start, novelty) else {
                    continue;
                };
                if hunting <= Novelty::SeenThisGame {
                    continue;
                }

                for budget in budgets() {
                    let remembering = answer::best_novelty(
                        graph,
                        start,
                        StartBranch::Either,
                        &seed,
                        &mut compiler,
                        &world,
                        COUNTER_CAP as u32,
                        novelty,
                        hunting,
                        &budget,
                        &shape,
                        Some(&memo),
                    );
                    let fresh = answer::best_novelty(
                        graph,
                        start,
                        StartBranch::Either,
                        &seed,
                        &mut compiler,
                        &world,
                        COUNTER_CAP as u32,
                        novelty,
                        hunting,
                        &budget,
                        &shape,
                        None,
                    );

                    assert_eq!(
                        remembering.best, fresh.best,
                        "round {round}, start {start:?}: a remembered pass changed the answer",
                    );
                    assert_eq!(
                        remembering.witness, fresh.witness,
                        "round {round}, start {start:?}: a remembered pass changed the witness",
                    );
                    assert_eq!(
                        remembering.by, fresh.by,
                        "round {round}, start {start:?}: a remembered pass changed which \
                         half answered",
                    );
                    compared += 1;
                }
            }
        }

        Some((compared, memo.stats()))
    })
}

/// The budget every start is asked under.
///
/// ## It is not held to the player's clock, and it has to be that way
///
/// A memo keeps a pass only if it SETTLED, so at the shipped two seconds whether anything is
/// kept depends on how busy the machine is - and this test failed exactly that way, passing
/// on its own and keeping nothing when the suite ran it beside thirteen other binaries. What
/// is being checked is that a remembered pass agrees with a fresh one, not that a pass
/// finishes inside a player's second, so the wall is set where the machine cannot reach it.
/// `tests/time_budget_binds.rs` is where the clock itself is pinned, and it pins the SHAPE
/// rather than timing a real search for the same reason.
fn budgets() -> [answer::Budget; 1] {
    let unhurried = std::time::Duration::from_secs(60);
    [answer::Budget {
        overall: unhurried,
        backwards: unhurried,
        each: unhurried,
    }]
}

/// A world in which everything has been read except a handful at the end, one fewer of them
/// each round.
///
/// ONLY `seen` MOVES between rounds, which is what makes the key constant and the seed not.
///
/// ## Why almost everything is read, which took a failing test to establish
///
/// A memo can only keep a pass that SETTLED WITHOUT MEETING, and on a world with plenty left
/// unread there are few such passes to keep. Two things get in the way, both of them the
/// search working correctly:
///
/// - the start may already carry the class being hunted, in which case the driver answers
///   without a pass at all;
/// - and where a pass does run, a near unread entry is REACHABLE, so it meets and stops
///   early - a proof for this seed, and not a fixed point anyone may keep.
///
/// What is left to remember is the refusals, and refusals are what a nearly-read
/// conversation is made of. That is the same regime `cacheable_asks` measured the memo to be
/// worth 68.6 per cent of its passes in, and it is what a player deep in a conversation they
/// have mostly exhausted actually has.
fn snapshot(entries: &[NodeRef], round: usize) -> WorldSnapshot {
    let mut world = WorldSnapshot {
        money: 500,
        day_minutes: 720,
        day_counter: 1,
        ..Default::default()
    };
    let unread = UNREAD.saturating_sub(round);
    for entry in entries.iter().take(entries.len().saturating_sub(unread)) {
        world.seen.insert(*entry);
    }
    world
}
