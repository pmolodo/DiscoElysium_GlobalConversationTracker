// SPDX-License-Identifier: MIT
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lookahead_engine::core::types::{DialogueNodeId, Novelty, DialogueCheckKind};
use lookahead_engine::core::state::{StateSymbols, LookAheadState};
use lookahead_engine::core::action::DialogueAction;
use lookahead_engine::core::guard::GuardExpression;
use lookahead_engine::graph::node::LookAheadNode;
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::world::test_world::TestWorld;
use lookahead_engine::engine::engine::LookAheadEngine;

fn bench_simple_crawl(c: &mut Criterion) {
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

    c.bench_function("simple_crawl", |b| {
        b.iter(|| {
            let result = engine.evaluate(&graph, id_a, &world, |id| {
                if id == id_c { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
            });
            black_box(result);
        });
    });
}

fn bench_cycle_with_money(c: &mut Criterion) {
    // A -> B -> A cycle with money cost
    let mut symbols = StateSymbols::new();
    let id_a = DialogueNodeId::new(1, 1);
    let id_b = DialogueNodeId::new(1, 2);

    let node_a = LookAheadNode::new(
        id_a, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![], vec![id_b], 0, false, false, -1, -1, false, -1
    );
    let node_b = LookAheadNode::new(
        id_b, false, DialogueCheckKind::None, GuardExpression::always_true(),
        vec![DialogueAction::money(false, 10, false, "test".into())], vec![id_a], 0, false, false, -1, -1, false, -1
    );

    let graph = LookAheadGraph::new(vec![node_a, node_b], symbols).unwrap();
    let world = TestWorld::new().with_money(1000);
    let engine = LookAheadEngine::default();

    c.bench_function("cycle_with_money", |b| {
        b.iter(|| {
            let result = engine.evaluate(&graph, id_a, &world, |_| Novelty::SeenThisGame);
            black_box(result);
        });
    });
}

fn bench_red_check_branching(c: &mut Criterion) {
    let mut symbols = StateSymbols::new();
    let id_a = DialogueNodeId::new(1, 1);
    let id_b = DialogueNodeId::new(1, 2);
    let id_c = DialogueNodeId::new(1, 3);

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
    let world = TestWorld::new().with_money(0);
    let engine = LookAheadEngine::default();

    c.bench_function("red_check_branching", |b| {
        b.iter(|| {
            let result = engine.evaluate(&graph, id_a, &world, |id| {
                if id == id_b { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
            });
            black_box(result);
        });
    });
}

criterion_group!(benches, bench_simple_crawl, bench_cycle_with_money, bench_red_check_branching);
criterion_main!(benches);
