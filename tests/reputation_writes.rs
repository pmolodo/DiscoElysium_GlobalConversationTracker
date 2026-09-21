// SPDX-License-Identifier: MIT
//! A reputation the conversation itself raises can change which reputation is winning.
//!
//! DREAM SEAFORT / DOLORES DEI, conversation 767, is where the game does it. "Let's try to
//! return Revachol to the likeness of the holy sun-queen", entry 802, is offered only with the
//! revacholian_nationhood thought fixed, and runs `ReputationGrows("revacholian_nationhood")`.
//! Its way on returns to the dream's hub and from there to 149, which splits on
//! `IsHighestPolitical("revacholian_nationhood")`: the nationalist line 134 where it holds,
//! and 471 where it does not.
//!
//! So a player TIED at the top with communism reads 471 if they never raise it and 134 if they
//! do. Neither half can be had by answering the question once: from the world it is false and
//! 134 is closed even past 802, and left undecided both lines are open everywhere. The search
//! has to carry the amounts and compare them where it stands, which is what these ask.
//!
//! No save reaches the dream, so the world is stated here: the four political amounts, the
//! thought where a test fixes it, and every other variable at what the database starts it as.

mod common;

use lookahead_engine::bridge::{
    DataAnswer, DataKind, DataRequest, LookAheadRequest, NodeRef, WireValue, WorldSnapshot, answer,
};
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{Index, build_group_graph, read_index};

const DOLORES_DEI: i32 = 767;

/// "Let's try to return Revachol to the likeness of the holy sun-queen", which raises
/// revacholian_nationhood by one.
const RAISES_NATIONHOOD: i32 = 802;

/// "Okay, I won't ask any more.", which returns to the hub without raising anything.
const RAISES_NOTHING: i32 = 742;

/// Rhetoric's line behind the nationhood side of the split, where it is winning.
const NATIONHOOD_WINNING: i32 = 134;

/// The same line's other half, where it is not.
const NATIONHOOD_NOT_WINNING: i32 = 471;

const THOUGHT: &str = "revacholian_nationhood";

const UNSEEN_ANY_GAME: i32 = 2;

fn shipped() -> Option<Index> {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return None;
    };
    Some(read_index(&path).expect("the shipped index reads"))
}

/// The request for `start` towards `target`, in a world holding these political amounts.
///
/// `amounts` is in the game's enum order: communist, revacholian_nationhood, ultraliberal,
/// moralist.
fn request(
    index: &Index,
    start: i32,
    target: i32,
    amounts: [i32; 4],
    thought_fixed: bool,
) -> LookAheadRequest {
    let variables = [
        "communist",
        "revacholian_nationhood",
        "ultraliberal",
        "moralist",
    ]
    .iter()
    .zip(amounts)
    .map(|(name, amount)| {
        (
            format!("reputation.{name}"),
            WireValue::Number {
                value: f64::from(amount),
            },
        )
    })
    .collect();

    let mut world = WorldSnapshot {
        variables,
        clock_locked: true,
        ..Default::default()
    };
    let fixed = if thought_fixed {
        vec![THOUGHT.to_string()]
    } else {
        Vec::new()
    };
    world.data.insert(
        DataRequest::set(DataKind::ThoughtsFixed),
        DataAnswer::of_names(fixed),
    );

    LookAheadRequest {
        conversation: DOLORES_DEI,
        starts: vec![NodeRef {
            conversation: DOLORES_DEI,
            entry: start,
        }],
        // ONE ENTRY UNREAD ANYWHERE, so everything else in the group has been shown by some
        // playthrough - which is what the request carries, and it has to be named rather than
        // left out. See `world::seen_state`.
        seen_any_game: build_group_graph(index, DOLORES_DEI)
            .expect("the group builds")
            .0
            .nodes()
            .map(|node| NodeRef::from(node.id))
            .filter(|node| {
                *node
                    != NodeRef {
                        conversation: DOLORES_DEI,
                        entry: target,
                    }
            })
            .collect(),
        world,
        ..Default::default()
    }
}

/// Whether `start` reaches `target`, in a world holding these political amounts.
fn reaches(index: &Index, start: i32, target: i32, amounts: [i32; 4], thought_fixed: bool) -> bool {
    let request = request(index, start, target, amounts, thought_fixed);
    let start = request.starts[0];
    let response = answer(index, None, None, &request);
    assert!(response.error.is_none(), "{:?}", response.error);
    let reply = response
        .find(start, None)
        .unwrap_or_else(|| panic!("no answer for {start:?}"));
    assert!(
        reply.complete,
        "{start:?} towards {target}: the search did not finish"
    );
    reply.best == UNSEEN_ANY_GAME
}

/// Tied with communism, and raised past it on the way: the nationalist line opens.
#[test]
fn raising_a_tied_reputation_makes_it_win() {
    let Some(index) = shipped() else { return };
    assert!(
        reaches(
            &index,
            RAISES_NATIONHOOD,
            NATIONHOOD_WINNING,
            [5, 5, 0, 0],
            true
        ),
        "802 raises nationhood from a tie to the lead, so 134 is open past it"
    );
}

/// The same raise from amounts a real save holds, above the counter cap: a reputation cannot
/// loop, so nothing caps it, and 29 still beats 28.
#[test]
fn raising_a_reputation_above_the_counter_cap_still_counts() {
    let Some(index) = shipped() else { return };
    assert!(
        reaches(
            &index,
            RAISES_NATIONHOOD,
            NATIONHOOD_WINNING,
            [28, 28, 0, 0],
            true
        ),
        "802 raises nationhood from 28 to 29, past communism's 28, so 134 is open past it"
    );
}

/// Tied with communism and never raised: nothing is winning, so only the other half is open.
#[test]
fn a_tie_left_alone_wins_nothing() {
    let Some(index) = shipped() else { return };
    assert!(
        !reaches(
            &index,
            RAISES_NOTHING,
            NATIONHOOD_WINNING,
            [5, 5, 0, 0],
            false
        ),
        "a tie clears the winner and nothing here raises nationhood, so 134 is closed"
    );
    assert!(
        reaches(
            &index,
            RAISES_NOTHING,
            NATIONHOOD_NOT_WINNING,
            [5, 5, 0, 0],
            false
        ),
        "with nothing winning, 471 is the half that is open"
    );
}

/// Already ahead with nothing raised: the world's own answer stands.
#[test]
fn a_reputation_already_winning_stays_winning() {
    let Some(index) = shipped() else { return };
    assert!(
        reaches(
            &index,
            RAISES_NOTHING,
            NATIONHOOD_WINNING,
            [5, 6, 0, 0],
            false
        ),
        "nationhood leads, so 134 is open"
    );
    assert!(
        !reaches(
            &index,
            RAISES_NOTHING,
            NATIONHOOD_NOT_WINNING,
            [5, 6, 0, 0],
            false
        ),
        "and 471 is closed"
    );
}

/// The two sides of the split, 150 and 151, as their guards are compiled for a request.
fn split_compiled(index: &Index, amounts: [i32; 4]) -> common::CompiledGuards {
    let split = [150, 151].map(|entry| DialogueNodeId::new(DOLORES_DEI, entry));
    common::compiled_guards(
        index,
        &request(index, RAISES_NOTHING, NATIONHOOD_WINNING, amounts, true),
        &split,
    )
}

/// Where the raise can go past the split's guards and nationhood is not already winning, the
/// question is left to the search: 802 is not behind any guard that nationhood is winning.
#[test]
fn a_raise_that_can_change_the_winner_is_left_to_the_search() {
    let Some(index) = shipped() else { return };
    let compiled = split_compiled(&index, [5, 5, 0, 0]);
    assert_eq!(compiled.reputation_from_world, 0);
    assert_eq!(compiled.fallbacks, 0, "and it is still decided, per state");
}

/// Where the only raise is to the reputation already winning, nothing can change the winner,
/// and both sides are answered from the world.
#[test]
fn a_raise_to_the_winner_is_answered_from_the_world() {
    let Some(index) = shipped() else { return };
    let compiled = split_compiled(&index, [5, 6, 0, 0]);
    assert_eq!(compiled.reputation_from_world, 2);
    assert_eq!(compiled.fallbacks, 0);
}
