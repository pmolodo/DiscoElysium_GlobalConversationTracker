// SPDX-License-Identifier: MIT
//! Does a question survive the crossing?
//!
//! The bridge's unit tests check the pieces against hand-written fixtures. This checks the
//! whole of it against the real corpus: ask a group what it wants to know, answer it from
//! a world the engine can also be handed directly, and require the two answers to match.
//!
//! ## Why the comparison is against the SAME engine
//!
//! Not against the symbolic searches, and not against the C#. What is being tested here is
//! the crossing - that a world put into JSON and taken out again is the world the engine
//! would have been given in-process. So the two sides of the comparison run the identical
//! crawl, and any difference is marshalling.

use std::collections::HashSet;

use lookahead_engine::bridge::{
    answer, questions_for, LookAheadRequest, NodeRef, SnapshotWorld, WireValue, WorldSnapshot,
};
use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::engine::engine::LookAheadEngine;
use lookahead_engine::index::{build_group_graph, read_index};

mod common;

/// Small enough to crawl exhaustively, so the comparison is about the crossing rather
/// than about a budget running out at different moments.
const CHECKABLE: [i32; 3] = [1123, 484, 1066];

/// For the key agreement, which does no crawling and so can afford the groups that
/// actually ask things: 631 and 368 between them cover items, tasks, thoughts, the clock
/// and several hundred world queries. A key that did not match would be missed entirely on
/// a group with one query in it.
const RICH: [i32; 3] = [631, 368, 14];

#[test]
fn the_engine_names_questions_the_snapshot_can_answer() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");

    for conversation in RICH.iter().copied().chain(CHECKABLE) {
        let questions = questions_for(&index, conversation).expect("the group builds");

        println!(
            "{conversation}: {} conversations, {} variables, {} queries, {} items, \
             {} tasks, {} thoughts, {} checks, {} entries",
            questions.conversations.len(),
            questions.variables.len(),
            questions.queries.len(),
            questions.items.len(),
            questions.tasks.len(),
            questions.thoughts.len(),
            questions.checks.len(),
            questions.entries.len(),
        );

        assert!(!questions.entries.is_empty(), "a group with no entries");
        assert!(
            questions.conversations.contains(&conversation),
            "the group must contain the conversation it was asked about",
        );

        // Every query key must be one the snapshot looks up under the same name. Answer
        // them all true and none may come back unknown.
        let mut world = WorldSnapshot::default();
        for key in &questions.queries {
            world.queries.insert(key.clone(), WireValue::Bool { value: true });
        }
        let filled = SnapshotWorld::new(world);

        // Asked through the graph's own guards rather than by re-rendering the keys here,
        // which would just be this test agreeing with itself.
        let (graph, _) = build_group_graph(&index, conversation).expect("the group builds");
        let mut asked = 0;
        for node in graph.nodes() {
            asked += answered_queries(&node.guard, &filled);
        }
        println!("  {asked} query answers came back under the keys given");
    }
}

/// How many of a guard's world queries the snapshot answered.
///
/// Walks the parsed guard and asks the world exactly what the engine would ask it, so a
/// key that does not match shows up as an unanswered question rather than as a passing
/// test.
fn answered_queries(
    guard: &lookahead_engine::core::guard::GuardExpression,
    world: &SnapshotWorld,
) -> usize {
    use lookahead_engine::core::guard::GuardExpression as G;
    use lookahead_engine::core::guard_value::{GuardValue, GuardValueKind};
    use lookahead_engine::world::world::ILookAheadWorld;

    match guard {
        G::Not(inner) => answered_queries(inner, world),
        G::And(a, b) | G::Or(a, b) | G::Comparison(_, a, b) => {
            answered_queries(a, world) + answered_queries(b, world)
        }
        G::Call(name, args) => {
            // The subject-taking three are answered from items/tasks/thoughts instead, and
            // a flag is a variable, so none of those is a query key.
            if matches!(name.as_str(), "CheckItem" | "IsTaskActive" | "IsTHCPresent" | "FlagSet") {
                return 0;
            }

            let values: Option<Vec<GuardValue>> = args
                .iter()
                .map(|arg| match arg {
                    G::Literal(value) => Some(value.clone()),
                    _ => None,
                })
                .collect();

            let Some(values) = values else { return 0 };
            let answer = world.query(name, &values);
            assert_ne!(
                answer.kind(),
                GuardValueKind::Unknown,
                "'{name}' was answered under a key the engine does not look up",
            );
            1
        }
        G::Literal(_) | G::Variable(_) => 0,
    }
}

/// The same world, once through JSON and once in the engine's hands, must score the same.
#[test]
fn an_answer_survives_the_crossing() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");

    let mut compared = 0;

    for conversation in CHECKABLE {
        let (graph, _) = build_group_graph(&index, conversation).expect("the group builds");
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        // A world with something in it, so the comparison is not between two empty
        // snapshots: the deepest entries are unseen and a couple of facts are known.
        let unseen: HashSet<NodeRef> = graph
            .nodes()
            .map(|node| NodeRef::from(node.id))
            .filter(|node| node.entry % 7 == 3)
            .collect();

        let mut snapshot = WorldSnapshot {
            money: 500,
            day_minutes: 12 * 60,
            day_counter: 1,
            ..Default::default()
        };
        let questions = questions_for(&index, conversation).expect("the group builds");
        for key in &questions.queries {
            snapshot.queries.insert(key.clone(), WireValue::Bool { value: true });
        }
        for name in &questions.variables {
            snapshot.variables.insert(name.clone(), WireValue::Bool { value: false });
        }

        let request = LookAheadRequest {
            conversation,
            starts: vec![NodeRef::from(start)],
            unseen_any_game: unseen.clone(),
            unseen_this_game: HashSet::new(),
            world: snapshot.clone(),
        };

        // Through JSON, exactly as it would travel.
        let text = serde_json::to_string(&request).expect("it serialises");
        let parsed: LookAheadRequest = serde_json::from_str(&text).expect("it comes back");
        let crossed = answer(&index, &parsed);

        // And in-process, with no crossing at all.
        let world = SnapshotWorld::new(snapshot);
        let direct = LookAheadEngine::default().evaluate(&graph, start, &world, |id| {
            if unseen.contains(&NodeRef::from(id)) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        });

        assert!(crossed.error.is_none(), "{:?}", crossed.error);
        assert_eq!(crossed.answers.len(), 1);

        println!(
            "{conversation}: crossed {:?}, direct {:?}",
            crossed.answers[0].best, direct.best,
        );
        assert_eq!(
            crossed.answers[0].best, direct.best as i32,
            "conversation {conversation}: the crossing changed the answer",
        );

        compared += 1;
    }

    assert!(compared > 0, "nothing was compared");
}
