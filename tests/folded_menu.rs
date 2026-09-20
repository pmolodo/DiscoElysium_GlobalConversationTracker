// SPDX-License-Identifier: MIT
//! Folding a group must not change what its menus answer.
//!
//! ## What is being checked, and why it is not the oracle's job
//!
//! `menu_oracle` holds the marking to exhaustive concrete-state distances over ONE graph. This
//! asks a different question: the same menu, asked of a group and of the same group folded, has
//! to come back with the same options marked at the same distances.
//!
//! IT SHOULD BE EXACT, not merely close. A layer is charged for LEAVING A CHOICE and a folded
//! chain holds none, so every entry in one stands at the distance its head does. Folding can
//! therefore move no distance at all, and a difference here is a fold that swallowed something
//! it should not have - an entry that could be stopped at, or one carrying a slot.
//!
//! ## What a folded group is asked
//!
//! The options are the menu's, which are choices and so are never swallowed. What a chain
//! REACHES is translated: a folded entry stands for its whole chain, so it counts as unseen
//! where any entry it swallowed is unseen, since arriving there shows all of them.

use std::collections::HashMap;

use lookahead_engine::core::types::{DialogueNodeId, SeenState, StartBranch};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::known::GroupShape;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::search::Search;
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::symbolic::{menu, seen_state_search};
use lookahead_engine::world::ILookAheadWorld;
use std::time::Duration;
mod common;

/// What a menu answered: per option, the class it reaches and how far away it is.
fn answers(
    graph: &LookAheadGraph,
    options: &[DialogueNodeId],
    world: &dyn ILookAheadWorld,
    seen_state: &impl Fn(DialogueNodeId) -> SeenState,
) -> Vec<(SeenState, Option<usize>)> {
    let layout = DataLayout::for_group(graph, world, 16);
    let vars = DataVars::new(&layout, graph.symbols(), DiagramBudget::modest());
    let mut compiler = GuardCompiler::new(&vars).with_world(world);
    let seed = seed_of(graph, world, &vars).expect("room for a seed");
    let contestants: Vec<_> = options
        .iter()
        .map(|&id| menu::Contestant {
            position: seen_state_search::Where::of(
                graph,
                id,
                StartBranch::Either,
                &seed,
                &mut compiler,
                world,
                16,
            )
            .position(id),
            baseline: seen_state(id),
            landing: vec![id],
        })
        .collect();
    let found = menu::mark_menu(
        Search {
            graph,
            compiler: &mut compiler,
            world,
            counter_cap: 16,
            arms: Default::default(),
        },
        seen_state,
        &contestants,
        &menu::Budget {
            wall: Duration::from_secs(30),
            each: Duration::from_secs(30),
        },
        &GroupShape::of(graph),
    );
    assert!(
        found.marks.iter().all(|mark| mark.complete),
        "the budget bound a menu this test needs answered"
    );
    found
        .marks
        .iter()
        .map(|mark| (mark.best, mark.distance))
        .collect()
}

/// The small real groups `menu_oracle` uses, so a failure here can be read beside one there.
const GROUPS: [i32; 7] = [1123, 484, 1066, 1147, 949, 511, 640];

#[test]
fn a_folded_group_answers_its_menus_exactly_as_the_whole_one_does() {
    let path = common::conversation_index().expect("conversation index is required");
    let index = lookahead_engine::index::read_index(&path).unwrap();
    let world = common::measurement_save();
    for conversation in GROUPS {
        let (graph, _) = lookahead_engine::index::build_group_graph(&index, conversation).unwrap();
        let options: Vec<_> = graph
            .nodes()
            .filter(|node| node.choice)
            .take(6)
            .map(|node| node.id)
            .collect();
        assert!(!options.is_empty(), "{conversation}: no choices detected");
        let seen_state = |id: DialogueNodeId| match id.entry_id % 3 == 0 {
            true => SeenState::UnseenAnyGame,
            false => SeenState::UnseenThisGame,
        };

        let folded = graph.collapsing_runs();
        // WHAT A CHAIN STANDS FOR. Arriving at a folded entry shows everything it swallowed, so
        // it is as unseen as the most novel of them, which is the GREATEST seen state: the order runs seen-this-game, unseen-this-game, unseen-any-game.
        let mut swallowed: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
        for (&member, &head) in &folded.into_head {
            swallowed.entry(head).or_default().push(member);
        }
        let folded_state = |id: DialogueNodeId| {
            let mine = seen_state(id);
            swallowed
                .get(&id)
                .map(|run| run.iter().map(|member| seen_state(*member)).max())
                .unwrap_or(None)
                .map_or(mine, |theirs| mine.max(theirs))
        };

        lookahead_engine::symbolic::isolated::on_its_own_thread(|| {
            let whole = answers(&graph, &options, &world, &seen_state);
            let short = answers(&folded.graph, &options, &world, &folded_state);
            assert_eq!(
                whole,
                short,
                "{conversation}: folding moved an answer, {} entries folded away",
                folded.into_head.len()
            );
            println!(
                "{conversation}: {} entries, {} folded away, answers unchanged",
                graph.nodes().count(),
                folded.into_head.len()
            );
        });
    }
}
