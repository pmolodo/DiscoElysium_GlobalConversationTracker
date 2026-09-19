// SPDX-License-Identifier: MIT
//! The index a measurement reads and the graph it builds from it, each derived once and kept.
//!
//! ## What this is for
//!
//! A process that measures one group spends most of its life getting ready to. Measured
//! whole-game on 2026-09-19, over 521 groups:
//!
//! ```text
//! reading the shipped index   127 s    237 ms a process
//! preparing the group          85 s    155 ms a group, cold; 12 s warm
//! the menus themselves          8 s
//! ```
//!
//! The index read is the largest single thing a run does, it is the same answer in every one of
//! those processes, and no column said so until `index ms`. See de-9z1u.
//!
//! ## Two caches, and they pay off at different times
//!
//! THE INDEX IS SHARED WITHIN A PASS. Every process in a whole-game run reads the same file and
//! gets the same answer, so the first one to derive it pays and the other five hundred do not -
//! within the very pass that filled the cache.
//!
//! A GRAPH IS ONE GROUP'S, and a pass measures a group once, so a kept graph is only ever read
//! by a LATER pass. That is the case worth having: a before-and-after comparison is four passes
//! at one binary, and three of them read what the first built.
//!
//! TOGETHER THEY TAKE THE INDEX OUT OF THE PICTURE, which is the point of reading it lazily: a
//! process whose graph and whose world are both kept never needs the index at all, and `index
//! ms` reports the zero to prove it.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{Index, build_group_graph, read_index};

#[path = "kept.rs"]
mod kept;

pub use kept::{no_cache, verifying};

/// An index that is read when something actually needs it, and not before.
///
/// READ LAZILY BECAUSE THE WHOLE POINT IS NOT TO READ IT. Everything a measurement derives from
/// the index can be kept - see [`group_graph`] and `save_world::of_save` - so a process whose
/// group is already prepared has no use for it, and reading it eagerly would spend the saving
/// before the cache could deliver it.
///
/// IT REMEMBERS WHAT THE READ COST, so a row can report it. A process that never read it
/// reports zero, which is the difference the cache made, said in the same column that showed
/// the cost in the first place.
pub struct Shipped {
    path: PathBuf,
    read: OnceLock<(Index, Duration)>,
}

impl Shipped {
    /// An index at `path`, not yet read.
    pub fn at(path: PathBuf) -> Self {
        Self {
            path,
            read: OnceLock::new(),
        }
    }

    /// An index already in hand, for a caller that read one for its own reasons.
    ///
    /// `took` is what that read cost, so a caller which timed it does not lose the number.
    pub fn read(path: PathBuf, index: Index, took: Duration) -> Self {
        let read = OnceLock::new();
        let _ = read.set((index, took));
        Self { path, read }
    }

    /// The index, read now if this is the first thing to ask for it.
    pub fn index(&self) -> &Index {
        &self
            .read
            .get_or_init(|| {
                let started = Instant::now();
                (read_index_kept(&self.path), started.elapsed())
            })
            .0
    }

    /// What reading it cost, or nothing at all where nothing has needed it.
    pub fn took(&self) -> Duration {
        self.read.get().map_or(Duration::ZERO, |(_, took)| *took)
    }

    /// What says this is the same index file as another run's. See `kept::stamp`.
    pub fn stamp(&self) -> Option<String> {
        kept::stamp(&self.path)
    }
}

/// The index at `path`, packed beside the build output the first time it is parsed.
///
/// WHY PACKING IT IS WORTH A CACHE OF ITS OWN. The file is sixteen megabytes of JSON with one
/// conversation per line, and `serde_json` is what costs: reading it takes about 237 ms in
/// every process, which over a whole-game pass is more than the preparation and the menus put
/// together. The packed form is the same records with nothing left to parse.
///
/// KEYED ON THE FILE AND THE EXECUTABLE, like everything else here - see `kept`. The executable
/// matters as much as the file: what is packed is this build's idea of what an index record
/// holds, and reading it back into another build's idea is exactly the silent staleness the
/// keys exist to prevent.
fn read_index_kept(path: &std::path::Path) -> Index {
    let at = (!no_cache())
        .then(|| kept::at("index", &kept::stamp(path)?))
        .flatten();

    if let Some(at) = at.as_ref()
        && let Some(held) = kept::read_packed::<Index>(at)
    {
        if verifying() {
            let fresh = read_index(path).expect("the index reads");
            assert_eq!(
                held.len(),
                fresh.len(),
                "a kept index disagrees with the one this build reads"
            );
        }
        return held;
    }

    let index = read_index(path).expect("the index reads");
    if let Some(at) = at.as_ref() {
        kept::write_packed(at, &index);
    }
    index
}

/// A group's graph and the conversations it is made of, built once and kept.
///
/// The same answer as [`build_group_graph`], which is what this calls when it has to. Errors are
/// the graph builder's own and are never kept: a group that does not build is a fast answer
/// already, and keeping a failure would mean keeping a reason to distrust the cache.
///
/// `DEGCT_NO_CACHE=1` builds it, and `DEGCT_CACHE_VERIFY=1` builds it AND checks what was kept
/// against what was built - see `verified`.
pub fn group_graph(
    shipped: &Shipped,
    conversation: i32,
) -> Result<(LookAheadGraph, Vec<i32>), String> {
    let at = (!no_cache())
        .then(|| {
            let stamp = shipped.stamp()?;
            kept::at("graphs", &format!("{conversation}\u{1}{stamp}"))
        })
        .flatten();

    if let Some(path) = at.as_ref()
        && let Some(held) = kept::read_packed::<(LookAheadGraph, Vec<i32>)>(path)
    {
        return Ok(verified(held, || {
            build_group_graph(shipped.index(), conversation)
        }));
    }

    let built = build_group_graph(shipped.index(), conversation)?;
    if let Some(path) = at.as_ref() {
        kept::write_packed(path, &built);
    }
    Ok(built)
}

/// `held`, having checked it against what building it fresh gives - but only where
/// `DEGCT_CACHE_VERIFY` asked for that check.
///
/// A CACHE NOTHING VERIFIES IS A CACHE NOBODY SHOULD TRUST, and this one sits underneath every
/// performance number the project produces. The comparison is on what a measurement reads off a
/// graph: how many entries it has and which conversations it covers. That is not the whole
/// value, and it is what would differ if the kept graph were built from another index or by
/// another build of the engine - the two ways it can be stale.
///
/// PANICS RATHER THAN REPORTS, because a run that has found its cache wrong has already
/// measured something nobody can interpret, and going on would file the numbers as if nothing
/// had happened.
fn verified(
    held: (LookAheadGraph, Vec<i32>),
    fresh: impl FnOnce() -> Result<(LookAheadGraph, Vec<i32>), String>,
) -> (LookAheadGraph, Vec<i32>) {
    if !verifying() {
        return held;
    }
    let built = fresh().expect("the graph builds, since the kept one did");
    assert_eq!(
        (held.0.count(), &held.1),
        (built.0.count(), &built.1),
        "a kept graph disagrees with the one this build derives"
    );
    held
}
