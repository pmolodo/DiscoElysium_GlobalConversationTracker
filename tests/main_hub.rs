// SPDX-License-Identifier: MIT
//! The hub stack, held against the conversations whose writers named their main hub.
//!
//! `symbolic::hub` reads links and nothing else. The full index still carries each entry's
//! title, and 95 conversations have exactly one entry titled like a main hub, so those are the
//! check. Every titled hub must be a hub CANDIDATE. And a player walking the shortest route from
//! the conversation's start to it should find it at the BOTTOM of the stack on arrival - no
//! other candidate passed earlier in the same loop still under it - which is reported and held
//! to a floor, since an introduction can pass a hub of its own inside the main loop.

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{Index, build_group_graph, discover_group, read_index};
use lookahead_engine::symbolic::hub::{Hubs, follow};
use lookahead_engine::symbolic::order::IterationOrder;

use gct_measure::common;

/// The field an entry's title is in.
const TITLE_FIELD: &str = "Title";

/// How many characters may separate "main" and "hub" in a title that names a main hub - the
/// underscore in "Garte_main_hub", the " - " in "ROSEMARY - MAIN HUB".
const TITLE_GAP: usize = 3;

/// Titles that are not a hub entry at all: a jump to one, or a condition naming its variable.
const NOT_A_HUB_PREFIXES: [&str; 3] = ["jump to", "variable", "!("];

/// Titles that name a hub that is explicitly not the main one.
const NOT_MAIN_WORDS: [&str; 4] = ["mini", "sub-main", "mainly", "little"];

/// The fewest titled conversations whose titled hub must be outermost on arrival.
///
/// 92 of 95 when this was set, 2026-09-14. In the other three - 35, 368 and 676 - the route in
/// passes one other candidate inside the main loop first, and the titled hub is stacked on it.
const OUTERMOST_ON_ARRIVAL_AT_LEAST: usize = 92;

/// Whether `second` starts within `TITLE_GAP` characters after some occurrence of `first`.
fn near(text: &str, first: &str, second: &str) -> bool {
    text.match_indices(first).any(|(at, _)| {
        let after = &text[at + first.len()..];
        (0..=TITLE_GAP).any(|gap| {
            after
                .get(gap..)
                .is_some_and(|rest| rest.starts_with(second))
        })
    })
}

fn names_a_main_hub(title: &str) -> bool {
    let title = title.to_lowercase();
    if NOT_A_HUB_PREFIXES
        .iter()
        .any(|prefix| title.starts_with(prefix))
        || NOT_MAIN_WORDS.iter().any(|word| title.contains(word))
    {
        return false;
    }
    near(&title, "main", "hub") || near(&title, "hub", "main")
}

/// Every conversation with exactly one entry titled like a main hub, and that entry.
fn titled_hubs(index: &Index) -> HashMap<i32, DialogueNodeId> {
    index
        .values()
        .filter_map(|conversation| {
            let titled: Vec<i32> = conversation
                .entries
                .iter()
                .filter(|entry| {
                    entry
                        .fields
                        .get(TITLE_FIELD)
                        .is_some_and(|title| names_a_main_hub(title))
                })
                .map(|entry| entry.id)
                .collect();
            match titled.as_slice() {
                [only] => Some((conversation.id, DialogueNodeId::new(conversation.id, *only))),
                _ => None,
            }
        })
        .collect()
}

/// Where a conversation starts: entry 0, or its lowest entry where it has none.
fn start_of(graph: &LookAheadGraph, conversation: i32) -> Option<DialogueNodeId> {
    graph
        .nodes()
        .map(|node| node.id)
        .filter(|id| id.conversation_id == conversation)
        .min_by_key(|id| id.entry_id)
}

/// The shortest route along links from `from` to `to`, both included.
fn route(
    graph: &LookAheadGraph,
    from: DialogueNodeId,
    to: DialogueNodeId,
) -> Option<Vec<DialogueNodeId>> {
    let mut came_from: HashMap<DialogueNodeId, DialogueNodeId> = HashMap::new();
    let mut seen = HashSet::from([from]);
    let mut queue = VecDeque::from([from]);
    while let Some(id) = queue.pop_front() {
        if id == to {
            let mut path = vec![to];
            let mut at = to;
            while let Some(&before) = came_from.get(&at) {
                path.push(before);
                at = before;
            }
            path.reverse();
            return Some(path);
        }
        for &child in graph
            .get(id)
            .map(|node| node.links.as_slice())
            .unwrap_or_default()
        {
            if graph.get(child).is_some() && seen.insert(child) {
                came_from.insert(child, id);
                queue.push_back(child);
            }
        }
    }
    None
}

#[test]
fn every_titled_main_hub_is_a_candidate_and_usually_outermost_on_arrival() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let titled = titled_hubs(&index);

    // ONE GRAPH PER DISTINCT GROUP, built and dropped in turn: a conversation's answer depends
    // only on what its own start reaches, which its own closure holds.
    let mut by_group: HashMap<Vec<i32>, Vec<i32>> = HashMap::new();
    for &conversation in index.keys() {
        by_group
            .entry(discover_group(&index, conversation))
            .or_default()
            .push(conversation);
    }

    let mut not_candidates = Vec::new();
    let mut unreached = Vec::new();
    let mut under_another = Vec::new();
    let mut outermost_on_arrival = 0;
    let mut with_a_candidate = 0;
    for members in by_group.values() {
        let (graph, _) = build_group_graph(&index, members[0]).expect("every group builds");
        let order = IterationOrder::of(&graph);
        let hubs = Hubs::of(&graph);
        for &conversation in members {
            if hubs
                .candidates()
                .iter()
                .any(|id| id.conversation_id == conversation)
            {
                with_a_candidate += 1;
            }
            let Some(&hub) = titled.get(&conversation) else {
                continue;
            };
            if !hubs.is_candidate(hub) {
                not_candidates.push(hub);
                continue;
            }
            let Some(walk) =
                start_of(&graph, conversation).and_then(|start| route(&graph, start, hub))
            else {
                unreached.push(hub);
                continue;
            };
            let stack = follow(&order, &hubs, &walk);
            if stack.outermost() == Some(hub) {
                outermost_on_arrival += 1;
            } else {
                under_another.push(format!("{hub} titled, stack {:?}", stack.hubs()));
            }
        }
    }

    println!(
        "{} titled main hubs: {} candidates, {outermost_on_arrival} outermost on arrival",
        titled.len(),
        titled.len() - not_candidates.len(),
    );
    println!("not reached from the start: {unreached:?}");
    println!("under another hub on arrival: {under_another:?}");
    println!(
        "{with_a_candidate} of {} conversations in the game hold a candidate",
        index.len()
    );

    assert!(
        not_candidates.is_empty(),
        "titled hubs that are not candidates: {not_candidates:?}"
    );
    assert!(
        outermost_on_arrival >= OUTERMOST_ON_ARRIVAL_AT_LEAST,
        "outermost on arrival in {outermost_on_arrival}, fewer than \
         {OUTERMOST_ON_ARRIVAL_AT_LEAST}"
    );
}
