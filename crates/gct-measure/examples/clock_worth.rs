// SPDX-License-Identifier: MIT
//! What a carried clock BUYS, as against what it costs.
//!
//! Two things bound it sharply. A world's clock is read to the hour - the plugin asks
//! `HourCount`, which is as fine as Lua gets, and the fixtures round to match - so a walk
//! starts at `:00`. And a `PassTime` is fifteen minutes. So FOUR steps on one path are needed
//! before the hour changes at all, and a group that cannot chain four buys nothing however
//! many hour questions it asks.
//!
//! For each group that carries a clock this reports how far a walk can get, and for how many
//! of the twenty-four starting hours some question in it answers differently at a reachable
//! step than it does at the start.

use std::collections::{BTreeMap, BTreeSet};

use lookahead_engine::core::clock::ClockTime;
use lookahead_engine::core::guard::{GuardExpression, GuardRef};
use lookahead_engine::core::guard_value::{GuardValue, GuardValueKind};
use lookahead_engine::index::{build_group_graph, discover_group, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::world::GameWorld;

use gct_measure::common;

/// How many steps it takes to cross an hour from `:00`.
const STEPS_PER_HOUR: i32 = 60 / ClockTime::PASS_TIME_MINUTES;

/// Every hour question the group asks, keyed by how it reads, with its arguments AS VALUES.
///
/// The values are carried rather than rebuilt from their text: a number literal's `text` is
/// empty, so a question reconstructed that way reaches `ClockTime::answer` with arguments it
/// cannot use, is answered Unknown, and quietly looks like a question whose answer never
/// changes. That is the whole result inverted, and it is what the first draft of this did.
fn questions(
    graph: &lookahead_engine::graph::LookAheadGraph,
) -> BTreeMap<String, (String, Vec<GuardValue>)> {
    fn literals(part: GuardRef<'_>) -> Option<Vec<GuardValue>> {
        let GuardExpression::Call(_, args) = part.expression() else {
            return None;
        };
        args.iter()
            .map(|arg| match arg.expression() {
                GuardExpression::Literal(value) => Some(value.clone()),
                _ => None,
            })
            .collect()
    }

    let mut found = BTreeMap::new();
    for node in graph.nodes() {
        for part in node.guard.nodes() {
            if let GuardExpression::Call(name, _) = part.expression()
                && ClockTime::owns(name)
                && let Some(args) = literals(part)
            {
                let shown: Vec<String> = args
                    .iter()
                    .map(|value| match value.kind() {
                        GuardValueKind::Number => format!("{}", value.number()),
                        GuardValueKind::Boolean => format!("{}", value.boolean()),
                        _ => value.text().to_string(),
                    })
                    .collect();
                let label = if shown.is_empty() {
                    name.to_string()
                } else {
                    format!("{name}({})", shown.join(","))
                };
                found.insert(label, (name.to_string(), args));
            }
        }
    }
    found
}

/// What a question answers at a minute of the day, as a comparable string.
///
/// `None` where the model cannot answer it - a malformed call in the database, which is
/// de-m11s.6 rather than anything about the clock.
fn answered(name: &str, args: &[GuardValue], minute: i32, day: i32) -> Option<String> {
    let answer = ClockTime::answer(name, args, minute, day);
    (answer.kind() != GuardValueKind::Unknown).then(|| format!("{answer:?}"))
}

fn main() {
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index, and nothing can build one here");
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let unlocked = GameWorld::blank().with_clock_locked(false);

    let mut groups: BTreeMap<BTreeSet<i32>, i32> = BTreeMap::new();
    for conversation in index.keys() {
        let reach: BTreeSet<i32> = discover_group(&index, *conversation).into_iter().collect();
        groups
            .entry(reach)
            .and_modify(|named| *named = (*named).min(*conversation))
            .or_insert(*conversation);
    }

    println!(
        "{:>7} {:>7} {:>9} {:>6} {:>13}  {}",
        "group", "steps", "questions", "hours", "worth it at", "what it asks"
    );

    let mut carriers = 0;
    let mut ever = 0;
    for start in groups.values() {
        let Ok((graph, _)) = build_group_graph(&index, *start) else {
            continue;
        };
        if !DataLayout::clock_can_move(&graph, &unlocked) {
            continue;
        }
        carriers += 1;

        let steps = match graph.minutes_passable() {
            Some(minutes) => minutes / ClockTime::PASS_TIME_MINUTES,
            None => ClockTime::MINUTES_IN_DAY / ClockTime::PASS_TIME_MINUTES - 1,
        };
        let asked = questions(&graph);

        // The starting hours at which SOME question answers differently once the walk has
        // taken a reachable number of steps.
        let mut moved: Vec<i32> = Vec::new();
        for hour in 0..ClockTime::HOURS_IN_DAY {
            let from = hour * 60;
            let differs = asked.values().any(|(name, args)| {
                let at_start = answered(name, args, from, 1);
                at_start.is_some()
                    && (1..=steps).any(|step| {
                        let minute = (from + step * ClockTime::PASS_TIME_MINUTES)
                            % ClockTime::MINUTES_IN_DAY;
                        answered(name, args, minute, 1) != at_start
                    })
            });
            if differs {
                moved.push(hour);
            }
        }
        if !moved.is_empty() {
            ever += 1;
        }

        let names: Vec<String> = asked.keys().cloned().collect();
        println!(
            "{start:>7} {steps:>7} {:>9} {:>6} {:>13}  {}",
            asked.len(),
            moved.len(),
            if moved.is_empty() {
                "never".to_string()
            } else {
                format!("{:?}", &moved[..moved.len().min(4)])
            },
            names.join(", "),
        );
    }

    println!();
    println!("groups carrying a clock:                      {carriers}");
    println!("of those, groups where it can change an answer:{ever:>3}");
    println!(
        "a walk needs {STEPS_PER_HOUR} steps to cross an hour from :00, which is where a save \
         always starts"
    );
}
