// SPDX-License-Identifier: MIT
//! The counter cap hides nothing, anywhere in the game.
//!
//! ## What would be wrong if it did
//!
//! A slot that stops at [`COUNTER_CAP`] when the dialogue could push it higher, AND whose
//! guards tell values above the cap apart, is a guard answered against a number the game would
//! never hold. The search reports a definite answer either way, so nothing says so - which is
//! why this is a test rather than something somebody remembers to measure.
//!
//! ## Both halves are needed, and the second is the one that is easy to leave out
//!
//! Exceeding the cap is not by itself a defect. Every value past the largest constant a guard
//! compares a slot against answers every guard alike, so stopping there loses no distinction
//! anybody can observe. Two slots in the shipped database CAN be raised past the cap -
//! `damage:VOLITION` in the group named by conversation 640, and `reputation.kim` in those
//! named by 14 and 1177 - and neither is compared against anything near it, so both are
//! harmless. A test that checked only the arithmetic would have called them defects, and the
//! first draft of this one did.
//!
//! ## What the cap has to get past first
//!
//! It is the LAST resort rather than the rule:
//!
//! - `DataLayout::narrow_to_thresholds` bounds a slot by the largest constant a guard compares
//!   it against, plus one.
//! - A counter that cannot loop is not capped at all. It is rebased to hold the search's own
//!   contribution as a distance from whatever the save brought, so its width is the sum of the
//!   group's increments and nothing is clamped.
//!
//! `DataLayout::saturates_at_cap` is what is left over: a slot that can loop and is held as a
//! value rather than a distance.

use std::collections::{BTreeMap, BTreeSet};

use lookahead_engine::bridge::COUNTER_CAP;
use lookahead_engine::core::action::DialogueActionKind;
use lookahead_engine::index::{build_group_graph, discover_group, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;

use gct_measure::common;

#[test]
fn the_counter_cap_hides_nothing_in_the_whole_game() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");

    // ONE GROUP PER SET OF CONVERSATIONS. `discover_group` answers for a conversation, and a
    // group of six answers the same six times - so the reach is what a group is named by.
    //
    // NAMED BY ITS SMALLEST MEMBER, not by whichever conversation arrived first: the index
    // holds its conversations in a `HashMap` and the order it offers them is a fact about the
    // process, so a representative taken from that order changes between runs and so does
    // every group id this prints.
    let mut groups: BTreeMap<BTreeSet<i32>, i32> = BTreeMap::new();
    for conversation in index.keys() {
        let reach: BTreeSet<i32> = discover_group(&index, *conversation).into_iter().collect();
        groups
            .entry(reach)
            .and_modify(|named| *named = (*named).min(*conversation))
            .or_insert(*conversation);
    }

    let mut observable: BTreeMap<String, Vec<i32>> = BTreeMap::new();
    let mut harmless: BTreeMap<String, Vec<i32>> = BTreeMap::new();
    let mut capped = 0;

    for start in groups.values() {
        let Ok((graph, _)) = build_group_graph(&index, *start) else {
            continue;
        };
        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
        let symbols = graph.symbols().clone();
        let compared = DataLayout::largest_compared(&graph);
        let unbounded = DataLayout::unbounded_reads(&graph);

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
            capped += 1;
            if sum <= COUNTER_CAP {
                continue;
            }

            // A slot read in a shape no constant describes counts as distinguishing: nothing
            // here can say where its distinctions stop, and a cap over an unknown is a guess.
            let distinguishes = match compared.get(&slot) {
                Some(high) => i32::try_from(*high).is_ok_and(|high| high >= COUNTER_CAP),
                None => unbounded.contains(&slot),
            };

            let name = symbols
                .name_of(slot)
                .map_or_else(|| format!("slot {slot}"), str::to_string);
            let into = if distinguishes {
                &mut observable
            } else {
                &mut harmless
            };
            into.entry(name).or_default().push(*start);
        }
    }

    println!(
        "{} groups, {capped} capped counter slot(s); {} raised past the cap, {} of them \
         observable",
        groups.len(),
        harmless.len() + observable.len(),
        observable.len(),
    );
    for (name, starts) in &harmless {
        println!("  raised past the cap but never compared near it: {name} in {starts:?}");
    }

    assert!(
        observable.is_empty(),
        "these slots can be raised past {COUNTER_CAP} AND are compared against something at \
         least that large, so the search is answering a guard against a number the game would \
         never hold: {observable:?}. See de-qkng, and do not simply add them to a list - this \
         is the condition the cap exists to avoid",
    );
}
