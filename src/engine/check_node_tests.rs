// SPDX-License-Identifier: MIT
//! The C# `CheckNodeTests`, ported: the entry-validity gate, one node type at a time.
//!
//! Every case here is a way an option disappears WITHOUT its condition being false, which
//! is the class of behaviour a guard-only model cannot see. It is also the subtlest part
//! of the engine - the difference between a red check and a white one is a single flag,
//! and getting it backwards silently changes what the player is offered.

use std::collections::HashSet;

use crate::core::types::{DialogueCheckKind, DialogueNodeId, Novelty, Ternary};
use crate::engine::engine::{LookAheadEngine, LookAheadResult};
use crate::graph::graph::LookAheadGraph;
use crate::test_graph::{node, Entry, GraphBuilder};
use crate::world::test_world::TestWorld;

/// Entries with these ids are unseen anywhere; everything else is spent.
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

/// A graph whose only route to entry 2 runs through the check at entry 1.
fn gated(kind: DialogueCheckKind, flag: Option<&str>, boolean_only: bool) -> LookAheadGraph {
    let mut check = Entry::new(1).kind(kind).links(&[2]);
    if let Some(flag) = flag {
        check = check.flag(flag);
    }
    if boolean_only {
        check = check.boolean_only();
    }

    GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(check)
        .add(Entry::new(2))
        .build()
}

// ---- test options -------------------------------------------------------------

/// HiddenTest entries are hidden outside developer mode, so in a real playthrough they
/// and everything behind them are unreachable.
#[test]
fn a_test_option_is_never_reachable() {
    let graph = gated(DialogueCheckKind::Test, None, false);
    assert_eq!(run(&graph, &TestWorld::new(), &[2]).best, Novelty::SeenThisGame);
}

// ---- fake checks --------------------------------------------------------------

/// A fake check is offered until it has been seen.
#[test]
fn a_fake_check_closes_once_seen() {
    let graph = gated(DialogueCheckKind::Fake, None, false);

    assert_eq!(run(&graph, &TestWorld::new(), &[2]).best, Novelty::UnseenAnyGame);

    let seen = TestWorld::new().set_seen(node(1), true);
    assert_eq!(run(&graph, &seen, &[2]).best, Novelty::SeenThisGame);
}

/// Walking a fake check displays it, so a path looping back finds it closed - the game
/// would not offer it twice and neither should the crawl.
#[test]
fn a_fake_check_closes_after_the_path_walks_through_it() {
    // 1 is the fake check; 2 loops back to it and also leads to 3.
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).kind(DialogueCheckKind::Fake).links(&[2]))
        .add(Entry::new(2).links(&[1, 3]))
        .add(Entry::new(3))
        .build();

    // Entry 3 is still found: the loop is not needed to reach it.
    assert_eq!(run(&graph, &TestWorld::new(), &[3]).best, Novelty::UnseenAnyGame);

    // And the crawl terminates rather than cycling through the check forever.
    assert!(!run(&graph, &TestWorld::new(), &[]).budget_exhausted());
}

/// The speculative marker is seeded from the save, so an entry the player really has seen
/// is closed from the first step - the two notions start equal.
#[test]
fn the_speculative_seen_marker_starts_from_the_save() {
    let graph = gated(DialogueCheckKind::Fake, None, false);

    assert_eq!(run(&graph, &TestWorld::new(), &[2]).best, Novelty::UnseenAnyGame);
    let seen = TestWorld::new().set_seen(node(1), true);
    assert_eq!(run(&graph, &seen, &[2]).best, Novelty::SeenThisGame);
}

/// Walking an entry does NOT make it count as read.
///
/// Novelty is what the player has actually seen and decides the marker; the speculative
/// flag only decides whether an option is still offered. Conflating them would silently
/// erase the novelty of everything a path touches.
#[test]
fn walking_an_entry_does_not_change_its_novelty() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(Entry::new(1).kind(DialogueCheckKind::Fake).links(&[2]))
        .add(Entry::new(2))
        .build();

    // The fake check itself is the unseen content, and the crawl walks it.
    assert_eq!(run(&graph, &TestWorld::new(), &[1]).best, Novelty::UnseenAnyGame);
}

// ---- Kim switches -------------------------------------------------------------

#[test]
fn a_kim_switch_closes_once_seen() {
    let graph = gated(DialogueCheckKind::KimSwitch, None, false);
    let seen = TestWorld::new().set_seen(node(1), true);
    assert_eq!(run(&graph, &seen, &[2]).best, Novelty::SeenThisGame);
}

/// A boolean_only switch stays available however often it is seen.
#[test]
fn a_boolean_only_kim_switch_stays_open_when_seen() {
    let graph = gated(DialogueCheckKind::KimSwitch, None, true);
    let seen = TestWorld::new().set_seen(node(1), true);
    assert_eq!(run(&graph, &seen, &[2]).best, Novelty::UnseenAnyGame);
}

/// And a loop does not close it either.
#[test]
fn a_boolean_only_kim_switch_stays_open_after_being_walked() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(
            Entry::new(1)
                .kind(DialogueCheckKind::KimSwitch)
                .boolean_only()
                .links(&[2]),
        )
        .add(Entry::new(2).links(&[1, 3]))
        .add(Entry::new(3))
        .build();

    let result = run(&graph, &TestWorld::new(), &[3]);
    assert_eq!(result.best, Novelty::UnseenAnyGame);
    assert!(!result.budget_exhausted());
}

// ---- red checks ---------------------------------------------------------------

/// A red check is rolled, so both results are possible and the look-ahead must carry
/// both. Content behind either outcome is reachable.
#[test]
fn a_red_check_explores_both_outcomes() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(
            Entry::new(1)
                .kind(DialogueCheckKind::Red)
                .flag("check.red")
                .links(&[2, 3]),
        )
        .add(Entry::new(2).guard(r#"Variable["check.red"]"#))
        .add(Entry::new(3).guard(r#"Variable["check.red_failed"]"#))
        .build();

    assert_eq!(run(&graph, &TestWorld::new(), &[2]).best, Novelty::UnseenAnyGame);
    assert_eq!(run(&graph, &TestWorld::new(), &[3]).best, Novelty::UnseenAnyGame);
}

/// One shot: a red check already decided either way is gone.
#[test]
fn a_red_check_closes_once_decided() {
    for flag in ["check.red", "check.red_failed"] {
        let graph = gated(DialogueCheckKind::Red, Some("check.red"), false);
        let world = TestWorld::new()
            .set_variable(flag, crate::core::guard_value::GuardValue::from_boolean(true));
        assert_eq!(run(&graph, &world, &[2]).best, Novelty::SeenThisGame, "{flag}");
    }
}

// ---- white checks -------------------------------------------------------------

/// A failed white check is CLOSED, the same as a red one - which is an approximation, and
/// the reasoning for it is worth having in full.
///
/// ## What the game does
///
/// It keeps failed white checks in `FailedWhiteChecks` - a real store, persisted across
/// saves by `FailedWhiteChecksPersister` - and reopens one only when
/// `IsFailedWhiteCheckPossible` says the odds have actually improved:
///
/// ```text
///     if (you.GetSkill(check.SkillType).rankValue > check.LastSkillValue
///         || check.difficulty + activeModifierBonuses < check.LastTargetValue)
///         return true;
/// ```
///
/// So a failure closes the check until the skill rank rises above what it was, or a
/// modifier lowers the effective target below what it was. It is NOT freely retryable,
/// which is what this test used to assert.
///
/// ## What is approximated, and which way it errs
///
/// Neither the skill rank nor the modifier expressions are modelled here, so a failure
/// closes the check for good. That is an UNDER-approximation: where a conversation really
/// does add a modifier, the crawl will not walk the retry and a marker can go missing.
/// Missing a marker is the direction this codebase normally refuses.
///
/// It is taken deliberately anyway, because the previous model was wrong in the other
/// direction and unboundedly so - a retryable check on a cycle is a loop nothing but the
/// state budget stops, and every white check multiplied the states explored. See de-1uy8
/// for the reopen rule, which is fully specified above and not yet built.
///
/// THE SHARPEST CASE IS THE ONE BELOW: a save that already holds `check.white_failed`. The
/// game would reopen it if the player has levelled the skill since, and this cannot know
/// that.
#[test]
fn a_failed_white_check_is_closed_like_a_red_one() {
    let graph = gated(DialogueCheckKind::White, Some("check.white"), false);
    let truth = crate::core::guard_value::GuardValue::from_boolean(true);

    let failed_before = TestWorld::new().set_variable("check.white_failed", truth.clone());
    assert_eq!(run(&graph, &failed_before, &[2]).best, Novelty::SeenThisGame);

    let passed_before = TestWorld::new().set_variable("check.white", truth);
    assert_eq!(run(&graph, &passed_before, &[2]).best, Novelty::SeenThisGame);
}

// ---- passive checks -----------------------------------------------------------

/// A failed passive check does not end the branch - the game sets the false-condition
/// action to pass through as it evaluates the entry. Its actions do not run, but the
/// conversation walks on.
#[test]
fn a_failed_passive_check_passes_through_to_its_children() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(
            Entry::new(1)
                .kind(DialogueCheckKind::Passive)
                .script(r#"SetVariableValue("fired", true)"#)
                .links(&[2]),
        )
        .add(Entry::new(2).guard(r#"(Variable["fired"]) == false"#))
        .build();

    // The check fails: entry 2's guard still holds, because the action never ran.
    let failed = TestWorld::new().set_check_result(node(1), Ternary::False);
    assert_eq!(run(&graph, &failed, &[2]).best, Novelty::UnseenAnyGame);

    // The check passes: the flag is set, and entry 2 is closed behind it.
    let passed = TestWorld::new().set_check_result(node(1), Ternary::True);
    assert_eq!(run(&graph, &passed, &[2]).best, Novelty::SeenThisGame);
}

/// With the outcome undetermined both branches are carried, so anything either outcome
/// reaches is reported. The soundness rule.
#[test]
fn an_undetermined_passive_check_reaches_either_side() {
    let graph = GraphBuilder::new()
        .add(Entry::new(0).links(&[1]))
        .add(
            Entry::new(1)
                .kind(DialogueCheckKind::Passive)
                .script(r#"SetVariableValue("fired", true)"#)
                .links(&[2, 3]),
        )
        .add(Entry::new(2).guard(r#"Variable["fired"]"#))
        .add(Entry::new(3).guard(r#"(Variable["fired"]) == false"#))
        .build();

    // TestWorld answers Unknown for a check it has not been told about.
    let world = TestWorld::new();
    assert_eq!(run(&graph, &world, &[2]).best, Novelty::UnseenAnyGame);
    assert_eq!(run(&graph, &world, &[3]).best, Novelty::UnseenAnyGame);
}
