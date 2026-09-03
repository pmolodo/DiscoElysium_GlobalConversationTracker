// SPDX-License-Identifier: MIT
//! Does a crawl's state set compress?
//!
//! The question de-sze rests on. Symbolic reachability is worth building only if the
//! states a crawl reaches share enough structure for a decision diagram to hold them in
//! far fewer nodes than there are states. This measures exactly that, on real content,
//! without building any transition relation: run the explicit crawl, collect the states
//! it visits, union them into one diagram, and compare.
//!
//! Opt-in the same way the C# corpus tests are. The index is extracted game content and
//! is not committed, so this passes silently where it has not been generated. Regenerate
//! with `dotnet run --project tools/DialogueExtract -- conversation-index`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lookahead_engine::core::state::LookAheadState;
use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions};
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::{Profile, StateEncoding, StateSet};
use lookahead_engine::world::test_world::TestWorld;

/// The conversations the C# all-seen suite opens, biggest first by group size.
const BIGGEST: [i32; 5] = [368, 631, 14, 28, 1030];

/// Enough states to see the shape without waiting all day. Building the diagram costs a
/// diagram operation per variable per state, so this is the cost driver, not the crawl.
const SAMPLE_LIMIT: usize = 20_000;

/// Generous, so the manager never reallocates mid-measurement.
const NODE_CAPACITY: usize = 1 << 22;
const CACHE_CAPACITY: usize = 1 << 20;

fn index_path() -> Option<PathBuf> {
    let mut dir: Option<&std::path::Path> = Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")));
    while let Some(d) = dir {
        let candidate = d.join(".game_reference_copies/derived/conversation_index.jsonl");
        if candidate.exists() {
            return Some(candidate);
        }
        dir = d.parent();
    }
    None
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
const MEASUREMENT_STACK: usize = 512 << 20;

#[test]
fn a_crawls_state_set_compresses() {
    std::thread::Builder::new()
        .stack_size(MEASUREMENT_STACK)
        .spawn(measure)
        .expect("spawning the measurement thread")
        .join()
        .expect("the measurement thread");
}

fn measure() {
    let Some(path) = index_path() else {
        eprintln!("conversation_index.jsonl not generated; skipping.");
        return;
    };
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
        let mut set = StateSet::new(encoding.total_vars(), NODE_CAPACITY, CACHE_CAPACITY);
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
            StateSet::new(flipped.total_vars(), NODE_CAPACITY, CACHE_CAPACITY);
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
