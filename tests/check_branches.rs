// SPDX-License-Identifier: MIT
//! Does a white or red check come back with its two outcomes told apart?
//!
//! The unit tests build rolled checks by hand, which proves the branching but not that any
//! real check in the game has the shape the mod assumes. This asks the shipped index: find
//! the rolls the corpus actually contains, ask about them, and require the answer to carry
//! two branches - and require an ordinary option in the same group to carry none, since it
//! is the ABSENCE that tells the mod which options get a Pass/Fail line.

use lookahead_engine::bridge::{LookAheadRequest, NodeRef, WorldRawData, answer};
use lookahead_engine::core::types::DialogueCheckKind;
use lookahead_engine::index::{build_group_graph, read_index};

use lookahead_engine::core::types::{DialogueNodeId, Ternary};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::symbolic::backward::{Backward, SettledPass};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::world::GameWorld;

use gct_measure::common;
use gct_measure::{prepared, save_world};

/// What a counter saturates at, which none of this turns on.
const CAP: i32 = 16;

/// The save that failed the mirror's check, in a real playthrough.
const SAVE: &str = "at-evart";

/// A world that decides nothing, so every check is open and both its branches are live.
fn undecided() -> WorldRawData {
    WorldRawData {
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

    let response = answer(&index, common::declared(), None, &request);
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

        let response = answer(&index, common::declared(), None, &request);
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

            let response = answer(&index, common::declared(), None, &request);
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
/// reachable can beat the option's own seen state - and this is the case it cannot cover, an
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

    let response = answer(&index, common::declared(), None, &request);
    assert!(response.error.is_none(), "{:?}", response.error);

    let (pass, fail) = response.outcomes(rolled).expect("a roll comes back as two");

    // AND THE MENU STILL COST SOMETHING, which is the other half of the claim and what makes
    // the passes the right counter to read. Laying out the group and compiling its guards
    // allocates diagram nodes before any search is considered, so that figure is never zero
    // for a menu answered at all - and a search that never ran cannot be told from one that
    // did by looking at it.
    let built: usize = response
        .answers
        .iter()
        .map(|answer| answer.diagram_nodes)
        .sum();
    assert!(
        built > 0,
        "{conversation}:{} answered a menu without building a diagram, so the figure is dead",
        rolled.entry,
    );

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

        // ON THE PASSES A SEARCH TOOK, which is the only counter here that can be zero. The
        // other cost figure cannot serve: building the layout and compiling the guards
        // allocates diagram nodes before any search is considered, so that one is never zero
        // for a menu that was answered at all.
        //
        // A BACKWARD PASS IS THE SEARCH ITSELF, and a refused one performs none.
        assert_eq!(
            branch.nodes_reached, 0,
            "{conversation}:{} {name} walked something for an outcome nothing can improve on",
            rolled.entry,
        );
    }
}

/// The mirror's white check, which `at-evart` failed, is offered again because the group can
/// lower its target.
///
/// ## Why this conversation, and why this pair
///
/// `10:3` subdues *The Expression* against a target of 6, and one of its six modifiers is
/// worth -2 when `whirling.mirror_expression_source_located` holds - which is the FLAG OF THE
/// OTHER WHITE CHECK in the same menu, `10:423`. So the game's rule closes on itself inside one
/// conversation: fail the first, pass the second, and the first is worth trying again. A save
/// the game wrote has both of them failed.
///
/// ## What makes the assertion mean something
///
/// `10:5` is a node guarded on `whirling.mirror_subdued_expression`, the first check's own pass
/// flag, and NO SCRIPT IN THE DATABASE WRITES THAT NAME - the check passing is the only thing
/// that sets it. So reaching 10:5 from the conversation's start means the search passed a check
/// the save had already failed, which it can only do if something reopened it.
///
/// The second half is the same question with this check's reopening taken away, on the same
/// graph and the same world. Without it the search must not reach 10:5 - which is what the
/// engine did before de-vdy9, and what it would do again if the rule were lost.
#[test]
fn a_shipped_failed_white_check_is_offered_again_when_its_target_can_fall() {
    let Some(path) = common::shipped_index() else {
        return;
    };
    let Some(index) = index() else {
        return;
    };

    const MIRROR: i32 = 10;
    let check = DialogueNodeId::new(MIRROR, 3);
    let start = DialogueNodeId::new(MIRROR, 0);
    // Guarded on the check's pass flag, which nothing but the check writes.
    let behind = DialogueNodeId::new(MIRROR, 5);

    let (graph, _) = build_group_graph(&index, MIRROR).expect("conversation 10 builds a group");
    assert!(
        graph
            .get(check)
            .expect("the mirror's check is in its own group")
            .reopening
            .is_some(),
        "{check:?} kept no way to be reopened, so this test is asking nothing",
    );

    let shipped = prepared::Shipped::at(path, Default::default());
    let raw = save_world::of_save(&graph, MIRROR, &shipped, SAVE);
    let world = GameWorld::declaring(raw, common::declared());

    // THE SAVE HAS FAILED IT, which is the premise. A world where the check is open would
    // reach what is behind it whatever this engine does about reopening.
    let failed = format!(
        "whirling.mirror_subdued_expression{}",
        lookahead_engine::index::FAILED_FLAG_SUFFIX,
    );
    assert_eq!(
        world.variable(&failed).as_condition(),
        Ternary::True,
        "{SAVE} has not failed the mirror's check, so there is nothing to reopen",
    );

    assert!(
        reaches(&graph, &world, start, behind),
        "the search never passed the reopened check",
    );

    let mut closed = graph.clone();
    closed.get_mut(check).expect("the check is there").reopening = None;
    assert!(
        !reaches(&closed, &world, start, behind),
        "the check's pass flag was set without anything to reopen it, so reaching {behind:?} \
         says nothing about the reopening",
    );
}

/// Whether the backward search reaches `target` from `start` in this world.
fn reaches(
    graph: &LookAheadGraph,
    world: &GameWorld,
    start: DialogueNodeId,
    target: DialogueNodeId,
) -> bool {
    let layout = DataLayout::for_group(graph, world, CAP);
    let symbols = graph.symbols().clone();
    let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, world, &vars).expect("room for a seed");
    let backward = Backward::reaching(graph, target, &mut compiler, world, CAP as u32);
    assert!(
        backward.stats().reached_fixed_point,
        "the backward pass did not settle, so its answer is a floor rather than an answer",
    );
    backward.reachable_from(start, &seed)
}
