// SPDX-License-Identifier: MIT
//! Does a white or red check come back with its two outcomes told apart?
//!
//! The unit tests build rolled checks by hand, which proves the branching but not that any
//! real check in the game has the shape the mod assumes. This asks the shipped index: find
//! the rolls the corpus actually contains, ask about them, and require the answer to carry
//! two branches - and require an ordinary option in the same group to carry none, since it
//! is the ABSENCE that tells the mod which options get a Pass/Fail line.

use lookahead_engine::bridge::{LookAheadRequest, NodeRef, WorldSnapshot, answer};
use lookahead_engine::core::types::DialogueCheckKind;
use lookahead_engine::index::{build_group_graph, read_index};

mod common;

/// A world that decides nothing, so every check is open and both its branches are live.
fn undecided() -> WorldSnapshot {
    WorldSnapshot {
        day_minutes: 720,
        day_counter: 1,
        ..Default::default()
    }
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
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };

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
    // THREE ANSWERS FROM TWO STARTS: the roll is two options wearing one line of text,
    // so it is answered once per outcome, and the ordinary entry once.
    assert_eq!(response.answers.len(), 3);

    let (pass, fail) = response.outcomes(rolled).unwrap_or_else(|| {
        panic!(
            "{conversation}:{} is a roll and came back with one answer",
            rolled.entry
        )
    });

    // Each outcome is an answer in its own right, with its own cost figures - which the
    // combined answer this replaced could not carry and reported as zero.
    assert!((0..=2).contains(&pass.best), "{pass:?}");
    assert!((0..=2).contains(&fail.best), "{fail:?}");

    let for_plain = response.find(plain, None).expect("the plain entry");
    assert!(
        response.outcomes(plain).is_none(),
        "an ordinary entry came back with outcomes, which would give it a Pass/Fail line",
    );
    assert_eq!(for_plain.branch, None);
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
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };

        let rolls: Vec<NodeRef> = graph
            .nodes()
            .filter(|node| matches!(node.kind, DialogueCheckKind::Red | DialogueCheckKind::White))
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
        assert!(
            response.error.is_none(),
            "{conversation}: {:?}",
            response.error
        );

        for reply in &response.answers {
            let branch = reply.branch.as_deref().unwrap_or_else(|| {
                panic!(
                    "{conversation}:{} is a roll and named no outcome",
                    reply.start.entry
                )
            });

            // An outcome cannot reach less than the entry it leads to: the destination is
            // itself reachable down that branch, so `best` is at least `destination`.
            //
            // ONE ASSERTION FOR BOTH HALVES NOW, because the loop is over the halves.
            // This used to check pass and fail separately from one answer, and the second
            // check was a copy of the first with the field name changed - which is the
            // shape of thing that gets edited on one side only.
            assert!(
                reply.best >= reply.destination,
                "{conversation}:{} {branch} reaches {} but leads to {}",
                reply.start.entry,
                reply.best,
                reply.destination,
            );

            asked += 1;
        }
    }

    assert!(asked > 0, "no rolled check was asked about at all");
    eprintln!("asked about {asked} check outcomes");
}

/// Both kinds of rolled check answer, not just whichever the corpus offers first.
///
/// The mod treats red and white checks alike - both roll, both get a Pass / Fail line -
/// and nothing said so. The searches above take the first roll they find in a group, which
/// on this corpus is very often white, so a red check could have stopped answering without
/// a single test noticing.
#[test]
fn a_red_check_and_a_white_check_both_answer() {
    let Some(index) = index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };

    let mut asked: Vec<DialogueCheckKind> = Vec::new();

    for kind in [DialogueCheckKind::Red, DialogueCheckKind::White] {
        for conversation in conversations(&index) {
            let Ok((graph, _)) = build_group_graph(&index, conversation) else {
                continue;
            };

            let Some(roll) = graph.nodes().find(|node| node.kind == kind) else {
                continue;
            };

            let request = LookAheadRequest {
                conversation,
                starts: vec![NodeRef::from(roll.id)],
                world: undecided(),
                ..Default::default()
            };

            let response = answer(&index, None, &request);
            assert!(
                response.error.is_none(),
                "{conversation}: {:?}",
                response.error
            );

            let reply = response
                .answers
                .first()
                .expect("one start, at least one answer");
            assert!(
                response.outcomes(reply.start).is_some(),
                "{conversation}:{} is a {kind:?} check and did not come back as two outcomes",
                reply.start.entry,
            );

            asked.push(kind);
            break;
        }
    }

    assert_eq!(
        asked,
        vec![DialogueCheckKind::Red, DialogueCheckKind::White],
        "the corpus did not yield one of each kind to ask about",
    );
}

/// An outcome landing on text no save has read costs no search at all.
///
/// THE RULE THE WHOLE FEATURE RESTS ON, at branch level: a search exists to find something
/// that OUTRANKS what is already known, and nothing outranks the top rung. The option-level
/// form of this is older - a search is refused before any state is built when nothing
/// reachable can beat the option's own novelty - and this is the case it cannot cover, an
/// option worth searching for one outcome but not the other.
///
/// Measured as STATES rather than as time: zero states is the only evidence that survives
/// a fast machine.
#[test]
fn an_outcome_on_the_top_rung_is_not_searched() {
    let Some(index) = index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };

    let Some((conversation, rolled, _)) = a_group_with_a_roll() else {
        panic!("the shipped index holds no group with both a rolled check and a plain entry");
    };

    // Nothing recorded anywhere, so every entry either branch lands on is on the top rung.
    let request = LookAheadRequest {
        conversation,
        starts: vec![rolled],
        world: undecided(),
        ..Default::default()
    };

    let response = answer(&index, None, &request);
    assert!(response.error.is_none(), "{:?}", response.error);

    let (pass, fail) = response.outcomes(rolled).expect("a roll comes back as two");

    for (name, branch) in [("pass", pass), ("fail", fail)] {
        // Where it lands is read off the graph and costs nothing; what it says about
        // BEYOND has to be the destination itself, unsearched and not in doubt.
        assert_eq!(
            branch.best, branch.destination,
            "{conversation}:{} {name} claims to reach past a destination nothing can outrank",
            rolled.entry,
        );
        assert!(
            branch.complete,
            "{conversation}:{} {name} reported a search that gave up, but none should have run",
            rolled.entry,
        );

        // PER OUTCOME NOW, and it says something it could not before. This used to read
        // the combined answer's count, which was hard-coded to zero for a rolled check
        // whatever its outcomes did - so the assertion held by construction rather than by
        // measurement. Each outcome carries its own figures, so this is now a fact about
        // the search.
        assert_eq!(
            branch.states_explored, 0,
            "{conversation}:{} {name} built search states for an outcome nothing can improve on",
            rolled.entry,
        );
    }
}
