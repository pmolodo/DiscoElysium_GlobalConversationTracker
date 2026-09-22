// SPDX-License-Identifier: MIT
//! A GENERATOR: one greedy playthrough per group, cached as a keypress sequence and a node walk.
//!
//! ## Why it exists
//!
//! The menu measurements ask a group in a state they ASSERT: `menu_profile` calls the
//! structurally deepest entries unseen in any game, and every other entry one an EARLIER
//! playthrough showed. That is coherent - it is a save that has never opened the conversation,
//! so nothing is seen this game and no `once` has fired - but nobody has demonstrated a play
//! that leaves exactly those entries unread.
//!
//! WHAT AN ASSERTION CANNOT DO is say the player read something IN THIS GAME, because
//! `state::seed_state` seeds a node's `once_slot` and `seen_slot` from `world.is_seen` and a
//! `seen:` slot shuts an entry that shuts once seen. Declare most of a conversation read this
//! game and the routes to the rest close: measured on 761, that menu settles in 511 ms STARRING
//! NOTHING, which is the cost of proving an empty menu rather than a faster answer.
//!
//! A WALKED state can say it. What this writes is a state reached by pressing keys, so the
//! entries it calls read this game are ones a play actually displayed, and the keypresses are
//! the witness.
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
//! One JSON object per group, one per line, under `analysis/outputs/playthroughs/` - which git ignores,
//! for the reason `performance/README.md` gives: a table derived from the index goes stale
//! silently the first time a group grows an entry, and a committed one is wrong without saying
//! so. Regenerate it rather than keeping it.
//!
//! Each row carries the legs, and each leg both readings of the same walk:
//!
//! - `inputs`, the keys pressed - `enter` for a waiting line, the option's number for a menu of
//!   several - which is what the in-game harness presses through the probe. NOT something
//!   `walkthrough::walk_inputs` takes back: that function must finish at a menu, and a session
//!   finishes wherever its last leg's target was, which on a greedy walk is characteristically
//!   a terminal line. `sessions_replay` measured it and found none accepted;
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
//! tools/run-logged.sh cargo playthrough -- \
//!   cargo run --release --example greedy_playthrough -- --conversation 761
//! ```
//!
//! With no `--conversation` it walks every group the index holds, which is the point of
//! caching it. `--ceiling` bounds one leg's search in walk positions and `--out` names the
//! folder.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use lookahead_engine::bridge::GameWorld;
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{Index, discover_group, read_index};
use lookahead_engine::walkthrough::{Playthrough, Stage, Stop, roll_escalation};

use gct_measure::common;

use gct_measure::options;

use gct_measure::prepared;
use prepared::Shipped;

use gct_measure::save_world;

/// What one leg's search may hold in walk positions before it gives up.
///
/// Well under `oracle::CEILING`, because this runs a search PER LEG and a group has as many
/// legs as it has entries worth reaching, where the oracle runs one. A group that needs more
/// says so in its row rather than costing the whole run.
const CEILING: usize = 200_000;

/// Where the walks are written when nothing names another folder.
const PLAYTHROUGHS_OUT: &str = "analysis/outputs/playthroughs";

/// What this driver takes.
#[derive(clap::Parser)]
#[command(about = "A greedy playthrough of each group, and what it leaves unshown.")]
struct Options {
    #[command(flatten)]
    groups: options::Groups,
    /// Where to write the walks
    #[arg(long = "out", value_name = "DIR", default_value = PLAYTHROUGHS_OUT)]
    out: PathBuf,
    /// Which save each walk starts from
    #[arg(long, value_name = "NAME", default_value = save_world::TEMPLATE)]
    save: String,
    /// What one leg's search may spend, in walk positions
    #[arg(long, value_name = "N", default_value_t = CEILING)]
    ceiling: usize,
    /// Also replay every session and report on stderr whether it still walks
    #[arg(long = "verify-replay")]
    verify_replay: bool,
    #[command(flatten)]
    caching: prepared::Caching,
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

/// Whether a session's keypresses are ones `walk_inputs` will take back.
///
/// ## What it found, 2026-09-17: NONE OF THEM, AND THE REASON IS THE CONTRACT
///
/// Measured over 229, 16, 631, 761 and 537: not one session of any of them replays, and every
/// failure is the same - "the conversation ends after X, before the inputs reach a menu".
///
/// `walk_inputs` GIVEN INPUTS MUST FINISH EXACTLY AT A MENU. A greedy session finishes wherever
/// its last leg's target was, and a walk that drives at the last entries nobody has been shown
/// characteristically ends on a terminal line - so the function plays past the inputs looking
/// for a menu and reaches the end of the conversation instead. Playing on first does not help,
/// because there is no menu ahead to play on to.
///
/// SO THIS IS NOT A VALIDATOR FOR THESE SEQUENCES, and the module doc no longer claims it is.
/// What the keys are good for is the in-game harness, which presses them and has no such
/// contract; validating them offline would want a walk that may end anywhere, which is a
/// different function from the one scenarios need. See de-iph8.
///
/// A SESSION RATHER THAN A LEG is the other half of the correction: a leg that did not restart
/// begins where the last one stopped, so its inputs mean nothing from the conversation's start.
/// What a keypress sequence corresponds to is the run of legs from the last restart onwards,
/// which is what `Leg::restarted` delimits.
///
/// Returns how many sessions were tried, how many replayed, and why the first failure failed.
fn sessions_replay(
    graph: &LookAheadGraph,
    world: &dyn lookahead_engine::world::ILookAheadWorld,
    conversation: i32,
    done: &Playthrough,
) -> (usize, usize, Option<String>) {
    let mut tried = 0;
    let mut replayed = 0;
    let mut why: Option<String> = None;
    let mut session: Vec<&lookahead_engine::walkthrough::Leg> = Vec::new();

    let check = |session: &[&lookahead_engine::walkthrough::Leg]| {
        if session.is_empty() {
            return (0, 0, None);
        }
        let inputs: Vec<_> = session.iter().flat_map(|leg| leg.inputs()).collect();
        let walked: Vec<DialogueNodeId> =
            session.iter().flat_map(|leg| leg.encountered()).collect();
        match lookahead_engine::walkthrough::walk_inputs(graph, world, conversation, Some(&inputs))
        {
            Ok(again) if again.encountered.starts_with(&walked) => (1, 1, None),
            Ok(_) => (
                1,
                0,
                Some("it replayed, but walked somewhere else".to_string()),
            ),
            Err(why) => (1, 0, Some(why)),
        }
    };

    for leg in &done.legs {
        if leg.restarted && !session.is_empty() {
            let (one, ok, reason) = check(&session);
            tried += one;
            replayed += ok;
            why = why.or(reason);
            session.clear();
        }
        session.push(leg);
    }
    let (one, ok, reason) = check(&session);
    (tried + one, replayed + ok, why.or(reason))
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
    let asked = <Options as clap::Parser>::parse();
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index; nothing to walk.");
        return;
    };
    // THE FULL INDEX, not the shipped one, which is why the walk's worlds are keyed on which
    // index built them - see `save_world::kept_at`.
    let started = Instant::now();
    let index = read_index(&path).expect("the index reads");
    let shipped = Shipped::read(path, index, started.elapsed(), asked.caching);

    let wanted: Vec<i32> = if asked.groups.conversations.is_empty() {
        group_starts(shipped.index())
    } else {
        asked.groups.conversations.clone()
    };

    let folder = asked.out.clone();
    fs::create_dir_all(&folder).expect("the output folder is writable");
    let file = folder.join("playthroughs.jsonl");
    let mut out = fs::File::create(&file).expect("the output file is writable");

    println!("conv\tstage\tconceded\tlegs\tpresses\tshown\tunshown\trefused\tstopped\tms");
    for conversation in wanted {
        let Ok(group) = prepared::group_graph(&shipped, conversation) else {
            continue;
        };
        let graph = group.graph;
        if graph.get(DialogueNodeId::new(conversation, 0)).is_none() {
            continue;
        }
        // THE TEMPLATE UNLESS `--save` NAMES ANOTHER. A walk is
        // only as good as the world it walks, and the template is the fair common denominator
        // rather than a state anyone reached - on 761 it leaves 2,219 of 2,263 entries
        // unreachable. Naming a save taken from a real playthrough asks the same question of a
        // world a player was actually in.
        let save = asked.save.clone();
        let world = GameWorld::declaring(
            save_world::of_save(&graph, conversation, &shipped, &save),
            save_world::declared(),
        );

        let began = Instant::now();
        let stages = roll_escalation(&graph, &world, conversation, asked.ceiling);
        let elapsed = began.elapsed().as_millis();

        // ON STDERR AND ONLY WHERE ASKED FOR, because it re-walks every session and the answer
        // is about the dataset rather than about this group.
        if asked.verify_replay {
            for stage in &stages {
                let (tried, replayed, why) =
                    sessions_replay(&graph, &world, conversation, &stage.walk);
                eprintln!(
                    "conversation {conversation}: replayed {replayed} of {tried} sessions{}",
                    why.map(|text| format!("; {text}")).unwrap_or_default(),
                );
            }
        }

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
