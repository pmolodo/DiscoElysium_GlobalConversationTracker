// SPDX-License-Identifier: MIT
//! Tests that cross module boundaries, and so belong to no single module.
//!
//! Moved out of `lib.rs`, whose job is declaring the crate's modules rather than holding
//! four hundred lines of tests. Unit tests stay inline beside what they test; this file is
//! for the ones that build a graph, a world and an engine together.
//!
//! Some of these predate the ported C# suites and overlap them - `test_red_check_branches`
//! and `check_node_tests::a_red_check_explores_both_outcomes` cover neighbouring ground,
//! for instance. Kept rather than pruned while the port is in progress, because the older
//! ones pin the numbers (`states_explored`, `nodes_reached`) that the ported ones mostly
//! do not.

use crate::core::guard::GuardExpression;
use crate::core::state::StateSymbols;
use crate::core::types::{DialogueCheckKind, DialogueNodeId, Novelty, Ternary};
use crate::engine::engine::LookAheadEngine;
use crate::graph::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::parser::action_parser::parse_actions;
use crate::parser::guard_parser::parse_guard;
use crate::world::test_world::TestWorld;

#[test]
fn test_guard_parser() {
    let expr = parse_guard("Variable[\"test\"] == true").unwrap();
    assert!(matches!(expr, GuardExpression::Comparison(_, _, _)));
}

#[test]
fn test_action_parser() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions(
        r#"SetVariableValue("test", 1); GainItem("sword"); PassTime()"#,
        &mut symbols,
    );
    assert_eq!(actions.len(), 3);
    assert!(matches!(actions[0].kind(), crate::core::action::DialogueActionKind::Assign));
    assert!(matches!(actions[1].kind(), crate::core::action::DialogueActionKind::Assign));
    assert!(matches!(actions[2].kind(), crate::core::action::DialogueActionKind::PassTime));
}

#[test]
fn test_simple_crawl() {
    // A simple linear graph: A -> B -> C
    let symbols = StateSymbols::new();
    let id_a = DialogueNodeId::new(1, 1);
    let id_b = DialogueNodeId::new(1, 2);
    let id_c = DialogueNodeId::new(1, 3);

    let node_a = LookAheadNode::new(
        id_a, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![id_b], 0, false, false, -1, -1, false, -1,
    );
    let node_b = LookAheadNode::new(
        id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![id_c], 0, false, false, -1, -1, false, -1,
    );
    let node_c = LookAheadNode::new(
        id_c, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![], 0, false, false, -1, -1, false, -1,
    );

    let graph = LookAheadGraph::new(vec![node_a, node_b, node_c], symbols).unwrap();
    let result = LookAheadEngine::default().evaluate(&graph, id_a, &TestWorld::new(), |id| {
        if id == id_c { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
    });

    assert_eq!(result.best, Novelty::UnseenAnyGame);
    // Two, not three. The crawl returns the moment it scores an UnseenAnyGame child,
    // because nothing outranks it and exploring further cannot improve the answer - so C
    // is counted as REACHED but its state is never added to the visited set. The C#
    // engine reports the same pair here, for the same reason.
    assert_eq!(result.states_explored, 2); // A and B; C ended the search
    assert_eq!(result.nodes_reached, 3); // A, B, C
}

#[test]
fn test_cost_option_blocks() {
    let symbols = StateSymbols::new();
    let id_a = DialogueNodeId::new(1, 1);
    let id_b = DialogueNodeId::new(1, 2);

    let node_a = LookAheadNode::new(
        id_a, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![id_b], 1000, false, false, -1, -1, false, -1,
    );
    let node_b = LookAheadNode::new(
        id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![], 0, false, false, -1, -1, false, -1,
    );

    let graph = LookAheadGraph::new(vec![node_a, node_b], symbols).unwrap();
    let world = TestWorld::new().with_money(500); // Cannot afford 1000.
    let result = LookAheadEngine::default().evaluate(&graph, id_a, &world, |_| {
        Novelty::UnseenAnyGame
    });

    assert_eq!(result.best, Novelty::SeenThisGame);
    assert_eq!(result.states_explored, 0); // The start node cannot be entered.
}

#[test]
fn test_money_once() {
    let symbols = StateSymbols::new();
    let id_a = DialogueNodeId::new(1, 1);
    let id_b = DialogueNodeId::new(1, 2);
    let id_c = DialogueNodeId::new(1, 3);

    // A costs 500 once, then is free.
    let node_a = LookAheadNode::new(
        id_a, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![id_b, id_c], 500, true, false, -1, -1, false, -1,
    );
    let node_b = LookAheadNode::new(
        id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![], 0, false, false, -1, -1, false, -1,
    );
    let node_c = LookAheadNode::new(
        id_c, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![], 0, false, false, -1, -1, false, -1,
    );

    let graph = LookAheadGraph::new(vec![node_a, node_b, node_c], symbols).unwrap();
    let world = TestWorld::new().with_money(500); // Exactly enough for one.
    let result = LookAheadEngine::default().evaluate(&graph, id_a, &world, |id| {
        if id == id_c { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
    });

    // Both B and C are reached, because the cost is paid only once.
    assert_eq!(result.best, Novelty::UnseenAnyGame);
    assert!(result.nodes_reached >= 2);
}

#[test]
fn test_red_check_branches() {
    let mut symbols = StateSymbols::new();
    let id_a = DialogueNodeId::new(1, 1);
    let id_b = DialogueNodeId::new(1, 2); // success
    let id_c = DialogueNodeId::new(1, 3); // failure

    let flag_slot = symbols.variable("check_flag");
    let fail_slot = symbols.variable("check_flag_failed");

    let node_a = LookAheadNode::new(
        id_a, false, DialogueCheckKind::Red, GuardExpression::always_true(),
        vec![], vec![id_b, id_c], 0, false, false,
        flag_slot as i32, fail_slot as i32, false, -1,
    );
    let node_b = LookAheadNode::new(
        id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![], 0, false, false, -1, -1, false, -1,
    );
    let node_c = LookAheadNode::new(
        id_c, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![], 0, false, false, -1, -1, false, -1,
    );

    let graph = LookAheadGraph::new(vec![node_a, node_b, node_c], symbols).unwrap();
    let world = TestWorld::new().set_check_result(id_a, Ternary::Unknown); // Both branches.

    // Deliberately UnseenThisGame rather than UnseenAnyGame: the strongest novelty ends
    // the search on sight, which would stop the crawl at its first child and measure
    // nothing about how far it got.
    let result = LookAheadEngine::default().evaluate(&graph, id_a, &world, |id| {
        if id == id_b { Novelty::UnseenThisGame } else { Novelty::SeenThisGame }
    });

    assert_eq!(result.best, Novelty::UnseenThisGame);
    // A, B and C. The start node is entered by try_enter, which takes the FIRST of a
    // rolled check's two outcomes rather than both - the marker answers "what follows from
    // picking this option", and picking it is a single act. The two-outcome branching
    // applies to rolled checks met further down, as children.
    assert_eq!(result.states_explored, 3);
}

#[test]
fn test_white_check_retry() {
    let mut symbols = StateSymbols::new();
    let id_a = DialogueNodeId::new(1, 1);
    let id_b = DialogueNodeId::new(1, 2);

    let flag_slot = symbols.variable("white_flag");

    let node_a = LookAheadNode::new(
        id_a, false, DialogueCheckKind::White, GuardExpression::always_true(),
        vec![], vec![id_b], 0, false, false, flag_slot as i32, -1, false, -1,
    );
    let node_b = LookAheadNode::new(
        id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![id_a], 0, false, false, -1, -1, false, -1, // loops back
    );

    let graph = LookAheadGraph::new(vec![node_a, node_b], symbols).unwrap();
    let world = TestWorld::new().set_check_result(id_a, Ternary::Unknown);

    let result = LookAheadEngine::default().evaluate(&graph, id_a, &world, |_| {
        Novelty::SeenThisGame
    });
    // The retryable loop must not run forever; the state budget bounds it.
    assert!(result.states_explored <= 200_000);
}

#[test]
fn test_passive_check_passthrough() {
    let symbols = StateSymbols::new();
    let id_a = DialogueNodeId::new(1, 1);
    let id_b = DialogueNodeId::new(1, 2);

    let node_a = LookAheadNode::new(
        id_a, false, DialogueCheckKind::Passive, GuardExpression::always_true(),
        vec![], vec![id_b], 0, false, false, -1, -1, false, -1,
    );
    let node_b = LookAheadNode::new(
        id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![], 0, false, false, -1, -1, false, -1,
    );

    let graph = LookAheadGraph::new(vec![node_a, node_b], symbols).unwrap();
    let world = TestWorld::new().set_check_result(id_a, Ternary::False); // Fails.

    let result = LookAheadEngine::default().evaluate(&graph, id_a, &world, |id| {
        if id == id_b { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
    });

    // A failed passive check passes through to its children.
    assert_eq!(result.best, Novelty::UnseenAnyGame);
}

#[test]
fn test_fake_check_once() {
    let mut symbols = StateSymbols::new();
    let id_a = DialogueNodeId::new(1, 1);
    let id_b = DialogueNodeId::new(1, 2);
    let id_c = DialogueNodeId::new(1, 3);

    let seen_slot = symbols.seen(id_a);

    let node_a = LookAheadNode::new(
        id_a, false, DialogueCheckKind::Fake, GuardExpression::always_true(),
        vec![], vec![id_b, id_c], 0, false, false, -1, -1, false, seen_slot as i32,
    );
    let node_b = LookAheadNode::new(
        id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![], 0, false, false, -1, -1, false, -1,
    );
    let node_c = LookAheadNode::new(
        id_c, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![], 0, false, false, -1, -1, false, -1,
    );

    let graph = LookAheadGraph::new(vec![node_a, node_b, node_c], symbols).unwrap();
    let world = TestWorld::new().set_seen(id_a, true); // Already seen.

    let result = LookAheadEngine::default().evaluate(&graph, id_a, &world, |id| {
        if id == id_b || id == id_c { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
    });

    // Nothing is reachable, and that is the point. A fake check that has already been
    // displayed is CLOSED: the game stops offering it, so no path runs through it and B
    // and C are not reachable by way of it at all. The seed puts A's seen slot in the
    // state - matching the C# engine, which seeds every SeenSlot from world.IsSeen - so
    // entering A yields no state and the crawl ends at once.
    assert_eq!(result.nodes_reached, 0);
    assert_eq!(result.states_explored, 0);
    assert_eq!(result.best, Novelty::SeenThisGame);
}

// ---------------------------------------------------------------------------
// The short-circuit
// ---------------------------------------------------------------------------

/// A plain entry with the given links.
fn plain(id: DialogueNodeId, links: Vec<DialogueNodeId>, is_group: bool) -> LookAheadNode {
    LookAheadNode::new(
        id, is_group, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], links, 0, false, false, -1, -1, false, -1,
    )
}

#[test]
fn no_potential_improvement_when_every_scoreable_node_is_already_seen() {
    let a = DialogueNodeId::new(1, 1);
    let b = DialogueNodeId::new(1, 2);
    let c = DialogueNodeId::new(1, 3);
    let graph = LookAheadGraph::new(
        vec![
            plain(a, vec![b], false),
            // A GROUP, and unseen. It must not count: the game never writes a group's
            // SimStatus, so every group reads as never displayed and counting them would
            // make the check useless.
            plain(b, vec![c], true),
            plain(c, vec![], false),
        ],
        StateSymbols::new(),
    )
    .unwrap();

    assert!(!LookAheadEngine::has_potential_improvement(
        &graph,
        Novelty::SeenThisGame,
        |id| if id == b { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame },
    ));
}

#[test]
fn no_potential_improvement_when_everything_matches_the_options_novelty() {
    let a = DialogueNodeId::new(1, 1);
    let b = DialogueNodeId::new(1, 2);
    let graph = LookAheadGraph::new(
        vec![plain(a, vec![b], false), plain(b, vec![], false)],
        StateSymbols::new(),
    )
    .unwrap();

    // Strictly better, not as good as: an option already drawn unseen-this-save gains
    // nothing from another node in the same state.
    assert!(!LookAheadEngine::has_potential_improvement(
        &graph,
        Novelty::UnseenThisGame,
        |_| Novelty::UnseenThisGame,
    ));
}

/// The check is structural, so an unreachable candidate still passes it.
///
/// That is the whole shape of the thing: a cheap exact NO, and a YES that only means the
/// crawl has to run. Here the crawl then finds nothing, and both are correct.
#[test]
fn an_unreachable_candidate_is_left_for_the_crawler() {
    let a = DialogueNodeId::new(1, 1);
    let b = DialogueNodeId::new(1, 2);
    let orphan = DialogueNodeId::new(1, 3);
    let graph = LookAheadGraph::new(
        vec![
            plain(a, vec![b], false),
            plain(b, vec![], false),
            // Nothing links to it.
            plain(orphan, vec![], false),
        ],
        StateSymbols::new(),
    )
    .unwrap();
    let novelty = |id: DialogueNodeId| {
        if id == orphan { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
    };

    assert!(LookAheadEngine::has_potential_improvement(
        &graph,
        Novelty::SeenThisGame,
        novelty,
    ));

    let result = LookAheadEngine::default().evaluate(&graph, a, &TestWorld::new(), novelty);
    assert_eq!(result.best, Novelty::SeenThisGame);
}

#[test]
fn nothing_outranks_the_strongest_novelty_there_is() {
    let a = DialogueNodeId::new(1, 1);
    let graph = LookAheadGraph::new(vec![plain(a, vec![], false)], StateSymbols::new()).unwrap();

    assert!(!LookAheadEngine::has_potential_improvement(
        &graph,
        Novelty::UnseenAnyGame,
        |_| Novelty::UnseenAnyGame,
    ));
}

// ---------------------------------------------------------------------------
// Flags
// ---------------------------------------------------------------------------

/// A flag set on the way opens a door guarded on that flag.
///
/// Both halves of the flag support at once, because either alone is useless: the action
/// has to write the variable and the guard has to read it. Left unmodelled the crawl never
/// reaches C, reports nothing reachable, and the option silently loses its marker.
///
/// A is the option, B sets the flag, C is gated on it and is the only novel entry.
#[test]
fn a_flag_set_on_the_way_opens_a_guard_that_reads_it() {
    let mut symbols = StateSymbols::new();
    let a = DialogueNodeId::new(1, 1);
    let b = DialogueNodeId::new(1, 2);
    let c = DialogueNodeId::new(1, 3);

    let setter = parse_actions(r#"SetFlag("canal.roy_flashlight_hub_seen")"#, &mut symbols);
    let gate = parse_guard(r#"FlagSet("canal.roy_flashlight_hub_seen")"#)
        .expect("the guard parses");

    let nodes = vec![
        plain(a, vec![b], false),
        LookAheadNode::new(
            b, false, DialogueCheckKind::None, GuardExpression::always_true(),
            setter, vec![c], 0, false, false, -1, -1, false, -1,
        ),
        LookAheadNode::new(
            c, false, DialogueCheckKind::None, gate,
            vec![], vec![], 0, false, false, -1, -1, false, -1,
        ),
    ];
    let graph = LookAheadGraph::new(nodes, symbols).unwrap();

    let result = LookAheadEngine::default().evaluate(&graph, a, &TestWorld::new(), |id| {
        if id == c { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
    });

    assert_eq!(result.best, Novelty::UnseenAnyGame);
}

/// And the gate stays shut when nothing sets the flag.
///
/// Without this the test above would pass just as well on a guard that was never
/// evaluated at all.
#[test]
fn a_flag_guard_stays_shut_when_nothing_sets_it() {
    let mut symbols = StateSymbols::new();
    let a = DialogueNodeId::new(1, 1);
    let c = DialogueNodeId::new(1, 3);

    // Interned so the crawl tracks it, but never written.
    symbols.variable("canal.roy_flashlight_hub_seen");
    let gate = parse_guard(r#"FlagSet("canal.roy_flashlight_hub_seen")"#)
        .expect("the guard parses");

    let nodes = vec![
        plain(a, vec![c], false),
        LookAheadNode::new(
            c, false, DialogueCheckKind::None, gate,
            vec![], vec![], 0, false, false, -1, -1, false, -1,
        ),
    ];
    let graph = LookAheadGraph::new(nodes, symbols).unwrap();

    let result = LookAheadEngine::default().evaluate(&graph, a, &TestWorld::new(), |id| {
        if id == c { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
    });

    assert_eq!(result.best, Novelty::SeenThisGame);
}
