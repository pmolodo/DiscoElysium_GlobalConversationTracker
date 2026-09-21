// SPDX-License-Identifier: MIT
//! The parsers, run over every distinct guard and action in the shipped database.
//!
//! A port of the C# `CorpusTests`, and the first piece of the oracle this engine is
//! supposed to be checked against (de-sze.3). Cases someone thought to write down test
//! what they thought of; a corpus tests what the game actually contains.
//!
//! The extracted game data this needs is not committed. It is REGENERATED automatically
//! when missing - see `tests/common` - rather than skipped, because a test that passes
//! without its data still reads as green and hides whatever it was meant to catch. The
//! only case that still skips is a machine with no game install at all, where nothing can
//! build it, and that says so loudly.

use std::collections::HashMap;

use lookahead_engine::core::action::{CounterCaps, DialogueAction, DialogueActionKind};
use lookahead_engine::core::guard::IGuardContext;
use lookahead_engine::core::guard_value::GuardValue;
use lookahead_engine::core::state::{LookAheadState, StateSymbols};
use lookahead_engine::core::types::{DialogueNodeId, Ternary};
use lookahead_engine::parser::action_parser::parse_actions;
use lookahead_engine::parser::guard_parser::parse_guard;

mod common;

const GUARD_CORPUS: &str = "distinct_guards.txt";
const ACTION_CORPUS: &str = "distinct_scripts.txt";

/// The counter cap the engine defaults to, and what the C# corpus test uses.
const COUNTER_CAP: i32 = 16;

/// The story's day the corpus scripts are applied on; nothing here reads it but a clock value.
const CORPUS_DAY: i32 = 1;

/// Keeps a failure message readable when a whole corpus regresses.
const SAMPLE_LIMIT: usize = 10;

/// The lines of a corpus file, regenerating it if it is not there.
fn load(file_name: &str) -> Option<Vec<String>> {
    let path = common::corpus_file(file_name)?;
    let text = std::fs::read_to_string(&path).expect("the corpus reads");
    Some(text.lines().map(str::to_string).collect())
}

/// Reverses the escaping the corpus writer applies.
///
/// A scan rather than chained replacements, because a script containing a literal
/// backslash followed by `n` must not turn into a newline.
fn unescape(line: &str) -> String {
    if !line.contains('\\') {
        return line.to_string();
    }

    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }

        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }

    out
}

fn sample(failures: &[String]) -> Vec<String> {
    if failures.len() <= SAMPLE_LIMIT {
        return failures.to_vec();
    }

    let mut shown = failures[..SAMPLE_LIMIT].to_vec();
    shown.push(format!("... and {} more", failures.len() - SAMPLE_LIMIT));
    shown
}

/// A context that knows nothing, so every lookup comes back unknown.
struct EmptyContext;

impl IGuardContext for EmptyContext {
    fn get_variable(&self, _name: &str) -> GuardValue {
        GuardValue::unknown()
    }

    fn query(&self, _name: &str, _arguments: &[GuardValue]) -> GuardValue {
        GuardValue::unknown()
    }
}

#[test]
fn every_guard_in_the_database_parses() {
    let Some(corpus) = load(GUARD_CORPUS) else {
        return;
    };

    let failures: Vec<String> = corpus
        .iter()
        .map(|line| unescape(line))
        .filter(|guard| parse_guard(guard).is_err())
        .collect();

    println!(
        "parsed {} of {} guards",
        corpus.len() - failures.len(),
        corpus.len()
    );
    assert_eq!(sample(&failures), Vec::<String>::new());
}

/// Every guard must also EVALUATE without panicking, against a context that knows
/// nothing.
///
/// Everything should come back Unknown or a definite value. The engine runs this inside
/// a UI callback in the game, where a panic is not a test failure but a crash.
#[test]
fn every_guard_evaluates_without_panicking() {
    let Some(corpus) = load(GUARD_CORPUS) else {
        return;
    };

    let context = EmptyContext;
    let mut counts: HashMap<&'static str, usize> = HashMap::new();
    let mut parsed = 0;

    for line in &corpus {
        if let Ok(expression) = parse_guard(&unescape(line)) {
            parsed += 1;
            let key = match expression.test(&context) {
                Ternary::True => "true",
                Ternary::False => "false",
                Ternary::Unknown => "unknown",
            };
            *counts.entry(key).or_default() += 1;
        }
    }

    println!(
        "of {parsed} parsed guards: true {}, false {}, unknown {}",
        counts.get("true").copied().unwrap_or(0),
        counts.get("false").copied().unwrap_or(0),
        counts.get("unknown").copied().unwrap_or(0),
    );
    assert!(parsed > 0, "the corpus yielded no parseable guards");
}

#[test]
fn every_action_in_the_database_parses() {
    let Some(corpus) = load(ACTION_CORPUS) else {
        return;
    };

    let mut symbols = StateSymbols::new();
    let mut modelled = 0;
    let mut declared = 0;
    let mut unmodelled = 0;
    let mut by_name: HashMap<String, usize> = HashMap::new();

    for line in &corpus {
        for action in parse_actions(&unescape(line), &mut symbols) {
            match action.kind() {
                // Recognised and deliberately doing nothing - see
                // `lookahead_engine::core::modelling`. Counted apart from the unknowns
                // because the whole point of declaring one is that it stops being a name
                // on this list.
                DialogueActionKind::Declared => declared += 1,
                DialogueActionKind::Unmodelled => {
                    unmodelled += 1;
                    *by_name.entry(action.name().to_string()).or_default() += 1;
                }
                _ => modelled += 1,
            }
        }
    }

    println!(
        "{} scripts: {modelled} modelled actions, {declared} declared, \
         {unmodelled} unknown, {} slots",
        corpus.len(),
        symbols.count()
    );

    // The unknown ones by name, which is the audit de-p95 wants for the action side.
    // Every one of these is either something to model or something to decide about.
    let mut rows: Vec<(&String, &usize)> = by_name.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("undecided action functions, most common first:");
    for (name, count) in &rows {
        println!("  {count:>6}  {name}");
    }

    assert!(modelled > 0, "the corpus yielded no modelled actions");
}

/// Applying every action in the database must leave money non-negative.
///
/// The invariant the search's termination argument rests on: money only ever falls, and
/// a state with negative money would mean the affordability check had been bypassed.
#[test]
fn no_action_drives_money_negative() {
    let Some(corpus) = load(ACTION_CORPUS) else {
        return;
    };

    let mut symbols = StateSymbols::new();
    let once = symbols.once(DialogueNodeId::new(0, 0)) as i32;
    let caps = CounterCaps::flat(COUNTER_CAP);

    // Built once the table is complete, so every slot the corpus mentions has room.
    let mut scripts = Vec::with_capacity(corpus.len());
    for line in &corpus {
        scripts.push(parse_actions(&unescape(line), &mut symbols));
    }

    let state = LookAheadState::empty(symbols.count(), 0, 0);
    for (script, line) in scripts.iter().zip(corpus.iter()) {
        let after = DialogueAction::apply(script, &state, once, &caps, false, CORPUS_DAY);
        assert!(
            after.money() >= 0,
            "money went negative on: {}",
            unescape(line)
        );
    }
}

/// Every call a guard makes must take LITERAL arguments only.
///
/// ## What a computed argument costs
///
/// `bridge::collect` walks a group's guards and places every call somewhere: answered from
/// what the engine already holds, turned into a `DataRequest` the plugin services, or asked
/// of the plugin by key. A call whose arguments are not all literals cannot be placed - the
/// key it would be asked under depends on a value that is only known per state, and a
/// snapshot is answered once - so it is left out, reads Unknown, and the guard turns
/// permissive.
///
/// ## Why it is worth a test rather than a comment
///
/// Nothing else would say. A guard that went permissive would keep marking, just more of the
/// menu than it should, and the only symptom is a marker no play can clear. The shipped
/// database has NONE, so this is a line the dialogue currently stays on the right side of
/// rather than a limit anybody is working around - and a game patch that introduced one would
/// reach a player as that marker unless something failed first.
///
/// See de-m11s.4, and `docs/modelling-gaps.md` for what else the engine does not model.
#[test]
fn no_guard_calls_anything_with_a_computed_argument() {
    use lookahead_engine::core::guard::{Guard, GuardExpression, GuardRef};

    /// Every call in the subtree at `node` whose arguments are not all literals.
    fn computed(guard: &Guard, node: GuardRef<'_>, found: &mut Vec<String>) {
        match node.expression() {
            GuardExpression::Call(name, arguments) => {
                for index in 0..arguments.len() {
                    let Some(argument) = arguments.get(index) else {
                        continue;
                    };
                    if !matches!(argument.expression(), GuardExpression::Literal(_)) {
                        found.push(name.to_string());
                    }
                    computed(guard, argument, found);
                }
            }
            GuardExpression::Not(inner) => computed(guard, inner, found),
            GuardExpression::And(left, right) | GuardExpression::Or(left, right) => {
                computed(guard, left, found);
                computed(guard, right, found);
            }
            GuardExpression::Comparison(_, left, right) => {
                computed(guard, left, found);
                computed(guard, right, found);
            }
            GuardExpression::Literal(_) | GuardExpression::Variable(_) => {}
        }
    }

    // THE DETECTOR HAS TO BE ABLE TO FIRE. This test has never failed and the corpus holds
    // nothing it would catch, so without this it could go vacuous - a walk that missed call
    // arguments entirely would pass just as loudly.
    let crafted = parse_guard(r#"IsHour(HourCount())"#).expect("the probe parses");
    let mut caught = Vec::new();
    computed(&crafted, crafted.as_ref(), &mut caught);
    assert_eq!(
        caught,
        vec!["IsHour".to_string()],
        "the detector does not detect"
    );

    let Some(corpus) = load(GUARD_CORPUS) else {
        return;
    };

    let mut failures: Vec<String> = Vec::new();
    let mut calls = 0;
    for line in &corpus {
        let text = unescape(line);
        let Ok(guard) = parse_guard(&text) else {
            // Whether every guard parses is `every_guard_in_the_database_parses`.
            continue;
        };
        let mut found = Vec::new();
        computed(&guard, guard.as_ref(), &mut found);
        calls += found.len();
        for name in found {
            failures.push(format!("{name} in: {text}"));
        }
    }

    println!(
        "{} guard(s) checked, {calls} computed argument(s)",
        corpus.len()
    );
    assert_eq!(sample(&failures), Vec::<String>::new());
}

/// Every reputation the game compares must be declared a NUMBER by the database.
///
/// ## What rests on it
///
/// `IsHighestPolitical` and `IsHighestCopotype` are answered by `reputation::highest`, which
/// walks its whole range and gives up - answering Unknown for the question - as soon as one
/// amount cannot be read as a number. It gives up rather than treating the gap as a zero on
/// purpose: a zero is a PARTICIPANT in that comparison, not an absence, and one invented in
/// the wrong place ties with a real zero and clears the winner.
///
/// So the question stays answerable exactly while the table declares all eight as numbers. It
/// does, and each starts at zero. A database that changed one to a Boolean would make every
/// reputation question in the game Unknown, and nothing else would say so - the guard would
/// simply turn permissive and mark entries no play can reach.
///
/// The group's half of it needs no test: `reputation::variables_read_by` returns the WHOLE
/// range, and `LookAheadGraph` adds all of them, so a guard that asks is a guard whose group
/// declares every amount the answer needs.
///
/// See de-m11s.8.
#[test]
fn every_reputation_the_game_compares_is_declared_a_number() {
    use lookahead_engine::core::guard_value::GuardValueKind;
    use lookahead_engine::world::IVariableTable;

    let Some(declared) = common::variable_table() else {
        return;
    };

    let mut wrong: Vec<String> = Vec::new();
    for reputation in lookahead_engine::core::reputation::IN_ENUM_ORDER {
        let name = lookahead_engine::core::reputation::variable_of(reputation);
        let value = declared.unset(&name);
        if value.kind() != GuardValueKind::Number {
            wrong.push(format!(
                "{name} is declared {:?}, not a number",
                value.kind()
            ));
        }
    }

    assert_eq!(wrong, Vec::<String>::new());
}

/// Every bonus the shipped database states is one the engine has been told about.
///
/// ## What goes wrong without it
///
/// `item_names.jsonl` carries an item's bonus as the database spells it, and
/// `core::garment` translates that into the engine's skills. The translation is a list
/// somebody wrote by reading the data - it has to be, since the data is inconsistent with
/// itself: `Electrochemisty` beside `Electrochemistry`, `Reaction` beside `Reaction Speed`.
///
/// A game patch that adds an item, or fixes one of those misspellings, states a name the list
/// has never seen. `garment::moved_by` answers `None` for it, which means the garment stops
/// unsettling the checks it moves - and the symptom is one entry marked wrongly, which nobody
/// would trace back to a table of names.
///
/// See de-sr1u.5.
#[test]
fn every_bonus_the_database_states_is_one_the_engine_knows() {
    use lookahead_engine::core::garment;

    let Some(path) = common::item_names() else {
        return;
    };
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} does not read: {error}", path.display()));

    let mut stated = 0;
    let mut unknown: Vec<String> = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let row: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let Some(bonuses) = row["bonuses"].as_array() else {
            continue;
        };
        for bonus in bonuses {
            let Some(moves) = bonus["moves"].as_str() else {
                continue;
            };
            stated += 1;
            if garment::moved_by(moves).is_none() {
                unknown.push(format!(
                    "{} states '{moves}', which core::garment has not been told about",
                    row["name"].as_str().unwrap_or("?"),
                ));
            }
        }
    }

    assert!(stated > 0, "no item states a bonus, so nothing was checked");
    println!("{stated} bonus(es) stated by the database");
    assert_eq!(sample(&unknown), Vec::<String>::new());
}
