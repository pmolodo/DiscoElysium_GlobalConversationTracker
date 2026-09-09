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
//!   conv  entries  unseen  knowing    verdict           ms      nodes  asked
//!   1030     1476     501  alone      UnseenAnyGame      0       2775      1
//!                          partial    UnseenAnyGame      1       3805      1  met
//!                          SETTLED    UnseenAnyGame      1       3805      1  met
//!     28     2186     707  alone      UnseenAnyGame     17      13530     26
//!                          partial    UnseenAnyGame     77     159808     26  met, 57+19
//!                          SETTLED    UnseenAnyGame    249     548635     26  met, 247+2
//!    631     4514    1423  alone      UnseenAnyGame     31      56380      4
//!                          partial    UnseenAnyGame     99     274883      4  met, 96+1
//! ```
//!
//! The last two columns of the shared rows are the forward half and the backward half, in
//! milliseconds, of the total beside them.
//!
//! THE MEET FIRES EVERYWHERE AND PAYS IN ONE PLACE. On 631 the backward half falls from 31
//! milliseconds to ONE once a forward run exists - nearly all the cost was the winning
//! candidate fixed point, and the meet cuts it off. On 28 it buys nothing: 25 of the 26
//! candidates asked about are REFUSALS, and no forward set can shorten a refusal. That is
//! the shape of the whole feature, and the reason de-fawk exists.
//!
//! SO IT IS A SUNK-COST RECOVERY, NOT A SPEED-UP. Every row is slower end to end than the
//! backward driver alone, because the forward half costs more than the meet saves. What it
//! is for is the SECOND question on a group whose forward run has already been paid for,
//! which is why the backward half is reported apart from the total.
//!
//! ## Pruning is sound, it does not overflow, and it works - 2026-09-07
//!
//! Narrowing a backward pass by a SETTLED forward run is the half that could shorten a
//! refusal. tests/backward_oracle.rs checks it against the explicit search on every target
//! of every group it can check both ways, and the pruned answer has never differed from the
//! plain one.
//!
//! IT USED TO OVERFLOW, and that table is gone because it was not measuring pruning. It
//! said 28 died with the full bound and 631 died with either, and de-8hh2.13's cause is
//! what it was recording: something accumulates per thread inside the diagram manager, so
//! this file's three or four searches in one thread died on the third and pruning only
//! moved which round that was. The searches each get their own thread now (de-fpax), and
//! at DEGCT_PRUNING=1 all five groups here run to completion. Nothing overflows.
//!
//! WHAT IT BUYS, on the SETTLED rows, which is the only place it applies:
//!
//! ```text
//!   conv  candidates asked  backward ms off  backward ms on
//!     28                26               19               2
//!    368                 1                0               0
//!    362                 3                0               0
//! ```
//!
//! Conversation 28 is the case de-fawk said would decide it - 26 candidates of which 25 are
//! REFUSALS - and the backward half falls from nineteen milliseconds to two. That is the
//! thing nothing else measured has touched: a meet can only ever accelerate a YES, and 28's
//! cost is in its NOs. 368 and 362 ask one and three candidates and were already at zero, so
//! there was nothing there to improve.
//!
//! 631 and 14 do not appear because their forward runs DO NOT SETTLE - sixty-six and
//! sixty-nine seconds and still going - and pruning may only narrow against a settled run.
//! The small backward differences in their rows are the meet and are noise beside a forward
//! half that long.
//!
//! ## The shipped path prunes, and the argument against it did not survive the game
//!
//! `portfolio::Budget::default` carries `pruning: true`. The case for leaving it off was
//! that the slice almost never has a settled run to prune with - fifty milliseconds, halted
//! the moment it finds what it is hunting, and neither a halted nor a starved run may
//! narrow anything. That was read off the five heaviest groups.
//!
//! `measurements/settles_within.rs` asked the whole game instead: at those same fifty
//! milliseconds 119 of 120 ordinary groups settle, and 25 of the 50 that span conversations.
//! Most of the game had a settled run and nothing was using it. Pruning is also
//! self-guarding - `Known` narrows nothing without `forward_settled` - so the groups the
//! argument was made about are exactly the ones it still costs nothing on.
//!
//! WHAT A BIGGER SLICE WOULD BUY IS SEPARATE, and it is small: 28 settles in about
//! FIFTY-SEVEN milliseconds, just past the fifty it is allowed. The same sweep prices the
//! rest - twenty times the budget settles four more groups out of a hundred and seventy -
//! and the budget is spent per START, so a menu pays for it two dozen times over.
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
//!     DEGCT_CONVERSATION=631 cargo run --release --example shared_symbolic

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated::on_its_own_thread;
use lookahead_engine::symbolic::known::Known;
use lookahead_engine::symbolic::novelty_search::{best_novelty, Budget as SearchBudget};
use lookahead_engine::symbolic::reachability::{seed_of, Budget as ForwardBudget, Reachability};
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::world::world::ILookAheadWorld;

#[path = "../tests/common/mod.rs"]
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
    match lookahead_engine::core::env::var("CONVERSATION") {
        Ok(named) => named.split(',').filter_map(|id| id.trim().parse().ok()).collect(),
        Err(_) => default.to_vec(),
    }
}

/// Whether the shared run narrows its backward passes. `DEGCT_PRUNING=0` turns it off.
///
/// ON BY DEFAULT HERE and off everywhere else, which is the arrangement de-fawk wants
/// measured: what a settled forward run is worth as a bound is the only thing measured so
/// far that can shorten a REFUSAL, and a refusal is where the driver's cost is - 25 of
/// conversation 28's 26 candidates.
fn pruning() -> bool {
    lookahead_engine::core::env::var("PRUNING").map(|on| on.trim() != "0").unwrap_or(true)
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
    /// Whether the forward half settled, which is what licenses pruning.
    settled: bool,
}

fn search_budget() -> SearchBudget {
    SearchBudget {
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

    // A THREAD PER SEARCH, with the manager built inside it. de-fpax; see
    // `symbolic::isolated` for why the manager has to be created in here rather than handed
    // in, and de-8hh2.13 for the accumulation it avoids.
    on_its_own_thread(|| {
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
        let seed = seed_of(graph, world, &vars).expect("room for a seed");
        let novelty = novelty_of(unseen);

        let known = Known::of(graph);
        let began = std::time::Instant::now();
        let answer = best_novelty(
            graph, start, StartBranch::Either, &seed, &mut compiler, world,
            COUNTER_CAP as u32, &novelty, &search_budget(), Some(&known), None,
        );

        Run {
            verdict: format!("{:?}", answer.best),
            millis: began.elapsed().as_millis(),
            nodes: vars.node_count(),
            asked: answer.targets_asked,
            guards: compiler.guard_cache(),
            met: answer.met_at.is_some(),
            backward_millis: began.elapsed().as_millis(),
            settled: false,
        }
    })
}

/// A forward run first, then the backward driver told about it.
///
/// `steps` is what the forward half is allowed. A SMALL number stands for work an earlier
/// query left half done, and only the meet can use it. A number large enough for the fixed
/// point to settle also lets the backward passes be PRUNED - a settled run bounds what can
/// arrive at each entry - which is the only thing here that can shorten a refusal.
fn shared(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    unseen: &HashSet<DialogueNodeId>,
    steps: usize,
) -> (Run, usize) {
    let symbols = graph.symbols().clone();
    let layout = DataLayout::for_graph(graph, COUNTER_CAP, None, false)
        .keeping_only_read(&symbols, &DataLayout::read_by(graph));
    // A THREAD PER SEARCH, with the manager built inside it - de-fpax. BOTH HALVES SHARE
    // ONE, and they have to: the forward run's sets are handed to the backward half through
    // `Known`, and two formulas built over different managers cannot be combined at all.
    on_its_own_thread(|| {
    let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, world, &vars).expect("room for a seed");
    let novelty = novelty_of(unseen);

    // BOTH HALVES ARE TIMED. The forward run is work this arrangement pays for, and a
    // comparison that started the clock after it would be measuring a free lunch.
    let began = std::time::Instant::now();
    let forward = Reachability::explore_within(
        graph, start, &seed, &mut compiler, world, COUNTER_CAP as u32,
        &ForwardBudget {
            steps,
            time: std::time::Duration::from_secs(60),
            ..Default::default()
        },
    );
    let forward_ms = began.elapsed().as_millis() as usize;
    // PRUNING FOLLOWS THE FORWARD RUN: it does nothing without a settled one, and it is
    // off by default everywhere else. Asked for here because measuring what it costs is
    // half of what this file is for.
    //
    // A SWITCH RATHER THAN A CONSTANT since de-fawk, because the number that matters is
    // the DIFFERENCE and one run cannot show it. `DEGCT_PRUNING=0` runs the same rows with the
    // narrowing off, so the two can be read against each other on the same machine.
    let known = Known::of(graph)
        .from(start, &seed)
        .with_forward(&forward)
        .pruning(pruning());

    // THE BACKWARD HALF ON ITS OWN, which is the honest number for the case this feature
    // is actually for: a forward run that happened earlier, for some other question, and
    // whose cost is already spent. The total beside it is the honest number for paying for
    // the forward half here and now, and the two answer different questions.
    let backward_began = std::time::Instant::now();
    let answer = best_novelty(
        graph, start, StartBranch::Either, &seed, &mut compiler, world, COUNTER_CAP as u32,
        &novelty, &search_budget(), Some(&known), None,
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
            settled: forward.stats().reached_fixed_point,
        },
        forward_ms,
    )
    })
}

fn novelty_of(unseen: &HashSet<DialogueNodeId>) -> impl Fn(DialogueNodeId) -> Novelty + '_ {
    move |id| {
        if unseen.contains(&id) { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
    }
}

/// What sharing a partial forward run does to the backward driver.
fn main() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>7}  {:9} {:>16} {:>8} {:>10} {:>6}",
        "conv", "entries", "unseen", "knowing", "verdict", "ms", "nodes", "asked",
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

        // PRINTED AS EACH ARRANGEMENT FINISHES, not gathered into one line at the end.
        // The third arrangement runs a forward fixed point to completion, and on the heavy
        // groups that overflows the stack inside a recursive diagram operation and takes
        // the process with it - so a row gathered at the end loses the two arrangements
        // that did work.
        let one = alone(&graph, start, &world, &unseen);
        println!(
            "{conversation:>6} {:>8} {:>7}  alone     {:>16} {:>8} {:>10} {:>6}",
            graph.count(), unseen.len(), one.verdict, one.millis, one.nodes, one.asked,
        );

        let (two, forward_ms) = shared(&graph, start, &world, &unseen, FORWARD_STEPS);
        println!(
            "{:>23}  partial   {:>16} {:>8} {:>10} {:>6}  met {}, {} fwd + {} bwd",
            "", two.verdict, two.millis, two.nodes, two.asked,
            if two.met { "yes" } else { "no" }, forward_ms, two.backward_millis,
        );
        assert_eq!(
            one.verdict, two.verdict,
            "conversation {conversation}: sharing changed the answer, which it may never do",
        );

        // AND AGAIN WITH THE FORWARD HALF ALLOWED TO FINISH, because only a settled run
        // bounds what can arrive at an entry, and that bound is the only thing measured
        // here that can shorten a REFUSAL.
        let (three, settled_forward_ms) = shared(&graph, start, &world, &unseen, usize::MAX);
        println!(
            "{:>23}  {:9} {:>16} {:>8} {:>10} {:>6}  met {}, {} fwd + {} bwd",
            "",
            if three.settled { "SETTLED" } else { "unsettled" },
            three.verdict, three.millis, three.nodes, three.asked,
            if three.met { "yes" } else { "no" },
            settled_forward_ms, three.backward_millis,
        );
        assert_eq!(
            one.verdict, three.verdict,
            "conversation {conversation}: pruning changed the answer, which it may never do",
        );

        println!(
            "{:>23}  guards held/reused: {} / {} alone, {} / {} partial, {} / {} settled",
            "",
            one.guards.0, one.guards.1, two.guards.0, two.guards.1,
            three.guards.0, three.guards.1,
        );
    }
}
