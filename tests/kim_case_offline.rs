// SPDX-License-Identifier: MIT
//! The kim-case menu, drawn by the engine rather than by the game.
//!
//! THE MENU IS THE GAME'S. An offline executor cannot compose a response menu - which
//! options the Dialogue System offers from a conversation is its own arithmetic, and all
//! this engine is ever asked is what lies beyond an option it has been handed. The six
//! entries below are the ones the in-game run of the same suite was offered, in the order
//! the game drew them, so this reports the same menu from the same fixture with no game
//! running.
//!
//! It REPORTS rather than asserts, like the suite it belongs to, whose markers policy is
//! ignored. What it does assert is that the engine answered at all, and that every option
//! came back - an answer missing from a report is not a smaller report, it is a wrong one.
//!
//! Run it with the output showing:
//!
//! ```text
//! cargo test --release --test kim_case_offline -- --nocapture
//! ```

use std::collections::HashSet;

use lookahead_engine::bridge::{LookAheadAnswer, LookAheadRequest, NodeRef, WorldSnapshot, answer};
use lookahead_engine::index::{build_group_graph, read_index};

mod common;

use common::fixtures;
use common::suites;

/// The suite this reads its fixture out of.
const SUITE: &str = "kim-case";

/// The clock the in-game run was at, in minutes past midnight, and the day it was on.
///
/// NAMED RATHER THAN DEFAULTED. A guard that compares the time answers differently at
/// midnight, so a report taken at some other hour than the one the game was at would be
/// reporting a different menu that happens to share its options.
const DAY_MINUTES: i32 = 635;
const DAY: i32 = 3;

/// The three rungs as the engine numbers them.
const SEEN_THIS_GAME: i32 = 0;
const UNSEEN_THIS_GAME: i32 = 1;
const UNSEEN_ANY_GAME: i32 = 2;

/// The menu the game composed, in the order it drew it.
const MENU: [(i32, &str); 6] = [
    (582, "\"Tell me about the case again.\""),
    (
        1008,
        "[Locked] Convince Kim there's a sexy dark mystery twist in the case (white check)",
    ),
    (
        90,
        "\"I think you should know that I can't remember *anything*.\"",
    ),
    (221, "\"Uhm. I want to talk about *you*.\""),
    (697, "\"You seem to be following me.\""),
    (882, "\"Nothing\". [Leave.]"),
];

/// What the mod would draw on an option, given its own novelty and what the search found.
fn drawn(own: i32, reply: &LookAheadAnswer) -> &'static str {
    if own == UNSEEN_ANY_GAME {
        return "none";
    }

    if reply.best <= own {
        return if reply.complete { "none" } else { "gaveUp" };
    }

    if reply.best == UNSEEN_ANY_GAME {
        "orange"
    } else {
        "red"
    }
}

/// What the mod would draw on one half of a check's Pass / Fail line.
///
/// THE HALF'S OWN COLOUR FIRST, which is where an outcome differs from an option: the word
/// is coloured by what the outcome lands on directly, and then carries a marker only if the
/// search found something better BEYOND that.
fn drawn_half(half: &LookAheadAnswer) -> String {
    let colour = match half.destination {
        UNSEEN_ANY_GAME => "orange",
        UNSEEN_THIS_GAME => "red",
        _ => "plain",
    };

    if half.best > half.destination {
        let marker = if half.best == UNSEEN_ANY_GAME {
            "an orange"
        } else {
            "a red"
        };
        return format!("{colour} with {marker} asterisk");
    }

    if !half.complete {
        return format!("{colour} with a grey '*?'");
    }

    colour.to_string()
}

#[test]
fn the_kim_case_menu_as_the_engine_answers_it() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    // The same reason `scenario_suites.rs` asks for it: which skill a passive check tests
    // is a question only the actor table answers, and without it every check is undecided
    // and the world staged is a more permissive one than the run it stands for.
    if common::actors().is_none() {
        eprintln!("no actor table; skipping.");
        return;
    }
    let index = read_index(&path).expect("the shipped index reads");

    let table = suites::table();
    let suite = table.suite(SUITE);
    let scenario = suite
        .scenarios
        .first()
        .expect("the kim-case suite has its scenario");
    let conversation = scenario.conversation;

    let (graph, group) =
        build_group_graph(&index, conversation).expect("conversation 29's group builds");

    let recorded = fixtures::recorded_elsewhere_in_group(&suite.state, &group);
    let read_here = fixtures::read_in_save_group(&scenario.save, &group);
    let checks = fixtures::checks_in_save(&scenario.save, &group)
        .expect("the actor table and the full index are both present");

    let novelty_of = |node: NodeRef| {
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
    let rung: HashSet<NodeRef> = everything
        .iter()
        .copied()
        .filter(|node| novelty_of(*node) == UNSEEN_ANY_GAME)
        .collect();

    let request = LookAheadRequest {
        conversation,
        starts: MENU
            .iter()
            .map(|(entry, _)| NodeRef {
                conversation,
                entry: *entry,
            })
            .collect(),
        unseen_any_game: rung.iter().copied().collect(),
        unseen_this_game: everything
            .iter()
            .copied()
            .filter(|node| novelty_of(*node) == UNSEEN_THIS_GAME)
            .collect(),
        state_budget: suite.state_budget,
        world: WorldSnapshot {
            money: scenario.money.unwrap_or_default(),
            day_minutes: scenario.day_minutes.unwrap_or(DAY_MINUTES),
            day_counter: DAY,
            variables: fixtures::variables_in_save(&scenario.save),
            checks_pass: checks.pass,
            checks_fail: checks.fail,
            ..Default::default()
        },
        ..Default::default()
    };

    let response = answer(&index, None, &request);
    assert!(response.error.is_none(), "{:?}", response.error);

    println!();
    println!(
        "conversation {conversation}, save '{}', state '{}'",
        scenario.save, suite.state,
    );
    println!("{}", scenario.what);
    println!();

    for (position, (entry, line)) in MENU.iter().enumerate() {
        let start = NodeRef {
            conversation,
            entry: *entry,
        };

        if let Some((pass, fail)) = response.outcomes(start) {
            println!("  {}. [check ] {conversation}:{entry} {line}", position + 1);
            println!(
                "               Pass: {} (lands on {}, best {}, {})",
                drawn_half(pass),
                pass.destination,
                pass.best,
                if pass.complete { "finished" } else { "gave up" },
            );
            println!(
                "               Fail: {} (lands on {}, best {}, {})",
                drawn_half(fail),
                fail.destination,
                fail.best,
                if fail.complete { "finished" } else { "gave up" },
            );
            continue;
        }

        let reply = response
            .answers
            .iter()
            .find(|reply| reply.start == start && reply.branch.is_none())
            .unwrap_or_else(|| panic!("nothing came back for {conversation}:{entry}"));
        let own = novelty_of(start);
        println!(
            "  {}. [{:6}] {conversation}:{entry} {line}",
            position + 1,
            drawn(own, reply),
        );
        println!(
            "               own {own}, best {}, {}, witness {}, {} states over {} entries",
            reply.best,
            if reply.complete {
                "finished"
            } else {
                "gave up"
            },
            match reply.witness {
                Some(node) => format!("{}:{}", node.conversation, node.entry),
                None => "none".to_string(),
            },
            reply.states_explored,
            reply.nodes_reached,
        );
    }
    println!();
}
