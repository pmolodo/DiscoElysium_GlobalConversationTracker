// SPDX-License-Identifier: MIT
//! Which counters the cap actually truncates, over the whole game.
//!
//! ## Why this is a test and not a measurement
//!
//! A slot that stops at [`COUNTER_CAP`] when the dialogue could push it higher is a guard
//! answered against a number the game would never hold - the quiet kind of wrong, since the
//! search reports a definite answer either way. Which slots those are was the result of a
//! measurement somebody had to remember to run; a test fails when the set changes.
//!
//! IT EARNED ITS KEEP AT ONCE. The measurement it replaces found one such slot over the 429
//! groups `group_list` names; asked of every group in the index it finds two, and the second
//! is in groups that can hold a menu just as the first is. See [`KNOWN`].
//!
//! ## What the cap is, and why so little reaches it
//!
//! It is the LAST resort rather than the rule, and two things get there first:
//!
//! - `DataLayout::narrow_to_thresholds` bounds a slot by the largest constant a guard compares
//!   it against, plus one. Above that ceiling every value answers every guard alike, so
//!   stopping there is EXACT rather than a truncation.
//! - A counter that cannot loop is not capped at all. It is rebased to hold the search's own
//!   contribution as a distance from whatever the save brought, so its width is the sum of the
//!   group's increments and nothing is clamped.
//!
//! `DataLayout::saturates_at_cap` is what is left over: a slot that can loop and whose guards
//! distinguish more values than the narrowing could bound it to.

use std::collections::{BTreeMap, BTreeSet};

use lookahead_engine::bridge::COUNTER_CAP;
use lookahead_engine::core::action::DialogueActionKind;
use lookahead_engine::index::{build_group_graph, discover_group, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;

use gct_measure::common;

/// The slots in the shipped database the cap truncates.
///
/// Named rather than counted, so a new one fails LOUDLY with its own name instead of turning a
/// 2 into a 3.
///
/// `damage:VOLITION` is truncated in the group from conversation 1220 and `reputation.kim` in
/// those from 1177 and 464. All three can hold a menu, so a crawl can run in each.
///
/// WHETHER EITHER IS A DEFECT is a different question from whether the arithmetic can reach
/// it, and this cannot answer it: `damage:VOLITION` needs 36 increment sites all firing, and
/// what decides whether the truncation is observable at all is whether any guard distinguishes
/// values above the cap. See de-qkng.
const KNOWN: [&str; 2] = ["damage:VOLITION", "reputation.kim"];

#[test]
fn the_counter_cap_truncates_two_slots_in_the_whole_game() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");

    // ONE GROUP PER SET OF CONVERSATIONS. `discover_group` answers for a conversation, and a
    // group of nine answers the same nine times - so the reach is what a group is named by.
    let mut groups: BTreeMap<BTreeSet<i32>, i32> = BTreeMap::new();
    for conversation in index.keys() {
        let reach: BTreeSet<i32> = discover_group(&index, *conversation).into_iter().collect();
        groups.entry(reach).or_insert(*conversation);
    }

    let mut truncated: BTreeMap<String, Vec<i32>> = BTreeMap::new();
    let mut counters = 0;
    for start in groups.values() {
        let Ok((graph, _)) = build_group_graph(&index, *start) else {
            continue;
        };
        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
        let symbols = graph.symbols().clone();

        // What the group can add to each slot, every site firing.
        let mut raised: BTreeMap<usize, i32> = BTreeMap::new();
        for node in graph.nodes() {
            for action in node.all_actions() {
                if action.kind() != DialogueActionKind::Increment {
                    continue;
                }
                if let Ok(slot) = usize::try_from(action.slot()) {
                    *raised.entry(slot).or_default() += action.value().max(0);
                }
            }
        }

        for (slot, sum) in raised {
            if !layout.saturates_at_cap(slot) || layout.slot(slot).is_none() {
                continue;
            }
            counters += 1;
            if sum > COUNTER_CAP {
                let name = symbols
                    .name_of(slot)
                    .map_or_else(|| format!("slot {slot}"), str::to_string);
                truncated.entry(name).or_default().push(*start);
            }
        }
    }

    let found: Vec<&str> = truncated.keys().map(String::as_str).collect();
    println!(
        "{} groups, {counters} capped counter slot(s), {} truncated",
        groups.len(),
        truncated.len(),
    );
    for (name, starts) in &truncated {
        println!("  {name}: in {} group(s), from {starts:?}", starts.len());
    }

    assert_eq!(
        found, KNOWN,
        "the cap truncates a different set of slots than it did when this was measured - a \
         slot here is one the dialogue can raise past {COUNTER_CAP}, so a guard comparing it \
         above that is answered against a number the game would never hold. See de-qkng before \
         changing this list",
    );
}
