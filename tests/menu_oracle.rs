// SPDX-License-Identifier: MIT
//! Compare greedy menu marking with exhaustive concrete-state distances.
use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::oracle;
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::known::GroupShape;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::symbolic::{menu, novelty_search};
use lookahead_engine::test_graph::{Entry, GraphBuilder, node};
use lookahead_engine::world::test_world::TestWorld;
use lookahead_engine::world::world::ILookAheadWorld;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
mod common;

fn compare(
    graph: &LookAheadGraph,
    options: &[DialogueNodeId],
    world: &dyn ILookAheadWorld,
    novelty: &impl Fn(DialogueNodeId) -> Novelty,
) {
    let layout = DataLayout::for_group(graph, world, 16);
    let vars = DataVars::new(&layout, graph.symbols(), DiagramBudget::modest());
    let mut compiler = GuardCompiler::new(&vars).with_world(world);
    let seed = seed_of(graph, world, &vars).unwrap();
    let contestants: Vec<_> = options
        .iter()
        .map(|&id| menu::Contestant {
            position: novelty_search::Where::of(
                graph,
                id,
                StartBranch::Either,
                &seed,
                &mut compiler,
                world,
                16,
            )
            .position(id),
            baseline: novelty(id),
            landing: vec![id],
        })
        .collect();
    let found = menu::mark_menu(
        graph,
        &mut compiler,
        world,
        16,
        novelty,
        &contestants,
        &menu::Budget {
            wall: Duration::from_secs(30),
            each: Duration::from_secs(30),
        },
        &GroupShape::of(graph),
    );
    assert!(found.marks.iter().all(|m| m.complete));
    let mut expected: Vec<_> = options.iter().map(|id| (novelty(*id), None)).collect();
    let mut claimed: HashSet<_> = options.iter().copied().collect();
    let mut marked = HashSet::new();
    let mut round = 0;
    for class in [Novelty::UnseenAnyGame, Novelty::UnseenThisGame] {
        let mut hunting: Vec<_> = (0..options.len())
            .filter(|i| !marked.contains(i) && novelty(options[*i]) < class)
            .collect();
        let mut cut: HashSet<_> = (0..options.len())
            .filter(|i| !hunting.contains(i))
            .map(|i| options[i])
            .collect();
        while !hunting.is_empty() {
            let walks: HashMap<_, _> = hunting
                .iter()
                .map(|&i| {
                    (
                        i,
                        oracle::choice_distances(
                            graph,
                            options[i],
                            StartBranch::Either,
                            &cut,
                            world,
                            16,
                        )
                        .expect("comparison must exhaust the concrete states"),
                    )
                })
                .collect();
            let nearest: HashMap<_, _> = hunting
                .iter()
                .filter_map(|&i| {
                    walks[&i]
                        .iter()
                        .filter(|(id, _)| novelty(**id) == class && !claimed.contains(id))
                        .map(|(_, d)| *d)
                        .min()
                        .map(|d| (i, d))
                })
                .collect();
            let Some(best) = nearest.values().copied().min() else {
                break;
            };
            round += 1;
            let (i, chosen) = found
                .marks
                .iter()
                .enumerate()
                .find(|(_, mark)| mark.round == Some(round))
                .expect("reachable content must have a winner");
            assert!(hunting.contains(&i));
            assert_eq!((chosen.best, chosen.distance), (class, Some(best)));
            let witness = chosen.witness.expect("a marker has a witness");
            assert_eq!(novelty(witness), class);
            assert!(!claimed.contains(&witness));
            assert_eq!(walks[&i].get(&witness), Some(&best));
            expected[i] = (class, Some(best));
            marked.insert(i);
            cut.insert(options[i]);
            claimed.insert(witness);
            hunting.retain(|i| !marked.contains(i));
        }
    }
    let actual: Vec<_> = found.marks.iter().map(|m| (m.best, m.distance)).collect();
    assert_eq!(actual, expected, "menu {options:?}");
}

#[test]
fn cyclic_menus_agree_for_every_assignment_of_novelty() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1, 2, 3]))
        .add(Entry::new(1).player().links(&[0, 4]))
        .add(
            Entry::new(2)
                .player()
                .script("SetVariableValue(\"open\", true)")
                .links(&[0, 5]),
        )
        .add(Entry::new(3).player().links(&[4, 5]))
        .add(
            Entry::new(4)
                .guard("Variable[\"open\"] == true")
                .links(&[6, 7]),
        )
        .add(Entry::new(5).links(&[0]))
        .add(Entry::new(6).player().links(&[8]))
        .add(Entry::new(7).player())
        .add(Entry::new(8))
        .build();
    for assignment in 0usize..729 {
        let novelty = |id: DialogueNodeId| {
            let digit = assignment / 3usize.pow((id.entry_id - 1).clamp(0, 5) as u32) % 3;
            if id.entry_id == 0 {
                return Novelty::SeenThisGame;
            }
            [
                Novelty::SeenThisGame,
                Novelty::UnseenThisGame,
                Novelty::UnseenAnyGame,
            ][digit]
        };
        compare(
            &graph,
            &[node(1), node(2), node(3)],
            &TestWorld::new(),
            &novelty,
        );
    }
}

#[test]
fn real_conversations_agree_with_the_greedy_walk() {
    let path = common::conversation_index().expect("conversation index is required");
    let index = lookahead_engine::index::read_index(&path).unwrap();
    let world = common::measurement_save();
    for conversation in [1123, 484, 1066, 1147, 949, 511, 640] {
        let (graph, _) = lookahead_engine::index::build_group_graph(&index, conversation).unwrap();
        let options: Vec<_> = graph
            .nodes()
            .filter(|n| n.choice)
            .take(6)
            .map(|n| n.id)
            .collect();
        assert!(!options.is_empty(), "{conversation}: no choices detected");
        lookahead_engine::symbolic::isolated::on_its_own_thread(|| {
            compare(&graph, &options, &world, &|id| {
                if options.contains(&id) {
                    Novelty::SeenThisGame
                } else if id.entry_id % 3 == 0 {
                    Novelty::UnseenAnyGame
                } else {
                    Novelty::UnseenThisGame
                }
            });
        });
        println!("{conversation}: exact menu agreement");
    }
}
