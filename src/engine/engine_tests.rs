// SPDX-License-Identifier: MIT
//! The C# `LookAheadEngineTests`, ported: the crawl itself.
//!
//! Covers what the older ad-hoc tests in `lib.rs` do not - guards opened by a path's own
//! actions, cycles terminating, the clock moving under a walking crawl, cross-conversation
//! links, and every way a crawl can stop.
//!
//! The money cases are the ones worth reading first. They are taken from conversation 451,
//! where reaching a fifty-centime purchase actually requires 5,050 because it sits behind
//! a five-thousand-centime one, and no state-free crawl nor per-cost check can get that
//! right.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::core::guard_value::GuardValue;
use crate::core::types::{DialogueCheckKind, DialogueNodeId, LookAheadLimit, Novelty};
use crate::engine::engine::{LookAheadEngine, LookAheadOptions, LookAheadResult};
use crate::graph::graph::LookAheadGraph;
use crate::test_graph::{node, Entry, GraphBuilder};
use crate::world::test_world::TestWorld;

fn novel(unseen: &[i32]) -> impl Fn(DialogueNodeId) -> Novelty + '_ {
    let set: HashSet<i32> = unseen.iter().copied().collect();
    move |id| {
        if set.contains(&id.entry_id) {
            Novelty::UnseenAnyGame
        } else {
            Novelty::SeenThisGame
        }
    }
}

fn run(graph: &LookAheadGraph, world: &TestWorld, unseen: &[i32]) -> LookAheadResult {
    LookAheadEngine::default().evaluate(graph, node(0), world, novel(unseen))
}

fn at_time(hour: i32, minute: i32) -> TestWorld {
    TestWorld::new().with_day_minutes(hour * 60 + minute)
}

fn truth() -> GuardValue {
    GuardValue::from_boolean(true)
}

// ---- guards -------------------------------------------------------------------

#[test]
fn a_false_guard_blocks() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).guard(r#"Variable["shut"]"#).links(&[2]))
        .add(Entry::new(2))
        .build();

    let world = TestWorld::new().set_variable("shut", GuardValue::from_boolean(false));
    assert_eq!(run(&graph, &world, &[2]).best, Novelty::SeenThisGame);
}

/// A guard nobody can decide must not close the branch - the crawl over-approximates
/// rather than losing a marker.
#[test]
fn an_unknown_guard_does_not_block() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).guard("IsKimHere()").links(&[2]))
        .add(Entry::new(2))
        .build();

    assert_eq!(run(&graph, &TestWorld::new(), &[2]).best, Novelty::UnseenAnyGame);
}

/// The whole point of carrying state: a path's own actions open guards further along it.
#[test]
fn actions_unlock_their_own_downstream_guards() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).script(r#"SetVariableValue("opened", true)"#).links(&[2]))
        .add(Entry::new(2).guard(r#"Variable["opened"]"#))
        .build();

    // The world says nothing about it; only the walk sets it.
    assert_eq!(run(&graph, &TestWorld::new(), &[2]).best, Novelty::UnseenAnyGame);
}

/// A thought gained on the path opens the fork that asks for it.
///
/// The shape conversation 636 is built out of: Joyce grants `jamais_vu`, and later the
/// conversation forks on whether it is in the cabinet. While `IsTHCPresent` was answered
/// from the save, the crawl took the "not present" side forever, because a save cannot
/// hear about a gain the crawl has just made.
#[test]
fn a_gained_thought_opens_the_fork_that_asks_for_it() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).script(r#"GainThought("jamais_vu")"#).links(&[2, 3]))
        .add(Entry::new(2).guard(r#"IsTHCPresent("jamais_vu")"#))
        .add(Entry::new(3).guard(r#"IsTHCPresent("jamais_vu") == false"#))
        .build();

    // The save says the cabinet is empty, which is what the fork used to be judged on.
    let world = TestWorld::new().set_thought("jamais_vu", false);
    assert_eq!(run(&graph, &world, &[2]).best, Novelty::UnseenAnyGame);

    // And the other side of the same fork closes, which is the half that says the slot is
    // being read rather than everything simply being let through.
    assert_eq!(run(&graph, &world, &[3]).best, Novelty::SeenThisGame);
}

/// A thought nothing on the path gains is the save's business, and stays it.
#[test]
fn an_ungained_thought_is_the_saves_answer() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).guard(r#"IsTHCPresent("guillaume_le_million")"#).links(&[2]))
        .add(Entry::new(2))
        .build();

    let carried = TestWorld::new().set_thought("guillaume_le_million", true);
    assert_eq!(run(&graph, &carried, &[2]).best, Novelty::UnseenAnyGame);

    let without = TestWorld::new().set_thought("guillaume_le_million", false);
    assert_eq!(run(&graph, &without, &[2]).best, Novelty::SeenThisGame);
}

#[test]
fn the_start_node_is_not_scored() {
    // The marker says what lies BEYOND an option, so the option's own novelty is not it.
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1))
        .build();

    assert_eq!(run(&graph, &TestWorld::new(), &[0]).best, Novelty::SeenThisGame);
}

#[test]
fn groups_are_traversed_but_never_scored() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).group().links(&[2]))
        .add(Entry::new(2))
        .build();

    // The group itself is unseen and must not count...
    assert_eq!(run(&graph, &TestWorld::new(), &[1]).best, Novelty::SeenThisGame);
    // ...but the crawl still walks through it to what lies beyond.
    assert_eq!(run(&graph, &TestWorld::new(), &[2]).best, Novelty::UnseenAnyGame);
}

#[test]
fn cycles_terminate() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).links(&[2]))
        .add(Entry::new(2).links(&[1]))
        .build();

    let result = run(&graph, &TestWorld::new(), &[]);
    assert!(!result.budget_exhausted());
}

// ---- money --------------------------------------------------------------------

/// Conversation 451: the speakers cost 50 but are gated behind the 5,000 sneakers.
fn siileng() -> LookAheadGraph {
    GraphBuilder::new()
        .add(Entry::new(0).links(&[100]))
        .add(Entry::new(100).group().links(&[86, 11]))
        .add(Entry::new(86).cost(5000).links(&[87]))
        .add(
            Entry::new(87)
                .script(
                    "GainItem(\"shoes_faln\");\n\
                     SetVariableValue(\"jam.siileng_bought_faln_sneakers\", true)",
                )
                .links(&[100]),
        )
        .add(
            Entry::new(11)
                .guard(concat!(
                    r#"Variable["jam.siileng_bought_faln_sneakers"] == true"#,
                    r#"  and  Variable["jam.siileng_learned_when_you_can_buy_speakers"] == true"#,
                    r#"  and  CheckItem("samaran_speakers") == false"#
                ))
                .cost(50)
                .links(&[12]),
        )
        .add(Entry::new(12).script(r#"GainItem("samaran_speakers")"#))
        .build()
}

/// Reaching the speakers needs 5,050 - not the 5,000 the larger cost alone suggests, and
/// not the 50 the speakers' own price suggests.
#[test]
fn the_siileng_speakers_need_the_sum_of_both_purchases() {
    for (money, expected) in [
        (5049, Novelty::SeenThisGame),
        (5050, Novelty::UnseenAnyGame),
        (5300, Novelty::UnseenAnyGame),
    ] {
        let world = at_time(12, 0)
            .with_money(money)
            .set_variable("jam.siileng_learned_when_you_can_buy_speakers", truth());
        assert_eq!(run(&siileng(), &world, &[11]).best, expected, "with {money}");
    }
}

/// The same graph, a different starting state, a different answer.
#[test]
fn the_siileng_speakers_are_cheap_once_the_sneakers_are_owned() {
    let world = TestWorld::new()
        .with_money(50)
        .set_variable("jam.siileng_learned_when_you_can_buy_speakers", truth())
        .set_variable("jam.siileng_bought_faln_sneakers", truth())
        .set_item("shoes_faln", true);

    assert_eq!(run(&siileng(), &world, &[11]).best, Novelty::UnseenAnyGame);
}

/// The guard also requires not already owning the speakers, so a replay finds nothing -
/// the CheckItem half of the condition, read from crawl state.
#[test]
fn the_siileng_speakers_are_not_offered_twice() {
    let world = TestWorld::new()
        .with_money(5300)
        .set_variable("jam.siileng_learned_when_you_can_buy_speakers", truth())
        .set_variable("jam.siileng_bought_faln_sneakers", truth())
        .set_item("shoes_faln", true)
        .set_item("samaran_speakers", true);

    assert_eq!(run(&siileng(), &world, &[11]).best, Novelty::SeenThisGame);
}

/// A cost marked once is charged once however many times the crawl walks back over it.
///
/// The cycle is what makes it a real test: entry 2 links back to 1, so a per-visit charge
/// would drain the balance on the second pass and close the guard.
#[test]
fn a_cost_marked_once_is_charged_only_once() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).cost(2000).cost_once().links(&[2]))
        .add(Entry::new(2).links(&[1, 3]))
        .add(Entry::new(3).guard("MoneyAmount() >= 1000"))
        .build();

    // 2,000 pays for the room once and leaves 0; a second charge would be refused, but
    // once-only means there is no second charge - and the balance still cannot reach
    // 1,000, so nothing downstream of the guard is found.
    let poor = TestWorld::new().with_money(2000);
    assert_eq!(run(&graph, &poor, &[3]).best, Novelty::SeenThisGame);

    // With 3,000 the balance after the single charge is 1,000, which clears it.
    let rich = TestWorld::new().with_money(3000);
    assert_eq!(run(&graph, &rich, &[3]).best, Novelty::UnseenAnyGame);
}

/// A repeatable purchase inside a cycle must run out of money rather than run forever.
#[test]
fn a_repeatable_purchase_in_a_cycle_terminates_on_money() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).cost(100).links(&[2]))
        .add(Entry::new(2).links(&[1]))
        .build();

    let world = TestWorld::new().with_money(1000);
    let result = run(&graph, &world, &[]);
    assert!(!result.budget_exhausted());
}

// ---- the clock ----------------------------------------------------------------

#[test]
fn pass_time_moves_guards_past_the_hour() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).script("PassTime()").links(&[2]))
        .add(Entry::new(2).script("PassTime()").links(&[3]))
        .add(Entry::new(3).guard("IsAfternoon()"))
        .build();

    // 11:40 plus two quarter-hours is 12:10 - noon, which IsAfternoon includes.
    assert_eq!(run(&graph, &at_time(11, 40), &[3]).best, Novelty::UnseenAnyGame);
    // 11:00 plus two is 11:30, still the morning.
    assert_eq!(run(&graph, &at_time(11, 0), &[3]).best, Novelty::SeenThisGame);
}

/// The mirror, and the reason this is not cosmetic: without the clock the engine would
/// report the morning guard as still open.
#[test]
fn pass_time_closes_guards_it_moves_past() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).script("PassTime()").links(&[2]))
        .add(Entry::new(2).script("PassTime()").links(&[3]))
        .add(Entry::new(3).guard("IsMorning()"))
        .build();

    assert_eq!(run(&graph, &at_time(11, 40), &[3]).best, Novelty::SeenThisGame);
}

#[test]
fn pass_time_does_nothing_while_the_clock_is_locked() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).script("PassTime()").links(&[2]))
        .add(Entry::new(2).script("PassTime()").links(&[3]))
        .add(Entry::new(3).guard("IsAfternoon()"))
        .build();

    let locked = at_time(11, 40).with_clock_locked(true);
    assert_eq!(run(&graph, &locked, &[3]).best, Novelty::SeenThisGame);
}

/// The day is the story's counter, which PassTime does not touch, so it stays the world's
/// answer however much time a path burns - and burning time to midnight does not roll it.
///
/// The crawl answers the day questions itself, from `world.day_counter()`, rather than
/// leaving each world to reimplement a comparison against a number it already supplies.
#[test]
fn pass_time_does_not_advance_the_day() {
    let shut = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).script("PassTime()").links(&[2]))
        // Day 2 has not arrived and no amount of PassTime brings it.
        .add(Entry::new(2).guard("IsDayFrom(2)"))
        .build();

    let midnight = at_time(23, 55).with_day_counter(1);
    assert_eq!(run(&shut, &midnight, &[2]).best, Novelty::SeenThisGame);

    // The same guard on day 2 is open, so the refusal above is the day and not a
    // question the crawl simply declined to answer.
    let open = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).script("PassTime()").links(&[2]))
        .add(Entry::new(2).guard("IsDayFrom(2)"))
        .build();
    let tomorrow = at_time(23, 55).with_day_counter(2);
    assert_eq!(run(&open, &tomorrow, &[2]).best, Novelty::UnseenAnyGame);
}

// ---- links and conversations ---------------------------------------------------

#[test]
fn a_link_outside_the_loaded_group_ends_that_branch_without_failing() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        // Entry 404 is not in the graph.
        .add(Entry::new(1).links(&[404]))
        .build();

    let result = run(&graph, &TestWorld::new(), &[1]);
    assert_eq!(result.best, Novelty::UnseenAnyGame);
    assert!(!result.budget_exhausted());
}

// ---- what stops a crawl --------------------------------------------------------

#[test]
fn it_reports_the_strongest_novelty_reachable() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1, 2]))
        .add(Entry::new(1))
        .add(Entry::new(2))
        .build();

    let result = LookAheadEngine::default().evaluate(&graph, node(0), &TestWorld::new(), |id| {
        match id.entry_id {
            1 => Novelty::UnseenThisGame,
            2 => Novelty::UnseenAnyGame,
            _ => Novelty::SeenThisGame,
        }
    });
    assert_eq!(result.best, Novelty::UnseenAnyGame);
}

#[test]
fn exhausting_the_state_budget_is_reported() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).script(r#"SetVariableValue("n", Variable["n"] + 1)"#).links(&[1]))
        .build();

    let engine = LookAheadEngine::new(LookAheadOptions {
        state_budget: 3,
        time_budget: Duration::ZERO,
        ..Default::default()
    });
    let result = engine.evaluate(&graph, node(0), &TestWorld::new(), |_| Novelty::SeenThisGame);

    assert_eq!(result.stopped_by, LookAheadLimit::States);
    assert!(result.budget_exhausted());
}

/// A crawl with no time budget must not consult a clock at all.
#[test]
fn no_time_budget_means_no_clock() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1))
        .build();

    let engine = LookAheadEngine::new(LookAheadOptions {
        time_budget: Duration::ZERO,
        ..Default::default()
    });
    let result = engine.evaluate(&graph, node(0), &TestWorld::new(), |_| Novelty::SeenThisGame);

    assert_eq!(result.stopped_by, LookAheadLimit::None);
}

/// A start node that cannot be entered reports nothing, rather than pretending to have
/// searched.
#[test]
fn an_unreachable_start_reports_nothing() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).guard(r#"Variable["shut"]"#).links(&[1]))
        .add(Entry::new(1))
        .build();

    let world = TestWorld::new().set_variable("shut", GuardValue::from_boolean(false));
    let result = run(&graph, &world, &[1]);

    assert_eq!(result.best, Novelty::SeenThisGame);
    assert_eq!(result.states_explored, 0);
    assert_eq!(result.nodes_reached, 0);
}

// ---- reporting -----------------------------------------------------------------

#[test]
fn no_trace_is_kept_unless_asked() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1))
        .build();

    assert!(run(&graph, &TestWorld::new(), &[1]).trace.is_none());
}

#[test]
fn a_trace_records_the_state_the_crawl_started_from() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1))
        .build();

    let engine = LookAheadEngine::new(LookAheadOptions {
        collect_trace: true,
        ..Default::default()
    });
    let world = at_time(9, 30).with_money(250);
    let result = engine.evaluate(&graph, node(0), &world, |_| Novelty::SeenThisGame);

    let trace = result.trace.expect("a trace was asked for");
    assert_eq!(trace.start, node(0));
    assert_eq!(trace.money, 250);
    assert_eq!(trace.day_minutes, 9 * 60 + 30);
    assert_eq!(trace.graph_node_count, 2);
}

/// Nothing is reported when nobody is listening - a crawl with no progress callback must
/// not pay for one.
#[test]
fn nothing_is_reported_when_nobody_is_listening() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1))
        .build();

    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let engine = LookAheadEngine::new(
        LookAheadOptions {
            progress_interval: Duration::ZERO,
            ..Default::default()
        }
        .on_progress(move |_, _, _, _| {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    );
    engine.evaluate(&graph, node(0), &TestWorld::new(), |_| Novelty::SeenThisGame);

    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

// ---- checks that gate by flag --------------------------------------------------

#[test]
fn a_passing_check_closes_the_options_its_flag_guards() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(
            Entry::new(1)
                .kind(DialogueCheckKind::Red)
                .flag("check.red")
                .links(&[2]),
        )
        .add(Entry::new(2).guard(r#"(Variable["check.red"]) == false"#))
        .build();

    // Already passed: the flag is set, so entry 2 is shut behind it.
    let world = TestWorld::new().set_variable("check.red", truth());
    assert_eq!(run(&graph, &world, &[2]).best, Novelty::SeenThisGame);
}
