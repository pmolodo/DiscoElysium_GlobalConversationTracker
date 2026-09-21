// SPDX-License-Identifier: MIT
//! What is still not modelled in the conversation the epic is about?
//!
//! Conversation 631's group is the shape that drives the cost - the explicit search cannot
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
//! ## Three buckets, not two
//!
//! MODELLED, DECLARED, UNKNOWN. The middle one is what keeps the list honest: an action
//! somebody looked at and decided to skip - see [`lookahead_engine::core::modelling`] -
//! parses to a stub that does nothing on purpose, so it stops sitting in the same list as
//! the ones nobody has read. Without that separation the report has to be re-read from
//! scratch every time, because it cannot say which of its entries are already settled.
//!
//! What a decision does not do is stop mattering. Where it holds something constant that
//! the group's own guards ask about, the model is answering from a save the group has
//! already made stale - so the report measures that too, per decision, rather than
//! leaving the reader to take the decision's word for it.
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
use lookahead_engine::core::modelling::{self, Decision};
use lookahead_engine::index::{Index, build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::vars::DataVars;

use gct_measure::common;

/// The conversation the epic turns on.
const SUBJECT: i32 = 631;

const COUNTER_CAP: i32 = 16;

/// How wide a decision's reasoning is printed.
const WHY_WIDTH: usize = 88;

/// Text broken into lines of at most `width`, on word boundaries.
///
/// A decision's reasoning is stored as one long string - it is prose, and prose that is
/// pre-broken to a width is prose that has to be re-broken every time somebody edits it.
/// The report is the only thing that cares how wide it looks.
fn wrapped(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }

    lines
}

/// Groups strings by how often they occur, most common first.
fn by_frequency(items: &[String]) -> Vec<(String, usize)> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for item in items {
        *counts.entry(item.as_str()).or_default() += 1;
    }

    let mut rows: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
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
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let subject = SUBJECT;
    let (graph, group) = build_group_graph(&index, SUBJECT).expect("the subject group builds");
    let symbols = graph.symbols().clone();
    let world = common::measurement_save();

    println!(
        "conversation {subject}: {} conversations, {} entries, {} slots",
        group.len(),
        graph.count(),
        symbols.count()
    );

    let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
    let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(&world)
        .with_constant_clock(DataLayout::group_passes_time(&graph));

    let mut action_gaps: Vec<String> = Vec::new();
    let mut declared_actions: Vec<String> = Vec::new();
    let mut modelled_actions = 0;
    let mut entries_with_a_guard = 0;

    for node in graph.nodes() {
        // An always-true guard is not evidence either way, and most entries have one.
        if !matches!(node.guard.expression(), GuardExpression::Literal(_)) {
            entries_with_a_guard += 1;
            let _ = compiler.compile(&node.guard);
        }

        for action in &node.actions {
            match action.kind() {
                DialogueActionKind::Unmodelled => action_gaps.push(action.name().to_string()),
                DialogueActionKind::Declared => declared_actions.push(action.name().to_string()),
                _ => modelled_actions += 1,
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

    // Nothing above this line is a gap: these compiled. They are the questions whose
    // answers are only as good as a decision, and they belong in a gaps report for the
    // same reason a declared action does - so that "100% compiled" is read as what it is.
    println!("\napproximations in force on the guard side:");
    println!(
        "  the clock is held at the world's time: {}",
        if compiler.clock_is_approximated() {
            "AN APPROXIMATION here - this group passes time"
        } else {
            "exact here - nothing in this group passes time"
        },
    );
    let declared_questions: Vec<String> = compiler
        .declared_constants()
        .iter()
        .map(|(query, _)| (*query).to_string())
        .collect();
    println!(
        "  {} questions answered from the world that a declared decision writes: {:?}",
        declared_questions.len(),
        by_frequency(&declared_questions),
    );

    println!(
        "\n{modelled_actions} actions modelled, {} declared, {} unknown.",
        declared_actions.len(),
        action_gaps.len(),
    );

    // The declared ones first, because they are the shorter story: somebody decided, the
    // reason is on file, and nothing here needs doing.
    println!("\ndeclared - recognised, deliberately doing nothing:");
    for (name, count) in by_frequency(&declared_actions) {
        let verdict = modelling::for_action(&name)
            .map(Decision::verdict)
            .unwrap_or("no decision found");
        println!("  x{count:<4} {name} - {verdict}");
    }

    // And then the ones that are actually open. This list is the point of the file.
    println!("\nunknown - nobody has decided:");
    if action_gaps.is_empty() {
        println!("  (none)");
    }
    for (name, count) in by_frequency(&action_gaps) {
        println!("  x{count:<4} {name}");
    }

    report_exposure(&index, &group, &declared_actions, &compiler);

    // A declared no-op is a decision, not a licence to stop looking: an UNKNOWN in the
    // conversation the epic turns on is a measurement running on a model nobody has read.
    assert!(
        action_gaps.is_empty(),
        "conversation {SUBJECT} has undecided actions: {:?}",
        by_frequency(&action_gaps),
    );

    assert!(graph.count() > 0, "the subject group has no entries");
}

/// What each decision in force here actually costs THIS group.
///
/// A decision to hold something constant is exact until the group both writes it and
/// reads it back. Whether it does is not a matter of opinion, so it is measured rather
/// than assumed: the writers come from the group's own scripts and the readers from the
/// questions the compiler answered out of the world, and where the two name the same
/// subject the model is judging a branch against an answer its own actions have staled.
fn report_exposure(
    index: &Index,
    group: &[i32],
    declared_actions: &[String],
    compiler: &GuardCompiler<'_>,
) {
    println!("\nwhat the decisions in force cost this group:");

    let scripts = raw_scripts(index, group);
    for decision in modelling::DECISIONS {
        let written: Vec<String> = declared_actions
            .iter()
            .filter(|name| decision.writers.contains(&name.as_str()))
            .cloned()
            .collect();
        if written.is_empty() {
            continue;
        }

        // Named by the calls this group actually makes rather than by the decision's
        // first entry, which would name a family after whichever member happened to be
        // written down first.
        println!(
            "\n  {} - {} calls, {}",
            by_frequency(&written)
                .iter()
                .map(|(name, count)| format!("{name} x{count}"))
                .collect::<Vec<_>>()
                .join(", "),
            written.len(),
            decision.verdict(),
        );

        for line in wrapped(decision.why, WHY_WIDTH) {
            println!("      | {line}");
        }

        if !decision.is_held_constant() {
            println!("      no guard in the database reads what it writes");
            continue;
        }

        // How often the group asks one of the questions this decision answers out of the
        // world, and about what. Both are needed: some of these queries name a subject -
        // WHICH thought - and some, like `HasVolitionDamage`, ask about the character
        // and take no argument at all.
        let asked: Vec<&(&str, String)> = compiler
            .declared_constants()
            .iter()
            .filter(|(query, _)| decision.readers.contains(query))
            .collect();

        let mut writes: Vec<String> = Vec::new();
        for name in decision.writers {
            for script in &scripts {
                writes.extend(subjects_of(script, name));
            }
        }

        let mut reads: Vec<String> = Vec::new();
        for (query, text) in &asked {
            reads.extend(subjects_of(text, query));
        }

        if writes.is_empty() {
            println!("      writes: nothing this can name - no call here takes a quoted subject");
        } else {
            println!("      writes: {:?}", by_frequency(&writes));
        }
        println!(
            "      the group's guards ask {} times: {:?}",
            asked.len(),
            by_frequency(&reads),
        );

        // Matched by subject where both sides name one, and by bare co-occurrence where
        // they do not. The second is the weaker statement and is reported as such - it
        // says the group writes this and asks about this, not that it is the same this.
        let mut both: Vec<&String> = writes.iter().filter(|w| reads.contains(w)).collect();
        both.sort();
        both.dedup();
        if !both.is_empty() {
            println!("      EXPOSED - written and asked about here: {both:?}");
        } else if writes.is_empty() && !asked.is_empty() {
            println!("      EXPOSED, unmatched - no subject to match on, and the group does both");
        } else if asked.is_empty() {
            println!("      the group asks none of these questions: no exposure");
        } else {
            println!("      no subject is both written and asked about here: no exposure");
        }
    }
}
