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
//! search, and any difference is marshalling.

use std::collections::HashSet;

use lookahead_engine::bridge::{
    LookAheadRequest, NodeRef, SnapshotWorld, WireValue, WorldSnapshot, answer, questions_for,
};
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};

mod common;

// THE MEASUREMENTS' ADVERSARIAL PROFILE, borrowed rather than restated. A menu whose starts
// have nothing better beyond them is refused by `bridge::class_worth_hunting` before a
// diagram is touched, so it answers in no time at all and a wall put around it binds
// nothing - which reads exactly like a wall that works. That module's own header records
// the same trap catching `menu_residue`, and it is the only thing here that avoids it.
#[path = "../measurements/menu_profile.rs"]
mod menu_profile;

/// Small enough to search exhaustively, so the comparison is about the crossing rather
/// than about a budget running out at different moments.
const CHECKABLE: [i32; 3] = [1123, 484, 1066];

/// For the key agreement, which does no searching and so can afford the groups that
/// actually ask things: 631 and 368 between them cover items, tasks, thoughts, the clock
/// and several hundred world queries. A key that did not match would be missed entirely on
/// a group with one query in it.
const RICH: [i32; 3] = [631, 368, 14];

#[test]
fn the_engine_names_questions_the_snapshot_can_answer() {
    let Some(path) = common::conversation_index() else {
        return;
    };
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
            world
                .queries
                .insert(key.clone(), WireValue::Bool { value: true });
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
fn answered_queries(guard: &lookahead_engine::core::guard::Guard, world: &SnapshotWorld) -> usize {
    use lookahead_engine::core::guard::GuardExpression as G;
    use lookahead_engine::core::guard_value::{GuardValue, GuardValueKind};
    use lookahead_engine::world::world::ILookAheadWorld;

    let mut answered = 0;
    for node in guard.nodes() {
        let G::Call(name, args) = node.expression() else {
            continue;
        };
        // The subject-taking three are answered from items/tasks/thoughts instead, and
        // a flag is a variable, so none of those is a query key.
        if matches!(
            name,
            "CheckItem" | "IsTaskActive" | "IsTHCPresent" | "FlagSet"
        ) {
            continue;
        }

        let values: Option<Vec<GuardValue>> = args
            .iter()
            .map(|arg| match arg.expression() {
                G::Literal(value) => Some(value.clone()),
                _ => None,
            })
            .collect();

        let Some(values) = values else { continue };
        let answer = world.query(name, &values);
        assert_ne!(
            answer.kind(),
            GuardValueKind::Unknown,
            "'{name}' was answered under a key the engine does not look up",
        );
        answered += 1;
    }

    answered
}

/// The same world, once through JSON and once in the engine's hands, must score the same.
#[test]
fn an_answer_survives_the_crossing() {
    let Some(path) = common::conversation_index() else {
        return;
    };
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
            snapshot
                .queries
                .insert(key.clone(), WireValue::Bool { value: true });
        }
        for name in &questions.variables {
            snapshot
                .variables
                .insert(name.clone(), WireValue::Bool { value: false });
        }

        let request = LookAheadRequest {
            conversation,
            starts: vec![NodeRef::from(start)],
            unseen_any_game: unseen.iter().copied().collect(),
            unseen_this_game: Default::default(),
            state_budget: 0,
            time_budget_ms: 0,
            menu_time_budget_ms: 0,
            memory_budget_mb: 0,
            world: snapshot.clone(),
        };

        // Through JSON, exactly as it would travel.
        let text = serde_json::to_string(&request).expect("it serialises");
        let parsed: LookAheadRequest = serde_json::from_str(&text).expect("it comes back");
        let crossed = answer(&index, None, &parsed);

        // And the same request WITHOUT the crossing, which is what this compares against.
        //
        // THE SAME ENGINE ON BOTH SIDES, deliberately. What is being tested is whether a
        // round trip through JSON changes an answer, so anything else that differed between
        // the two sides would be measuring something other than the crossing.
        let direct = answer(&index, None, &request);

        assert!(crossed.error.is_none(), "{:?}", crossed.error);
        assert!(direct.error.is_none(), "{:?}", direct.error);
        assert_eq!(crossed.answers.len(), 1);
        assert_eq!(direct.answers.len(), 1);

        println!(
            "{conversation}: crossed {:?}, direct {:?}",
            crossed.answers[0].best, direct.answers[0].best,
        );
        assert_eq!(
            crossed.answers[0].best, direct.answers[0].best,
            "conversation {conversation}: the crossing changed the answer",
        );
        assert_eq!(
            crossed.answers[0].destination, direct.answers[0].destination,
            "conversation {conversation}: the crossing changed the baseline",
        );

        compared += 1;
    }

    assert!(compared > 0, "nothing was compared");
}

/// A menu that runs out of its wall still comes back as a whole menu.
///
/// ## What is being protected
///
/// de-dt75.3 put a wall around the whole request, and the failure it could produce is a
/// half-drawn menu: options the wall stopped left out of the response entirely, so the mod
/// has nothing to look up for them. Every start must still be answered - as "did not finish"
/// where the wall ran out, which is a thing the mod already draws - because a missing answer
/// and a gave-up answer mean opposite things to a player. See `bridge::answer_starts`.
///
/// ## Why one millisecond rather than a realistic figure
///
/// This is not a timing test. A wall of 1 ms cannot answer a group of this size on any
/// machine, so what the number buys is a wall that certainly binds, and the assertions are
/// about the SHAPE of what comes back rather than about how long anything took.
#[test]
fn a_menu_that_runs_out_of_its_wall_still_answers_every_option() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");

    let conversation = RICH[0];
    let (graph, _) = build_group_graph(&index, conversation).expect("the group builds");
    let root = DialogueNodeId::new(conversation, 0);
    let profile = menu_profile::MenuProfile::of(&graph, root, 10, 24)
        .expect("the group is big enough to draw an adversarial menu from");

    let starts: Vec<NodeRef> = profile.starts.iter().map(|id| NodeRef::from(*id)).collect();
    assert!(
        starts.len() > 1,
        "the group is too small to run out of anything"
    );

    let request = LookAheadRequest {
        conversation,
        starts: starts.clone(),
        unseen_any_game: profile.unseen.iter().map(|id| NodeRef::from(*id)).collect(),
        unseen_this_game: Default::default(),
        state_budget: 0,
        time_budget_ms: 1000,
        menu_time_budget_ms: 1,
        memory_budget_mb: 0,
        world: WorldSnapshot {
            money: 500,
            day_minutes: 12 * 60,
            day_counter: 1,
            ..Default::default()
        },
    };

    let response = answer(&index, None, &request);
    assert!(response.error.is_none(), "{:?}", response.error);

    for start in &starts {
        assert!(
            response
                .answers
                .iter()
                .any(|answered| answered.start == *start),
            "conversation {conversation}: {start:?} was left out of the menu entirely",
        );
    }

    // THE WALL ACTUALLY BOUND, which is what makes the assertion above worth making. An
    // answer the wall refused before it began is the only one that reports no elapsed time
    // at all, so that is what tells it apart from an option whose own search ran out.
    let walled = response
        .answers
        .iter()
        .filter(|answered| answered.stopped_by == "time" && answered.elapsed_ms == 0)
        .count();
    println!(
        "{conversation}: {walled} of {} answers refused by the menu wall",
        starts.len()
    );
    assert!(
        walled > 0,
        "a wall of one millisecond stopped nothing, so nothing was tested"
    );
}
