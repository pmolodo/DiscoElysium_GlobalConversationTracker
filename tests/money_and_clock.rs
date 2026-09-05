// SPDX-License-Identifier: MIT
//! What money and the clock cost the symbolic encoding.
//!
//! ## The criterion that was never met
//!
//! de-sze named arithmetic comparison on bit-blasted integers as the likely place the
//! symbolic approach would fail, and then every measurement in the epic ran with money and
//! the clock OUTSIDE the layout. So the predicted failure was never once exercised - a
//! different one arrived first, the diagrams doubling every five thousand steps of the
//! fixed point - and de-sze.5 kept "money and the clock encoded and exercised, with the
//! cost stated" as the criterion it had not met.
//!
//! This is that measurement. It runs the same question twice over the same group, once
//! with the two registers laid out and once without, and prints what changed.
//!
//! ## What was needed to make it possible at all
//!
//! Two things, both in `symbolic::register`:
//!
//! - COMPARISONS BY BITS. The old encoding enumerated the values satisfying a comparison
//!   and unioned them, which a counter can afford at five bits and money cannot at
//!   thirteen: eight thousand conjunctions to say `MoneyAmount() >= 50`. A ripple
//!   comparator is O(bits).
//! - SHIFTS BY SUBSTITUTION. `money += 50` was a case split over the register's values,
//!   for the same reason and with the same cost. Adding a constant is a bijection, and the
//!   image of a set under a bijection is the pre-image under its inverse, which is what
//!   substitution computes.
//!
//! ## What this does not claim
//!
//! That the encoding is now good. It says what the two registers cost on the groups this
//! repository measures, and a cost is not a verdict on its own - the surrounding numbers
//! in de-sze are what it has to be read against.

use std::collections::HashSet;

use lookahead_engine::core::action::DialogueActionKind;
use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::novelty_search::{best_novelty, Budget};
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::symbolic::budget::DiagramBudget;

mod common;

/// The counter cap every symbolic measurement here uses.
const COUNTER_CAP: i32 = 16;


/// What a balance is allowed to reach before it saturates.
///
/// Not a guess at how rich the player is. It is the largest number any guard or cost in
/// the group compares against, rounded up to a bit boundary - above that every balance
/// answers every question in the group identically, so telling them apart would be paying
/// for a distinction the content cannot observe.
const MONEY_CEILING: u32 = 8_191;

/// The groups every symbolic measurement in this repository is taken on.
const MEASURED: [i32; 5] = [368, 631, 14, 28, 1030];

/// Which groups to measure, from `CONVERSATION` where it is set.
///
/// One conversation per process is how these are run - see `tools/measure-symbolic.sh`.
/// They die in ways that take the process with them, and a run that measured 368 and then
/// overflowed its stack on 631 reported nothing at all for the three after it.
fn conversations() -> Vec<i32> {
    match std::env::var("CONVERSATION").ok().and_then(|v| v.trim().parse().ok()) {
        Some(one) => vec![one],
        None => MEASURED.to_vec(),
    }
}

/// How much of a group is about money or the clock.
#[derive(Default, Debug)]
struct Touches {
    money_actions: usize,
    time_actions: usize,
    money_guards: usize,
    clock_guards: usize,
}

fn touches(graph: &LookAheadGraph) -> Touches {
    let mut found = Touches::default();
    for node in graph.nodes() {
        for action in &node.actions {
            match action.kind() {
                DialogueActionKind::GainMoney | DialogueActionKind::LoseMoney => {
                    found.money_actions += 1;
                }
                DialogueActionKind::PassTime => found.time_actions += 1,
                _ => {}
            }
        }

        // The guard is walked as text, which is enough to count subjects and is what a
        // reader of this table wants: how much of the group even asks.
        let guard = node.guard.to_string();
        if guard.contains("MoneyAmount") {
            found.money_guards += 1;
        }
        if guard.contains("Hour") || guard.contains("IsMorning") || guard.contains("IsNight")
            || guard.contains("IsEvening") || guard.contains("IsAfternoon")
            || guard.contains("IsDaytime") || guard.contains("IsNighttime")
            || guard.contains("IsDawn") || guard.contains("IsDusk")
            || guard.contains("IsNoon") || guard.contains("IsMidnight")
        {
            found.clock_guards += 1;
        }
    }

    found
}

/// How much of each group is about money or the clock at all.
///
/// Run first and reported on its own, because a cost measured on a group that never asks
/// the question is not a measurement of anything. The whole epic's warning is about
/// content that compares a balance or a time, and it is worth knowing on the record how
/// much of that there is.
#[test]
fn how_much_of_the_measured_groups_is_about_money_or_the_clock() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");

    println!(
        "{:>6} {:>8} {:>14} {:>13} {:>13} {:>12}",
        "conv", "entries", "money actions", "time actions", "money guards", "clock guards",
    );

    let mut any_money = 0;
    let mut any_time = 0;
    for conversation in conversations() {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let found = touches(&graph);
        println!(
            "{conversation:>6} {:>8} {:>14} {:>13} {:>13} {:>12}",
            graph.nodes().count(),
            found.money_actions,
            found.time_actions,
            found.money_guards,
            found.clock_guards,
        );
        any_money += found.money_actions + found.money_guards;
        any_time += found.time_actions + found.clock_guards;
    }

    println!(
        "\nacross the five groups: {any_money} mentions of money, {any_time} of the clock",
    );
}

/// Every guard in the group, compiled both ways.
///
/// ## Why this and not only the search
///
/// The search halts as soon as it can answer, which is what makes it useful and what makes
/// it a poor place to measure an encoding: conversation 28 is the group that actually uses
/// money - 17 of its guards ask about a balance - and it answers in three milliseconds
/// having compiled almost none of them. A measurement taken there says nothing about the
/// arithmetic it was supposed to be about.
///
/// Compiling every guard in the group has no such escape. It is also the honest upper
/// bound on what the registers cost, since no search compiles more than all of them.
#[test]
fn what_the_registers_cost_the_guard_compiler() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>9} {:>7} {:>9} {:>7} {:>8} {:>7}",
        "conv", "guards", "compiled", "gaps", "compiled", "gaps", "newly", "ms",
    );
    println!(
        "{:>6} {:>8} {:>9} {:>7} {:>9} {:>7} {:>8} {:>7}",
        "", "", "without", "", "with", "", "decided", "with",
    );

    for conversation in conversations() {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };

        let without = compile_every_guard(&graph, &world, None, false);
        let with = compile_every_guard(&graph, &world, Some(MONEY_CEILING), true);

        println!(
            "{conversation:>6} {:>8} {:>9} {:>7} {:>9} {:>7} {:>8} {:>7}",
            graph.nodes().count(),
            without.compiled,
            without.fallbacks,
            with.compiled,
            with.fallbacks,
            without.fallbacks as i64 - with.fallbacks as i64,
            with.milliseconds,
        );

        // The gap that closes is the measurement. A run where it does not close has not
        // exercised the registers at all, whatever its timings say.
        if with.fallbacks > without.fallbacks {
            println!(
                "         MORE guards fell back WITH the registers, which should be \
                 impossible: they only ever add an answer."
            );
        }
    }
}

/// Compiles every guard in a group, once, and says what it cost.
fn compile_every_guard(
    graph: &LookAheadGraph,
    world: &dyn lookahead_engine::world::world::ILookAheadWorld,
    money: Option<u32>,
    clock: bool,
) -> Asked {
    let layout = DataLayout::for_graph(graph, COUNTER_CAP, money, clock);
    let symbols = graph.symbols().clone();
    let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());

    let mut compiler = GuardCompiler::new(&vars).with_world(world);
    if !clock {
        compiler = compiler.with_constant_clock(DataLayout::group_passes_time(graph));
    }

    let began = std::time::Instant::now();
    for node in graph.nodes() {
        let _ = compiler.compile(&node.guard);
    }

    Asked {
        answer: String::new(),
        milliseconds: began.elapsed().as_millis(),
        vars: layout.total_vars(),
        compiled: compiler.compiled(),
        fallbacks: compiler.fallbacks(),
    }
}

/// One question, asked twice: with the two registers, and without them.
///
/// The cost is the difference. Everything else about the run is held identical - the same
/// group, the same start, the same target, the same budget - so what moves is what the
/// registers cost.
#[test]
#[ignore = "a measurement, not a test: tools/measure-symbolic.sh runs it one per process"]
fn what_money_and_the_clock_cost_the_encoding() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>6} {:>6} {:>21} {:>7} {:>7} {:>21} {:>7} {:>7}",
        "conv", "vars", "+regs", "without", "ms", "gaps", "with", "ms", "gaps",
    );

    for conversation in conversations() {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        // One unseen entry, reachable by links from the start, which is the shape de-sze.5.7
        // established as the fair question. An unreachable target makes every search look
        // the same, which is how an earlier table in this epic came to be retracted.
        let Some(target) = deep_reachable_target(&graph, start) else {
            println!("{conversation:>6}  no reachable target; skipped");
            continue;
        };

        let unseen: HashSet<DialogueNodeId> = HashSet::from([target]);
        let novelty = |id: DialogueNodeId| {
            if unseen.contains(&id) { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
        };

        let (without, _) = ask(&graph, start, &world, &novelty, None, false);
        let (with, layout) = ask(&graph, start, &world, &novelty, Some(MONEY_CEILING), true);

        println!(
            "{conversation:>6} {:>6} {:>6} {:>21} {:>7} {:>7} {:>21} {:>7} {:>7}",
            without.vars,
            with.vars - without.vars,
            without.answer,
            without.milliseconds,
            without.fallbacks,
            with.answer,
            with.milliseconds,
            with.fallbacks,
        );

        println!(
            "         money {:?} bits, clock {:?} bits; {} guards compiled without, {} with",
            layout.money().map(|(_, bits)| bits),
            layout.clock().map(|(_, bits)| bits),
            without.compiled,
            with.compiled,
        );

        if without.answer != with.answer {
            println!(
                "         THE ANSWER CHANGED. Tracking the two registers is strictly more \
                 precise than treating them as constants, so a difference is the encoding \
                 either earning its variables or losing a branch it should have kept."
            );
        }
    }
}

/// Asks one group's question, with or without the two registers, and says what it cost.
fn ask(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn lookahead_engine::world::world::ILookAheadWorld,
    novelty: &dyn Fn(DialogueNodeId) -> Novelty,
    money: Option<u32>,
    clock: bool,
) -> (Asked, DataLayout) {
    let layout = DataLayout::for_graph(graph, COUNTER_CAP, money, clock);
    let symbols = graph.symbols().clone();
    let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());

    let mut compiler = GuardCompiler::new(&vars).with_world(world);
    if !clock {
        // Without a clock register the compiler answers clock questions from the world and
        // pretends the conversation cannot move it, which is the approximation this whole
        // measurement exists to price.
        compiler = compiler.with_constant_clock(DataLayout::group_passes_time(graph));
    }

    let seed = seed_of(graph, world, &vars);
    let began = std::time::Instant::now();
    let answer = best_novelty(
        graph,
        start,
        &seed,
        &mut compiler,
        world,
        COUNTER_CAP as u32,
        novelty,
        &Budget { targets: 1, time: std::time::Duration::from_secs(60), ..Default::default() },
        None,
    );

    (
        Asked {
            answer: format!("{:?}/{:?}", answer.best, answer.stopped_by),
            milliseconds: began.elapsed().as_millis(),
            vars: layout.total_vars(),
            compiled: compiler.compiled(),
            fallbacks: compiler.fallbacks(),
        },
        layout,
    )
}

/// What one run of the question cost, and how much of the group it could read.
///
/// The compiled and fallback counts are not decoration. A run where every money guard fell
/// back would produce exactly the timings a run where none did produces, and would be a
/// measurement of nothing - so the two numbers are what say the encoding was exercised at
/// all.
struct Asked {
    answer: String,
    milliseconds: u128,
    vars: u32,
    compiled: usize,
    fallbacks: usize,
}

/// An entry reachable from `start` by links, as far from it as the walk gets.
///
/// By LINKS, deliberately. A target picked out of the graph's own iteration order is at an
/// arbitrary position in a hash-ordered walk, and picking one that way is the mistake that
/// invalidated an earlier table in this epic: 368's group is only 38% reachable from its
/// own entry 0, so most such targets were not reachable at all and every search agreed
/// about them for the wrong reason.
fn deep_reachable_target(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
) -> Option<DialogueNodeId> {
    let mut seen = HashSet::from([start]);
    let mut frontier = vec![start];
    let mut last = None;

    while !frontier.is_empty() {
        let mut next = Vec::new();
        for id in frontier {
            last = Some(id);
            let Some(node) = graph.get(id) else { continue };
            for link in &node.links {
                if seen.insert(*link) {
                    next.push(*link);
                }
            }
        }

        frontier = next;
    }

    last.filter(|id| *id != start)
}
