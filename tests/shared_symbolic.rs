// SPDX-License-Identifier: MIT
//! Does sharing work between the symbolic searches actually pay?
//!
//! de-cnjw's measurement. Every symbolic search starts from nothing today: it rebuilds the
//! parent map, recompiles every guard, and re-derives sets an earlier run over the same
//! group already had. Three things can be handed on, and they are worth measuring apart
//! because they are not the same kind of claim:
//!
//! 1. THE PARENT MAP, a fact about the graph. Pure saving, no answer changes.
//! 2. THE COMPILED GUARDS, a fact about the graph and the world. Pure saving in time, paid
//!    for in nodes the manager cannot reclaim while they are held.
//! 3. A FORWARD RUN'S SETS, which are a proof. A backward pass that MEETS one has shown the
//!    target reachable and stops - see `Known`.
//!
//! ## Why the meet is measured on a profile where the answer is yes
//!
//! Because a meet can only ever prove reachable. On a profile whose answer is no, sharing
//! a forward run buys nothing at all and costs the forward run - so measuring the meet on
//! `deepest-1`, where conversation 14 answers not-there, would be measuring the overhead
//! and calling it the feature.
//!
//! Half the entries unseen at random is the shape a real save has, and it is the shape the
//! portfolio would be asked about. That is where a meet either pays or does not.
//!
//! ## What is compared
//!
//! ALONE     the backward driver with the parent map and nothing else known.
//! SHARED    a forward search run first under a SMALL budget - deliberately aborted, to
//!           stand for work an earlier query left behind - and then the backward driver
//!           told about it. The time reported is BOTH halves, because the forward half is
//!           not free and a comparison that hid it would be dishonest.
//!
//! One manager for both halves of SHARED, and it is not optional: two formulas built over
//! different managers cannot be combined at all, so a meet across two managers would be
//! meaningless rather than merely wrong.
//!
//! ## What it said, 2026-09-05
//!
//! ```text
//!   conv  entries  unseen  alone  shared: total = forward + backward   met  asked
//!   1030     1476     501    0ms      1ms =      1ms +        0ms      yes    1/1
//!     28     2186     707   18ms     84ms =     64ms +       19ms      yes  26/26
//!    631     4514    1423   26ms     99ms =     96ms +        1ms      yes    4/4
//! ```
//!
//! THE MEET FIRES EVERYWHERE AND PAYS IN ONE PLACE. On 631 the backward half drops from 26
//! milliseconds to ONE once the forward work exists - the winning candidate's fixed point
//! is where nearly all the cost was, and the meet cuts it off. On 28 it buys nothing: 25 of
//! the 26 candidates asked about are REFUSALS, and a meet cannot help a refusal. That is
//! the shape of the whole feature.
//!
//! SO IT IS NOT A SPEED-UP, IT IS A SUNK-COST RECOVERY. Every row is slower end to end than
//! the backward driver alone, because the forward half costs more than the meet saves. What
//! it is for is the second question on a group whose forward run has already been paid for -
//! which is why the backward half is reported separately, and why the total is reported too.
//!
//! ## The three heaviest groups take the process down at this profile
//!
//! 368, 362 and 14 overflow the stack inside a recursive diagram operation before the row
//! prints - on the plain backward driver, before anything is shared, and on the build
//! before this measurement existed as well. It is the failure de-8hh2.13 is about, and it
//! is reached here and not by the matrix because a randomly drawn profile asks about a
//! different first candidate than `deepest-1` does.
//!
//! So run this ONE CONVERSATION PER PROCESS, the way every other measurement here is run:
//!
//!     CONVERSATION=631 cargo test --release --test shared_symbolic -- --ignored --nocapture

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::known::Known;
use lookahead_engine::symbolic::novelty_search::{best_novelty, Budget as SearchBudget};
use lookahead_engine::symbolic::reachability::{seed_of, Budget as ForwardBudget, Reachability};
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::world::world::ILookAheadWorld;

mod common;

const COUNTER_CAP: i32 = 16;

/// The groups worth asking, heaviest first.
const HEAVIEST: [i32; 6] = [368, 631, 14, 362, 28, 1030];

/// How much of the group a profile has read, as a percentage.
const PERCENT_SEEN: u32 = 50;

/// What the forward half is allowed before the backward half begins.
///
/// SMALL ON PURPOSE. The point is that a PARTIAL forward run is worth something: its sets
/// only ever grow, so every state in one is genuinely reachable there and a meet against it
/// is a proof whether or not it settled. A generous budget here would measure a completed
/// forward search instead, which is a different claim and one the matrix already makes.
const FORWARD_STEPS: usize = 5_000;

fn conversations(default: &[i32]) -> Vec<i32> {
    match std::env::var("CONVERSATION") {
        Ok(named) => named.split(',').filter_map(|id| id.trim().parse().ok()).collect(),
        Err(_) => default.to_vec(),
    }
}

/// The same xorshift the matrix uses, so the two measurements draw the same profiles.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(2685821657736338717).max(1))
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(2685821657736338717)
    }
}

/// Which entries a link walk can arrive at, ignoring guards.
fn structurally_reachable(graph: &LookAheadGraph, start: DialogueNodeId) -> Vec<DialogueNodeId> {
    let mut seen: HashMap<DialogueNodeId, ()> = HashMap::new();
    let mut queue = VecDeque::from([start]);
    seen.insert(start, ());

    while let Some(id) = queue.pop_front() {
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if graph.get(child).is_some() && seen.insert(child, ()).is_none() {
                queue.push_back(child);
            }
        }
    }

    let mut all: Vec<DialogueNodeId> = seen
        .into_keys()
        .filter(|id| *id != start)
        .filter(|id| graph.get(*id).is_some_and(|node| !node.is_group))
        .collect();
    all.sort_unstable_by_key(|id| (id.conversation_id, id.entry_id));
    all
}

/// The entries a profile leaves unseen: the same draw the matrix's percentage rows make.
fn unseen_for(candidates: &[DialogueNodeId], percent: u32) -> HashSet<DialogueNodeId> {
    let mut rng = Rng::new(percent as u64);
    let mut shuffled = candidates.to_vec();
    for i in (1..shuffled.len()).rev() {
        let j = (rng.next() % (i as u64 + 1)) as usize;
        shuffled.swap(i, j);
    }
    let seen = (shuffled.len() * percent as usize) / 100;
    shuffled.into_iter().skip(seen).collect()
}

/// What one arrangement cost and what it answered.
struct Run {
    verdict: String,
    millis: u128,
    nodes: usize,
    asked: usize,
    /// Compiled guards held, and how often one was reused.
    guards: (usize, usize),
    met: bool,
    /// Just the backward half, where there was a forward half to pay for separately.
    backward_millis: u128,
}

fn search_budget() -> SearchBudget {
    SearchBudget {
        targets: usize::MAX,
        time: std::time::Duration::from_secs(120),
        each: lookahead_engine::symbolic::backward::Budget {
            steps: usize::MAX,
            time: std::time::Duration::from_secs(120),
            ..Default::default()
        },
    }
}

/// The backward driver alone, knowing only the shape of the graph.
fn alone(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    unseen: &HashSet<DialogueNodeId>,
) -> Run {
    let symbols = graph.symbols().clone();
    let layout = DataLayout::for_graph(graph, COUNTER_CAP, None, false)
        .keeping_only_read(&symbols, &DataLayout::read_by(graph));
    let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, world, &vars);
    let novelty = novelty_of(unseen);

    let known = Known::of(graph);
    let began = std::time::Instant::now();
    let answer = best_novelty(
        graph, start, &seed, &mut compiler, world, COUNTER_CAP as u32, &novelty,
        &search_budget(), Some(&known),
    );

    Run {
        verdict: format!("{:?}", answer.best),
        millis: began.elapsed().as_millis(),
        nodes: vars.node_count(),
        asked: answer.targets_asked,
        guards: compiler.guard_cache(),
        met: answer.met_at.is_some(),
        backward_millis: began.elapsed().as_millis(),
    }
}

/// A partial forward run first, then the backward driver told about it.
fn shared(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    unseen: &HashSet<DialogueNodeId>,
) -> (Run, usize) {
    let symbols = graph.symbols().clone();
    let layout = DataLayout::for_graph(graph, COUNTER_CAP, None, false)
        .keeping_only_read(&symbols, &DataLayout::read_by(graph));
    let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, world, &vars);
    let novelty = novelty_of(unseen);

    // BOTH HALVES ARE TIMED. The forward run is work this arrangement pays for, and a
    // comparison that started the clock after it would be measuring a free lunch.
    let began = std::time::Instant::now();
    let forward = Reachability::explore_within(
        graph, start, &seed, &mut compiler, world, COUNTER_CAP as u32,
        &ForwardBudget {
            steps: FORWARD_STEPS,
            time: std::time::Duration::from_secs(30),
            ..Default::default()
        },
    );
    let forward_ms = began.elapsed().as_millis() as usize;
    let known = Known::of(graph).from(start, &seed).with_forward(&forward);

    // THE BACKWARD HALF ON ITS OWN, which is the honest number for the case this feature
    // is actually for: a forward run that happened earlier, for some other question, and
    // whose cost is already spent. The total beside it is the honest number for paying for
    // the forward half here and now, and the two answer different questions.
    let backward_began = std::time::Instant::now();
    let answer = best_novelty(
        graph, start, &seed, &mut compiler, world, COUNTER_CAP as u32, &novelty,
        &search_budget(), Some(&known),
    );

    (
        Run {
            verdict: format!("{:?}", answer.best),
            millis: began.elapsed().as_millis(),
            nodes: vars.node_count(),
            asked: answer.targets_asked,
            guards: compiler.guard_cache(),
            met: answer.met_at.is_some(),
            backward_millis: backward_began.elapsed().as_millis(),
        },
        forward_ms,
    )
}

fn novelty_of(unseen: &HashSet<DialogueNodeId>) -> impl Fn(DialogueNodeId) -> Novelty + '_ {
    move |id| {
        if unseen.contains(&id) { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
    }
}

/// What sharing a partial forward run does to the backward driver.
#[test]
#[ignore = "a measurement, not a test: run it with --ignored --release"]
fn what_sharing_a_forward_run_buys() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>7} {:>16} {:>8} {:>10} {:>6} {:>16} {:>8} {:>10} {:>6} {:>5} {:>7}",
        "conv", "entries", "unseen",
        "alone", "ms", "nodes", "asked",
        "shared", "ms", "nodes", "asked", "met", "bwd ms",
    );

    for conversation in conversations(&HEAVIEST) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let reachable = structurally_reachable(&graph, start);
        if reachable.is_empty() {
            continue;
        }
        let unseen = unseen_for(&reachable, PERCENT_SEEN);

        let one = alone(&graph, start, &world, &unseen);
        let (two, forward_ms) = shared(&graph, start, &world, &unseen);

        println!(
            "{conversation:>6} {:>8} {:>7} {:>16} {:>8} {:>10} {:>6} {:>16} {:>8} {:>10} {:>6} {:>5} {:>7}",
            graph.count(),
            unseen.len(),
            one.verdict, one.millis, one.nodes, one.asked,
            two.verdict, two.millis, two.nodes, two.asked,
            if two.met { "yes" } else { "no" },
            two.backward_millis,
        );
        println!(
            "         shared total {} ms = {} forward + {} backward",
            two.millis, forward_ms, two.backward_millis,
        );

        // THE ANSWERS MUST AGREE. A meet is a proof, so sharing may make a search faster
        // and may make it stop earlier - it may never make it answer differently. This is
        // the only assertion here, and it is the one worth having.
        assert_eq!(
            one.verdict, two.verdict,
            "conversation {conversation}: sharing changed the answer, which it may never do",
        );

        println!(
            "         guards: {} held / {} reuses alone, {} held / {} reuses shared",
            one.guards.0, one.guards.1, two.guards.0, two.guards.1,
        );
    }
}
