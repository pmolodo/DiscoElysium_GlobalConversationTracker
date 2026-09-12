// SPDX-License-Identifier: MIT
//! Does a kept manager answer what a fresh one answers?
//!
//! ## What is at risk
//!
//! de-2wtl keeps the diagram manager alive between requests instead of building one per
//! request. That is a performance change and must not be an ANSWER change, and there are
//! two distinct ways it could become one:
//!
//! - THE SECOND IMPLEMENTATION. A workspace that assembled a menu slightly differently from
//!   `bridge::answer` - a world not resolved, a rolled check not split into two starts,
//!   a budget read from the wrong place - would answer plausibly and wrongly.
//!   [`lookahead_engine::bridge::answer_starts`] exists so there is only one assembly, and
//!   this checks that it is really the only one.
//! - THE STALE WORKSPACE. The manager is kept across worlds, and the compiled guards and
//!   the seed are rebuilt per request because the world is baked into them. If a workspace
//!   were reused where it should have been rebuilt, it would answer an earlier world's
//!   question with this world's confidence.
//!
//! ## How it is checked
//!
//! The same requests through `Service::look_ahead`, which routes through a workspace, and
//! through `bridge::answer`, which does not. Every answer must match field for field except
//! the elapsed time.
//!
//! THE REQUESTS VARY THE WORLD, and specifically what has been SEEN - the field that moves
//! between two menus and the one the compiler bakes. A test that asked the same question
//! twice would pass with a workspace that ignored the world entirely.

use std::collections::HashSet;

use lookahead_engine::bridge::{
    LookAheadAnswer, LookAheadRequest, NodeRef, WireValue, WorldSnapshot, answer, questions_for,
};
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::service::Service;

mod common;

/// Small enough to answer exhaustively, so a difference is a difference rather than two
/// budgets running out at different moments.
const GROUPS: [i32; 3] = [1123, 484, 1066];

/// How many worlds to ask each group about.
const ROUNDS: usize = 4;

#[test]
fn a_kept_manager_answers_what_a_fresh_one_answers() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let service = Service::open(&path, None).expect("the engine opens over the index");

    let mut compared = 0;
    for conversation in GROUPS {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let Ok(questions) = questions_for(&index, conversation) else {
            continue;
        };

        let mut entries: Vec<NodeRef> = graph.nodes().map(|node| NodeRef::from(node.id)).collect();
        entries.sort_by_key(|node| (node.conversation, node.entry));
        if entries.len() < 8 {
            continue;
        }

        for round in 0..ROUNDS {
            // A DIFFERENT WORLD EACH ROUND, and different in what has been seen - which is
            // what the compiler bakes and therefore what a stale workspace would get wrong.
            let seen: HashSet<NodeRef> = entries.iter().take(round * 3).copied().collect();
            let unseen: HashSet<NodeRef> = entries.iter().rev().take(4).copied().collect();

            let world = WorldSnapshot {
                day_minutes: 720,
                day_counter: 1,
                seen: seen.iter().copied().collect(),
                // Answered so `resolve` succeeds; the values themselves are not the subject.
                variables: questions
                    .variables
                    .iter()
                    .map(|name| (name.clone(), WireValue::Unknown))
                    .collect(),
                ..Default::default()
            };

            let request = LookAheadRequest {
                conversation,
                starts: entries.iter().take(6).copied().collect(),
                unseen_any_game: unseen.iter().copied().collect(),
                unseen_this_game: Default::default(),
                world: world.clone(),
                ..Default::default()
            };

            let through_workspace = service.answer_request(request.clone());
            let direct = answer(&index, None, &request);

            assert_eq!(
                through_workspace.error, direct.error,
                "conversation {conversation}, round {round}",
            );
            assert_eq!(
                through_workspace.answers.len(),
                direct.answers.len(),
                "conversation {conversation}, round {round}: a different number of answers",
            );

            for (kept, fresh) in through_workspace.answers.iter().zip(direct.answers.iter()) {
                assert!(
                    same(kept, fresh),
                    "conversation {conversation}, round {round}: a kept manager answered \
                     {kept:?} where a fresh one answered {fresh:?}",
                );
                compared += 1;
            }
        }
    }

    assert!(
        compared > 0,
        "nothing was compared, so this test proves nothing - check the groups above still \
         exist in the index",
    );
}

/// A world answering the wrong questions is REFUSED through a workspace, as it is direct.
///
/// de-r4e0, and the reason the agreement test above did not catch it: every world it builds
/// resolves, so the refusal path was never taken through a workspace. It was taken in
/// earnest by the plugin, where the whole point of a positional answer list is that a stale
/// one is caught - and the workspace sent back an empty answer list and no error, which the
/// caller can only read as "nothing was established" rather than "this was not asked".
///
/// Answers positionally through `variable_values`, which is what the plugin uses and what
/// the agreement test above does not: it names its variables, and a named world has no
/// length to get wrong.
#[test]
fn a_world_answering_the_wrong_questions_is_refused_through_a_workspace() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let service = Service::open(&path, None).expect("the engine opens over the index");

    let mut refused = 0;
    for conversation in GROUPS {
        let Ok(questions) = questions_for(&index, conversation) else {
            continue;
        };

        // ONE ANSWER TOO FEW, whatever the group asks - and a group that asks for exactly
        // one would make this an empty list, which `place` reads as "not answering
        // positionally at all" rather than as a mismatch.
        if questions.variables.len() < 2 {
            continue;
        }
        let short: Vec<WireValue> = questions
            .variables
            .iter()
            .skip(1)
            .map(|_| WireValue::Unknown)
            .collect();

        let world = WorldSnapshot {
            day_minutes: 720,
            day_counter: 1,
            variable_values: short,
            ..Default::default()
        };
        let request = LookAheadRequest {
            conversation,
            starts: vec![NodeRef {
                conversation,
                entry: 0,
            }],
            world,
            ..Default::default()
        };

        let through_workspace = service.answer_request(request.clone());
        let direct = answer(&index, None, &request);

        let reason = through_workspace
            .error
            .as_ref()
            .unwrap_or_else(|| panic!("conversation {conversation} was not refused"));
        assert!(
            reason.contains("different list"),
            "conversation {conversation}: refused for {reason:?}, which is not the mismatch",
        );
        assert!(
            through_workspace.answers.is_empty(),
            "conversation {conversation}: a refused request still answered something",
        );

        // THE SAME REFUSAL, WORD FOR WORD, as the path that does not keep a manager. The
        // two must not drift into refusing for different reasons.
        assert_eq!(
            through_workspace.error, direct.error,
            "conversation {conversation}: the two paths refused differently",
        );
        refused += 1;
    }

    assert!(
        refused > 0,
        "nothing was refused, so this test proves nothing - check the groups above still \
         ask about at least two variables",
    );
}

/// A second group in between, so the workspace is genuinely REPLACED rather than only
/// re-used, and the replacement still answers correctly.
///
/// This is the case a workspace keyed on nothing at all would pass and a workspace with a
/// wrong key would fail: ask about A, then B, then A again, and the third answer must match
/// the first.
#[test]
fn a_workspace_replaced_by_another_group_still_answers_the_first() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let service = Service::open(&path, None).expect("the engine opens over the index");

    let requests: Vec<LookAheadRequest> = GROUPS
        .iter()
        .filter_map(|conversation| {
            let (graph, _) = build_group_graph(&index, *conversation).ok()?;
            let mut entries: Vec<NodeRef> =
                graph.nodes().map(|node| NodeRef::from(node.id)).collect();
            entries.sort_by_key(|node| (node.conversation, node.entry));
            (entries.len() >= 8).then(|| LookAheadRequest {
                conversation: *conversation,
                starts: entries.iter().take(4).copied().collect(),
                unseen_any_game: entries.iter().rev().take(3).copied().collect(),
                world: WorldSnapshot {
                    day_minutes: 720,
                    day_counter: 1,
                    ..Default::default()
                },
                ..Default::default()
            })
        })
        .collect();

    if requests.len() < 2 {
        return;
    }

    let ask = |request: &LookAheadRequest| service.answer_request(request.clone()).answers;

    let first = ask(&requests[0]);
    for other in &requests[1..] {
        let _ = ask(other);
    }
    let again = ask(&requests[0]);

    assert_eq!(
        first.len(),
        again.len(),
        "the same request gave a different shape"
    );
    for (before, after) in first.iter().zip(again.iter()) {
        assert!(
            same(before, after),
            "asking about another group in between changed the answer: {before:?} then \
             {after:?}",
        );
    }
}

/// Everything about an answer except how long it took.
fn same(left: &LookAheadAnswer, right: &LookAheadAnswer) -> bool {
    left.start == right.start
        && left.branch == right.branch
        && left.destination == right.destination
        && left.best == right.best
        && left.witness == right.witness
        && left.complete == right.complete
        && left.stopped_by == right.stopped_by
}
