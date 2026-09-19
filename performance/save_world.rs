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
use lookahead_engine::index::discover_group;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "kept.rs"]
mod kept;

// THE INCLUDER'S `prepared`, not a copy of it. A module brought in by path is a module of the
// example that included it, so two copies of this one would be two distinct `Shipped` types and
// the world a measurement asks for could not be asked with the one it holds. Every example that
// includes this file declares `mod prepared` beside it.
use crate::prepared::Shipped;

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
    shipped: &Shipped,
    save: &str,
) -> WorldSnapshot {
    let key = (conversation, save.to_string(), graph.count());
    // NOT WHILE VERIFYING, because the memo is what the disk cache would otherwise never be
    // asked past: a run that asked for the same world twice would check the first answer and
    // hand back the second unchecked. A verifying run pays for every world it is given.
    if !kept::no_cache()
        && !kept::verifying()
        && let Some(held) = memo().lock().expect("the cache is not poisoned").get(&key)
    {
        return held.clone();
    }

    let on_disk = (!kept::no_cache())
        .then(|| kept_at(graph, conversation, shipped, save))
        .flatten();
    let built = match on_disk.as_ref().and_then(|path| read_kept(path)) {
        Some(held) => verified(held, || build_of_save(graph, conversation, shipped, save)),
        None => {
            let fresh = build_of_save(graph, conversation, shipped, save);
            if let Some(path) = on_disk.as_ref() {
                write_kept(path, &fresh);
            }
            fresh
        }
    };

    if !kept::no_cache() {
        memo()
            .lock()
            .expect("the cache is not poisoned")
            .insert(key, built.clone());
    }
    built
}

/// Where this world is kept between processes, or `None` where it cannot safely be kept.
///
/// THE INDEX IT WAS BUILT FROM IS PART OF THE KEY, not just the group: there are two indexes in
/// this repository - the shipped one and the full one - and a world built from one answers
/// differently from a world built from the other. `kept::at` adds the executable, which is the
/// other thing every kept value depends on.
///
/// THE SAVE IS KEYED BY NAME, since a save the game wrote is never edited - that is a rule of
/// this repository rather than an assumption about this cache - and `DEGCT_NO_CACHE=1` is the
/// way out if one ever is.
fn kept_at(
    graph: &LookAheadGraph,
    conversation: i32,
    shipped: &Shipped,
    save: &str,
) -> Option<std::path::PathBuf> {
    kept::at(
        "worlds",
        &format!(
            "{save}\u{1}{conversation}\u{1}{}\u{1}{}",
            graph.count(),
            shipped.stamp()?
        ),
    )
}

/// `held`, having checked it against a freshly built world - but only where
/// `DEGCT_CACHE_VERIFY` asked for that check. See `kept::verifying`.
///
/// ON THE ANSWERS, which is what a measurement reads a world for: the variables it holds, what
/// it calls seen, and which checks pass. A world stale for either reason the key guards against
/// - another index, another build of the engine - differs in exactly those.
fn verified(held: WorldSnapshot, fresh: impl FnOnce() -> WorldSnapshot) -> WorldSnapshot {
    if !kept::verifying() {
        return held;
    }
    let built = fresh();
    assert_eq!(
        answers(&held),
        answers(&built),
        "a kept world disagrees with the one this build derives"
    );
    held
}

/// What a world answers, rendered so that two of them can be compared.
///
/// THROUGH SERDE rather than field by field, because the fields are sets and maps that do not
/// compare: `NodeSet` has no equality of its own, and a `HashMap`'s rendering depends on its
/// iteration order - so the variables go through a `BTreeMap` first and the sets through their
/// own serializer, which writes them as runs in conversation order.
fn answers(world: &WorldSnapshot) -> String {
    let variables: std::collections::BTreeMap<_, _> = world.variables.iter().collect();
    serde_json::to_string(&(
        &variables,
        &world.seen,
        &world.checks_pass,
        &world.checks_fail,
    ))
    .expect("a world's answers render")
}

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

/// A kept world, or `None` for anything at all going wrong - see `kept`, which says why a file
/// that does not read back is ignored rather than failed on.
fn read_kept(path: &std::path::Path) -> Option<WorldSnapshot> {
    let held: Kept = kept::read_json(path)?;
    let mut world = held.world;
    world.data = held.data.into_iter().collect();
    Some(world)
}

/// Keeps a world, with its data map carried beside it - see [`Kept`].
fn write_kept(path: &std::path::Path, world: &WorldSnapshot) {
    let mut without = world.clone();
    let data = std::mem::take(&mut without.data).into_iter().collect();
    kept::write_json(
        path,
        &Kept {
            world: without,
            data,
        },
    );
}

fn build_of_save(
    graph: &LookAheadGraph,
    conversation: i32,
    shipped: &Shipped,
    save: &str,
) -> WorldSnapshot {
    let group: Vec<i32> = discover_group(shipped.index(), conversation)
        .into_iter()
        .collect();
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
