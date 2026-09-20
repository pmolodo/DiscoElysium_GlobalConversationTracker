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
//! ## What is kept, and what each of them is a function of
//!
//! ```text
//! the parsed index        the index file, and the code that parses it
//! a group's graph         the index, and the code that builds a graph from it
//! what a group reaches    the conversations themselves
//! whether it has a menu   the conversations themselves
//! ```
//!
//! THE FIRST TWO ARE THE CODE'S and are keyed on it, so a rebuild throws them away - see
//! `kept::at`. THE OTHER TWO ARE THE DIALOGUE'S: the engine reads the conversations, it does not
//! decide what is in them, so those survive any number of rebuilds and are keyed on the content
//! of the conversations alone - see `kept::at_data`. The dialogue database is stable for months
//! at a time and the engine is rebuilt many times a day, which is what makes that distinction
//! worth drawing.
//!
//! THEY PAY OFF AT DIFFERENT TIMES. The index is shared within a pass - every process reads the
//! same file, so the first to derive it pays and the other five hundred do not. A graph is one
//! group's, and a pass measures a group once, so a kept graph is only ever read by a LATER pass:
//! a before-and-after comparison is four passes at one binary, and three of them read what the
//! first built.
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

pub use kept::Caching;

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
    /// WHAT THIS RUN WAS TOLD ABOUT THE CACHE, carried rather than read where it is wanted.
    /// Everything under this module that consults the cache already has a `Shipped`, so this
    /// is what lets the driver decide it once. See `kept::Caching`.
    caching: kept::Caching,
}

impl Shipped {
    /// An index at `path`, not yet read.
    pub fn at(path: PathBuf, caching: kept::Caching) -> Self {
        Self {
            path,
            read: OnceLock::new(),
            caching,
        }
    }

    /// An index already in hand, for a caller that read one for its own reasons.
    ///
    /// `took` is what that read cost, so a caller which timed it does not lose the number.
    pub fn read(path: PathBuf, index: Index, took: Duration, caching: kept::Caching) -> Self {
        let read = OnceLock::new();
        let _ = read.set((index, took));
        Self {
            path,
            read,
            caching,
        }
    }

    /// The index, read now if this is the first thing to ask for it.
    pub fn index(&self) -> &Index {
        &self
            .read
            .get_or_init(|| {
                let started = Instant::now();
                (read_index_kept(&self.path, self.caching), started.elapsed())
            })
            .0
    }

    /// What this run was told about the cache, for everything that has a `Shipped` in hand.
    pub fn caching(&self) -> kept::Caching {
        self.caching
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
fn read_index_kept(path: &std::path::Path, caching: kept::Caching) -> Index {
    let at = (!caching.no_cache())
        .then(|| kept::at("index", &kept::stamp(path)?))
        .flatten();

    if let Some(at) = at.as_ref()
        && let Some(held) = kept::read_packed::<Index>(at)
    {
        if caching.verifying_reads() {
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

/// A group, as a measurement needs it before it can measure anything.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Prepared {
    pub graph: LookAheadGraph,
    /// The conversations the group is made of, which `discover_group` decided.
    pub conversations: Vec<i32>,
    /// WHAT THE GROUP'S OWN DATA REDUCES TO, and empty where it cannot be established.
    ///
    /// The shipped index carries a content hash per conversation - written by
    /// `ConversationHasher`, which is the one routine that reduces a conversation - so a
    /// group's content is its members' hashes and costs nothing to take.
    ///
    /// WHY A CONTENT HASH AND NOT THE FILE. Everything else here is keyed on the index FILE,
    /// by length and modification time, because that is what can be asked without reading it.
    /// A file stamp changes whenever the index is regenerated, whether or not a single
    /// conversation changed - and the dialogue database is stable for months at a time. A
    /// verdict about a group is worth keeping across those regenerations, so it is keyed on
    /// this instead. See de-ealo.
    ///
    /// EMPTY IS NOT A KEY. The full index carries no hashes, and a group whose content cannot
    /// be established simply has nothing kept about it.
    pub content: String,
}

/// What a group's data reduces to, from the content hashes the index carries.
pub fn content_of(index: &Index, conversations: &[i32]) -> String {
    let mut ordered: Vec<i32> = conversations.to_vec();
    ordered.sort_unstable();
    let mut key = String::new();
    for conversation in ordered {
        let Some(record) = index.get(&conversation) else {
            return String::new();
        };
        if record.hash.is_empty() {
            return String::new();
        }
        key.push_str(&format!("{conversation}:{}\u{1}", record.hash));
    }
    key
}

/// A group's graph and the conversations it is made of, built once and kept.
///
/// The same answer as [`build_group_graph`], which is what this calls when it has to. Errors are
/// the graph builder's own and are never kept: a group that does not build is a fast answer
/// already, and keeping a failure would mean keeping a reason to distrust the cache.
///
/// `--no-cache` builds it, and `--cache-verify` builds it AND checks what was kept
/// against what was built - see `verified`.
pub fn group_graph(shipped: &Shipped, conversation: i32) -> Result<Prepared, String> {
    let at = (!shipped.caching().no_cache())
        .then(|| {
            let stamp = shipped.stamp()?;
            kept::at("graphs", &format!("{conversation}\u{1}{stamp}"))
        })
        .flatten();

    if let Some(path) = at.as_ref()
        && let Some(held) = kept::read_packed::<Prepared>(path)
    {
        return Ok(verified(
            held,
            || build(shipped, conversation),
            shipped.caching(),
        ));
    }

    let built = build(shipped, conversation)?;
    if let Some(path) = at.as_ref() {
        kept::write_packed(path, &built);
    }
    Ok(built)
}

/// A group prepared from the index, which is what `group_graph` answers with when it has to.
///
/// THE CONTENT STAMP IS TAKEN HERE, where the index is already in hand, so that a process which
/// reads a kept graph has the group's content without having to read the index to get it. That
/// is what lets a verdict about the group be keyed on the group - see `Prepared::content`.
fn build(shipped: &Shipped, conversation: i32) -> Result<Prepared, String> {
    let (graph, conversations) = build_group_graph(shipped.index(), conversation)?;
    let content = content_of(shipped.index(), &conversations);
    Ok(Prepared {
        graph,
        conversations,
        content,
    })
}

/// `held`, having checked it against what building it fresh gives - but only where
/// `--cache-verify` asked for that check.
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
    held: Prepared,
    fresh: impl FnOnce() -> Result<Prepared, String>,
    caching: kept::Caching,
) -> Prepared {
    if !caching.verifying_reads() {
        return held;
    }
    let built = fresh().expect("the graph builds, since the kept one did");
    assert_eq!(
        (held.graph.count(), &held.conversations, &held.content),
        (built.graph.count(), &built.conversations, &built.content),
        "a kept graph disagrees with the one this build derives"
    );
    held
}

/// One group, as the list of what can be measured describes it.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Group {
    /// The canonical start: the smallest conversation whose own closure is the whole group.
    pub start: i32,
    /// How many conversations the group is made of.
    pub conversations: usize,
    /// Every entry in those conversations, walkable or not.
    pub entries: usize,
    /// How many entries a profile could be built from. A zero is a group with nothing to
    /// measure, and the commonest kind by far: 901 of the game's 1,422 conversations reach
    /// nothing from their start.
    pub reachable: usize,
    /// How many of those entries offer the player anything at all.
    ///
    /// A MENU IS A SET OF PLAYER OPTIONS, so a group where nothing reachable offers any has no
    /// menu under any walk, whatever else is asked of it. That is a fact about the dialogue -
    /// edge analysis, no world and no settings - and it is why a zero here means the group can
    /// be left out of every run rather than of this one.
    pub menus: usize,
    /// What the group's own data reduces to, which is what anything kept ABOUT the group is
    /// keyed on. See [`Prepared::content`].
    pub content: String,
}

/// Where the list of what can be measured is kept, or `None` where it must not be kept.
///
/// ## Why the list is kept and not just recomputed
///
/// Answering it builds a graph for every conversation in the game - 1,422 of them - to find how
/// much each group reaches, and the driver asks for it once per run. It is the same answer every
/// time for the same dialogue, which is the definition of cacheable, and it is what makes a run
/// ask only about groups worth asking about.
///
/// KEYED ON THE INDEX'S CONTENT AND NOTHING ELSE. What a group reaches from its start, and
/// whether anything it reaches offers the player a choice, are properties of the dialogue: the
/// engine reads them, it does not decide them, and no setting can change them. So the answer
/// outlives any number of rebuilds and any way of asking. It is every conversation's content
/// hash rather than the index FILE, so a regenerated index that says the same thing keeps it.
fn list_at(content: &str, caching: kept::Caching) -> Option<std::path::PathBuf> {
    if caching.no_cache() || content.is_empty() {
        return None;
    }
    kept::at_data("groups", &format!("{DERIVATION}\u{1}{content}"))
}

/// Bump this when what the list MEANS changes, which is the one thing its key cannot notice.
///
/// A KEY ON THE DIALOGUE ALONE IS THE POINT, and it has one cost: a change to how these facts
/// are worked out does not invalidate them, because the dialogue did not change. That is not
/// hypothetical - deciding "offers the player" without expanding group links called thirteen
/// groups menu-less that a measurement had already found menus in, and fixing it left every
/// wrong answer in place. So the meaning carries a number, and changing the meaning means
/// changing the number.
///
/// 1: reaches anything from its start, and anything it reaches offers the player a choice,
///    following group links the way `walkthrough::offer` does.
const DERIVATION: u32 = 1;

/// The list of what can be measured, derived by `build` if it is not already kept.
///
/// THE INDEX IS ONLY READ IF IT HAS TO BE - the key is taken from the index's own content, so
/// this reads it to work out whether it can avoid deriving. That is the one cost a kept list
/// cannot avoid, and it is the packed read rather than the JSON one.
pub fn group_list(shipped: &Shipped, build: impl FnOnce() -> Vec<Group>) -> Vec<Group> {
    let index = shipped.index();
    let content = content_of(index, &index.keys().copied().collect::<Vec<i32>>());
    let at = list_at(&content, shipped.caching());

    if let Some(at) = at.as_ref()
        && let Some(held) = kept::read_packed::<Vec<Group>>(at)
        && !shipped.caching().verifying_reads()
    {
        return held;
    }

    let built = build();
    if let Some(at) = at.as_ref() {
        kept::write_packed(at, &built);
    }
    built
}
