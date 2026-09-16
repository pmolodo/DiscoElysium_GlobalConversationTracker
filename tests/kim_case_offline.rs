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
use common::repo_root;
use common::suites;

/// The suite this reads its fixture out of.
const SUITE: &str = "kim-case";

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
    // THE FIRST SCENARIO UNLESS DEGCT_SAVE NAMES ANOTHER, so a variant of the trash can save
    // can be reported the same way without a second copy of this test.
    let wanted = lookahead_engine::core::env::var("SAVE").ok();
    let scenario = suite
        .scenarios
        .iter()
        .find(|scenario| wanted.as_deref().is_none_or(|save| scenario.save == save))
        .unwrap_or_else(|| panic!("the kim-case suite has no scenario for {wanted:?}"));
    let conversation = scenario.conversation;

    let (graph, group) =
        build_group_graph(&index, conversation).expect("conversation 29's group builds");

    let recorded = fixtures::recorded_elsewhere_in_group(&suite.state, &group);
    let read_here = fixtures::read_in_save_group(&scenario.save, &group);
    let checks = fixtures::checks_in_save(&scenario.save, &group)
        .expect("the actor table and the full index are both present");
    let holdings = fixtures::holdings_in_save(&scenario.save);

    let asked = lookahead_engine::bridge::questions_of(&graph, group.clone());
    let answered = holdings.answers_to(&asked.queries);

    // WHAT THE WORLD COULD NOT BE TOLD, on every run of this report rather than in a note
    // that goes stale. The plugin answers every one of these from the running game; a
    // question left unanswered here is a question this menu is answered more permissively
    // than the game answers it, and the list is the shortest statement of what de-bnh6 has
    // left to do.
    let unanswered: Vec<&String> = asked
        .queries
        .iter()
        .filter(|key| !answered.contains_key(*key))
        .collect();

    println!(
        "\nthe group asks {} variables, {} queries, {} items, {} tasks and {} thoughts",
        asked.variables.len(),
        asked.queries.len(),
        asked.items.len(),
        asked.tasks.len(),
        asked.thoughts.len(),
    );
    println!(
        "unanswered offline: {} of {} queries",
        unanswered.len(),
        asked.queries.len(),
    );
    println!("  {unanswered:?}");

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
            // THE SAVE'S OWN WORLD, down to the hour it was saved at: a guard comparing the
            // time answers differently at midnight, and a report taken at some other hour
            // than the game was at would be a different menu that happens to share options.
            money: scenario.money.unwrap_or(holdings.money),
            day_minutes: scenario.day_minutes.unwrap_or(holdings.day_minutes),
            day_counter: holdings.day_counter,
            // LOCKED, as the plugin sends it: nothing the game exposes to Lua says whether
            // its clock is locked, so the mod reports locked and a crawl leaves the hour
            // where it found it. A fixture that let time pass would be staging a world no
            // run of the game is in. See de-3jec.
            clock_locked: true,
            queries: answered,
            items: holdings.items,
            tasks: holdings.tasks,
            thoughts: holdings.thoughts,
            variables: fixtures::variables_sent(&scenario.save, &asked),
            // WHAT THIS SAVE HAS ALREADY SHOWN, which the engine seeds its seen slots from
            // and which no offline run has ever sent. The same set the novelty rungs are
            // built out of, put where a guard on having been shown can read it: without it
            // every once-only entry starts unfired, and a route that is spent in the save
            // is open to the crawl. Found by diffing against what the game sends - de-v702.
            seen: read_here
                .iter()
                .map(|&(conversation, entry)| NodeRef {
                    conversation,
                    entry,
                })
                .collect(),
            checks_pass: checks.pass,
            checks_fail: checks.fail,
            // WHAT THE SAVE'S THOUGHTS DO TO RED CHECKS, as the plugin reads it from the game:
            // a cooking precarious_world forces every red roll to fail.
            red_checks_fail: fixtures::passive_thoughts_in_save(&scenario.save).red_checks_fail,
            ..Default::default()
        },
        ..Default::default()
    };

    // THE WORLD THIS ASSEMBLED, written out beside the answer. The game writes its own -
    // the mod's KeepLookAheadRequests setting, which this suite turns on - and the two are
    // a diff apart, which is the only way to find out which FIELD the two executors
    // disagree about rather than which marker. See de-v702.
    let written = repo_root().join(".build").join("offline-requests");
    std::fs::create_dir_all(&written).expect("a folder to write the request into");
    std::fs::write(
        written.join(format!("look-ahead-request-{conversation}.json")),
        serde_json::to_string(&request).expect("the request serialises"),
    )
    .expect("the request writes");

    let response = answer(&index, None, &request);
    assert!(response.error.is_none(), "{:?}", response.error);

    println!();
    println!(
        "conversation {conversation}, save '{}', state '{}'",
        scenario.save, suite.state,
    );
    // THE FIRST STOP'S, which is this scenario's: one menu is checked here, against MENU
    // below. The same reading the C# side takes, where `Why` is `Stops[0].What`.
    let what = &scenario
        .stops
        .first()
        .expect("a scenario names at least one stop")
        .what;
    println!("{what}");
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
