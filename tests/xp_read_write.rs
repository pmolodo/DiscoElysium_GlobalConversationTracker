// SPDX-License-Identifier: MIT
//! Does anything ever branch on the XP flags the search models?
//!
//! `XPPicoSetBool` and its siblings assign a variable, and the search models that. The
//! question is whether it needs to: a variable no guard in the group READS is constant as
//! far as reachability is concerned, and modelling the write costs a slot - which is a
//! decision-diagram variable - for nothing.
//!
//! The distinction that matters is not "does any guard anywhere read an XP variable".
//! Plenty do. It is whether a guard reads one THAT A SEARCH OVER THE SAME GROUP CAN WRITE,
//! because only then does the value depend on the path taken. Anywhere else the world
//! answers it once and the slot is dead weight.

use std::collections::HashSet;

use lookahead_engine::core::action::DialogueActionKind;
use lookahead_engine::core::guard::{Guard, GuardExpression};
use lookahead_engine::index::{build_group_graph, discover_group, read_index};

mod common;

/// The XP flags' shared prefix, as the scripts name them.
const XP_PREFIX: &str = "XP.";

/// The five biggest groups, plus the two the question was asked about.
const GROUPS: [i32; 6] = [368, 631, 14, 28, 1030, 361];

/// Every variable name a guard mentions.
fn variables_of(guard: &Guard, out: &mut HashSet<String>) {
    for node in guard.nodes() {
        if let GuardExpression::Variable(name) = node.expression() {
            out.insert(name.to_string());
        }
    }
}

#[test]
fn is_any_xp_flag_both_written_and_read_inside_one_group() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");

    // The whole database first, which is the loosest possible test: if a name is never
    // both written and read ANYWHERE, it certainly is not within a group.
    let mut written_anywhere: HashSet<String> = HashSet::new();
    let mut read_anywhere: HashSet<String> = HashSet::new();

    for conversation in index.values() {
        for entry in &conversation.entries {
            for name in entry.script.split('"') {
                if name.starts_with(XP_PREFIX) {
                    written_anywhere.insert(name.to_string());
                }
            }

            if let Ok(guard) = lookahead_engine::parser::guard_parser::parse_guard(&entry.guard) {
                let mut mentioned = HashSet::new();
                variables_of(&guard, &mut mentioned);
                for name in mentioned {
                    if name.starts_with(XP_PREFIX) {
                        read_anywhere.insert(name);
                    }
                }
            }
        }
    }

    let both: HashSet<&String> = written_anywhere.intersection(&read_anywhere).collect();
    println!(
        "across the whole database: {} XP flags written, {} read, {} both",
        written_anywhere.len(),
        read_anywhere.len(),
        both.len(),
    );

    let read_only: Vec<&String> = read_anywhere.difference(&written_anywhere).collect();
    println!(
        "  {} read but never written by an XP call - those are somebody else's variables",
        read_only.len(),
    );
    for name in read_only.iter().take(5) {
        println!("      {name}");
    }

    // Now the question that actually decides whether modelling pays: within ONE group,
    // does a search write a flag that a guard in the same group reads?
    println!("\nper group, an XP flag both written by an action and read by a guard:");
    let mut any_group_needs_them = false;

    for start in GROUPS {
        if !index.contains_key(&start) {
            println!("{start:>6}  not in the index");
            continue;
        }

        let Ok((graph, _)) = build_group_graph(&index, start) else { continue };
        let symbols = graph.symbols();

        // Written: an action assigns a slot whose name is an XP flag.
        let mut written: HashSet<String> = HashSet::new();
        let mut read: HashSet<String> = HashSet::new();
        for node in graph.nodes() {
            for action in &node.actions {
                if action.kind() != DialogueActionKind::Assign {
                    continue;
                }
                if let Ok(slot) = usize::try_from(action.slot()) {
                    if let Some(name) = symbols.name_of(slot) {
                        if name.starts_with(XP_PREFIX) {
                            written.insert(name.to_string());
                        }
                    }
                }
            }

            let mut mentioned = HashSet::new();
            variables_of(&node.guard, &mut mentioned);
            for name in mentioned {
                if name.starts_with(XP_PREFIX) {
                    read.insert(name);
                }
            }
        }

        let overlap: Vec<&String> = written.intersection(&read).collect();
        if !overlap.is_empty() {
            any_group_needs_them = true;
        }

        println!(
            "{start:>6}  {} conversations, {} written, {} read, {} BOTH{}",
            discover_group(&index, start).len(),
            written.len(),
            read.len(),
            overlap.len(),
            if overlap.is_empty() { "" } else { " <-- the slot earns its place" },
        );
        for name in overlap.iter().take(6) {
            println!("            {name}");
        }
    }

    println!(
        "\nverdict: {}",
        if any_group_needs_them {
            "at least one group branches on a flag its own search can set - keep modelling them"
        } else {
            "no group reads a flag its own search writes - the slots are dead weight"
        },
    );

    // Deliberately no assertion on the verdict. This is a question about the shipped
    // content, and the answer is allowed to change when the content does; a test that
    // failed on a rewrite of the database would be reporting the wrong thing.
    assert!(!written_anywhere.is_empty(), "no XP flags found at all - the corpus is wrong");
}
