// SPDX-License-Identifier: MIT
//! Compare greedy menu marking with exhaustive concrete-state distances.
use gct_measure::common;
use lookahead_engine::core::guard_value::GuardValue;
use lookahead_engine::core::types::{DialogueNodeId, SeenState, StartBranch};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::oracle;
use lookahead_engine::symbolic::arms::Arms;
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::known::GroupShape;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::search::Search;
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::symbolic::{menu, seen_state_search};
use lookahead_engine::test_graph::{Entry, GraphBuilder, node};
use lookahead_engine::world::GameWorld;
use lookahead_engine::world::ILookAheadWorld;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// WHICH OPTION WON A ROUND IS READ OFF THE MARKING rather than asserted, because where several
/// options are equally near, which one a round takes is the search's to decide and two arms may
/// decide it differently. What is asserted of the winner is its CLASS and its DISTANCE, and that
/// its witness is unclaimed and really that far away - the claim, which no arm gets to vary.
fn compare(
    graph: &LookAheadGraph,
    options: &[DialogueNodeId],
    world: &dyn ILookAheadWorld,
    seen_state: &impl Fn(DialogueNodeId) -> SeenState,
) -> menu::MenuAnswer {
    compare_under(&GroupShape::of(graph), graph, options, world, seen_state)
}

fn compare_under(
    shape: &GroupShape,
    graph: &LookAheadGraph,
    options: &[DialogueNodeId],
    world: &dyn ILookAheadWorld,
    seen_state: &impl Fn(DialogueNodeId) -> SeenState,
) -> menu::MenuAnswer {
    let layout = DataLayout::for_group(graph, world, 16);
    let vars = DataVars::new(&layout, graph.symbols(), DiagramBudget::modest());
    let mut compiler = GuardCompiler::new(&vars).with_world(world);
    let seed = seed_of(graph, world, &vars).unwrap();
    let contestants: Vec<_> = options
        .iter()
        .map(|&id| menu::Contestant {
            position: seen_state_search::Where::of(
                graph,
                id,
                StartBranch::Either,
                &seed,
                &mut compiler,
                world,
                16,
            )
            .position(id),
            baseline: seen_state(id),
            landing: vec![id],
        })
        .collect();
    let found = menu::mark_menu(
        Search {
            graph,
            compiler: &mut compiler,
            world,
            counter_cap: 16,
            arms: Arms::shipped(),
        },
        seen_state,
        &contestants,
        &menu::Budget {
            wall: Duration::from_secs(30),
            each: Duration::from_secs(30),
        },
        shape,
    );
    assert!(found.marks.iter().all(|m| m.complete));
    let mut expected: Vec<_> = options.iter().map(|id| (seen_state(*id), None)).collect();
    let mut claimed: HashSet<_> = options.iter().copied().collect();
    let mut marked = HashSet::new();
    let mut round = 0;
    for class in [SeenState::UnseenAnyGame, SeenState::UnseenThisGame] {
        let mut hunting: Vec<_> = (0..options.len())
            .filter(|i| !marked.contains(i) && seen_state(options[*i]) < class)
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
                        .filter(|(id, _)| seen_state(**id) == class && !claimed.contains(id))
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
            assert_eq!(seen_state(witness), class);
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
    found
}

/// A round that has to rule targets out across conversations, the shape of the Wild Pines menu.
///
/// ```text
///   1    the option; offers the choices 3 and 4, and passes four shut doors
///   3    a choice, offering 5 and 6; 5 goes on to 200
///   4    a choice, going on to 400
///   2, 7, 8, 9, 10    shut doors, which is what makes every target's link bound 0
///
///   conversation 1   101-103    behind 10 only - unreachable
///   conversation 2   200        through 3 and 5 - distance 2
///   conversation 3   301-316    behind 8 only - unreachable
///   conversation 4   400        through 4 - distance 1
///                    401-402    behind 9 only - unreachable
/// ```
///
/// Every target shares one level, and the level spans four conversations, so it is asked a
/// conversation at a time. The walk takes conversation 1 first, and one pass rules all three of
/// its targets out. It finds 200 at 2, which leaves nineteen targets
/// whose bounds could beat that - enough for one pass to find the least, 1, and to name 400 as
/// the target whose front met the option. So conversation 4 is asked next: one pass says its
/// part is reachable, and 400 alone is at 1, which ends the round without asking any of the
/// sixteen in conversation 3.
///
/// SIX PASSES: the round's reachability, conversation 1, 200, the least, conversation 4 and 400.
/// Asked in bound order one target at a time, the same round takes twenty-four, asking every one
/// of the nineteen unreachable targets on its own.
#[test]
fn a_round_rules_targets_out_a_conversation_at_a_time() {
    let unreachable_behind = |door: i32, targets: std::ops::RangeInclusive<i32>| {
        Entry::new(door)
            .guard(r#"Variable["shut"]"#)
            .links(&targets.collect::<Vec<_>>())
    };
    let mut builder = GraphBuilder::new()
        .add(Entry::new(1).player().links(&[2, 3, 4, 7, 8, 9, 10]))
        .add(unreachable_behind(2, 200..=200))
        .add(Entry::new(3).player().links(&[5, 6]))
        .add(Entry::new(4).player().links(&[400]))
        .add(Entry::new(5).player().links(&[200]))
        .add(Entry::new(6).player())
        .add(unreachable_behind(7, 400..=400))
        .add(unreachable_behind(8, 301..=316))
        .add(unreachable_behind(9, 401..=402))
        .add(unreachable_behind(10, 101..=103))
        .add(Entry::new(200).in_conversation(2));
    for (conversation, targets) in [(1, 101..=103), (3, 301..=316), (4, 400..=402)] {
        for id in targets {
            builder = builder.add(Entry::new(id).in_conversation(conversation));
        }
    }
    let graph = builder.build();
    let world = GameWorld::blank().set_variable("shut", GuardValue::from_boolean(false));
    let answer = compare(&graph, &[node(1)], &world, &|id| {
        if id.entry_id >= 100 {
            SeenState::UnseenAnyGame
        } else {
            SeenState::SeenThisGame
        }
    });
    assert_eq!(answer.marks[0].witness.map(|id| id.entry_id), Some(400));
    assert_eq!(answer.passes, 6);
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
        let seen_state = |id: DialogueNodeId| {
            let digit = assignment / 3usize.pow((id.entry_id - 1).clamp(0, 5) as u32) % 3;
            if id.entry_id == 0 {
                return SeenState::SeenThisGame;
            }
            [
                SeenState::SeenThisGame,
                SeenState::UnseenThisGame,
                SeenState::UnseenAnyGame,
            ][digit]
        };
        compare(
            &graph,
            &[node(1), node(2), node(3)],
            &GameWorld::blank(),
            &seen_state,
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
                    SeenState::SeenThisGame
                } else if id.entry_id % 3 == 0 {
                    SeenState::UnseenAnyGame
                } else {
                    SeenState::UnseenThisGame
                }
            });
        });
        println!("{conversation}: exact menu agreement");
    }
}

/// THE MOVING CLOCK, judged by the executor that moves it in concrete states.
///
/// Every other check of the carried clock compares the symbolic side against what a test
/// expected of it. This compares it against `oracle`, which walks concrete states through
/// `core::action` - and that advances `day_minutes` on its own. A disagreement here is the two
/// executors answering the same menu differently, which is the one thing "one algorithm by
/// default, everywhere it runs" forbids.
///
/// THE MENU IS TWO OPTIONS THAT DIFFER ONLY IN THE CLOCK. Both reach the same entry; one
/// passes a quarter of an hour on the way and the other does not, and what lies past them
/// opens at seven. From a quarter to seven that is the whole difference between an option that
/// leads somewhere new and one that leads nowhere.
///
/// THE WORLD IS UNLOCKED ON PURPOSE, which is an arm rather than the default: the plugin sends
/// whatever the game says and the corpus fixtures derive it, and a locked clock carries no
/// register at all - so a locked world here would be testing nothing.
#[test]
fn a_menu_that_turns_the_hour_agrees_with_the_concrete_walk() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1, 2]))
        .add(Entry::new(1).player().script("PassTime()").links(&[3]))
        .add(Entry::new(2).player().links(&[3]))
        .add(Entry::new(3).guard("IsHour(7)").links(&[4]))
        .add(Entry::new(4))
        .build();

    let world = GameWorld::blank()
        .with_day_minutes(6 * 60 + 45)
        .with_clock_locked(false);

    assert!(
        DataLayout::for_group(&graph, &world, 16).clock().is_some(),
        "this menu is about a clock, so the layout has to be carrying one",
    );

    // Every way round of which entries are new, so the agreement is not one lucky assignment.
    for assignment in 0usize..243 {
        let seen_state = |id: DialogueNodeId| {
            let digit = assignment / 3usize.pow(id.entry_id.clamp(0, 4) as u32) % 3;
            [
                SeenState::SeenThisGame,
                SeenState::UnseenThisGame,
                SeenState::UnseenAnyGame,
            ][digit]
        };
        compare(&graph, &[node(1), node(2)], &world, &seen_state);
    }
}
