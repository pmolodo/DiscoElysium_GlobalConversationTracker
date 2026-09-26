// SPDX-License-Identifier: MIT
//! Hardcore mode doubles what Frittte charges for magnesium, and the look-ahead pays it.
//!
//! Two saves a single field apart: `at-trashcan`, a playthrough in HARDCORE mode, and
//! `at-trashcan-normal-mode`, the same save with `gameModeState.gameMode` set to NORMAL. Each
//! is asked about the pharmacy, conversation 475, from the option beside the purchases, with
//! 100 centimes in hand and only the magnesium Frittte hands over left unseen. Magnesium is
//! 90 in normal mode and 180 in hardcore, and Nosaphed beside it the same - see
//! `core::price`.

use std::path::PathBuf;

use lookahead_engine::bridge::{
    LookAheadRequest, LookAheadResponse, NodeRef, WorldRawData, answer,
};
use lookahead_engine::index::{Index, build_group_graph, read_index};
use lookahead_engine::service::Service;

use gct_measure::common;

use common::fixtures;

/// Frittte's pharmacy.
const PHARMACY: i32 = 475;

/// "Who is Saint-Batiste?", which returns to the hub the purchases are offered from.
const START: NodeRef = NodeRef {
    conversation: PHARMACY,
    entry: 15,
};

/// "Mkay, here." with `GainItem("magnesium")`, beyond the 90-centime purchase.
const MAGNESIUM_HANDED_OVER: NodeRef = NodeRef {
    conversation: PHARMACY,
    entry: 33,
};

/// Enough for the purchase at its normal price, and not at double it.
const MONEY: i32 = 100;

const UNSEEN_ANY_GAME: i32 = 2;

const NORMAL_SAVE: &str = "at-trashcan-normal-mode";
const HARDCORE_SAVE: &str = "at-trashcan";

/// The shipped index, where this checkout has one and the actor table beside it.
fn shipped() -> Option<(PathBuf, Index)> {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return None;
    };
    if common::actors().is_none() {
        eprintln!("no actor table; skipping.");
        return None;
    }
    let index = read_index(&path).expect("the shipped index reads");
    Some((path, index))
}

/// The request the pharmacy's start is asked with in `save`.
fn request_in(index: &Index, save: &str) -> LookAheadRequest {
    let (graph, group) = build_group_graph(index, PHARMACY).expect("the pharmacy's group builds");
    let asked = lookahead_engine::bridge::questions_of(&graph, group.clone());
    let holdings = fixtures::holdings_in_save(save);
    let checks = fixtures::checks_in_save(save, &group)
        .expect("the actor table and the full index are both present");

    LookAheadRequest {
        conversation: PHARMACY,
        starts: vec![START],
        // ONE ENTRY UNREAD ANYWHERE, so the rest of the group has been shown by some
        // playthrough - which is what the request carries. See `world::seen_state`.
        seen_any_game: graph
            .nodes()
            .map(|node| NodeRef::from(node.id))
            .filter(|node| *node != MAGNESIUM_HANDED_OVER)
            .collect(),
        world: WorldRawData {
            money: MONEY,
            day_minutes: holdings.day_minutes,
            day_counter: holdings.day_counter,
            clock_locked: fixtures::clock_locked_in_save(
                save,
                holdings.day_minutes,
                holdings.day_counter,
            ),
            data_values: holdings.data_for(&asked),
            items: holdings.items_asked(&asked),
            thoughts: holdings.thoughts_asked(&asked),
            variables: fixtures::variables_sent(save, &asked),
            failed_white_checks: fixtures::failed_white_checks_in_save(save),
            checks_pass: checks.pass,
            checks_fail: checks.fail,
            check_margins: checks.margins,
            red_checks_fail: fixtures::passive_thoughts_in_save(save).red_checks_fail,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// The best the start reached, from a finished search.
fn best_of(response: &LookAheadResponse, save: &str) -> i32 {
    assert!(response.error.is_none(), "{save}: {:?}", response.error);
    let reply = response
        .find(START, None)
        .unwrap_or_else(|| panic!("{save}: no answer for the start"));
    assert!(reply.complete, "{save}: the search did not finish");
    reply.best
}

#[test]
fn magnesium_is_affordable_in_normal_mode() {
    let Some((_, index)) = shipped() else { return };
    let response = answer(
        &index,
        common::declared(),
        None,
        &request_in(&index, NORMAL_SAVE),
    );
    assert_eq!(best_of(&response, NORMAL_SAVE), UNSEEN_ANY_GAME);
}

#[test]
fn magnesium_costs_double_in_hardcore_mode() {
    let Some((_, index)) = shipped() else { return };
    let response = answer(
        &index,
        common::declared(),
        None,
        &request_in(&index, HARDCORE_SAVE),
    );
    assert!(
        best_of(&response, HARDCORE_SAVE) < UNSEEN_ANY_GAME,
        "reached the magnesium at 180 with {MONEY}"
    );
}

/// Every purchase the game scales, and nothing else - the pharmacy's four healing items and the
/// drinks and smokes at two counters. Conversation 28's room reaches a Commodore Red only past
/// guards, so it is held at its `ClickCost`.
#[test]
fn the_scaled_purchases_are_the_ones_the_game_scales() {
    use lookahead_engine::core::price::PriceScale::{Drug, Healing};

    let Some((_, index)) = shipped() else { return };
    let mut scaled = Vec::new();
    for conversation in index.values() {
        for entry in &conversation.entries {
            let (cost, _, _) = lookahead_engine::index::parse_cost(&entry.fields);
            if cost <= 0 {
                continue;
            }
            if let Some(scale) =
                lookahead_engine::index::price::scale_of(&conversation.entries, entry)
            {
                scaled.push((conversation.id, entry.id, scale));
            }
        }
    }
    scaled.sort_by_key(|&(conversation, entry, _)| (conversation, entry));

    assert_eq!(
        scaled,
        vec![
            (475, 2, Healing),
            (475, 14, Healing),
            (475, 24, Healing),
            (475, 45, Healing),
            (552, 126, Drug),
            (552, 137, Drug),
            (902, 21, Drug),
            (902, 22, Drug),
            (902, 48, Drug),
        ]
    );
}

/// The shipped path keeps a priced graph between requests, so a change of mode has to reprice
/// it rather than answer from the last mode's prices - in either direction.
#[test]
fn a_kept_workspace_is_repriced_when_the_mode_changes() {
    let Some((path, index)) = shipped() else {
        return;
    };
    let service = Service::open(&path, &common::declared_path()).expect("the service opens");

    for (save, reaches) in [
        (NORMAL_SAVE, true),
        (HARDCORE_SAVE, false),
        (NORMAL_SAVE, true),
    ] {
        let response = service.answer_request(request_in(&index, save));
        assert_eq!(
            best_of(&response, save) == UNSEEN_ANY_GAME,
            reaches,
            "{save} through the kept workspace"
        );
    }
}
