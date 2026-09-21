// SPDX-License-Identifier: MIT
//! A passive check the character sheet fails is not a line the player can reach.
//!
//! BOARDWALK / TRASH CAN is the conversation that makes the point on its own. Every unread
//! line in it is a passive check - the tare, the kebab wrapper and the cigarette package
//! each lead to one - and the `at-trashcan` save holds no skill above 2 where the cheapest
//! of them wants 4. So a player at that bin has read everything there is to read, and a
//! menu marked otherwise is promising something the game will never show.
//!
//! The distinction the engine has to draw is between ENTERED and DISPLAYED. A failing check
//! is entered by every state that gets to it and passes through uncharged, which is how the
//! conversation carries on past it; what must not follow is that the line itself counts as
//! somewhere the search can arrive.
//!
//! INVENTORY / PRIMER covers the mirror of it. Its Encyclopedia line at entry 11 is
//! ANTIPASSIVE - the one that shows when you are NOT sharp enough - and it tests the same
//! skill against the same threshold as the trash can's entry 9. One sheet, one comparison,
//! opposite answers, which is the whole of what the inversion is.
//!
//! Every outcome here is decided from the save's own character sheet, by the same
//! arithmetic the plugin uses. Nothing is declared: a check written down beside the
//! expectation would agree with the game only until one of them changed.

mod common;

use std::collections::HashSet;

use common::fixtures;
use lookahead_engine::bridge::{
    LookAheadAnswer, LookAheadRequest, NodeRef, WorldRawData, answer, questions_of,
};
use lookahead_engine::index::{build_group_graph, read_index};

const TRASH_CAN: i32 = 1174;
const SAVE: &str = "at-trashcan";
const STATE: &str = "global-state-at-trashcan.json";

/// The hub menu, in the order the hub group links its options.
///
/// Three examine options and a way out. The three lead to lines this save has read, which
/// is what makes them worth asking about: an option unread in any game is never marked, so
/// a marker here can only come from something further on.
const MENU: [i32; 4] = [2, 17, 4, 19];

/// MARTINAISE, DAY 3, 10-35.
const DAY_COUNTER: i32 = 3;
const DAY_MINUTES: i32 = 10 * 60 + 35;

const SEEN_THIS_GAME: i32 = 0;
const UNSEEN_THIS_GAME: i32 = 1;
const UNSEEN_ANY_GAME: i32 = 2;

/// What the mod would draw, given the option's own seen state and what the search found.
fn drawn(own: i32, answer: &LookAheadAnswer) -> &'static str {
    if own == UNSEEN_ANY_GAME {
        return "none";
    }

    if answer.best <= own {
        return if answer.complete { "none" } else { "gaveUp" };
    }

    if answer.best == UNSEEN_ANY_GAME {
        "orange"
    } else {
        "red"
    }
}

/// The menu as the mod would draw it, with the checks decided or left undecided.
///
/// `consult_the_sheet` is what separates the two tests below. False is a world that knows
/// nothing about the checks, which is what an offline run had before there was anything to
/// read a sheet with, and it is here to show that the answer changes.
fn menu_markers(consult_the_sheet: bool) -> Option<Vec<(i32, String)>> {
    let path = common::shipped_index()?;
    let index = read_index(&path).expect("the shipped index reads");
    let (graph, group) = build_group_graph(&index, TRASH_CAN).expect("the group builds");

    let recorded = fixtures::recorded_elsewhere_in_group(STATE, &group);
    let read_here = fixtures::read_in_save_group(SAVE, &group);

    let seen_state_of = |node: NodeRef| -> i32 {
        let key = (node.conversation, node.entry);
        if read_here.contains(&key) {
            SEEN_THIS_GAME
        } else if recorded.contains(&key) {
            UNSEEN_THIS_GAME
        } else {
            UNSEEN_ANY_GAME
        }
    };

    let everything: Vec<NodeRef> = graph.nodes().map(|node| NodeRef::from(node.id)).collect();
    let by_class = |wanted: i32| -> HashSet<NodeRef> {
        everything
            .iter()
            .copied()
            .filter(|node| seen_state_of(*node) == wanted)
            .collect()
    };

    let checks = match consult_the_sheet {
        true => fixtures::checks_in_save(SAVE, &group)?,
        false => fixtures::Checks::default(),
    };

    let request = LookAheadRequest {
        conversation: TRASH_CAN,
        starts: MENU
            .iter()
            .map(|entry| NodeRef {
                conversation: TRASH_CAN,
                entry: *entry,
            })
            .collect(),
        // WHAT SOME PLAYTHROUGH SHOWED: the two lower rungs together, since seen this game
        // implies seen any game.
        seen_any_game: by_class(SEEN_THIS_GAME)
            .into_iter()
            .chain(by_class(UNSEEN_THIS_GAME))
            .collect(),
        world: WorldRawData {
            day_minutes: DAY_MINUTES,
            day_counter: DAY_COUNTER,
            // WHAT THIS SAVE HAS READ, which is the world's to say. Nothing sends the middle
            // rung: an entry this world has not seen and `unseen_any_game` does not name IS
            // that rung - see `world::seen_state`.
            seen: by_class(SEEN_THIS_GAME).into_iter().collect(),
            // FROM THE SAVE. The hub is guarded on nothing here, but the world a scenario
            // means is the one the in-game run loads, and a fixture that quietly meant a
            // different one would agree with the game by accident.
            variables: fixtures::variables_sent(SAVE, &questions_of(&graph, group.clone())),
            checks_pass: checks.pass,
            checks_fail: checks.fail,
            check_margins: checks.margins,
            ..Default::default()
        },
        ..Default::default()
    };

    let response = answer(&index, common::declared(), None, &request);
    assert!(response.error.is_none(), "{:?}", response.error);

    Some(
        response
            .answers
            .iter()
            .map(|reply| {
                (
                    reply.start.entry,
                    drawn(seen_state_of(reply.start), reply).to_string(),
                )
            })
            .collect(),
    )
}

#[test]
fn a_check_this_sheet_fails_marks_nothing() {
    let Some(markers) = menu_markers(true) else {
        eprintln!("no shipped index; skipping.");
        return;
    };

    for (entry, marker) in &markers {
        assert_eq!(
            marker, "none",
            "{TRASH_CAN}:{entry} was marked, and every line it can reach is a check this \
             sheet fails",
        );
    }
}

#[test]
fn an_undecided_check_still_marks() {
    let Some(markers) = menu_markers(false) else {
        eprintln!("no shipped index; skipping.");
        return;
    };

    // WHAT THE OTHER TEST WOULD PASS WITHOUT. A world that answered nothing about the
    // checks would make both tests read "none", one of them for the wrong reason. An
    // undecided check is carried both ways deliberately - more markers than earned, never
    // fewer - so the same menu marks here, and the difference between the two is the whole
    // of what the sheet decides.
    let marked: Vec<i32> = markers
        .iter()
        .filter(|(_, marker)| marker != "none")
        .map(|(entry, _)| *entry)
        .collect();
    assert_eq!(
        marked,
        vec![2, 17, 4],
        "the three examine options should be marked while nothing is known about the checks",
    );
}

/// INVENTORY / PRIMER, a group of one conversation and 31 entries.
const PRIMER: i32 = 1123;

/// The two Encyclopedia checks this file turns on, as conversation and entry.
///
/// Both want Encyclopedia 4, which is difficulty AVERAGE once the flat bonus is added, and
/// this save holds Encyclopedia 1. The trash can's is an ordinary passive and so does not
/// fire; the primer's is antipassive and so does.
const ORDINARY: (i32, i32) = (TRASH_CAN, 9);
const INVERTED: (i32, i32) = (PRIMER, 11);

/// The best seen state an option can reach, with one entry the only thing worth reaching.
///
/// Narrowed to one candidate on purpose. The answer is then about that entry and nothing
/// else, which is what makes it a statement about the check rather than about whatever the
/// conversation happened to leave unread.
fn best_reachable(start: (i32, i32), candidate: (i32, i32)) -> Option<i32> {
    let path = common::shipped_index()?;
    let index = read_index(&path).expect("the shipped index reads");
    let (graph, group) = build_group_graph(&index, start.0).expect("the group builds");
    let checks = fixtures::checks_in_save(SAVE, &group)?;

    let of = |(conversation, entry): (i32, i32)| NodeRef {
        conversation,
        entry,
    };

    let request = LookAheadRequest {
        conversation: start.0,
        starts: vec![of(start)],
        // EVERYTHING BUT THE CANDIDATE HAS BEEN SHOWN SOMEWHERE, which with the world below -
        // which has read all of it but the candidate too - leaves the candidate the only thing
        // unread anywhere and so the only thing worth reaching.
        seen_any_game: graph
            .nodes()
            .map(|node| NodeRef::from(node.id))
            .filter(|node| *node != of(candidate))
            .collect(),
        world: WorldRawData {
            day_minutes: DAY_MINUTES,
            day_counter: DAY_COUNTER,
            // EVERYTHING ELSE IS READ, which is what makes the candidate the only thing worth
            // reaching. It has to be SAID rather than left out: an entry this world has not seen
            // and `unseen_any_game` does not name is one the global tracking holds and this save
            // has not read - unseen this game, and so worth reaching.
            seen: graph
                .nodes()
                .map(|node| NodeRef::from(node.id))
                .filter(|node| *node != of(candidate))
                .collect(),
            variables: fixtures::variables_sent(SAVE, &questions_of(&graph, group.clone())),
            checks_pass: checks.pass,
            checks_fail: checks.fail,
            check_margins: checks.margins,
            ..Default::default()
        },
        ..Default::default()
    };

    let response = answer(&index, common::declared(), None, &request);
    assert!(response.error.is_none(), "{:?}", response.error);
    Some(response.answers.first().expect("one start").best)
}

#[test]
fn the_inversion_turns_the_same_comparison_around() {
    let Some(shipped) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&shipped).expect("the shipped index reads");

    for (conversation, entry) in [ORDINARY, INVERTED] {
        let (graph, group) = build_group_graph(&index, conversation).expect("the group builds");
        let node = NodeRef {
            conversation,
            entry,
        };

        // THE INDEX STILL CALLS IT A CHECK, which is what makes the rest of this reachable
        // at all: the plugin only evaluates the entries the engine asks about, and it asks
        // by this list. The field marking the inversion does not survive into the shipped
        // index, so nothing but the difficulty is left to notice the entry by - and the
        // antipassive one has to be noticed all the same.
        assert!(
            questions_of(&graph, group.clone()).checks.contains(&node),
            "{conversation}:{entry} is a passive check and must be asked about",
        );

        let checks = fixtures::checks_in_save(SAVE, &group).expect("the sheet reads");
        let fires = checks.pass.contains(&node);
        assert_eq!(
            fires,
            (conversation, entry) == INVERTED,
            "{conversation}:{entry} fires: {fires}, from a sheet that holds Encyclopedia \
             below what the comparison wants",
        );
    }
}

#[test]
fn an_antipassive_line_this_sheet_earns_is_reachable() {
    // Entry 6 is "Flip through the pages.", which leads to the antipassive line.
    let Some(best) = best_reachable((PRIMER, 6), INVERTED) else {
        eprintln!("no shipped index; skipping.");
        return;
    };

    assert_eq!(
        best, UNSEEN_ANY_GAME,
        "a line that fires is a line to read, however it came to fire",
    );
}

#[test]
fn an_ordinary_line_this_sheet_fails_is_not() {
    // Entry 17 is "Examine the cigarette package.", which leads to the Encyclopedia line.
    let Some(best) = best_reachable((TRASH_CAN, 17), ORDINARY) else {
        eprintln!("no shipped index; skipping.");
        return;
    };

    assert_eq!(
        best, SEEN_THIS_GAME,
        "the same skill against the same threshold, uninverted, reaches nothing",
    );
}
