// SPDX-License-Identifier: MIT
//! Do the symbolic searches find what a state-at-a-time walk finds, on real conversations?
//!
//! The independent corner. Every other test of the searches compares one against the other,
//! and they share a layout, a guard compiler, their action images and three deliberate
//! approximations - so a fault in any of those is invisible to all of them, and the tests
//! that used to say so were deleted along with the engine they used (de-eonm). This is what
//! replaces them: [`lookahead_engine::oracle`], which enumerates `(entry, state)` pairs one
//! at a time and shares none of that machinery.
//!
//! ## What is compared, and what is not
//!
//! The set of ENTRIES reached, not the states at them. The two do not carry the same thing -
//! one holds `(entry, state)` pairs and the other one data-state set per entry - so their
//! state counts are not comparable by construction. The reachable entry set is the thing
//! both compute and the thing the look-ahead actually uses: a marker depends on whether an
//! unseen entry can be reached, not on how many ways.
//!
//! ## The direction a disagreement is allowed to run
//!
//! The symbolic side may reach MORE. Its guards let an undecided answer through, money is
//! not in its layout so no cost can be refused (de-95t6), and a once action fires every time
//! round a loop while de-sze.15 is open - three deliberate over-approximations. A set that
//! is too big loses precision; one that is too small loses markers. So the assertion is
//! containment, and the surplus is reported rather than tolerated silently.
//!
//! ## Why a walk that gives up proves nothing
//!
//! Every test here asks [`Walk::exhausted`] first and skips the conversation if it is true.
//! A walk cut off at its ceiling reaches fewer entries than the graph allows, so a search
//! reaching more of them would be right and the comparison would be measuring the ceiling.

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::oracle::{self, Walk};
use lookahead_engine::symbolic::backward::{Backward, Budget as BackwardBudget};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated::on_its_own_thread;
use lookahead_engine::symbolic::known::Known;
use lookahead_engine::symbolic::novelty_search::{best_novelty, Budget as SearchBudget};
use lookahead_engine::symbolic::portfolio;
use lookahead_engine::symbolic::reachability::{seed_of, Reachability};
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::world::world::ILookAheadWorld;

mod common;

const COUNTER_CAP: i32 = 16;

/// Small enough that the walk can exhaust them, which is what makes them an oracle at all.
///
/// The same six the deleted comparisons used, deliberately: a conversation whose result
/// changed should be a change in the searches and not in what they were asked about.
const CHECKABLE: [i32; 6] = [1123, 484, 1066, 1147, 949, 511];

/// How many targets to ask the backward search about per group.
///
/// One backward pass each, so this is the whole cost of that test. Spread across the depth
/// range rather than taken from the front, because the first entries of a conversation are
/// its opening lines and they are all trivially reachable.
const TARGETS: usize = 40;

/// Which conversations this process should check, one per process being the intended way.
fn conversations(default: &[i32]) -> Vec<i32> {
    match std::env::var("CONVERSATION") {
        Ok(named) => named.split(',').filter_map(|id| id.trim().parse().ok()).collect(),
        Err(_) => default.to_vec(),
    }
}

/// Entries reachable from `start` by following links alone, with how far away they are.
///
/// Guards ignored, so it is an upper bound no stateful search can exceed - which is exactly
/// what a spread of questions wants: an entry it cannot find is unreachable for certain, and
/// asking about one proves nothing.
fn structural_depths(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
) -> HashMap<DialogueNodeId, usize> {
    let mut depth = HashMap::new();
    let mut queue = VecDeque::new();
    depth.insert(start, 0usize);
    queue.push_back(start);

    while let Some(id) = queue.pop_front() {
        let here = depth[&id];
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if graph.get(child).is_some() && !depth.contains_key(&child) {
                depth.insert(child, here + 1);
                queue.push_back(child);
            }
        }
    }

    depth
}

/// A spread of targets across the depth range, in a fixed order.
///
/// Sorted before sampling so the same targets are asked about on every run: a test that
/// picks different questions each time reports a different answer each time, and a rare
/// disagreement would look like a flake rather than a bug.
fn targets(depths: &HashMap<DialogueNodeId, usize>) -> Vec<DialogueNodeId> {
    let mut ordered: Vec<(usize, DialogueNodeId)> =
        depths.iter().map(|(id, d)| (*d, *id)).collect();
    ordered.sort_by_key(|(depth, id)| (*depth, id.conversation_id, id.entry_id));

    let step = (ordered.len() / TARGETS).max(1);
    ordered.iter().step_by(step).map(|(_, id)| *id).collect()
}

/// The group, its start, and the reference walk over it - or nothing, when it cannot be an
/// oracle for this conversation.
///
/// Shared by all three tests, which differ in what they ask afterwards rather than in what
/// they set up.
fn group(
    index: &lookahead_engine::index::Index,
    conversation: i32,
    world: &dyn ILookAheadWorld,
) -> Option<(LookAheadGraph, DialogueNodeId, Walk)> {
    let (graph, _) = build_group_graph(index, conversation).ok()?;
    let start = DialogueNodeId::new(conversation, 0);
    graph.get(start)?;

    let walk = oracle::walk(&graph, start, world, COUNTER_CAP);
    if walk.exhausted() {
        println!("{conversation:>6}  the reference walk ran out of room; skipped");
        return None;
    }

    Some((graph, start, walk))
}

#[test]
fn the_forward_search_reaches_what_the_reference_walk_reaches() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>9} {:>9} {:>8} {:>9} {:>7}",
        "conv", "entries", "walked", "symbolic", "surplus", "bddnodes", "steps"
    );

    let mut compared = 0;

    for conversation in conversations(&CHECKABLE) {
        let Some((graph, start, walk)) = group(&index, conversation, &world) else { continue };

        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
        let symbols = graph.symbols().clone();

        // A THREAD OF ITS OWN, with the manager built inside it - de-fpax. The entry set and
        // the counts are plain data and come back; the sets they were read off belong to the
        // manager and end with the thread.
        let (symbolic, stats, fallbacks) = on_its_own_thread(|| {
            let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
            let mut compiler = GuardCompiler::new(&vars)
                .with_world(&world)
                .with_constant_clock(DataLayout::group_passes_time(&graph));

            // The same state the walk starts in, encoded - not every data state. Starting
            // from all of them would walk paths needing an item the player has not got, and
            // report entries no real search can reach: a surplus that says nothing about the
            // encoding.
            let seed = seed_of(&graph, &world, &vars);
            let found = Reachability::explore(
                &graph, start, &seed, &mut compiler, &world, COUNTER_CAP as u32,
            );
            let entries: HashSet<DialogueNodeId> = found.entries().collect();
            (entries, found.stats().clone(), compiler.fallbacks())
        });

        let missed: Vec<&DialogueNodeId> = walk.entries().difference(&symbolic).collect();
        let surplus = symbolic.difference(walk.entries()).count();

        println!(
            "{conversation:>6} {:>8} {:>9} {:>9} {:>8} {:>9} {:>7}",
            graph.count(),
            walk.entries().len(),
            symbolic.len(),
            surplus,
            stats.diagram_nodes,
            stats.steps,
        );

        // A surplus is expected, but it should be explainable rather than mysterious. The
        // two known sources are an undecided guard let through and a cost check that cannot
        // be refused because money is not in the layout - and with a save holding no money
        // at all, the second is the one that bites: the walk declines every priced option
        // and the symbolic search takes them all.
        if surplus > 0 {
            println!(
                "         {surplus} extra: {fallbacks} guard fallbacks, {} cost checks \
                 undecidable",
                stats.unaffordable_unknown,
            );
        }

        assert!(
            missed.is_empty(),
            "conversation {conversation}: the symbolic search MISSED {} entries the walk \
             reached, which is the direction it is never allowed to be wrong in: {:?}",
            missed.len(),
            missed.iter().take(10).collect::<Vec<_>>(),
        );

        compared += 1;
    }

    assert!(compared > 0, "no conversation could be compared both ways");
}

#[test]
fn the_backward_search_finds_what_the_reference_walk_reaches() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9} {:>8}",
        "conv", "entries", "targets", "walked", "agreed", "surplus", "bddnodes", "ms"
    );

    let mut compared = 0;

    for conversation in conversations(&CHECKABLE) {
        let Some((graph, start, walk)) = group(&index, conversation, &world) else { continue };

        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
        let symbols = graph.symbols().clone();
        let depths = structural_depths(&graph, start);
        let asked = targets(&depths);

        // A THREAD FOR THE ORACLE'S COUNTERPART, with the manager built inside it - de-fpax.
        // This runs a forward fixed point and then two backward passes per target over one
        // manager, which is the arrangement that accumulates; the assertions inside are
        // re-raised here, so a disagreement still fails the test exactly as it would have.
        let (agreed, surplus, missed, diagram_nodes, took) = on_its_own_thread(|| {
            let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
            let mut compiler = GuardCompiler::new(&vars)
                .with_world(&world)
                .with_constant_clock(DataLayout::group_passes_time(&graph));
            let seed = seed_of(&graph, &world, &vars);

            // A SETTLED forward run, which is what licenses pruning a backward pass: it
            // bounds what can arrive at each entry, and says outright that some entries can
            // never be arrived at. Every target below is then asked TWICE - plain, and
            // pruned - because an unsound bound would show up here and nowhere else: this is
            // the only test that checks a backward answer against a walk rather than against
            // another symbolic search.
            let forward = Reachability::explore(
                &graph, start, &seed, &mut compiler, &world, COUNTER_CAP as u32,
            );
            let settled = forward.stats().reached_fixed_point;
            // PRUNING ON, because checking it is the point of asking twice. It is off by
            // default everywhere else - see Known::restricted - and this is what keeps it
            // honest while it waits for de-fawk.
            let known = Known::of(&graph)
                .from(start, &seed)
                .with_forward(&forward)
                .pruning(true);
            if !settled {
                println!("{conversation:>6}  the forward run did not settle; pruning not checked");
            }

            let began = std::time::Instant::now();
            let mut missed: Vec<DialogueNodeId> = Vec::new();
            let mut agreed = 0;
            let mut surplus = 0;
            let mut diagram_nodes = 0;

            for target in &asked {
                let backward =
                    Backward::reaching(&graph, *target, &mut compiler, &world, COUNTER_CAP as u32);
                let stats = backward.stats();
                assert!(
                    !stats.out_of_memory,
                    "conversation {conversation}: the manager ran out of room on {target}",
                );
                diagram_nodes += stats.diagram_nodes;

                let says_reachable = backward.reachable_from(start, &seed);

                // THE SAME QUESTION, PRUNED. A pass that meets the forward run stops early
                // having proved yes, so met_at is asked before the set is - reading a no out
                // of a pass that stopped on a yes is the mistake this arrangement invites.
                if settled {
                    let pruned = Backward::reaching_knowing(
                        &graph, *target, &mut compiler, &world, COUNTER_CAP as u32,
                        &BackwardBudget::default(), Some(&known),
                    );
                    let pruned_says =
                        pruned.stats().met_at.is_some() || pruned.reachable_from(start, &seed);
                    assert_eq!(
                        pruned_says, says_reachable,
                        "conversation {conversation}: pruning changed the answer about \
                         {target}, which it may never do",
                    );
                }

                match (walk.reached(*target), says_reachable) {
                    (true, true) => agreed += 1,
                    (true, false) => missed.push(*target),
                    (false, true) => surplus += 1,
                    (false, false) => agreed += 1,
                }
            }

            (agreed, surplus, missed, diagram_nodes, began.elapsed().as_millis())
        });

        println!(
            "{conversation:>6} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9} {:>8}",
            graph.count(),
            asked.len(),
            asked.iter().filter(|t| walk.reached(**t)).count(),
            agreed,
            surplus,
            diagram_nodes,
            took,
        );

        assert!(
            missed.is_empty(),
            "conversation {conversation}: the backward search called {} entries unreachable \
             that the reference walk got to, which is the direction it is never allowed to \
             be wrong in: {:?}",
            missed.len(),
            missed.iter().take(10).collect::<Vec<_>>(),
        );

        compared += 1;
    }

    assert!(compared > 0, "no conversation could be checked both ways");
}

/// The whole driver and the portfolio against the walk, on real conversations.
///
/// The tests above check reachability one entry at a time, which is the part that can be
/// wrong quietly. This checks the answer the bridge actually asks for.
///
/// ## The question is asked the hard way round
///
/// Against a fresh save everything is unseen, the first entry reached answers it, and both
/// sides return instantly having proved nothing. So the novelty function here says almost
/// everything is SEEN, leaving a handful of entries deep in the group unseen - which is the
/// shape that costs.
///
/// ## And the witness is checked by walking to it
///
/// An answer that names an entry can be checked against a walk that either got there or did
/// not. REPORTED, NOT ASSERTED: the searches over-approximate on purpose, so a witness the
/// walk cannot reach is a known-shaped imprecision rather than a fault - but a run where the
/// count starts climbing is worth seeing.
#[test]
fn the_driver_and_the_portfolio_find_what_the_reference_walk_finds() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>10} {:>10} {:>10} {:>8} {:>7}",
        "conv", "entries", "walked", "driver", "portfolio", "witness", "ms"
    );

    let mut compared = 0;
    let mut unwalkable_witnesses = 0;

    for conversation in conversations(&CHECKABLE) {
        let Some((graph, start, walk)) = group(&index, conversation, &world) else { continue };

        // The deepest few entries are the unseen ones: far from the start, so the answer
        // cannot be had by glancing at the first link.
        let depths = structural_depths(&graph, start);
        let mut by_depth: Vec<(usize, DialogueNodeId)> =
            depths.iter().map(|(id, d)| (*d, *id)).collect();
        by_depth.sort_by_key(|(depth, id)| {
            (std::cmp::Reverse(*depth), id.conversation_id, id.entry_id)
        });
        let unseen: HashSet<DialogueNodeId> =
            by_depth.iter().take(3).map(|(_, id)| *id).collect();

        let novelty = |id: DialogueNodeId| {
            if unseen.contains(&id) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        };

        let expected = walk.best_novelty(&novelty);
        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
        let symbols = graph.symbols().clone();

        let (driver, answer, took) = on_its_own_thread(|| {
            let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
            let mut compiler = GuardCompiler::new(&vars)
                .with_world(&world)
                .with_constant_clock(DataLayout::group_passes_time(&graph));
            let seed = seed_of(&graph, &world, &vars);
            let began = std::time::Instant::now();

            let driver = best_novelty(
                &graph,
                start,
                StartBranch::Either,
                &seed,
                &mut compiler,
                &world,
                COUNTER_CAP as u32,
                &novelty,
                &SearchBudget::default(),
                None,
            );

            // And the portfolio, which is what the bridge actually runs.
            let answer = portfolio::best_novelty(
                &graph,
                start,
                StartBranch::Either,
                &seed,
                &mut compiler,
                &world,
                COUNTER_CAP as u32,
                &novelty,
                graph.best_linked_class(start, &novelty).unwrap_or(Novelty::SeenThisGame),
                &portfolio::Budget::default(),
            );

            (driver.best, answer, began.elapsed().as_millis())
        });

        let witness = match answer.witness {
            Some(id) => {
                if !walk.reached(id) {
                    unwalkable_witnesses += 1;
                }
                format!("{}", id.entry_id)
            }
            None => "-".to_string(),
        };

        println!(
            "{conversation:>6} {:>8} {:>10} {:>10} {:>10} {:>8} {:>7}",
            graph.count(),
            format!("{expected:?}"),
            format!("{driver:?}"),
            format!("{:?}", answer.best),
            witness,
            took,
        );

        assert!(
            driver >= expected,
            "conversation {conversation}: the driver said {driver:?} where the walk found \
             {expected:?}, which is a marker lost",
        );
        assert!(
            answer.best >= expected,
            "conversation {conversation}: the portfolio said {:?} where the walk found \
             {expected:?}, so there IS a case a state-at-a-time search answers and the two \
             symbolic halves do not",
            answer.best,
        );

        compared += 1;
    }

    if unwalkable_witnesses > 0 {
        println!(
            "{unwalkable_witnesses} witness(es) the walk could not reach - see the note on \
             over-approximation above",
        );
    }

    assert!(compared > 0, "no conversation could be compared");
}
