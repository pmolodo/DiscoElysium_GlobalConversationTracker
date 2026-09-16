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
use crate::index::journal::Journal;
use crate::index::{ConversationRecord, Index};
use crate::parser::action_parser::{parse_actions, parse_actions_with_journal};

const COUNTER_CAP: i32 = 16;

/// The story's day these tests run on, which nothing they apply moves.
const DAY: i32 = 1;

fn caps() -> CounterCaps<'static> {
    CounterCaps::flat(COUNTER_CAP)
}

/// Parses and applies, the way the search does.
fn run(
    script: &str,
    symbols: &mut StateSymbols,
    state: &LookAheadState,
    once_slot: i32,
) -> LookAheadState {
    let actions = parse_actions(script, symbols);
    DialogueAction::apply(&actions, state, once_slot, &caps(), false, DAY)
}

/// A journal of the tasks these tests name, each with show, done and cancel variables.
fn journal() -> Journal {
    let mut index = Index::new();
    for (id, task) in [
        "TASK.x",
        "TASK.advanced_ballistics_analysis",
        "TASK.locate_the_firearm",
        "TASK.find_the_body",
        "TASK.become_man_of_plenty",
    ]
    .into_iter()
    .enumerate()
    {
        let id = id as i32;
        let condition = |suffix: &str| format!("Variable[\"{task}{suffix}\"]");
        index.insert(
            id,
            ConversationRecord {
                id,
                hash: String::new(),
                fields: [
                    ("display_condition_main", condition("")),
                    ("done_condition_main", condition("_done")),
                    ("cancel_condition_main", condition("_cancelled")),
                ]
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect(),
                entries: Vec::new(),
            },
        );
    }
    Journal::from_index(&index)
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

/// A journal action writes the variables the game's journal writes, and refuses where it
/// refuses: no revealing a cancelled task, no cancelling a done one.
#[test]
fn journal_calls_write_the_variables_the_game_writes() {
    // (script, show, done, cancel set before, show, done, cancel after)
    let cases = [
        (r#"GainTask("TASK.x")"#, [0, 0, 0], [1, 0, 0]),
        (r#"GainTask("TASK.x")"#, [0, 0, 1], [0, 0, 1]),
        (r#"FinishTask("TASK.x_done")"#, [0, 0, 0], [1, 1, 0]),
        (r#"FinishTask("TASK.x")"#, [1, 0, 0], [1, 1, 0]),
        (r#"CancelTask("TASK.x_cancelled")"#, [1, 0, 0], [1, 0, 1]),
        (r#"CancelTask("TASK.x")"#, [1, 1, 0], [1, 1, 0]),
    ];
    let journal = journal();
    for (script, before, after) in cases {
        let mut symbols = StateSymbols::new();
        let slots = ["TASK.x", "TASK.x_done", "TASK.x_cancelled"].map(|v| symbols.variable(v));
        let actions = parse_actions_with_journal(script, &mut symbols, &journal);

        let mut start = empty(&symbols, 0);
        for (slot, value) in slots.iter().zip(before) {
            start = start.with(*slot, value);
        }
        let state = DialogueAction::apply(&actions, &start, -1, &caps(), false, DAY);
        let held = slots.map(|slot| state.get(slot));
        assert_eq!(held, after, "{script} from {before:?}");
    }
}

/// An action naming no task writes nothing, as the game logs and returns.
#[test]
fn a_journal_action_naming_no_task_writes_nothing() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions_with_journal(
        r#"FinishTask("TASK.nowhere_done")"#,
        &mut symbols,
        &journal(),
    );
    assert!(actions.is_empty(), "got {actions:?}");
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
    let raised = DialogueAction::apply(&actions, &state, -1, &caps(), false, DAY);

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
///
/// Kept with the DECISION that says which of the two it is, rather than as a bare name:
/// see `crate::core::modelling`.
#[test]
fn declared_calls_are_recorded_but_change_nothing() {
    let mut symbols = StateSymbols::new();
    // Both write state the search does not carry: one the character sheet's morale, the
    // other the screen. Both have a decision on file saying so, so both parse to a stub
    // rather than to an unknown.
    let actions = parse_actions(
        "DamageVolition(1);\nShowDialogueImage(\"darkness\")",
        &mut symbols,
    );

    assert_eq!(actions.len(), 2);
    assert!(
        actions
            .iter()
            .all(|a| a.kind() == DialogueActionKind::Declared)
    );
    assert!(actions.iter().any(|a| a.name() == "ShowDialogueImage"));
    assert!(
        actions.iter().all(|a| a.decision().is_some()),
        "a declared action carries the decision that declared it, got {actions:?}",
    );

    let before = LookAheadState::empty(symbols.count(), 250, 8 * 60);
    let after = DialogueAction::apply(&actions, &before, -1, &caps(), false, DAY);
    assert_eq!(after.money(), 250);
    assert_eq!(after.day_minutes(), 8 * 60);
}

/// A thought joins the cabinet, which is a slot like an item's.
///
/// `THCLuaFunctions.GainThought` runs `CharacterThoughts.GainThought`, which is
/// `gainedThoughts.Add(project)` - and `IsTHCPresent` is `gainedThoughts.Contains`, so
/// this is the same shape as GainItem and CheckItem. Assigning 1 rather than incrementing
/// is what the game does too: `Inventory.CanBeGained` refuses a thought already gained,
/// so gaining twice is gaining once.
#[test]
fn gaining_a_thought_sets_the_thought_slot() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions("GainThought(\"jamais_vu\")", &mut symbols);

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind(), DialogueActionKind::Assign);

    let slot = symbols
        .find("thought:jamais_vu")
        .expect("a thought slot is interned");
    let before = empty(&symbols, 0);
    assert!(!before.is_set(slot));

    let after = DialogueAction::apply(&actions, &before, -1, &caps(), false, DAY);
    assert!(after.is_set(slot));

    // Twice is once, the way the game has it.
    let again = DialogueAction::apply(&actions, &after, -1, &caps(), false, DAY);
    assert_eq!(again.get(slot), 1);
}

/// `Reputation(name, amount)` is ReputationGrows with the step written out.
///
/// Settled from the decompiled game rather than guessed: `Reputation`, `ModifyOnce`,
/// `ReputationGrows` and `ReputationLowers` all reach `ReputationAlterant.ReputationOption`,
/// which wraps the change in the same `once()` the parser already models. Which is why
/// this was left undecided until the source could be read - the difference between Modify
/// and ModifyOnce is a counter that climbs in a loop and one that does not.
#[test]
fn reputation_moves_by_its_own_amount_and_only_once() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions("Reputation(\"kim\", 2)", &mut symbols);

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind(), DialogueActionKind::Increment);
    assert_eq!(actions[0].value(), 2);
    assert!(actions[0].is_once(), "the game gates it on once()");

    let slot = symbols
        .find("reputation.kim")
        .expect("a reputation slot is interned");
    let once = symbols.once(DialogueNodeId::new(1, 0)) as i32;
    let before = empty(&symbols, 0);

    let after = DialogueAction::apply(&actions, &before, once, &caps(), false, DAY);
    assert_eq!(after.get(slot), 2);

    // And a second visit does not move it, which is the half that matters in a loop.
    let again = DialogueAction::apply(&actions, &after, once, &caps(), false, DAY);
    assert_eq!(again.get(slot), 2);
}

/// A negative amount lowers it, which is how the database writes a penalty.
#[test]
fn a_negative_reputation_amount_lowers_it() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions("Reputation(\"kim\", -2)", &mut symbols);

    assert_eq!(actions[0].value(), -2);
}

/// A call nobody has decided about stays UNKNOWN, and looks nothing like a stub.
///
/// The distinction this whole arrangement exists for. A stub is work finished and an
/// unknown is work outstanding, and they apply identically - so if the parser stopped
/// telling them apart, nothing else would notice and the outstanding list would quietly
/// read as empty.
#[test]
fn a_call_with_no_decision_behind_it_is_unknown() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions("EatTheRadio(\"loud\")", &mut symbols);

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind(), DialogueActionKind::Unmodelled);
    assert_eq!(actions[0].name(), "EatTheRadio");
    assert!(actions[0].decision().is_none());
}

/// Reputation is a dialogue variable, and the guards read it as one.
///
/// `ReputationGrows("x")` is `Variable["reputation.x"] = Variable["reputation.x"] + 1`
/// under a `once`, which is what `KarmaLuaFunctions` reduces to through
/// `ReputationAlterant.ModifyReputation`.
#[test]
fn reputation_grows_by_one_under_the_variable_the_guards_read() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions("ReputationGrows(\"apocalypse_cop\")", &mut symbols);

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind(), DialogueActionKind::Increment);
    let slot = symbols
        .find("reputation.apocalypse_cop")
        .expect("interned under its real name");

    let once_slot = symbols.once(DialogueNodeId::new(1, 0)) as i32;
    let before = LookAheadState::empty(symbols.count(), 0, 0);
    let after = DialogueAction::apply(&actions, &before, once_slot, &caps(), false, DAY);
    assert_eq!(after.get(slot), 1);

    // Once, so walking the same entry again does not raise it further.
    let again = DialogueAction::apply(&actions, &after, once_slot, &caps(), false, DAY);
    assert_eq!(again.get(slot), 1);
}

/// Losing reputation subtracts, and a slot has nowhere below zero to go.
///
/// The floor is not the game's - reputation can go negative there - it is what a state
/// can represent, and the symbolic image floors it the same way so the two agree.
#[test]
fn reputation_lowers_and_stops_at_zero() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions("ReputationLowers(\"honour\")", &mut symbols);
    let slot = symbols.find("reputation.honour").unwrap();

    let before = LookAheadState::empty(symbols.count(), 0, 0).with(slot, 2);
    let after = DialogueAction::apply(&actions, &before, -1, &caps(), false, DAY);
    assert_eq!(after.get(slot), 1);

    let floor = DialogueAction::apply(
        &actions,
        &LookAheadState::empty(symbols.count(), 0, 0),
        -1,
        &caps(),
        false,
        DAY,
    );
    assert_eq!(floor.get(slot), 0);
}

/// The experience is not search state; the variable recording it was awarded is.
#[test]
fn an_xp_award_sets_the_variable_it_records_itself_in() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions(
        "XPPicoSetBool(\"XP.butter_sign_i_did_this\");\\nXPTinySetBool(\"XP.tree_kicked\")",
        &mut symbols,
    );

    assert_eq!(actions.len(), 2);
    assert!(
        actions
            .iter()
            .all(|a| a.kind() == DialogueActionKind::Assign)
    );

    let sign = symbols.find("XP.butter_sign_i_did_this").unwrap();
    let tree = symbols.find("XP.tree_kicked").unwrap();
    let after = DialogueAction::apply(&actions, &empty(&symbols, 0), -1, &caps(), false, DAY);
    assert_eq!(after.get(sign), 1);
    assert_eq!(after.get(tree), 1);
}

/// Every use in the database is the bare call, which moves the clock a quarter hour.
#[test]
fn pass_time_advances_the_clock() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions("PassTime()", &mut symbols);

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind(), DialogueActionKind::PassTime);

    let before = LookAheadState::empty(symbols.count(), 0, 11 * 60);
    let after = DialogueAction::apply(&actions, &before, -1, &caps(), false, DAY);
    assert_eq!(after.day_minutes(), 11 * 60 + 15);
}

/// A locked clock does not move, so the action becomes a no-op.
#[test]
fn pass_time_is_ignored_when_the_clock_is_locked() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions("PassTime()", &mut symbols);

    let before = LookAheadState::empty(symbols.count(), 0, 11 * 60);
    let after = DialogueAction::apply(&actions, &before, -1, &caps(), true, DAY);
    assert_eq!(after.day_minutes(), 11 * 60);
}

#[test]
fn an_empty_script_produces_no_actions() {
    let mut symbols = StateSymbols::new();
    assert!(parse_actions("", &mut symbols).is_empty());
    assert!(parse_actions("   ", &mut symbols).is_empty());
}

/// The separator between statements is a literal backslash and the letter `n`, not a
/// newline, and every statement after the first depends on it being read as one.
///
/// Verbatim from the database. Read wrong, the scan starts at the `n` - a backslash is
/// no name start and a letter is - and the second and third calls become
/// `nGainTask` and `nSetVariableValue`, which match nothing and land as unmodelled.
#[test]
fn every_statement_after_the_first_is_read() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions_with_journal(
        "FinishTask(\"TASK.advanced_ballistics_analysis_done\");\
         \\nGainTask(\"TASK.locate_the_firearm\");\
         \\nSetVariableValue(\"tc.belle_magrave\", true) --[[ Variable[ ]]",
        &mut symbols,
        &journal(),
    );

    // FinishTask is two writes - reveal, then done - and the other two are one each.
    assert_eq!(actions.len(), 4, "got {actions:?}");
    assert!(
        actions.iter().all(|a| !matches!(
            a.kind(),
            DialogueActionKind::Unmodelled | DialogueActionKind::Declared
        )),
        "every call should be modelled, got {actions:?}",
    );

    let done = symbols
        .find("TASK.advanced_ballistics_analysis_done")
        .unwrap();
    let firearm = symbols.find("TASK.locate_the_firearm").unwrap();
    let belle = symbols.find("tc.belle_magrave").unwrap();

    let after = DialogueAction::apply(&actions, &empty(&symbols, 0), -1, &caps(), false, DAY);

    assert_eq!(after.get(done), 1);
    assert_eq!(after.get(firearm), 1);
    assert_eq!(after.get(belle), 1);
}

/// Prose inside a string argument does not end it, however many quotes it escapes.
///
/// Shortened from the newspaper text of `NewspaperEndgame`, which runs to two kilobytes
/// of reported speech. A scan that reads `\"` as the closing quote resumes tokenizing in
/// the middle of a sentence, where any word followed by a bracket becomes a call.
#[test]
fn an_escaped_quote_does_not_end_a_string() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions(
        "NewspaperEndgame(\"GIVING_UP\",\"COP GIVES UP\",\
         \"He shouted, \\\"I never loved that woman!\\\" GainItem(\\\"x\\\")\");\
         \\nGainItem(\"white_envelope\")",
        &mut symbols,
    );

    // The newspaper itself is not modelled; the point is that it is ONE action and the
    // item after it survives, rather than the prose fragmenting into several.
    assert_eq!(actions.len(), 2, "got {actions:?}");
    assert_eq!(actions[0].kind(), DialogueActionKind::Declared);
    assert_eq!(actions[0].name(), "NewspaperEndgame");

    let envelope = symbols.find("item:white_envelope").unwrap();
    let after = DialogueAction::apply(&actions, &empty(&symbols, 0), -1, &caps(), false, DAY);
    assert_eq!(after.get(envelope), 1);
}

/// A line comment ends at the separator, not at the end of the script.
///
/// There is no newline in a script to end it at, so looking for one swallowed everything
/// that followed. Only two scripts in the database write a bare `--`, both of them
/// em-dashes in prose, but where it happens nothing after it is read at all.
#[test]
fn a_line_comment_ends_at_the_separator() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions_with_journal(
        "GainItem(\"badge\") -- he kept it after all\\nGainTask(\"TASK.find_the_body\")",
        &mut symbols,
        &journal(),
    );

    assert_eq!(actions.len(), 2, "got {actions:?}");
    let badge = symbols.find("item:badge").unwrap();
    let body = symbols.find("TASK.find_the_body").unwrap();
    let after = DialogueAction::apply(&actions, &empty(&symbols, 0), -1, &caps(), false, DAY);

    assert_eq!(after.get(badge), 1);
    assert_eq!(after.get(body), 1);
}

/// A variable assigned as a Lua statement is written like one assigned through a call.
///
/// Verbatim shape from the database, between two calls so the order is checked too.
#[test]
fn a_direct_assignment_is_a_write() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions(
        r#"GainItem("badge");\nVariable["tc.electronic_locks"] = true;\nSetVariableValue("tc.electronic_locks", false)"#,
        &mut symbols,
    );

    assert_eq!(actions.len(), 3, "got {actions:?}");
    let locks = symbols.find("tc.electronic_locks").unwrap();
    assert_eq!(actions[1].slot(), locks as i32);
    assert_eq!(actions[1].kind(), DialogueActionKind::Assign);
    let after = DialogueAction::apply(&actions, &empty(&symbols, 0), -1, &caps(), false, DAY);
    assert_eq!(after.get(locks), 0, "the later call wins");
}

/// A comparison is not an assignment.
#[test]
fn a_comparison_statement_writes_nothing() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions(r#"Variable["x"] == true"#, &mut symbols);
    assert!(actions.is_empty(), "got {actions:?}");
}

/// A deadline stored from the clock holds the clock's reading, not a guessed number.
///
/// Day 2 at 10:00 is total hour 34, so `TotalHourCount() + 8` is 42; `NextMorningTime()` is
/// seven in the morning of day 3, total hour 55; `DayCount()` is 2.
#[test]
fn a_value_read_off_the_clock_is_the_clock_s_reading() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions(
        r#"SetVariableValue("deadline", TotalHourCount() + 8) ;\nSetVariableValue("meeting", NextMorningTime()) ;\nSetVariableValue("day", DayCount())"#,
        &mut symbols,
    );
    assert!(
        actions
            .iter()
            .all(|a| a.kind() == DialogueActionKind::AssignClock),
        "got {actions:?}",
    );

    let ten_in_the_morning = 10 * 60;
    let start = LookAheadState::empty(symbols.count(), 0, ten_in_the_morning);
    let after = DialogueAction::apply(&actions, &start, -1, &caps(), true, 2);
    assert_eq!(after.get(symbols.find("deadline").unwrap()), 42);
    assert_eq!(after.get(symbols.find("meeting").unwrap()), 55);
    assert_eq!(after.get(symbols.find("day").unwrap()), 2);
}

/// A value that cannot be one number when the script is read is a visible gap, not a 1.
#[test]
fn a_value_that_is_not_one_number_is_unmodelled() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions(
        r#"SetVariableValue("plaza.kineema_lights", not(Variable["plaza.kineema_lights"]))"#,
        &mut symbols,
    );
    assert_eq!(actions.len(), 1, "got {actions:?}");
    assert_eq!(actions[0].kind(), DialogueActionKind::Unmodelled);
}

/// A journal write hidden inside a value still happens, before the value is stored.
#[test]
fn a_write_inside_a_value_is_applied() {
    let mut symbols = StateSymbols::new();
    let actions = parse_actions_with_journal(
        r#"SetVariableValue("x", true  and  CancelTask("TASK.become_man_of_plenty_cancelled"))"#,
        &mut symbols,
        &journal(),
    );
    let cancel = symbols
        .find("TASK.become_man_of_plenty_cancelled")
        .expect("the nested call is read");
    assert_eq!(actions[0].slot(), cancel as i32);
}
