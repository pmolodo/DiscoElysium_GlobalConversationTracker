// SPDX-License-Identifier: MIT
//! Does a crawl's state set compress?
//!
//! The question de-sze rests on. Symbolic reachability is worth building only if the
//! states a crawl reaches share enough structure for a decision diagram to hold them in
//! far fewer nodes than there are states. This measures exactly that, on real content,
//! without building any transition relation: run the explicit crawl, collect the states
//! it visits, union them into one diagram, and compare.
//!
//! The extracted game data this needs is not committed. It is REGENERATED automatically
//! when missing - see `tests/common` - rather than skipped, because a test that passes
//! without its data still reads as green and hides whatever it was meant to catch. The
//! only case that still skips is a machine with no game install at all, where nothing can
//! build it, and that says so loudly.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lookahead_engine::core::state::LookAheadState;
use lookahead_engine::core::types::{DialogueNodeId, LookAheadLimit, Novelty};
use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions};
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::{Profile, StateEncoding, StateSet};
use lookahead_engine::world::test_world::TestWorld;

/// The conversations the C# all-seen suite opens, biggest first by group size.
mod common;

const BIGGEST: [i32; 5] = [368, 631, 14, 28, 1030];

/// Enough states to see the shape without waiting all day. Building the diagram costs a
/// diagram operation per variable per state, so this is the cost driver, not the crawl.
///
/// Held at ten thousand rather than twenty for reliability, not for the result: releasing
/// a diagram walks it recursively, and the twenty-thousand-state diagrams for
/// conversations 631 and 14 overflow even a half-gigabyte stack on teardown. The ratios
/// at twenty thousand were 0.99x and 0.65x against 1.63x and 5.65x for the smaller
/// groups - the same ordering this produces, so nothing about the conclusion rests on the
/// larger sample.
const SAMPLE_LIMIT: usize = 10_000;

/// Manager capacities, one pair per measurement, each the value that measurement was
/// actually observed to survive at.
///
/// Not one shared number, because the failure is not monotonic in it: the 20,000-state
/// prefixes need the larger table and overflow the stack with the smaller one, while the
/// complete sets are the other way round and overflow during teardown with the larger.
/// A manager is torn down by walking what it holds, and how deep that walk goes depends
/// on the table as well as on the diagram. Sized by measurement rather than guessed.
const PREFIX_NODE_CAPACITY: usize = 1 << 22;
const PREFIX_CACHE_CAPACITY: usize = 1 << 20;
const COMPLETE_NODE_CAPACITY: usize = 1 << 18;
const COMPLETE_CACHE_CAPACITY: usize = 1 << 16;

/// The conversation index, regenerating it if it is not there.
fn index_path() -> Option<PathBuf> {
    common::conversation_index()
}

/// Collects the states one crawl visits, up to a limit.
fn sample_states(
    conversation_id: i32,
    index: &lookahead_engine::index::Index,
) -> Option<(Vec<(DialogueNodeId, LookAheadState)>, usize)> {
    let (graph, _) = build_group_graph(index, conversation_id).ok()?;

    // The option to crawl from: the first non-group entry with links. Any option in the
    // group would do - this measures the shape of a state set, not a particular answer.
    let start = graph.nodes()
        .filter(|n| !n.is_group && !n.links.is_empty())
        .map(|n| n.id)
        .min_by_key(|id| (id.conversation_id, id.entry_id))?;

    // The engine's sampling hook is `Fn + Send + Sync + 'static` and the options own it,
    // so the states come back through a shared handle rather than a captured local.
    let sink: Arc<Mutex<Vec<(DialogueNodeId, LookAheadState)>>> = Arc::default();
    let writer = Arc::clone(&sink);
    let engine = LookAheadEngine::new(
        LookAheadOptions {
            state_budget: SAMPLE_LIMIT,
            time_budget: Duration::from_secs(60),
            // Every state, not a sample of them: the set is the subject.
            state_sample_interval: 1,
            ..Default::default()
        }
        .on_state_reached(move |node, state, _| {
            writer.lock().unwrap().push((node, state.clone()));
        }),
    );

    // Novelty that never ends the search early, so the crawl explores the space instead
    // of returning on its first find: every entry reads as already seen.
    let result = engine.evaluate(&graph, start, &TestWorld::new(), |_| Novelty::SeenThisGame);
    let states = std::mem::take(&mut *sink.lock().unwrap());
    Some((states, result.states_explored))
}

/// Releasing a diagram walks it, so a set of this size needs far more stack than a test
/// thread is given. Run on a thread with an explicit one rather than leaving the
/// measurement to depend on RUST_MIN_STACK being set from outside.
const MEASUREMENT_STACK: usize = 2 << 30;

/// Both measurements, on one thread, one after the other.
///
/// Deliberately a single test. Cargo runs tests in parallel, and two of these at once -
/// each holding a diagram manager and a half-gigabyte stack - run the process out of
/// stack during teardown, after the numbers have been printed. Sequential is also how
/// they want to be read: the second exists to qualify the first.
#[test]
fn how_well_a_crawls_state_set_compresses() {
    std::thread::Builder::new()
        .stack_size(MEASUREMENT_STACK)
        .spawn(|| {
            measure();
            measure_complete();
        })
        .expect("spawning the measurement thread")
        .join()
        .expect("the measurement thread");
}

fn measure() {
    let Some(path) = index_path() else { return };
    let index = read_index(&path).expect("the index reads");

    println!(
        "{:>6} {:>8} {:>7} {:>6} {:>7} {:>9} {:>8}",
        "conv", "states", "entries", "vars", "bddnodes", "ratio", "ms"
    );

    let mut measured = 0;
    for conversation_id in BIGGEST {
        let Some((states, explored)) = sample_states(conversation_id, &index) else {
            continue;
        };
        if states.is_empty() {
            println!("{conversation_id:>6}  no states");
            continue;
        }

        let mut profile = Profile::new();
        for (node, state) in &states {
            profile.observe(*node, state);
        }

        let encoding = StateEncoding::for_profile(&profile);
        let started = Instant::now();
        let mut set = StateSet::new(encoding.total_vars(), PREFIX_NODE_CAPACITY, PREFIX_CACHE_CAPACITY);
        let mut distinct = HashMap::new();
        for (node, state) in &states {
            let bits = encoding.encode(*node, state).expect("a profiled state encodes");
            distinct.insert(bits.clone(), ());
            set.insert(&bits);
        }
        let elapsed = started.elapsed();
        let nodes = set.node_count();

        // Every state that went in must still be there, or the ratio is measuring a
        // diagram that lost some of them. Checked before the set is dropped.
        for (node, state) in states.iter().take(200) {
            let bits = encoding.encode(*node, state).unwrap();
            assert!(set.contains(&bits), "state on {node} went missing from the set");
        }

        // One diagram at a time: two live managers over sets this size overflow the
        // stack when the first is dropped, because releasing a diagram walks it.
        drop(set);

        // The same states under a reversed variable numbering. If the set has structure
        // an order can exploit, two orders this different should disagree markedly; if
        // they agree, the set is close to an arbitrary subset and no order will save it.
        let flipped = StateEncoding::for_profile(&profile).reversed();
        let mut flipped_set =
            StateSet::new(flipped.total_vars(), PREFIX_NODE_CAPACITY, PREFIX_CACHE_CAPACITY);
        for (node, state) in &states {
            flipped_set.insert(&flipped.encode(*node, state).expect("a profiled state encodes"));
        }
        let flipped_nodes = flipped_set.node_count();
        drop(flipped_set);
        let ratio = distinct.len() as f64 / nodes.max(1) as f64;
        println!(
            "{:>6} {:>8} {:>7} {:>6} {:>7} {:>9.2} {:>8}  reversed {} ({:.2}x)",
            conversation_id,
            distinct.len(),
            profile.distinct_nodes(),
            encoding.total_vars(),
            nodes,
            ratio,
            elapsed.as_millis(),
            flipped_nodes,
            distinct.len() as f64 / flipped_nodes.max(1) as f64
        );
        println!(
            "       crawl explored {explored}, moving slots {}, money moves {}, clock moves {}",
            profile.moving_slots().len(),
            profile.money_moves(),
            profile.clock_moves()
        );

        measured += 1;
    }

    assert!(measured > 0, "no conversation yielded states, so nothing was measured");
}

/// Crawls one option to exhaustion, if it can be done inside `budget`.
///
/// Returns the states only when the search finished of its own accord. A run stopped by
/// a limit is a prefix, which is the thing this is trying not to measure.
fn complete_states(
    conversation_id: i32,
    index: &lookahead_engine::index::Index,
    budget: usize,
) -> Option<Vec<(DialogueNodeId, LookAheadState)>> {
    let (graph, _) = build_group_graph(index, conversation_id).ok()?;
    let start = graph.nodes()
        .filter(|n| !n.is_group && !n.links.is_empty())
        .map(|n| n.id)
        .min_by_key(|id| (id.conversation_id, id.entry_id))?;

    let sink: Arc<Mutex<Vec<(DialogueNodeId, LookAheadState)>>> = Arc::default();
    let writer = Arc::clone(&sink);
    let engine = LookAheadEngine::new(
        LookAheadOptions {
            state_budget: budget,
            time_budget: Duration::from_secs(30),
            state_sample_interval: 1,
            ..Default::default()
        }
        .on_state_reached(move |node, state, _| {
            writer.lock().unwrap().push((node, state.clone()));
        }),
    );

    let result = engine.evaluate(&graph, start, &TestWorld::new(), |_| Novelty::SeenThisGame);
    if result.stopped_by != LookAheadLimit::None {
        return None;
    }

    Some(std::mem::take(&mut *sink.lock().unwrap()))
}

/// The ratio of distinct states to diagram nodes for a sample.
fn ratio_of(states: &[(DialogueNodeId, LookAheadState)]) -> (usize, usize) {
    let mut profile = Profile::new();
    for (node, state) in states {
        profile.observe(*node, state);
    }

    let encoding = StateEncoding::for_profile(&profile);
    let mut set =
        StateSet::new(encoding.total_vars(), COMPLETE_NODE_CAPACITY, COMPLETE_CACHE_CAPACITY);
    let mut distinct = HashMap::new();
    for (node, state) in states {
        let bits = encoding.encode(*node, state).expect("a profiled state encodes");
        distinct.insert(bits.clone(), ());
        set.insert(&bits);
    }

    let nodes = set.node_count();
    drop(set);
    (distinct.len(), nodes)
}

/// Does a COMPLETE reachable set compress better than a prefix of one?
///
/// The caveat that decides whether the negative result above is real. Decision diagrams
/// often do markedly better on closed sets than on arbitrary prefixes, so measuring only
/// the first 20,000 states of a breadth-first walk could be pessimistic. This measures
/// conversations small enough to explore exhaustively, and compares each complete set
/// against its own first half.
fn measure_complete() {
    let Some(path) = index_path() else { return };
    let index = read_index(&path).expect("the index reads");

    // Smallest first, so the exhaustible ones come up early.
    let mut candidates: Vec<(usize, i32)> = index
        .values()
        .filter(|c| c.entries.len() >= 20)
        .map(|c| (c.entries.len(), c.id))
        .collect();
    candidates.sort_unstable();

    println!(
        "{:>6} {:>8} {:>8} {:>8}   {:>8} {:>8} {:>8}",
        "conv", "full", "bddnodes", "ratio", "half", "bddnodes", "ratio"
    );

    let mut measured = 0;
    for (_, conversation_id) in candidates {
        if measured >= 6 {
            break;
        }

        let Some(states) = complete_states(conversation_id, &index, 60_000) else {
            continue;
        };
        // Big enough to be worth measuring, small enough that releasing the diagram
        // does not run the thread out of stack - that walk is recursive and a set much
        // past this depth overflows even a 512 MB stack.
        if states.len() < 500 || states.len() > SAMPLE_LIMIT {
            continue;
        }

        let (full_states, full_nodes) = ratio_of(&states);
        let half = &states[..states.len() / 2];
        let (half_states, half_nodes) = ratio_of(half);

        println!(
            "{:>6} {:>8} {:>8} {:>8.2}   {:>8} {:>8} {:>8.2}",
            conversation_id,
            full_states,
            full_nodes,
            full_states as f64 / full_nodes.max(1) as f64,
            half_states,
            half_nodes,
            half_states as f64 / half_nodes.max(1) as f64,
        );
        measured += 1;
    }

    if measured == 0 {
        println!("no conversation was both exhaustible and large enough to be worth it");
    }
}
