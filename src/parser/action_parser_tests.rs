// SPDX-License-Identifier: MIT
//! The C# `ActionParserTests`, ported.
//!
//! Every case parses a script AND applies it, because the two halves are only meaningful
//! together - a parser that produces the right actions and an `apply` that ignores them
//! is as wrong as the reverse.
//!
//! Several scripts are copied verbatim from the shipped database, including the counter
//! idiom from conversation 825 and the trailing block comment holding an unbalanced
//! bracket that the exporter leaves on many scripts.

use crate::core::action::{CounterCaps, DialogueAction, DialogueActionKind};
use crate::core::state::{LookAheadState, StateSymbols};
use crate::core::types::DialogueNodeId;
use crate::parser::action_parser::parse_actions;

const COUNTER_CAP: i32 = 16;

fn caps() -> CounterCaps<'static> {
    CounterCaps::flat(COUNTER_CAP)
}

/// Parses and applies, the way the crawl does.
fn run(
    script: &str,
    symbols: &mut StateSymbols,
    state: &LookAheadState,
    once_slot: i32,
) -> LookAheadState {
    let actions = parse_actions(script, symbols);
    DialogueAction::apply(&actions, state, once_slot, &caps(), false)
}

/// An empty state wide enough for every slot interned so far.
fn empty(symbols: &StateSymbols, money: i32) -> LookAheadState {
    LookAheadState::empty(symbols.count(), money, 0)
}

#[test]
fn set_variable_value_assigns_true() {
    let mut symbols = StateSymbols::new();
    let once = symbols.once(DialogueNodeId::new(1, 0)) as i32;
    let slot = symbols.variable("whirling.lena_intro_done");
    let start = empty(&symbols, 0);

    // With the trailing block comment the exporter leaves on real scripts.
    let state = run(
        r#"SetVariableValue("whirling.lena_intro_done", true) --[[ Variable[ ]]"#,
        &mut symbols,
        &start,
        once,
    );

    assert!(state.is_set(slot));
}

#[test]
fn multiple_statements_all_apply() {
    let mut symbols = StateSymbols::new();
    let once = symbols.once(DialogueNodeId::new(1, 0)) as i32;
    let item = symbols.item("shoes_faln");
    let flag = symbols.variable("jam.siileng_bought_faln_sneakers");
    let start = empty(&symbols, 0);

    let state = run(
        "GainItem(\"shoes_faln\");\n\
         SetVariableValue(\"jam.siileng_bought_faln_sneakers\", true) --[[ Variable[ ]]",
        &mut symbols,
        &start,
        once,
    );

    assert!(state.is_set(item));
    assert!(state.is_set(flag));
}

#[test]
fn lose_item_clears_the_slot() {
    let mut symbols = StateSymbols::new();
    let slot = symbols.item("commemorative_pin");
    let start = empty(&symbols, 0).with(slot, 1);

    let state = run(r#"LoseItem("commemorative_pin")"#, &mut symbols, &start, -1);

    assert!(!state.is_set(slot));
}

#[test]
fn task_calls_set_and_clear() {
    for (script, expected) in [
        (r#"GainTask("TASK.x")"#, true),
        (r#"FinishTask("TASK.x")"#, false),
        (r#"CancelTask("TASK.x")"#, false),
    ] {
        let mut symbols = StateSymbols::new();
        let slot = symbols.task("TASK.x");
        let start = empty(&symbols, 0).with(slot, 1);

        let state = run(script, &mut symbols, &start, -1);
        assert_eq!(state.is_set(slot), expected, "{script}");
    }
}

/// The counter idiom from conversation 825, verbatim.
///
/// Reaching the same node twice on one path must not add twice, which is what keeps a
/// counter inside a loop from running away.
#[test]
fn a_once_increment_adds_then_stops() {
    let mut symbols = StateSymbols::new();
    let counter = symbols.variable("whirling.lena_quiz_wrong_counter");
    let once = symbols.once(DialogueNodeId::new(1, 7)) as i32;
    const SCRIPT: &str = concat!(
        r#"SetVariableValue("whirling.lena_quiz_wrong_counter", "#,
        r#"Variable["whirling.lena_quiz_wrong_counter"] +once(2)) "#
    );

    let start = empty(&symbols, 0);
    let first = run(SCRIPT, &mut symbols, &start, once);
    assert_eq!(first.get(counter), 2);

    let second = run(SCRIPT, &mut symbols, &first, once);
    assert_eq!(second.get(counter), 2);
}

#[test]
fn an_increment_saturates_at_the_cap() {
    let mut symbols = StateSymbols::new();
    let counter = symbols.variable("q.count");
    let state = empty(&symbols, 0).with(counter, COUNTER_CAP);

    let actions = parse_actions(
        r#"SetVariableValue("q.count", Variable["q.count"] + 3)"#,
        &mut symbols,
    );
    let raised = DialogueAction::apply(&actions, &state, -1, &caps(), false);

    assert_eq!(raised.get(counter), COUNTER_CAP);
}

#[test]
fn money_always_moves_the_balance() {
    for (script, expected) in [("GainMoneyAlways(40)", 140), ("LoseMoneyAlways(40)", 60)] {
        let mut symbols = StateSymbols::new();
        let start = empty(&symbols, 100);
        let state = run(script, &mut symbols, &start, -1);
        assert_eq!(state.money(), expected, "{script}");
    }
}

/// Every GainMoneyOnce node in the database sits inside a cycle, so "once" is what stops
/// the search minting money.
#[test]
fn gain_money_once_pays_only_the_first_time() {
    let mut symbols = StateSymbols::new();
    let once = symbols.once(DialogueNodeId::new(1, 3)) as i32;
    let start = empty(&symbols, 0);

    let first = run("GainMoneyOnce(500)", &mut symbols, &start, once);
    assert_eq!(first.money(), 500);

    let second = run("GainMoneyOnce(500)", &mut symbols, &first, once);
    assert_eq!(second.money(), 500);
}

/// The invariant the search's termination argument rests on.
#[test]
fn money_never_goes_negative() {
    let mut symbols = StateSymbols::new();
    let start = empty(&symbols, 100);
    let state = run("LoseMoneyAlways(500)", &mut symbols, &start, -1);
    assert_eq!(state.money(), 0);
}

/// Calls outside the model are KEPT rather than dropped, so a later pass can find them
/// and a reader can see they were considered.
///
/// That is what makes the unmodelled-action audit possible at all - see de-p95, and
/// de-6i8g, which was one of these turning out to matter.
#[test]
fn unmodelled_calls_are_recorded_but_change_nothing() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions(
        "ReputationGrows(\"honour\");\nShowDialogueImage(\"darkness\")",
        &mut symbols,
    );

    assert_eq!(actions.len(), 2);
    assert!(actions.iter().all(|a| a.kind() == DialogueActionKind::Unmodelled));
    assert!(actions.iter().any(|a| a.name() == "ShowDialogueImage"));

    let before = LookAheadState::empty(symbols.count(), 250, 8 * 60);
    let after = DialogueAction::apply(&actions, &before, -1, &caps(), false);
    assert_eq!(after.money(), 250);
    assert_eq!(after.day_minutes(), 8 * 60);
}

/// Every use in the database is the bare call, which moves the clock a quarter hour.
#[test]
fn pass_time_advances_the_clock() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions("PassTime()", &mut symbols);

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind(), DialogueActionKind::PassTime);

    let before = LookAheadState::empty(symbols.count(), 0, 11 * 60);
    let after = DialogueAction::apply(&actions, &before, -1, &caps(), false);
    assert_eq!(after.day_minutes(), 11 * 60 + 15);
}

/// A locked clock does not move, so the action becomes a no-op.
#[test]
fn pass_time_is_ignored_when_the_clock_is_locked() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions("PassTime()", &mut symbols);

    let before = LookAheadState::empty(symbols.count(), 0, 11 * 60);
    let after = DialogueAction::apply(&actions, &before, -1, &caps(), true);
    assert_eq!(after.day_minutes(), 11 * 60);
}

#[test]
fn an_empty_script_produces_no_actions() {
    let mut symbols = StateSymbols::new();
    assert!(parse_actions("", &mut symbols).is_empty());
    assert!(parse_actions("   ", &mut symbols).is_empty());
}
