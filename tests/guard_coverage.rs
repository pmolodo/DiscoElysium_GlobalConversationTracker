// SPDX-License-Identifier: MIT
//! How much of the real guard corpus can be compiled into a formula?
//!
//! The question that decides whether explicit-control/symbolic-data reachability can
//! carry this content. Every guard the compiler cannot read becomes "undecided
//! everywhere", which is safe but useless: a branch that is always takeable prunes
//! nothing, and a reachable set built mostly from those is the whole state space.
//!
//! So the fallback rate is the headline. A low one means the formulas carry real
//! information; a high one means symbolic reachability would explore everything and the
//! approach is dead whatever the diagrams cost.
//!
//! The extracted game data this needs is not committed. It is REGENERATED automatically
//! when missing - see `tests/common` - rather than skipped, because a test that passes
//! without its data still reads as green and hides whatever it was meant to catch. The
//! only case that still skips is a machine with no game install at all, where nothing can
//! build it, and that says so loudly.

use std::collections::HashMap;
use std::path::PathBuf;

use lookahead_engine::core::guard::GuardExpression;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::vars::DataVars;

mod common;

const BIGGEST: [i32; 5] = [368, 631, 14, 28, 1030];
const COUNTER_CAP: i32 = 16;
const NODE_CAPACITY: usize = 1 << 20;
const CACHE_CAPACITY: usize = 1 << 18;

/// The conversation index, regenerating it if it is not there.
fn index_path() -> Option<PathBuf> {
    common::conversation_index()
}

/// Counts the world queries a guard makes, by name.
fn calls_of(guard: &GuardExpression, counts: &mut HashMap<String, usize>) {
    match guard {
        GuardExpression::Call(name, args) => {
            *counts.entry(name.clone()).or_default() += 1;
            for arg in args {
                calls_of(arg, counts);
            }
        }
        GuardExpression::Not(inner) => calls_of(inner, counts),
        GuardExpression::And(a, b)
        | GuardExpression::Or(a, b)
        | GuardExpression::Comparison(_, a, b) => {
            calls_of(a, counts);
            calls_of(b, counts);
        }
        GuardExpression::Literal(_) | GuardExpression::Variable(_) => {}
    }
}

/// Collects the variable names a guard mentions.
fn variables_of(guard: &GuardExpression, names: &mut Vec<String>) {
    match guard {
        GuardExpression::Variable(name) => names.push(name.clone()),
        GuardExpression::Not(inner) => variables_of(inner, names),
        GuardExpression::And(a, b) | GuardExpression::Or(a, b) => {
            variables_of(a, names);
            variables_of(b, names);
        }
        GuardExpression::Comparison(_, a, b) => {
            variables_of(a, names);
            variables_of(b, names);
        }
        GuardExpression::Call(_, args) => {
            for arg in args {
                variables_of(arg, names);
            }
        }
        GuardExpression::Literal(_) => {}
    }
}

/// Counts the leaves of a guard by the kind the compiler will treat them as.
fn tally(guard: &GuardExpression, counts: &mut HashMap<&'static str, usize>) {
    let key = match guard {
        GuardExpression::Literal(_) => "literal",
        GuardExpression::Variable(_) => "variable",
        GuardExpression::Call(_, _) => "call (world query)",
        GuardExpression::Comparison(op, _, _) => match op.as_str() {
            "==" | "~=" => "comparison (equality)",
            _ => "comparison (ordering)",
        },
        GuardExpression::Not(inner) => {
            tally(inner, counts);
            return;
        }
        GuardExpression::And(a, b) | GuardExpression::Or(a, b) => {
            tally(a, counts);
            tally(b, counts);
            return;
        }
    };
    *counts.entry(key).or_default() += 1;
}

#[test]
fn how_much_of_the_guard_corpus_compiles() {
    let Some(path) = index_path() else { return };
    let index = read_index(&path).expect("the index reads");

    println!(
        "{:>6} {:>7} {:>8} {:>9} {:>9} {:>8}",
        "conv", "guards", "compiled", "fallbacks", "precise%", "vars"
    );

    let mut shapes: HashMap<&'static str, usize> = HashMap::new();

    let mut measured = 0;

    for conversation_id in BIGGEST {
        let Ok((graph, _)) = build_group_graph(&index, conversation_id) else { continue };
        // Money and the clock are reached through world queries, which fall back anyway,
        // so they cost no variables here.
        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
        let symbols = graph.symbols().clone();
        // An empty world still ANSWERS for a variable it has never heard of - a variable
        // nothing has set is false - which is exactly the case a real save is in for most
        // of the database. That is what makes an untracked variable decidable.
        let world = common::measurement_save();
        let vars = DataVars::new(&layout, &symbols, NODE_CAPACITY, CACHE_CAPACITY);
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(&graph));

        let mut guards = 0;
        let mut mentioned: Vec<String> = Vec::new();
        let mut calls: HashMap<String, usize> = HashMap::new();
        for node in graph.nodes() {
            // An always-true guard is not evidence either way: most entries have none,
            // and counting them would flatter the result.
            if matches!(&node.guard, GuardExpression::Literal(_)) {
                continue;
            }

            guards += 1;
            tally(&node.guard, &mut shapes);
            variables_of(&node.guard, &mut mentioned);
            calls_of(&node.guard, &mut calls);
            let _ = compiler.compile(&node.guard);
        }

        // The hypothesis for why the compiled rate is so far below the compilable-by-
        // shape rate: the symbol table only holds names an ACTION mentions, because that
        // is what interns them. A guard reading a variable no action in the group ever
        // writes finds nothing in the table and falls back - even though such a variable
        // is CONSTANT for the whole crawl and so is the easiest thing there is to decide.
        let mut distinct: Vec<&String> = mentioned.iter().collect();
        distinct.sort();
        distinct.dedup();
        let known = distinct.iter().filter(|n| symbols.find(n).is_some()).count();
        println!(
            "       guard variables: {} distinct, {} in the symbol table, {} not",
            distinct.len(),
            known,
            distinct.len() - known
        );

        let compiled = compiler.compiled();
        let fallbacks = compiler.fallbacks();
        let total = compiled + fallbacks;
        println!(
            "{:>6} {:>7} {:>8} {:>9} {:>8.1}% {:>8}",
            conversation_id,
            guards,
            compiled,
            fallbacks,
            if total == 0 { 0.0 } else { 100.0 * compiled as f64 / total as f64 },
            layout.total_vars(),
        );
        println!(
            "         slots: {} item, {} task, {} total",
            DataLayout::slots_named(&symbols, "item:"),
            DataLayout::slots_named(&symbols, "task:"),
            symbols.count(),
        );
        for (reason, count) in compiler.fallback_reasons() {
            println!("         {count:>6}  {reason}");
        }
        let mut call_rows: Vec<(&String, &usize)> = calls.iter().collect();
        call_rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let shown: Vec<String> =
            call_rows.iter().take(8).map(|(n, c)| format!("{n} x{c}")).collect();
        println!("         world queries: {}", shown.join(", "));
        measured += 1;
    }

    println!("\nleaf shapes across all of them, most common first:");
    let mut rows: Vec<(&&str, &usize)> = shapes.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1));
    for (shape, count) in rows {
        println!("  {count:>7}  {shape}");
    }

    assert!(measured > 0, "no conversation was measured");
}

/// How many of the raw guard strings does the parser actually read?
///
/// This has to be asked separately, because `build_group_graph` turns a parse failure
/// into `always_true` - the same shape as "this entry has no guard". So a guard the
/// parser cannot read is invisible to the measurement above, which skips literals, AND
/// invisible to the crawl, which simply walks through it. A high rate here would mean
/// both the compile figures and the engine's own answers are being taken over content
/// nobody has read.
#[test]
fn how_much_of_the_guard_corpus_parses() {
    let Some(path) = index_path() else { return };
    let index = read_index(&path).expect("the index reads");

    let mut total = 0;
    let mut empty = 0;
    let mut parsed = 0;
    let mut failed_examples: Vec<String> = Vec::new();
    let mut failures = 0;

    for conversation in index.values() {
        for entry in &conversation.entries {
            total += 1;
            if entry.guard.trim().is_empty() {
                empty += 1;
                continue;
            }

            match lookahead_engine::parser::guard_parser::parse_guard(&entry.guard) {
                Ok(_) => parsed += 1,
                Err(_) => {
                    failures += 1;
                    if failed_examples.len() < 10 {
                        failed_examples.push(entry.guard.clone());
                    }
                }
            }
        }
    }

    let with_guard = total - empty;
    println!(
        "{total} entries in the whole index, {empty} with no guard, {with_guard} with one:\n  \
         {parsed} parsed, {failures} failed ({:.2}% of those with a guard)",
        if with_guard == 0 { 0.0 } else { 100.0 * failures as f64 / with_guard as f64 }
    );

    if !failed_examples.is_empty() {
        println!("\nfirst few the parser could not read:");
        for guard in &failed_examples {
            let shown: String = guard.chars().take(160).collect();
            println!("  {shown}");
        }
    }

    assert!(total > 0, "the index yielded no entries");
}
