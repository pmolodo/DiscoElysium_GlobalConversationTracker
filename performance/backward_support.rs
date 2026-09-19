// SPDX-License-Identifier: MIT
//! What do the backward sets that blow up actually constrain?
//!
//! de-sze.14.4 found the backward pass cheap on conversations 631 and 28 and expensive on
//! 368 and 1030, and that the difference is diagram size - 363k nodes at the worst target
//! against 9k. It said nothing about WHAT those nodes are about, and de-sze.14.6 proposes
//! a dominator-region analysis on the assumption that they are loop-local variables.
//!
//! That assumption is a guess. This checks it.
//!
//! ## How a slot is known to matter
//!
//! Forget it and see whether the set changes: quantify the slot's variables away and
//! compare. Decision diagrams are canonical for a fixed variable order, so two sets are
//! equal exactly when they are the same node, and "the set does not depend on this slot"
//! is a structural fact rather than a sampled one.
//!
//! ## What the shape of the answer decides
//!
//! Ordinary variables clustered in one region would justify de-sze.14.6 as written. A
//! wash of `seen:` and `once:` markers would not - those are per-entry booleans and the
//! fix is `DataLayout::without_visit_flags`, which exists and was measured on the FORWARD
//! pass and found not to pay. A few wide counters would point at the counter cap. No
//! concentration at all would mean the size is real content, and the answer is the budget
//! and the fallback that de-sze.14.7 already built.

use std::collections::HashMap;

use lookahead_engine::core::state::{ITEM_PREFIX, ONCE_PREFIX, SEEN_PREFIX, THOUGHT_PREFIX};
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::backward::Backward;
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated::on_its_own_thread;
use lookahead_engine::symbolic::vars::DataVars;
use oxidd::{BooleanFunctionQuant, Function};

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "options.rs"]
mod options;

const COUNTER_CAP: i32 = 16;

const EXPENSIVE: [i32; 5] = [368, 631, 14, 28, 1030];

/// How many of the group's own entries to try before settling on the worst.
const PROBES: usize = 24;

/// What this driver takes. With no group named it uses the list below.
#[derive(clap::Parser)]
#[command(about = "What a backward pass carries, group by group.")]
struct Options {
    #[command(flatten)]
    groups: options::Groups,
}

/// What kind of thing a slot is, by the prefix the symbol table gave it.
fn kind_of(name: &str) -> &'static str {
    if name.starts_with(SEEN_PREFIX) {
        "seen:"
    } else if name.starts_with(ONCE_PREFIX) {
        "once:"
    } else if name.starts_with(ITEM_PREFIX) {
        "item:"
    } else if name.starts_with(THOUGHT_PREFIX) {
        "thought:"
    } else if name.contains("check") || name.ends_with("_failed") {
        "check flag"
    } else {
        "variable"
    }
}

/// Entries by how far they are from the start, following links.
fn depths(graph: &LookAheadGraph, start: DialogueNodeId) -> HashMap<DialogueNodeId, usize> {
    let mut depth = HashMap::from([(start, 0usize)]);
    let mut queue = std::collections::VecDeque::from([start]);
    while let Some(id) = queue.pop_front() {
        let here = depth[&id];
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if graph.get(child).is_some() && !depth.contains_key(&child) {
                depth.insert(child, here + 1);
                queue.push_back(child);
            }
        }
    }
    depth
}

fn main() {
    let asked = <Options as clap::Parser>::parse();
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    for conversation in asked.groups.or(&EXPENSIVE) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
        let symbols = graph.symbols().clone();
        // A spread across the depth range, which is what de-sze.14.4 timed. The DEEPEST
        // entries are not the expensive ones - probing those on conversation 631 found a
        // worst set of five diagram nodes, against a median of 3,942 over the spread - so
        // "hardest question" and "furthest away" are not the same thing here.
        let mut ordered: Vec<(usize, DialogueNodeId)> = depths(&graph, start)
            .into_iter()
            .map(|(id, d)| (d, id))
            .collect();
        ordered.sort_by_key(|(depth, id)| (*depth, id.conversation_id, id.entry_id));
        let step = (ordered.len() / PROBES).max(1);

        // A THREAD FOR THE PROBES, with the manager built inside it - de-fpax. The slot
        // numbers that come back are plain data; the sets they were read off are not, and
        // do not leave.
        let worst = on_its_own_thread(|| {
            let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
            let mut compiler = GuardCompiler::new(&vars)
                .with_world(&world)
                .with_constant_clock(DataLayout::group_passes_time(&graph));

            let mut worst: Option<(usize, DialogueNodeId, Vec<usize>)> = None;
            for (_, target) in ordered.iter().step_by(step) {
                let backward =
                    Backward::reaching(&graph, *target, &mut compiler, &world, COUNTER_CAP as u32);
                let size = backward.stats().largest_set;
                if worst.as_ref().is_none_or(|(biggest, _, _)| size > *biggest) {
                    // The one entry holding the biggest set, and the slots it constrains.
                    let mut widest: Option<(usize, Vec<usize>)> = None;
                    for id in backward.entries().collect::<Vec<_>>() {
                        let Some(set) = backward.states_at(id) else {
                            continue;
                        };
                        if set.node_count() < size {
                            continue;
                        }

                        let mut constrained = Vec::new();
                        for slot in 0..layout.slot_count() {
                            let Some(cube) = vars.slot_cube(slot) else {
                                continue;
                            };
                            // Forgetting a slot the set does not depend on changes nothing,
                            // and diagrams are canonical, so this is exact.
                            if set.exists(&cube).is_ok_and(|forgotten| forgotten != *set) {
                                constrained.push(slot);
                            }
                        }
                        widest = Some((set.node_count(), constrained));
                        break;
                    }

                    if let Some((_, constrained)) = widest {
                        worst = Some((size, *target, constrained));
                    }
                }
            }

            worst
        });

        let Some((size, target, constrained)) = worst else {
            println!("{conversation:>6}  nothing measurable");
            continue;
        };

        let mut by_kind: HashMap<&'static str, usize> = HashMap::new();
        for slot in &constrained {
            let name = symbols.name_of(*slot).unwrap_or("");
            *by_kind.entry(kind_of(name)).or_default() += 1;
        }
        let mut rows: Vec<(&&str, &usize)> = by_kind.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));

        println!(
            "\n=== {conversation}: worst target {target}, largest set {size} diagram nodes ===\n\
             {} of {} slots constrained: {}",
            constrained.len(),
            layout.slot_count(),
            rows.iter()
                .map(|(kind, count)| format!("{kind} x{count}"))
                .collect::<Vec<_>>()
                .join(", "),
        );

        // The wide slots among them, since a counter costs more than a boolean.
        let wide: Vec<String> = constrained
            .iter()
            .filter(|slot| layout.slot(**slot).is_some_and(|(_, bits)| bits > 1))
            .filter_map(|slot| symbols.name_of(*slot).map(|n| n.to_string()))
            .take(10)
            .collect();
        println!("counters among them: {wide:?}");
    }
}
