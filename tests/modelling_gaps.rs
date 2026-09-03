// SPDX-License-Identifier: MIT
//! What is still not modelled in the conversation the epic is about?
//!
//! Conversation 631's group is the shape that drives the cost - the explicit crawl cannot
//! exhaust it - so it is the one symbolic reachability has to be tried on. A trial is
//! only worth anything if the model underneath is honest: every guard the compiler cannot
//! read becomes "undecided everywhere" and every action it cannot model is silently not
//! applied, and both make the reachable set bigger than the real one for reasons that
//! have nothing to do with whether decision diagrams pay.
//!
//! So this names the gaps rather than counting them. `guard_coverage` reports HOW MANY
//! sub-expressions fell back; this reports WHICH, so they can be modelled one at a time
//! and the list can be watched shrinking.

use std::collections::HashMap;

use lookahead_engine::core::action::DialogueActionKind;
use lookahead_engine::core::guard::GuardExpression;
use lookahead_engine::core::guard_value::GuardValue;
use lookahead_engine::core::types::{DialogueNodeId, Ternary};
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::world::world::ILookAheadWorld;

mod common;

/// The conversation the epic turns on.
const SUBJECT: i32 = 631;

/// A world shaped like a real save: a variable nothing set reads false, not unknown.
///
/// Deliberately the same shape as `guard_coverage`'s, because the two measurements are
/// only comparable if they run against the same world.
struct SaveWorld;

impl ILookAheadWorld for SaveWorld {
    fn money(&self) -> i32 { 0 }
    fn day_minutes(&self) -> i32 { 720 }
    fn day_counter(&self) -> i32 { 1 }
    fn is_clock_locked(&self) -> bool { false }
    fn get_variable(&self, _name: &str) -> GuardValue { GuardValue::from_boolean(false) }
    fn initially_has_item(&self, _name: &str) -> bool { false }
    fn initially_task_active(&self, _name: &str) -> bool { false }
    fn query(&self, name: &str, _arguments: &[GuardValue]) -> GuardValue {
        match name {
            "IsKimHere" => GuardValue::from_boolean(true),
            "IsCunoInParty" | "IsTHCPresent" => GuardValue::from_boolean(false),
            _ => GuardValue::unknown(),
        }
    }
    fn check_passes(&self, _node: DialogueNodeId) -> Ternary { Ternary::Unknown }
    fn is_seen(&self, _node: DialogueNodeId) -> bool { false }
}

/// Every sub-expression the compiler would have to give up on, rendered.
///
/// Mirrors `GuardCompiler::compile`'s reading rules rather than calling it, because the
/// compiler reports reasons and counts and what is wanted here is the text - the actual
/// guard fragment somebody has to sit down and model.
fn unreadable(
    guard: &GuardExpression,
    world: &dyn ILookAheadWorld,
    known: &dyn Fn(&str) -> bool,
    out: &mut Vec<String>,
) {
    match guard {
        GuardExpression::Literal(_) => {}

        GuardExpression::Variable(name) => {
            if !known(name) && world.get_variable(name).as_condition() == Ternary::Unknown {
                out.push(guard.to_string());
            }
        }

        GuardExpression::Not(inner) => unreadable(inner, world, known, out),

        GuardExpression::And(a, b) | GuardExpression::Or(a, b) => {
            unreadable(a, world, known, out);
            unreadable(b, world, known, out);
        }

        GuardExpression::Comparison(op, a, b) => {
            if op == "==" || op == "~=" {
                unreadable(a, world, known, out);
                unreadable(b, world, known, out);
            } else {
                out.push(guard.to_string());
            }
        }

        GuardExpression::Call(name, args) => {
            let subject = match &args[..] {
                [GuardExpression::Literal(value)] => Some(value.text().to_string()),
                _ => None,
            };

            let readable = match name.as_str() {
                // Answered from a slot when the group touches the item or task, and from
                // the world when it does not.
                "CheckItem" => subject.as_deref().is_some_and(|s| known(&format!("item:{s}"))),
                "IsTaskActive" => subject.as_deref().is_some_and(|s| known(&format!("task:{s}"))),
                _ if lookahead_engine::core::clock::ClockTime::owns(name) => {
                    let values: Vec<GuardValue> = args
                        .iter()
                        .filter_map(|a| match a {
                            GuardExpression::Literal(v) => Some(v.clone()),
                            _ => None,
                        })
                        .collect();
                    values.len() == args.len()
                        && lookahead_engine::core::clock::ClockTime::answer(
                            name,
                            &values,
                            world.day_minutes(),
                            world.day_counter(),
                        )
                        .as_condition()
                            != Ternary::Unknown
                }
                "MoneyAmount" => false,
                _ => {
                    let values: Vec<GuardValue> = args
                        .iter()
                        .filter_map(|a| match a {
                            GuardExpression::Literal(v) => Some(v.clone()),
                            _ => None,
                        })
                        .collect();
                    values.len() == args.len()
                        && world.query(name, &values).as_condition() != Ternary::Unknown
                }
            };

            if !readable {
                out.push(guard.to_string());
            }
        }
    }
}

/// Groups strings by how often they occur, most common first.
fn by_frequency(items: &[String]) -> Vec<(String, usize)> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for item in items {
        *counts.entry(item.as_str()).or_default() += 1;
    }

    let mut rows: Vec<(String, usize)> =
        counts.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    rows
}

#[test]
fn what_is_still_unmodelled_in_the_subject_conversation() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let (graph, group) =
        build_group_graph(&index, SUBJECT).expect("the subject group builds");
    let symbols = graph.symbols();
    let world = SaveWorld;
    let known = |name: &str| symbols.find(name).is_some();

    println!(
        "conversation {SUBJECT}: {} conversations, {} entries, {} slots",
        group.len(),
        graph.count(),
        symbols.count()
    );

    let mut guard_gaps: Vec<String> = Vec::new();
    let mut action_gaps: Vec<String> = Vec::new();
    let mut modelled_actions = 0;
    let mut entries_with_a_guard = 0;

    for node in graph.nodes() {
        if !matches!(&node.guard, GuardExpression::Literal(_)) {
            entries_with_a_guard += 1;
            unreadable(&node.guard, &world, &known, &mut guard_gaps);
        }

        for action in &node.actions {
            if action.kind() == DialogueActionKind::Unmodelled {
                action_gaps.push(action.name().to_string());
            } else {
                modelled_actions += 1;
            }
        }
    }

    println!(
        "\n{} entries carry a guard; {} sub-expressions in them cannot be read:",
        entries_with_a_guard,
        guard_gaps.len()
    );
    for (text, count) in by_frequency(&guard_gaps) {
        let shown: String = text.chars().take(110).collect();
        println!("  x{count:<4} {shown}");
    }

    println!(
        "\n{} actions modelled, {} not:",
        modelled_actions,
        action_gaps.len()
    );
    for (name, count) in by_frequency(&action_gaps) {
        println!("  x{count:<4} {name}");
    }

    // The question that decides whether an unmodelled action matters: does the group's
    // own guards ask about the subject it writes? A world query the crawl cannot change
    // is a constant and the world answers it, so an action nothing here reads is a gap
    // only on paper. One the guards DO read is a branch held shut.
    let asked: Vec<String> = raw_scripts(&index, &group)
        .iter()
        .flat_map(|script| subjects_of(script, "GainThought"))
        .collect();
    println!("\nthoughts this group gains: {:?}", by_frequency(&asked));

    assert!(graph.count() > 0, "the subject group has no entries");
}

/// Every userScript in the group, as written.
fn raw_scripts(
    index: &lookahead_engine::index::Index,
    group: &[i32],
) -> Vec<String> {
    group
        .iter()
        .filter_map(|id| index.get(id))
        .flat_map(|conversation| conversation.entries.iter().map(|e| e.script.clone()))
        .collect()
}

/// The quoted first argument of every call to `name` in a script.
fn subjects_of(script: &str, name: &str) -> Vec<String> {
    let needle = format!("{name}(\"");
    let mut found = Vec::new();
    let mut rest = script;
    while let Some(at) = rest.find(&needle) {
        rest = &rest[at + needle.len()..];
        match rest.find('"') {
            Some(end) => found.push(rest[..end].to_string()),
            None => break,
        }
    }

    found
}
