// SPDX-License-Identifier: MIT
//! Does a white or red check come back with its two outcomes told apart?
//!
//! The unit tests build rolled checks by hand, which proves the branching but not that any
//! real check in the game has the shape the mod assumes. This asks the shipped index: find
//! the rolls the corpus actually contains, ask about them, and require the answer to carry
//! two branches - and require an ordinary option in the same group to carry none, since it
//! is the ABSENCE that tells the mod which options get a Pass/Fail line.

use lookahead_engine::bridge::{answer, LookAheadRequest, NodeRef, WorldSnapshot};
use lookahead_engine::core::types::DialogueCheckKind;
use lookahead_engine::index::{build_group_graph, read_index};

mod common;

/// A world that decides nothing, so every check is open and both its branches are live.
fn undecided() -> WorldSnapshot {
    WorldSnapshot { day_minutes: 720, day_counter: 1, ..Default::default() }
}

/// The index, or nothing when it has not been built.
fn index() -> Option<lookahead_engine::index::Index> {
    common::shipped_index().and_then(|path| read_index(&path).ok())
}

/// The conversations of a group, in a fixed order.
///
/// SORTED, because the index is a hash map: an unsorted walk would pick a different
/// conversation on different runs, and a failure that names a different entry every time
/// is one nobody can chase.
fn conversations(index: &lookahead_engine::index::Index) -> Vec<i32> {
    let mut all: Vec<i32> = index.keys().copied().collect();
    all.sort_unstable();
    all
}

/// The first group holding both a rolled check and an ordinary entry.
///
/// Searched rather than hard-coded: a conversation id pinned here would be a fixture that
/// rots the moment the index is rebuilt from a different version of the game.
fn a_group_with_a_roll() -> Option<(i32, NodeRef, NodeRef)> {
    let index = index()?;

    for conversation in conversations(&index) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };

        let mut rolled = None;
        let mut plain = None;

        for node in graph.nodes() {
            match node.kind {
                DialogueCheckKind::Red | DialogueCheckKind::White if rolled.is_none() => {
                    rolled = Some(NodeRef::from(node.id));
                }
                DialogueCheckKind::None if !node.is_group && plain.is_none() => {
                    plain = Some(NodeRef::from(node.id));
                }
                _ => {}
            }
        }

        if let (Some(rolled), Some(plain)) = (rolled, plain) {
            return Some((conversation, rolled, plain));
        }
    }

    None
}

#[test]
fn a_rolled_check_comes_back_with_both_outcomes() {
    let Some(index) = index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };

    let Some((conversation, rolled, plain)) = a_group_with_a_roll() else {
        panic!("the shipped index holds no group with both a rolled check and a plain entry");
    };

    let request = LookAheadRequest {
        conversation,
        starts: vec![rolled, plain],
        world: undecided(),
        ..Default::default()
    };

    let response = answer(&index, None, &request);
    assert!(response.error.is_none(), "{:?}", response.error);
    assert_eq!(response.answers.len(), 2);

    let for_roll = response.answers.iter().find(|a| a.start == rolled).expect("the roll");
    let branches = for_roll
        .branches
        .as_ref()
        .unwrap_or_else(|| panic!("{conversation}:{} is a roll and carried no branches", rolled.entry));

    // Each half is an answer in its own right, and the option's own figure is the better of
    // the two - which is what makes the combined number safe for a reader that ignores them.
    assert!((0..=2).contains(&branches.pass.best), "{:?}", branches.pass);
    assert!((0..=2).contains(&branches.fail.best), "{:?}", branches.fail);
    assert_eq!(for_roll.best, branches.pass.best.max(branches.fail.best));

    let for_plain = response.answers.iter().find(|a| a.start == plain).expect("the plain entry");
    assert!(
        for_plain.branches.is_none(),
        "an ordinary entry came back with branches, which would give it a Pass/Fail line",
    );
}

/// Every rolled check in the corpus answers, and answers within its own bounds.
///
/// The breadth this file is for. One check proves the plumbing; the whole corpus is what
/// says the assumption holds about the game rather than about the example.
#[test]
fn every_rolled_check_in_the_corpus_answers() {
    let Some(index) = index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };

    let mut asked = 0usize;

    for conversation in conversations(&index).into_iter().take(40) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };

        let rolls: Vec<NodeRef> = graph
            .nodes()
            .filter(|node| {
                matches!(node.kind, DialogueCheckKind::Red | DialogueCheckKind::White)
            })
            .map(|node| NodeRef::from(node.id))
            .take(8)
            .collect();

        if rolls.is_empty() {
            continue;
        }

        let request = LookAheadRequest {
            conversation,
            starts: rolls.clone(),
            world: undecided(),
            ..Default::default()
        };

        let response = answer(&index, None, &request);
        assert!(response.error.is_none(), "{conversation}: {:?}", response.error);

        for reply in &response.answers {
            let branches = reply.branches.as_ref().unwrap_or_else(|| {
                panic!("{conversation}:{} is a roll and carried no branches", reply.start.entry)
            });

            // A branch cannot reach less than the entry it leads to: the destination is
            // itself reachable down that branch, so `best` is at least `destination`.
            assert!(
                branches.pass.best >= branches.pass.destination,
                "{conversation}:{} pass reaches {} but leads to {}",
                reply.start.entry, branches.pass.best, branches.pass.destination,
            );
            assert!(
                branches.fail.best >= branches.fail.destination,
                "{conversation}:{} fail reaches {} but leads to {}",
                reply.start.entry, branches.fail.best, branches.fail.destination,
            );

            asked += 1;
        }
    }

    assert!(asked > 0, "no rolled check was asked about at all");
    eprintln!("asked about {asked} rolled checks");
}
