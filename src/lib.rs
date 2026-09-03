// SPDX-License-Identifier: MIT

pub mod core;
pub mod parser;
pub mod graph;
pub mod world;
pub mod engine;
pub mod index;

#[cfg(test)]
mod integration_tests {
    use crate::core::types::{DialogueNodeId, Novelty, DialogueCheckKind, Ternary};
    use crate::core::state::{StateSymbols, LookAheadState};
    use crate::core::action::DialogueAction;
    use crate::core::guard::GuardExpression;
    use crate::core::guard_value::GuardValue;
    use crate::parser::guard_parser::parse_guard;
    use crate::parser::action_parser::parse_actions;
    use crate::graph::node::LookAheadNode;
    use crate::graph::graph::LookAheadGraph;
    use crate::world::test_world::TestWorld;
    use crate::engine::engine::{LookAheadEngine, LookAheadOptions};

    #[test]
    fn test_guard_parser() {
        let expr = parse_guard("Variable[\"test\"] == true").unwrap();
        assert!(matches!(expr, GuardExpression::Comparison(_, _, _)));
    }

    #[test]
    fn test_action_parser() {
        let mut symbols = StateSymbols::new();
        let actions = parse_actions(r#"SetVariableValue("test", 1); GainItem("sword"); PassTime()"#, &mut symbols);
        assert_eq!(actions.len(), 3);
        assert!(matches!(actions[0].kind(), crate::core::action::DialogueActionKind::Assign));
        assert!(matches!(actions[1].kind(), crate::core::action::DialogueActionKind::Assign));
        assert!(matches!(actions[2].kind(), crate::core::action::DialogueActionKind::PassTime));
    }

    #[test]
    fn test_simple_crawl() {
        // Build a simple linear graph: A -> B -> C
        let mut symbols = StateSymbols::new();
        let id_a = DialogueNodeId::new(1, 1);
        let id_b = DialogueNodeId::new(1, 2);
        let id_c = DialogueNodeId::new(1, 3);

        let node_a = LookAheadNode::new(
            id_a, false, DialogueCheckKind::None, GuardExpression::always_true(),
            vec![], vec![id_b], 0, false, false, -1, -1, false, -1
        );
        let node_b = LookAheadNode::new(
            id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
            vec![], vec![id_c], 0, false, false, -1, -1, false, -1
        );
        let node_c = LookAheadNode::new(
            id_c, false, DialogueCheckKind::None, GuardExpression::always_true(),
            vec![], vec![], 0, false, false, -1, -1, false, -1
        );

        let graph = LookAheadGraph::new(vec![node_a, node_b, node_c], symbols).unwrap();
        let world = TestWorld::new();
        let engine = LookAheadEngine::default();

        let result = engine.evaluate(&graph, id_a, &world, |id| {
            if id == id_c { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
        });

        assert_eq!(result.best, Novelty::UnseenAnyGame);
        // Two, not three. The crawl returns the moment it scores an UnseenAnyGame child,
        // because nothing outranks it and exploring further cannot improve the answer -
        // so C is counted as REACHED but its state is never added to the visited set.
        // The C# engine reports the same pair here, for the same reason.
        assert_eq!(result.states_explored, 2); // A and B; C ended the search
        assert_eq!(result.nodes_reached, 3); // A, B, C
    }

    #[test]
    fn test_cost_option_blocks() {
        let mut symbols = StateSymbols::new();
        let id_a = DialogueNodeId::new(1, 1);
        let id_b = DialogueNodeId::new(1, 2);

        let node_a = LookAheadNode::new(
            id_a, false, DialogueCheckKind::None, GuardExpression::always_true(),
            vec![], vec![id_b], 1000, false, false, -1, -1, false, -1
        );
        let node_b = LookAheadNode::new(
            id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
            vec![], vec![], 0, false, false, -1, -1, false, -1
        );

        let graph = LookAheadGraph::new(vec![node_a, node_b], symbols).unwrap();
        let world = TestWorld::new().with_money(500); // Can't afford 1000
        let engine = LookAheadEngine::default();

        let result = engine.evaluate(&graph, id_a, &world, |_| Novelty::UnseenAnyGame);
        assert_eq!(result.best, Novelty::SeenThisGame);
        assert_eq!(result.states_explored, 0); // Can't enter start node
    }

    #[test]
    fn test_money_once() {
        let mut symbols = StateSymbols::new();
        let id_a = DialogueNodeId::new(1, 1);
        let id_b = DialogueNodeId::new(1, 2);
        let id_c = DialogueNodeId::new(1, 3);

        // A costs 500 once, then free
        let node_a = LookAheadNode::new(
            id_a, false, DialogueCheckKind::None, GuardExpression::always_true(),
            vec![], vec![id_b, id_c], 500, true, false, -1, -1, false, -1
        );
        let node_b = LookAheadNode::new(
            id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
            vec![], vec![], 0, false, false, -1, -1, false, -1
        );
        let node_c = LookAheadNode::new(
            id_c, false, DialogueCheckKind::None, GuardExpression::always_true(),
            vec![], vec![], 0, false, false, -1, -1, false, -1
        );

        let graph = LookAheadGraph::new(vec![node_a, node_b, node_c], symbols).unwrap();
        let world = TestWorld::new().with_money(500); // Exactly enough for one
        let engine = LookAheadEngine::default();

        let result = engine.evaluate(&graph, id_a, &world, |id| {
            if id == id_c { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
        });

        // Should reach both B and C (cost only paid once)
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
            vec![], vec![id_b, id_c], 0, false, false, flag_slot as i32, fail_slot as i32, false, -1
        );
        let node_b = LookAheadNode::new(
            id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
            vec![], vec![], 0, false, false, -1, -1, false, -1
        );
        let node_c = LookAheadNode::new(
            id_c, false, DialogueCheckKind::None, GuardExpression::always_true(),
            vec![], vec![], 0, false, false, -1, -1, false, -1
        );

        let graph = LookAheadGraph::new(vec![node_a, node_b, node_c], symbols).unwrap();
        let world = TestWorld::new().set_check_result(id_a, Ternary::Unknown); // Both branches
        let engine = LookAheadEngine::default();

        // Deliberately UnseenThisGame rather than UnseenAnyGame: the strongest novelty
        // ends the search on sight, which would stop the crawl at its first child and
        // measure nothing about how far it got.
        let result = engine.evaluate(&graph, id_a, &world, |id| {
            if id == id_b { Novelty::UnseenThisGame } else { Novelty::SeenThisGame }
        });

        assert_eq!(result.best, Novelty::UnseenThisGame);
        // A, B and C. The start node is entered by try_enter, which takes the FIRST of a
        // rolled check's two outcomes rather than both - the marker answers "what follows
        // from picking this option", and picking it is a single act. The two-outcome
        // branching applies to rolled checks met further down, as children.
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
            vec![], vec![id_b], 0, false, false, flag_slot as i32, -1, false, -1
        );
        let node_b = LookAheadNode::new(
            id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
            vec![], vec![id_a], 0, false, false, -1, -1, false, -1 // loops back
        );

        let graph = LookAheadGraph::new(vec![node_a, node_b], symbols).unwrap();
        let world = TestWorld::new().set_check_result(id_a, Ternary::Unknown);
        let engine = LookAheadEngine::default();

        let result = engine.evaluate(&graph, id_a, &world, |_| Novelty::SeenThisGame);
        // Should not infinite loop - state budget stops it
        assert!(result.states_explored <= 200_000);
    }

    #[test]
    fn test_passive_check_passthrough() {
        let mut symbols = StateSymbols::new();
        let id_a = DialogueNodeId::new(1, 1);
        let id_b = DialogueNodeId::new(1, 2);

        let node_a = LookAheadNode::new(
            id_a, false, DialogueCheckKind::Passive, GuardExpression::always_true(),
            vec![], vec![id_b], 0, false, false, -1, -1, false, -1
        );
        let node_b = LookAheadNode::new(
            id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
            vec![], vec![], 0, false, false, -1, -1, false, -1
        );

        let graph = LookAheadGraph::new(vec![node_a, node_b], symbols).unwrap();
        let world = TestWorld::new().set_check_result(id_a, Ternary::False); // Fails
        let engine = LookAheadEngine::default();

        let result = engine.evaluate(&graph, id_a, &world, |id| {
            if id == id_b { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
        });

        // Should pass through to B
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
            vec![], vec![id_b, id_c], 0, false, false, -1, -1, false, seen_slot as i32
        );
        let node_b = LookAheadNode::new(
            id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
            vec![], vec![], 0, false, false, -1, -1, false, -1
        );
        let node_c = LookAheadNode::new(
            id_c, false, DialogueCheckKind::None, GuardExpression::always_true(),
            vec![], vec![], 0, false, false, -1, -1, false, -1
        );

        let graph = LookAheadGraph::new(vec![node_a, node_b, node_c], symbols).unwrap();
        let world = TestWorld::new().set_seen(id_a, true); // Already seen
        let engine = LookAheadEngine::default();

        let result = engine.evaluate(&graph, id_a, &world, |id| {
            if id == id_b || id == id_c { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
        });

        // Nothing is reachable, and that is the point. A fake check that has already been
        // displayed is CLOSED: the game stops offering it, so no path runs through it and
        // B and C are not reachable by way of it at all. The seed puts A's seen slot in
        // the state - matching the C# engine, which seeds every SeenSlot from
        // world.IsSeen - so entering A yields no state and the crawl ends at once.
        assert_eq!(result.nodes_reached, 0);
        assert_eq!(result.states_explored, 0);
        assert_eq!(result.best, Novelty::SeenThisGame);
    }
}
