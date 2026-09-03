// SPDX-License-Identifier: MIT
//! When every entry is already recorded, is any option worth crawling?
//!
//! It should not be, and the claim is about the ALGORITHM rather than about the game, so
//! it does not need one. An in-game run of the same claim costs a launch, five save loads
//! and several minutes of driving the response screen; this costs the time to read the
//! index.
//!
//! ## Where this came from
//!
//! Ported from the C# `AllSeenOfflineTests`, which asked it of the C# engine through
//! `tools/LookAheadOffline`. Both are being retired with the C# look-ahead - see de-i5xj -
//! and the claim is worth keeping, so it moves here rather than going with them.
//!
//! ## What the in-game suite still earns, and this cannot
//!
//! That the Harmony patch is wired up at all, that a real response menu is composed, and
//! that the marker reaches the text the game draws. Those need the game and stay there.

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::engine::engine::LookAheadEngine;
use lookahead_engine::index::{build_group_graph, read_index};

mod common;

/// The conversations the in-game all-seen suite opens.
const BIGGEST: [i32; 5] = [368, 14, 631, 28, 1030];

#[test]
fn no_option_is_worth_crawling_when_every_entry_is_recorded() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");

    let mut options = 0;
    let mut crawlable: Vec<DialogueNodeId> = Vec::new();

    for conversation in BIGGEST {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };

        for node in graph.nodes() {
            // A group is expanded in place and never scored, so it is not an option.
            if node.is_group {
                continue;
            }

            options += 1;

            // Everything seen, which is what makes this the all-seen claim: nothing
            // outranks anything, so nothing is worth walking towards.
            if LookAheadEngine::reaches_potential_improvement(
                &graph,
                node.id,
                Novelty::SeenThisGame,
                |_| Novelty::SeenThisGame,
            ) {
                crawlable.push(node.id);
            }
        }
    }

    println!("{options} options across {} conversations", BIGGEST.len());

    assert!(options > 0, "no options were examined; the index may be empty");
    assert!(
        crawlable.is_empty(),
        "{} options would still be crawled with everything seen: {:?}",
        crawlable.len(),
        crawlable.iter().take(10).collect::<Vec<_>>(),
    );
}

/// And the whole engine agrees, not just the short-circuit in front of it.
///
/// Worth asking separately. `reaches_potential_improvement` is a prefilter and could
/// refuse for the wrong reason; `evaluate` is what the plugin's marker comes from. If the
/// two ever disagree the prefilter is either wrong or pointless.
#[test]
fn the_engine_finds_nothing_either() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    for conversation in BIGGEST {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let engine = LookAheadEngine::default();
        let result = engine.evaluate(&graph, start, &world, |_| Novelty::SeenThisGame);

        assert_eq!(
            result.best,
            Novelty::SeenThisGame,
            "conversation {conversation} found novelty where everything is seen",
        );
    }
}
