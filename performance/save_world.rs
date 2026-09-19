// SPDX-License-Identifier: MIT
//! The world a committed save puts a group in, for the measurements that need one that answers.
//!
//! ## Why a measurement would want this
//!
//! A `WorldSnapshot::default()` answers nothing: no skill check, no item, no variable, no
//! equipment query. That is fine for a measurement whose search only has to be given SOMETHING
//! to chew, and fatal for one that walks, because `walkthrough` refuses rather than guesses
//! wherever it cannot decide what the game would show. Measured on 537, a default world walks
//! 0 -> 171 -> 574 -> 482 and then stops.
//!
//! Shared rather than copied because it is easy to build almost right, and almost right here
//! is silent: every field left out is a question the world answers Unknown, and the only
//! symptom is a walk that stops early while reporting itself finished.
//!
//! ## The two that were built almost right, and cost an afternoon
//!
//! `checks_in_save` RETURNS AN OPTION AND THE NONE IS NOT "the save cannot decide". It computes
//! outcomes from the character sheet; a `None` says the actor table or the full index is
//! missing. Defaulting it to empty answers Unknown for every check, and a walk then refuses at
//! the first one.
//!
//! A SNAPSHOT'S DATA ANSWERS ARRIVE POSITIONALLY and a request resolves them against the
//! questions that asked for them. A world built directly has no request to do that, so without
//! [`WorldSnapshot::resolve`] every query stays Unknown - which stopped 761 one step past its
//! start, at `CheckEquipped("jacket_carabineer")` on 761:87.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use lookahead_engine::bridge::{NodeRef, WorldSnapshot};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{Index, discover_group};

#[path = "../tests/common/mod.rs"]
mod common;

/// What this process has already built, so it is not built twice.
///
/// ## Why there is a cache here at all
///
/// Building a world reads the save four times over - holdings, checks, variables, what it has
/// shown - and the cost does not depend on the group: measured whole-game on 2026-09-18,
/// preparing a group took a median of 198 ms whether it had three entries or 4,724, and
/// preparation was 93 per cent of the run. See de-ealo.
///
/// THE DEFAULT PATH ASKED FOR THE SAME WORLD TWICE per group - once for the walk to stand in,
/// and again afterwards to hold what the walk changed - so half of that was rebuilding
/// something the process had just built.
///
/// ## What the key assumes
///
/// That within one process, one conversation and one save name the same world. They do: a
/// process loads one index and builds one graph per group. The entry count is in the key as a
/// cheap guard against a caller that passes a DIFFERENT graph for the same conversation - a
/// trimmed one, say - since the questions a world answers are taken from the graph.
///
/// It does NOT survive the process, and the measurement driver runs one process per group, so
/// this does nothing for a whole-game run on its own. That is what the disk cache is for.
type Built = HashMap<(i32, String, usize), WorldSnapshot>;

fn memo() -> &'static Mutex<Built> {
    static MEMO: OnceLock<Mutex<Built>> = OnceLock::new();
    MEMO.get_or_init(Default::default)
}

/// The save the walked measurements take their world from.
///
/// THE FAIR COMMON DENOMINATOR: the blank slate every committed scenario save is eventually a
/// diff over, so every group is walked from the same place and none is favoured by a save that
/// happens to suit it. Which saves are legitimate for which conversations is a real question
/// and a much larger one - a conversation can assume variables that only hold at a point in the
/// game - and this does not answer it. What it costs is visible rather than hidden: a group
/// whose content assumes a later game is not walked far, and its row says so.
pub const TEMPLATE: &str = "save_template";

/// The world `save` puts `conversation`'s group in, built the way `tests/scenario_suites.rs`
/// builds one.
///
/// Built once per process per group and kept - see [`Built`]. `DEGCT_NO_CACHE=1` builds it
/// every time, for when an answer is in doubt.
pub fn of_save(
    graph: &LookAheadGraph,
    conversation: i32,
    index: &Index,
    save: &str,
) -> WorldSnapshot {
    let key = (conversation, save.to_string(), graph.count());
    if !no_cache()
        && let Some(held) = memo().lock().expect("the cache is not poisoned").get(&key)
    {
        return held.clone();
    }

    let on_disk = (!no_cache())
        .then(|| kept_at(graph, conversation, save))
        .flatten();
    let built = match on_disk.as_ref().and_then(|path| read_kept(path)) {
        Some(held) => held,
        None => {
            let fresh = build_of_save(graph, conversation, index, save);
            if let Some(path) = on_disk.as_ref() {
                write_kept(path, &fresh);
            }
            fresh
        }
    };

    if !no_cache() {
        memo()
            .lock()
            .expect("the cache is not poisoned")
            .insert(key, built.clone());
    }
    built
}

/// Where this world is kept between processes, or `None` where it cannot safely be kept.
///
/// ## Why the key carries the engine and not only the group
///
/// A kept world is only valid for the code that built it. `questions_of` and `discover_group`
/// are engine code, and when either changes, every world kept before it is wrong - SILENTLY,
/// which is the worst thing a measurement cache can be: a run would report numbers for a world
/// the current code would not build, and nothing would look unusual.
///
/// So the key covers the executable as well as the inputs. It identifies files by their length
/// and modification time rather than their contents, which is what `cargo` itself does for
/// rebuild decisions: hashing the index and the save on every process would cost a good part of
/// what the cache saves.
///
/// Returns `None` if any of that cannot be established, and a world that cannot be keyed is
/// simply built - the cache is an optimisation and is never the reason an answer is missing.
fn kept_at(graph: &LookAheadGraph, conversation: i32, save: &str) -> Option<std::path::PathBuf> {
    let mut key = format!("{save}\u{1}{conversation}\u{1}{}", graph.count());
    for path in [std::env::current_exe().ok()?, common::shipped_index()?] {
        let about = std::fs::metadata(&path).ok()?;
        let when = about
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        key.push('\u{1}');
        key.push_str(&format!("{}:{}", about.len(), when.as_nanos()));
    }
    Some(cache_dir()?.join(format!("{}.json", fingerprint(&key))))
}

/// FNV-1a, so the name of a kept world does not depend on a hasher whose output is allowed to
/// change between Rust releases - and a stale name is a stale world.
fn fingerprint(of: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in of.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The folder kept worlds live in, made if it is not there.
///
/// UNDER THE BUILD OUTPUT, never in the repository: it is derived, it is large, and it is
/// invalidated by the very thing `target/` is invalidated by.
fn cache_dir() -> Option<std::path::PathBuf> {
    let root = match lookahead_engine::core::env::foreign("CARGO_TARGET_DIR") {
        Ok(named) if !named.is_empty() => std::path::PathBuf::from(named),
        _ => std::path::PathBuf::from("target"),
    };
    let dir = root.join("degct-cache").join("worlds");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// A kept world, or `None` for anything at all going wrong.
///
/// A FILE THAT DOES NOT READ IS A FILE TO IGNORE, not one to fail on - it may be half-written
/// by a process still running, or left by a build that no longer exists. The cost of ignoring
/// it is building the world; the cost of trusting it is a wrong measurement.
/// A world as it is kept, which is a world with one map turned inside out.
///
/// ## Why it is not just the world
///
/// `WorldSnapshot::data` is keyed by a `DataRequest`, a struct - and a JSON object's keys must
/// be strings, so `serde_json` refuses the whole world with "key must be a string". The binary
/// formats this crate already has do not help either: `bincode` is not self-describing, and a
/// world holds a `WireValue`, whose deserializer asks what the next value IS. So the map is
/// carried beside the world as pairs, where nothing needs it to be a key.
///
/// JSON is then what is left, and it is enough. A kept world is about twenty kilobytes and
/// reading one back costs a millisecond or so, against the hundred and more that building it
/// costs - see [`Built`].
#[derive(serde::Serialize, serde::Deserialize)]
struct Kept {
    world: WorldSnapshot,
    data: Vec<(
        lookahead_engine::bridge::DataRequest,
        lookahead_engine::bridge::DataAnswer,
    )>,
}

fn read_kept(path: &std::path::Path) -> Option<WorldSnapshot> {
    let held: Kept = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let mut world = held.world;
    world.data = held.data.into_iter().collect();
    Some(world)
}

/// Keeps a world, through a temporary file so that no reader can see a partial one.
///
/// THE DRIVER RUNS GROUPS IN PARALLEL, so two processes can want the same world at the same
/// moment. Writing in place would let one read what the other is still writing; a rename is
/// atomic enough that a reader sees the whole file or no file. Two writers racing both produce
/// the same bytes, so whichever lands last is right.
fn write_kept(path: &std::path::Path, world: &WorldSnapshot) {
    let mut without = world.clone();
    let data = std::mem::take(&mut without.data).into_iter().collect();
    let Ok(rendered) = serde_json::to_vec(&Kept {
        world: without,
        data,
    }) else {
        return;
    };
    let mine = path.with_extension(format!("{}.part", std::process::id()));
    if std::fs::write(&mine, rendered).is_ok() && std::fs::rename(&mine, path).is_err() {
        let _ = std::fs::remove_file(&mine);
    }
}

/// Whether `DEGCT_NO_CACHE` says to build everything rather than trusting what was kept.
///
/// A CACHE UNDERNEATH A MEASUREMENT HAS TO HAVE A WAY OFF. Every performance number this
/// repository produces is taken against a world built here, and a cache that cannot be
/// disabled is one whose correctness can only be argued about.
fn no_cache() -> bool {
    lookahead_engine::core::env::var("NO_CACHE").as_deref() == Ok("1")
}

fn build_of_save(
    graph: &LookAheadGraph,
    conversation: i32,
    index: &Index,
    save: &str,
) -> WorldSnapshot {
    let group: Vec<i32> = discover_group(index, conversation).into_iter().collect();
    let asked = lookahead_engine::bridge::questions_of(graph, group.clone());
    let holdings = common::fixtures::holdings_in_save(save);
    let checks = common::fixtures::checks_in_save(save, &group)
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
        variables: common::fixtures::variables_sent(save, &asked),
        // WHAT THIS SAVE HAS ALREADY SHOWN, which the engine seeds its `once` and `seen` slots
        // from: without it every one-time effect starts unfired and a route the save has
        // already spent is open to the walk.
        seen: common::fixtures::read_in_save_group(save, &group)
            .into_iter()
            .map(|(conversation, entry)| NodeRef {
                conversation,
                entry,
            })
            .collect(),
        checks_pass: checks.pass,
        checks_fail: checks.fail,
        check_margins: checks.margins,
        red_checks_fail: common::fixtures::passive_thoughts_in_save(save).red_checks_fail,
        ..Default::default()
    };
    snapshot
        .resolve(&asked)
        .expect("the answers were built from these very questions");
    snapshot
}

/// What the database declares its variables to be, for the world to fall back on.
///
/// WHY A WORLD BUILT FROM A SAVE STILL WANTS IT. The save answers the variables it holds,
/// and it holds nearly all of them - 10,653 against the 10,645 the database declares - so
/// this changes almost no answer. What it changes is the handful it cannot answer, where
/// the difference is between a value and a shrug. It is also the only way to tell a name
/// the save merely lacks from a name NOTHING declares: without the table every unanswered
/// variable looks alike, and `SnapshotWorld::get_variable` will not call one undeclared on
/// that evidence.
pub fn declared() -> Option<std::sync::Arc<lookahead_engine::index::VariableTable>> {
    common::variable_table()
}
