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
//!
//! ## It asks the compiler rather than reimplementing it
//!
//! The first version of this file carried its own copy of the compiler's reading rules,
//! and the copy drifted immediately: it called every untracked `CheckItem` unreadable
//! when the compiler answers those from the world, and so reported 58 gaps where there
//! were 22. `GuardCompiler::fallback_subjects` exists to make that mistake impossible -
//! the report can only say what the compiler actually did.

use std::collections::HashMap;

use lookahead_engine::core::action::DialogueActionKind;
use lookahead_engine::core::guard::GuardExpression;
use lookahead_engine::index::{build_group_graph, read_index, Index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::vars::DataVars;

mod common;

/// The conversation the epic turns on.
const SUBJECT: i32 = 631;

/// Which conversation to survey, so any group can be asked about without an edit.
///
/// One per process, the way every measurement over a group is run here - see
/// `tools/measure-symbolic.sh` for why.
fn subject() -> i32 {
    std::env::var("CONVERSATION")
        .ok()
        .and_then(|named| named.trim().parse().ok())
        .unwrap_or(SUBJECT)
}
const COUNTER_CAP: i32 = 16;
const NODE_CAPACITY: usize = 1 << 20;
const CACHE_CAPACITY: usize = 1 << 18;

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

/// Every userScript in the group, as written.
fn raw_scripts(index: &Index, group: &[i32]) -> Vec<String> {
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

#[test]
fn what_is_still_unmodelled_in_the_subject_conversation() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let subject = subject();
    let (graph, group) =
        build_group_graph(&index, subject).expect("the subject group builds");
    let symbols = graph.symbols().clone();
    let world = common::measurement_save();

    println!(
        "conversation {subject}: {} conversations, {} entries, {} slots",
        group.len(),
        graph.count(),
        symbols.count()
    );

    let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
    let vars = DataVars::new(&layout, &symbols, NODE_CAPACITY, CACHE_CAPACITY);
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(&world)
        .with_constant_clock(DataLayout::group_passes_time(&graph));

    let mut action_gaps: Vec<String> = Vec::new();
    let mut modelled_actions = 0;
    let mut entries_with_a_guard = 0;

    for node in graph.nodes() {
        // An always-true guard is not evidence either way, and most entries have one.
        if !matches!(&node.guard, GuardExpression::Literal(_)) {
            entries_with_a_guard += 1;
            let _ = compiler.compile(&node.guard);
        }

        for action in &node.actions {
            if action.kind() == DialogueActionKind::Unmodelled {
                action_gaps.push(action.name().to_string());
            } else {
                modelled_actions += 1;
            }
        }
    }

    let total = compiler.compiled() + compiler.fallbacks();
    println!(
        "\n{entries_with_a_guard} entries carry a guard, holding {total} sub-expressions; \
         {} compiled ({:.1}%), {} did not:",
        compiler.compiled(),
        100.0 * compiler.compiled() as f64 / total as f64,
        compiler.fallbacks(),
    );

    // Grouped by reason, because the reason is what says whose gap it is: the compiler's,
    // or a world that will not answer a question the compiler was right to ask it.
    let mut by_reason: HashMap<&str, Vec<String>> = HashMap::new();
    for (reason, subject) in compiler.fallback_subjects() {
        by_reason.entry(reason).or_default().push(subject.clone());
    }

    let mut reasons: Vec<(&&str, &Vec<String>)> = by_reason.iter().collect();
    reasons.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
    for (reason, subjects) in reasons {
        println!("  {} x{}", reason, subjects.len());
        for (text, count) in by_frequency(subjects) {
            let shown: String = text.chars().take(100).collect();
            println!("      x{count:<4} {shown}");
        }
    }

    println!("\n{modelled_actions} actions modelled, {} not:", action_gaps.len());
    for (name, count) in by_frequency(&action_gaps) {
        println!("  x{count:<4} {name}");
    }

    // The question that decides whether an unmodelled action matters: do the group's own
    // guards ask about the subject it writes? A world query the crawl cannot change is a
    // constant and the world answers it, so an action nothing here reads is a gap only on
    // paper. One the guards DO read is a branch held shut.
    let gained: Vec<String> = raw_scripts(&index, &group)
        .iter()
        .flat_map(|script| subjects_of(script, "GainThought"))
        .collect();
    println!("\nthoughts this group gains: {:?}", by_frequency(&gained));

    assert!(graph.count() > 0, "the subject group has no entries");
}
