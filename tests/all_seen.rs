// SPDX-License-Identifier: MIT
//! The two things about the all-seen suite that the shared executor cannot say.
//!
//! ## What moved out of here
//!
//! This file used to hold the all-seen claim itself - that no option is worth searching once
//! every entry is recorded - and its own list of five conversations to ask it of. Both are
//! gone. The claim is now `nothingIsWorthCrawling` in `testing/scenarios/suites.json` and
//! is run by `scenario_suites.rs`, over the same rows `tools/GameHarness` builds the
//! in-game suite from; the list is the suite's scenarios.
//!
//! That is the point of the shared definition, and this file was the clearest case of what
//! it is for: an offline test written separately from the run it stood in for, agreeing
//! with it only for as long as whoever edited one remembered the other.
//!
//! ## What is left, and why it is here
//!
//! Two claims that are about these particular conversations rather than about a fixture:
//!
//! - THE FIVE ARE THE BIGGEST, which is the whole reason they were chosen - "not even here"
//!   is a stronger statement of the cheap case than "not in some small conversation". It
//!   can only be checked against the index, so it could not live in C# beside the suite,
//!   where it used to be a comment.
//! - THE WHOLE ENGINE AGREES WITH THE PREFILTER. `scenario_suites.rs` asks
//!   `reaches_potential_improvement`, which is a prefilter and could refuse for the wrong
//!   reason; `evaluate` is what the plugin's marker actually comes from. If the two ever
//!   disagree, the prefilter is either wrong or pointless.

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::index::{build_group_graph, read_index};

mod common;

use common::suites;

/// The suite whose rows these claims are about.
const SUITE: &str = "all-seen";

/// How far down the game's conversations, by entry count, the suite is allowed to reach.
///
/// SIX, and measured rather than chosen: the five are ranks two to six of 1,501, from
/// 1,770 entries down to 1,476. The largest, 362, is not among them. A bound rather than an
/// exact list because the exact list is the definition's job and repeating it here would be
/// the drift this file exists to have removed - what is worth holding is that nobody has
/// quietly swapped in a small conversation, which is what would make the claim weak without
/// making it fail.
const BIGGEST: usize = 6;

#[test]
fn the_suite_still_asks_the_biggest_conversations() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");

    let mut sizes: Vec<(i32, usize)> = index
        .iter()
        .map(|(id, conversation)| (*id, conversation.entries.len()))
        .collect();
    sizes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let biggest: Vec<i32> = sizes.iter().take(BIGGEST).map(|(id, _)| *id).collect();
    println!(
        "the {BIGGEST} biggest: {:?}",
        sizes.iter().take(BIGGEST).collect::<Vec<_>>()
    );

    let table = suites::table();
    let suite = table.suite(SUITE);

    let small: Vec<String> = suite
        .scenarios
        .iter()
        .filter(|scenario| !biggest.contains(&scenario.conversation))
        .map(|scenario| {
            let rank = sizes
                .iter()
                .position(|(id, _)| *id == scenario.conversation)
                .map(|at: usize| (at + 1).to_string())
                .unwrap_or_else(|| "not in the index".to_string());
            format!("{} ({}) is rank {rank}", scenario.conversation, scenario.save)
        })
        .collect();

    assert!(
        small.is_empty(),
        "{SUITE} is meant to ask the biggest conversations in the game, and {}",
        small.join(", "),
    );
}

/// And the whole engine agrees, not just the short-circuit in front of it.
#[test]
fn the_engine_finds_nothing_either() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    let table = suites::table();
    let suite = table.suite(SUITE);
    let mut asked = 0;

    for scenario in &suite.scenarios {
        let conversation = scenario.conversation;
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        // ASKED OF THE LINKS RATHER THAN OF A SEARCH. What the suite claims is that there
        // is nothing TO find, which is the refusal the bridge makes before any search is
        // built - one walk of the links, and no diagram at all. Stronger as well as
        // cheaper: a search that finds nothing might be a search that gave up.
        assert_eq!(
            graph.best_linked_class(start, |_| Novelty::SeenThisGame),
            None,
            "conversation {conversation} ({}) found novelty where everything is seen",
            scenario.save,
        );
        asked += 1;
    }

    assert!(asked > 0, "{SUITE} named no conversation the index carries");
    println!("{asked} conversations searched and nothing found, as claimed");
}
