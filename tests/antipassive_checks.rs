// SPDX-License-Identifier: MIT
//! Antipassive checks, as the offline fixture decides them, held to what the game was
//! measured doing.
//!
//! ## The measurement
//!
//! The kim-case run from at-trashcan and from scene-thoughts, with the mod keeping the request
//! it sent for group 29 (2026-09-13). Every entry below is an ANTIPASSIVE line - the one shown
//! when the player is not sharp enough - whose skill falls well short of its threshold, and
//! the game reported every one of them as firing, from both saves.
//!
//! ## Why these eight
//!
//! Read without the inversion, they look like checks the game passes and the sheet fails:
//! Authority 0 against 14, Perception 1 against 11. That reading was reported as a
//! disagreement nothing explained (de-t2j8). With the inversion there is none - the check
//! fails, so the line fires - and this keeps the fixture and anybody reading a capture honest
//! about which way an antipassive entry goes.

use lookahead_engine::index::{build_group_graph, read_index};

use gct_measure::common;

use common::fixtures;

/// The conversation whose group was measured.
const CONVERSATION: i32 = 29;

/// The saves the group was measured from; scene-thoughts is at-trashcan with three thoughts
/// fixed that move no check below.
const SAVES: [&str; 2] = ["at-trashcan", "scene-thoughts"];

/// Antipassive entries the game fired from both saves, as (conversation, entry).
const MEASURED_FIRING: [(i32, i32); 8] = [
    (29, 316),
    (29, 722),
    (29, 809),
    (29, 810),
    (29, 914),
    (937, 56),
    (1193, 61),
    (1193, 103),
];

#[test]
fn a_failed_antipassive_check_fires_as_the_game_fires_it() {
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

    for save in SAVES {
        let checks = fixtures::checks_in_save(save, &group)
            .expect("the actor table and the full index are both present");
        let not_firing: Vec<(i32, i32)> = MEASURED_FIRING
            .into_iter()
            .filter(|&(conversation, entry)| {
                !checks
                    .pass
                    .iter()
                    .any(|node| node.conversation == conversation && node.entry == entry)
            })
            .collect();

        assert!(
            not_firing.is_empty(),
            "from {save}, the game fired these antipassive entries and the fixture does not: \
             {not_firing:?}",
        );
    }
}
