// SPDX-License-Identifier: MIT
//! A GENERATOR: one greedy playthrough per group, cached as a keypress sequence and a node walk.
//!
//! ## Why it exists
//!
//! The menu measurements ask a group in a state they ASSERT: `menu_profile` calls the
//! structurally deepest entries unseen and everything else seen, and `menu_matrix` hands the
//! engine a world whose `seen` set is empty regardless. Those two disagree, and the disagreement
//! is not harmless - `state::seed_state` seeds a node's `once_slot` and `seen_slot` from
//! `world.is_seen`, so a default row asks its menu with almost every line read and every
//! one-time effect still pending, which is a state no save can hold.
//!
//! Making the world agree by asserting the same set does not fix it and is worth knowing about:
//! a `seen:` slot shuts an entry that shuts once seen, so declaring most of a conversation seen
//! closes the routes to the rest. Measured on 761, `DEGCT_SEEN_WORLD=all` took the menu from
//! 2,513 ms unsettled to 511 ms fully settled AND STARRING NOTHING, because its unread content
//! had become unreachable. An asserted state can contradict itself.
//!
//! A WALKED state cannot. What this writes is a state reached by pressing keys, and the
//! keypresses are the witness.
//!
//! ## What a playthrough is
//!
//! [`walkthrough::greedy_playthrough`] holds the rule; in short, always walk to the nearest
//! entry not yet shown, counting presses, continuing from where the last leg stopped and
//! restarting at the conversation's start only where the game forces it. It ends when nothing
//! unshown can be reached from either place.
//!
//! ## What it writes
//!
//! One JSON object per group, one per line, under `analysis/playthroughs/` - which git ignores,
//! for the reason `measurements/README.md` gives: a table derived from the index goes stale
//! silently the first time a group grows an entry, and a committed one is wrong without saying
//! so. Regenerate it rather than keeping it.
//!
//! Each row carries the legs, and each leg both readings of the same walk:
//!
//! - `inputs`, the keys pressed - `enter` for a waiting line, the option's number for a menu of
//!   several - which is what the in-game harness replays through the probe and what the offline
//!   runner replays through `walkthrough::walk_inputs`;
//! - `steps`, one per press, saying where it was pressed, what menu was on screen, and every
//!   entry the game stepped through in answer;
//! - `encountered`, the flat node walk, which is what `LookAheadRequest::encountered` carries.
//!
//! `restarted` marks a leg that began at the conversation's start, so consecutive legs up to
//! the next restart are one sitting and their inputs run together into one keypress sequence.
//!
//! ## How to run it
//!
//! ```text
//! DEGCT_CONVERSATION=761 \
//!   tools/run-logged.sh cargo playthrough -- cargo run --release --example greedy_playthrough
//! ```
//!
//! With no `DEGCT_CONVERSATION` it walks every group the index holds, which is the point of
//! caching it. `DEGCT_CEILING` bounds one leg's search in walk positions and
//! `DEGCT_PLAYTHROUGHS_OUT` names the folder.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use lookahead_engine::bridge::{NodeRef, SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{Index, build_group_graph, discover_group, read_index};
use lookahead_engine::walkthrough::{Playthrough, Stage, Stop, roll_escalation};

#[path = "../tests/common/mod.rs"]
mod common;

/// What one leg's search may hold in walk positions before it gives up.
///
/// Well under `oracle::CEILING`, because this runs a search PER LEG and a group has as many
/// legs as it has entries worth reaching, where the oracle runs one. A group that needs more
/// says so in its row rather than costing the whole run.
const CEILING: usize = 200_000;

/// The save every walk is taken against.
///
/// ## Why one save, and why this one
///
/// A default snapshot cannot walk: `walkthrough` refuses rather than guesses where it cannot
/// decide what the game would show, so a world that answers no skill check, no item test and no
/// variable stops after a step or two. Measured on 537, which walks 0 -> 171 -> 574 -> 482 and
/// then refuses.
///
/// So a walk needs a save. Which save is not a free choice in principle: a conversation can be
/// written to assume variables that only hold at a point in the game, and walking it from a save
/// that could never stand there gives a degenerate walk. Establishing which saves are legitimate
/// for which conversations is a far bigger question than this answers.
///
/// The template is the fair common denominator - the blank slate every committed scenario save
/// is eventually a diff over - so every group is walked from the same place and no group is
/// favoured by a save that happens to suit it. What that costs is stated rather than hidden: a
/// group whose content assumes a later game will not be walked far, and its row says so in
/// `unshown`.
const SAVE: &str = "save_template";

fn out_dir() -> PathBuf {
    lookahead_engine::core::env::var("PLAYTHROUGHS_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("analysis/playthroughs"))
}

fn ceiling() -> usize {
    lookahead_engine::core::env::number("CEILING", CEILING)
}

/// One canonical start per distinct group, smallest first.
///
/// A CONVERSATION IS NOT A GROUP, and walking every conversation from its own entry 0 walks
/// most of them from a dead end. Conversation 824 is the case that showed it: its start links
/// to a group entry with no links, because it is entered only from 537:505, and the eight
/// conversations around it are one group whose walkable start is 537.
///
/// A CANONICAL START IS NOT SIMPLY THE SMALLEST MEMBER, for the reason `menu_matrix::group_list`
/// gives: `discover_group` is the forward closure of a start rather than an equivalence
/// relation, so the smallest conversation in a group may reach only part of it. The start named
/// is the smallest one whose own closure IS the whole set, which is the only kind that
/// reproduces the group it came from.
fn group_starts(index: &Index) -> Vec<i32> {
    let mut conversations: Vec<i32> = index.keys().copied().collect();
    conversations.sort_unstable();

    let mut canonical: HashMap<BTreeSet<i32>, i32> = HashMap::new();
    for &conversation in &conversations {
        let group: BTreeSet<i32> = discover_group(index, conversation).into_iter().collect();
        // Ascending, so the first start to produce a set is the smallest that reaches it.
        canonical.entry(group).or_insert(conversation);
    }

    let mut starts: Vec<i32> = canonical.into_values().collect();
    starts.sort_unstable();
    starts
}

/// The world [`SAVE`] puts this group in, built the way `tests/scenario_suites.rs` builds one.
///
/// EVERY FIELD THAT LETS THE WALK DECIDE SOMETHING, because a field left out is a question the
/// world cannot answer and a position the walk refuses. The `seen` set matters twice over: the
/// engine seeds its `once` and `seen` slots from it, so without it every one-time effect starts
/// unfired and a route the save has already spent is open to the walk.
fn world_of(graph: &LookAheadGraph, conversation: i32, index: &Index) -> WorldSnapshot {
    let group: Vec<i32> = discover_group(index, conversation).into_iter().collect();
    let asked = lookahead_engine::bridge::questions_of(graph, group.clone());
    let holdings = common::fixtures::holdings_in_save(SAVE);
    // LOUDLY, because the quiet failure here is not a stricter world but a different one: a
    // `None` says the actor table or the full index is missing, NOT that the save cannot decide
    // its checks - it computes them from the character sheet. Defaulting it to empty answers
    // "unknown" for every check, and the walk then refuses at the first one. That is what
    // stopped 761 a step past its start, at the Perception (Sight) check on 761:302.
    let checks = common::fixtures::checks_in_save(SAVE, &group)
        .expect("the actor table and the full index are both present");

    let mut snapshot = WorldSnapshot {
        money: holdings.money,
        day_minutes: holdings.day_minutes,
        day_counter: holdings.day_counter,
        // LOCKED, as the plugin sends it, for the reason the scenario suites give: nothing the
        // game exposes to Lua says whether its clock is locked, so a walk that let time pass
        // would be walking a world no run of the game is in.
        clock_locked: true,
        data_values: holdings.data_for(&asked.data),
        items: holdings.items.clone(),
        thoughts: holdings.thoughts.clone(),
        variables: common::fixtures::variables_sent(SAVE, &asked),
        seen: common::fixtures::read_in_save_group(SAVE, &group)
            .into_iter()
            .map(|(conversation, entry)| NodeRef {
                conversation,
                entry,
            })
            .collect(),
        checks_pass: checks.pass,
        checks_fail: checks.fail,
        check_margins: checks.margins,
        red_checks_fail: common::fixtures::passive_thoughts_in_save(SAVE).red_checks_fail,
        ..Default::default()
    };
    // NAMED BY THE REQUESTS THAT ASKED FOR THEM. The data answers go over the wire positionally
    // and a request resolves them on arrival; a world built here has no request to do it, so
    // every query would stay Unknown and the walk would refuse at the first one. That is what
    // stopped 761 at `CheckEquipped("jacket_carabineer")` on 761:87.
    snapshot
        .resolve(&asked)
        .expect("the answers were built from these very questions");
    snapshot
}

/// An entry as `conversation:entry`, which is how every log in this repository names one.
fn named(id: DialogueNodeId) -> String {
    format!("{}:{}", id.conversation_id, id.entry_id)
}

fn names(ids: &[DialogueNodeId]) -> Vec<String> {
    ids.iter().copied().map(named).collect()
}

fn why(stopped: &Stop) -> &'static str {
    match stopped {
        Stop::Exhausted => "exhausted",
        Stop::OutOfRoom => "out-of-room",
        Stop::NoStart => "no-start",
    }
}

/// The entries the playthrough never put on screen, in a deterministic order.
///
/// ONLY MEANINGFUL WHERE IT EXHAUSTED. A run that hit its ceiling did not establish that what
/// is left is out of reach, only that it did not get there.
fn unshown(graph: &LookAheadGraph, done: &Playthrough) -> Vec<DialogueNodeId> {
    let shown: HashSet<DialogueNodeId> = done.shown.iter().copied().collect();
    let mut left: Vec<DialogueNodeId> = graph
        .nodes()
        .filter(|node| !node.is_group && !shown.contains(&node.id))
        .map(|node| node.id)
        .collect();
    left.sort_unstable_by_key(|id| (id.conversation_id, id.entry_id));
    left
}

fn stage_row(graph: &LookAheadGraph, number: usize, stage: &Stage) -> serde_json::Value {
    let mut body = row_of(graph, &stage.walk);
    body["stage"] = serde_json::json!(number);
    body["passing"] = serde_json::json!(names(&stage.passing));
    body["added"] = serde_json::json!(stage.added.map(named));
    body
}

fn row_of(graph: &LookAheadGraph, done: &Playthrough) -> serde_json::Value {
    let legs: Vec<serde_json::Value> = done
        .legs
        .iter()
        .map(|leg| {
            let steps: Vec<serde_json::Value> = leg
                .steps
                .iter()
                .map(|step| {
                    serde_json::json!({
                        "input": step.input.map(|input| input.to_string()),
                        "at": named(step.at),
                        "menu": names(&step.menu),
                        // WHAT A REPLAY MUST ARRANGE, where the step entered a rolled check:
                        // true is a 12, false a 2, absent where no die was involved.
                        "rolled": step.rolled,
                        "encountered": names(&step.encountered),
                        "displayed": names(&step.displayed),
                    })
                })
                .collect();
            serde_json::json!({
                "target": named(leg.target),
                "restarted": leg.restarted,
                "presses": leg.inputs().len(),
                "inputs": leg.inputs().iter().map(ToString::to_string).collect::<Vec<_>>(),
                "encountered": names(&leg.encountered()),
                "steps": steps,
            })
        })
        .collect();

    serde_json::json!({
        "stopped": why(&done.stopped),
        "refused": done.refused,
        "blocked": done
            .blocked
            .iter()
            .map(|(at, why)| serde_json::json!({"at": named(*at), "why": why}))
            .collect::<Vec<_>>(),
        "shown": names(&done.shown),
        "unshown": names(&unshown(graph, done)),
        "legs": legs,
    })
}

fn main() {
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index; nothing to walk.");
        return;
    };
    let index = read_index(&path).expect("the index reads");

    let wanted: Vec<i32> = match lookahead_engine::core::env::var("CONVERSATION") {
        Ok(value) => value
            .split(',')
            .filter_map(|part| part.trim().parse().ok())
            .collect(),
        Err(_) => group_starts(&index),
    };

    let folder = out_dir();
    fs::create_dir_all(&folder).expect("the output folder is writable");
    let file = folder.join("playthroughs.jsonl");
    let mut out = fs::File::create(&file).expect("the output file is writable");

    println!("conv\tstage\tconceded\tlegs\tpresses\tshown\tunshown\trefused\tstopped\tms");
    for conversation in wanted {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        if graph.get(DialogueNodeId::new(conversation, 0)).is_none() {
            continue;
        }
        let world = SnapshotWorld::declaring(world_of(&graph, conversation, &index), None);

        let began = Instant::now();
        let stages = roll_escalation(&graph, &world, conversation, ceiling());
        let elapsed = began.elapsed().as_millis();

        for (number, stage) in stages.iter().enumerate() {
            let done = &stage.walk;
            let presses: usize = done.legs.iter().map(|leg| leg.inputs().len()).sum();
            let left = unshown(&graph, done).len();
            println!(
                "{conversation}\t{number}\t{}\t{}\t{presses}\t{}\t{left}\t{}\t{}\t{}",
                stage.added.map(named).unwrap_or_else(|| "-".to_string()),
                done.legs.len(),
                done.shown.len(),
                done.refused,
                why(&done.stopped),
                // ONCE PER GROUP, on its first row: the schedule is timed whole, since a later
                // stage's cost is only meaningful beside the ones that had to run before it.
                if number == 0 {
                    elapsed.to_string()
                } else {
                    String::new()
                },
            );
            let mut body = stage_row(&graph, number, stage);
            body["conversation"] = serde_json::json!(conversation);
            writeln!(out, "{body}").expect("the row writes");
        }
    }
    eprintln!("written to {}", file.display());
}
