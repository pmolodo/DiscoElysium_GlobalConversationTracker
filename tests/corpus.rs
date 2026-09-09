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
        let after = DialogueAction::apply(script, &state, once, &caps, false);
        assert!(
            after.money() >= 0,
            "money went negative on: {}",
            unescape(line)
        );
    }
}
