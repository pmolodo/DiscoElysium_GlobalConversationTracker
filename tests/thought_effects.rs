// SPDX-License-Identifier: MIT
//! What internalised thoughts do to passive checks, as the offline fixture decides them, held
//! to what the game was measured doing.
//!
//! ## The measurement
//!
//! `scene-thoughts` is at-trashcan with the only three thoughts in the game that carry a
//! passive-check effect - lawbringer, remote_viewer and age_bracket - FIXED and slotted, and
//! nothing else changed. Run in game on 2026-09-13 (de-2jlj), with the mod keeping the request
//! it sent for group 29, the two saves' checks differed in exactly the nine entries below,
//! every one failing from at-trashcan and passing from scene-thoughts:
//!
//! - eight at a threshold of 8 for a psyche or fysique skill of 1, which a threshold lowered
//!   by one lets through - remote_viewer's and age_bracket's modifiers;
//! - 29:276, Hand/Eye Coordination of 2 against 13, which only lawbringer forcing the skill
//!   through explains.
//!
//! So the claim is the whole difference, not a subset: the fixture must move these nine and no
//! other check.

use std::collections::{BTreeSet, HashMap};

use lookahead_engine::index::{build_group_graph, read_index};

mod common;

use common::fixtures;

/// The conversation whose group was measured.
const CONVERSATION: i32 = 29;

/// Every check the game moved, as (conversation, entry).
const MEASURED: [(i32, i32); 9] = [
    (29, 276),
    (29, 379),
    (29, 385),
    (29, 527),
    (29, 597),
    (29, 1085),
    (680, 31),
    (930, 37),
    (1019, 15),
];

#[test]
fn internalised_thoughts_move_exactly_the_checks_the_game_moved() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    if common::actors().is_none() {
        eprintln!("no actor table; skipping.");
        return;
    }
    let index = read_index(&path).expect("the shipped index reads");
    let (_, group) =
        build_group_graph(&index, CONVERSATION).expect("conversation 29's group builds");

    let plain = fixtures::checks_in_save("at-trashcan", &group)
        .expect("the actor table and the full index are both present");
    let thinking = fixtures::checks_in_save("scene-thoughts", &group)
        .expect("the actor table and the full index are both present");

    let now_passing: BTreeSet<(i32, i32)> = plain
        .fail
        .iter()
        .filter(|node| thinking.pass.contains(node))
        .map(|node| (node.conversation, node.entry))
        .collect();
    let now_failing: BTreeSet<(i32, i32)> = plain
        .pass
        .iter()
        .filter(|node| thinking.fail.contains(node))
        .map(|node| (node.conversation, node.entry))
        .collect();

    assert_eq!(
        now_passing,
        MEASURED.into_iter().collect::<BTreeSet<_>>(),
        "the checks the thoughts let through"
    );
    assert!(
        now_failing.is_empty(),
        "no thought makes a check harder, yet these now fail: {now_failing:?}"
    );
}

/// A research effect applies while its thought is cooking, and lifts once it is fixed.
///
/// No thought in the shipped game carries a research-phase passive effect, so the table here
/// is made up and the rule is held to the game's load code instead of a measurement:
/// `CharacterSheetPersister.DeserializeItemsAndThoughts` applies a COOKING thought's research
/// effects and a FIXED thought's completion effects, and neither applies the other list.
#[test]
fn a_research_effect_applies_only_while_its_thought_is_cooking() {
    const THOUGHT: &str = "made_up";
    const SHIFT: i32 = -2;

    let effects = [serde_json::json!({
        "ability": "PSY",
        "amount": SHIFT,
        "effect": "PASSIVE_TARGET_MODIFIER",
        "phase": "research",
        "thought": THOUGHT,
    })];
    let ability_of = HashMap::from([
        ("LOGIC".to_string(), "INT".to_string()),
        ("VOLITION".to_string(), "PSY".to_string()),
    ]);

    for (state, expected) in [("COOKING", SHIFT), ("FIXED", 0), ("UNKNOWN", 0)] {
        let states = HashMap::from([(THOUGHT.to_string(), state.to_string())]);
        let thoughts = fixtures::passive_thoughts(&states, &ability_of, &effects);
        assert_eq!(
            thoughts.threshold_shift("VOLITION"),
            expected,
            "a psyche skill, with the thought {state}"
        );
        assert_eq!(
            thoughts.threshold_shift("LOGIC"),
            0,
            "an intellect skill, with the thought {state}"
        );
    }
}

/// Red checks are forced to fail only while the thought forcing it is cooking.
///
/// The row is the one the shipped game carries - precarious_world's research effect - and the
/// cabinet is made up, since no committed save has the thought in each state.
#[test]
fn red_checks_fail_only_while_the_thought_forcing_it_is_cooking() {
    const THOUGHT: &str = "precarious_world";

    let effects = [serde_json::json!({
        "effect": "THC_RED_CHECK_FAILURE",
        "phase": "research",
        "thought": THOUGHT,
    })];

    for (state, expected) in [("COOKING", true), ("FIXED", false), ("UNKNOWN", false)] {
        let states = HashMap::from([(THOUGHT.to_string(), state.to_string())]);
        let thoughts = fixtures::passive_thoughts(&states, &HashMap::new(), &effects);
        assert_eq!(thoughts.red_checks_fail, expected, "with {THOUGHT} {state}");
    }
}
