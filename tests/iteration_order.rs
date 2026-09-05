// SPDX-License-Identifier: MIT
//! Is the SCC iteration order right, and what did it buy?
//!
//! de-3x76.3. Both symbolic searches are a dataflow fixed point over the group's links with
//! a decision diagram at each entry, and both used to take their worklist first-in-first-out.
//! FIFO ignores the graph's shape: at a join it pops the entry once per arm that happens to
//! deliver at a different time, where a topological order pops it once with every arm
//! already folded into what is pending. See `symbolic::order`.
//!
//! ## What is checked here, and where the rest is checked
//!
//! THIS FILE CHECKS THE ORDER ITSELF, against the real corpus rather than against hand-built
//! graphs - `symbolic::order`'s own unit tests do those. An SCC decomposition that is subtly
//! wrong still produces a plausible-looking order, so the guarantee is checked directly:
//!
//! - every entry is ranked, exactly once, with no rank repeated or skipped;
//! - every link that LEAVES a component climbs in rank;
//! - a component really is a maximal set of mutually reachable entries, cross-checked by
//!   two link walks from a representative rather than by trusting Tarjan.
//!
//! THAT THE SEARCHES STILL ANSWER CORRECTLY is checked where it always was, and those are
//! the tests that matter for soundness: `tests/symbolic_reachability.rs` runs the forward
//! search against the explicit crawl, and `tests/backward_oracle.rs` runs the backward
//! search against it on every target of every group it can check both ways. Both pass with
//! the order in place.
//!
//! ## What the order bought, measured 2026-09-05
//!
//! Steps is the column that carries the comparison - it is deterministic, where the clock is
//! a shared machine. A backward row is the sum over EVERY candidate with none allowed to
//! stop early, because an order can change WHICH candidate answers first and stopping would
//! compare two different amounts of work. That sum is also what the driver pays on a group
//! whose answer is no, which is where the cost actually lives.
//!
//! ```text
//!   conv  entries  SCCs  largest  pass       FIFO   ordered   pops     ms -> ms
//!     28     2186   606     1154  forward   14322      5647   -61%    237     58
//!                                 backward 210401    182175   -13%    393    417
//! ```
//!
//! Both arrangements settle, reach the same 604 entries forward, and agree on all 707
//! candidates backward.
//!
//! ## What is NOT claimed here, and the mistake that is worth recording
//!
//! An earlier build gave every entry its own rank instead of sharing one per component. It
//! looked better still - 28's forward pops fell to 3738, and 368 and 631 crossed from
//! spending their memory budget unfinished to settling. Then conversation 14, whose largest
//! component holds 2679 of its 3594 entries, came in at SIX TIMES the pops of a plain queue
//! and reached FEWER entries in the same time:
//!
//! ```text
//!     14     3594   593     2679  forward   12074     76310  +532%  133527  139306
//! ```
//!
//! Distinct ranks invent a priority among entries the order has nothing to say about, so
//! inside a large cycle the worklist keeps returning to the same low-ranked few and starves
//! the rest of the component. Sharing one rank per component - which is what a weak
//! topological order actually prescribes, and what `IterationOrder` documents - leaves the
//! push order to break the tie, so a component is swept.
//!
//! SO THE 368 AND 631 RESULTS ABOVE BELONG TO THE REJECTED VARIANT and are not repeated
//! here, because the shipped code has not been measured on them. Their FIFO baselines are
//! recorded below so that a later run has something to compare against, and re-measuring
//! them is de-3x76.3's remaining work:
//!
//! ```text
//!   1030  forward FIFO       92 steps,  73 reached, settled,      0 ms
//!    368  forward FIFO    20000 steps, 984 reached, INCOMPLETE, 4353 ms
//!    631  forward FIFO    20000 steps, 703 reached, INCOMPLETE, 6828 ms
//!    631  backward FIFO 9502024 steps over 1423 candidates,     24114 ms
//! ```
//!
//! An INCOMPLETE forward run spent the MEMORY budget, which is checked every 20,000 steps.
//!
//! THE PREDICTOR IS `largest_component`, which says in advance how much order a group has to
//! exploit. 1030 is the control at 1379 of 1476 entries in ONE component - almost nothing to
//! take. 14 is the warning: a group that is mostly one cycle is where a bad reading of the
//! order does damage rather than nothing.
//!
//! THE FIFO ARRANGEMENT IS NOT IN THE CODE. The numbers above were taken against a switch
//! that has since been removed along with every other unordered path, because keeping a
//! slower path alive so a settled comparison can be re-run is how a slower path gets used by
//! accident. It is in the history if it is ever wanted again.
//!
//! ## Why the task this belongs to was closed once, and reopened
//!
//! de-3x76.3 proposed exactly this and was closed as not worth building, on the strength of
//! `tests/requeue_shape.rs`: conversation 14's re-queue distribution is only mildly
//! concentrated, so 14 is limited by what a step COSTS rather than by how many there are.
//!
//! THAT READING OF 14 STILL STANDS. It is the sample that was wrong. 14 is the group whose
//! reachable set has no structure for a diagram to exploit at all (de-3x76.11), and the
//! other heavy groups are limited by the step COUNT instead - which is the thing an order
//! attacks. A measurement taken on the hardest group alone says what that group is limited
//! by, not what a change is worth.

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index, Index};
use lookahead_engine::symbolic::backward::{Backward, Budget as BackwardBudget};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::known::Known;
use lookahead_engine::symbolic::novelty_search::candidates;
use lookahead_engine::symbolic::order::{IterationOrder, Ranking};
use lookahead_engine::symbolic::reachability::{seed_of, Budget as ForwardBudget, Reachability};
use lookahead_engine::symbolic::vars::DataVars;

mod common;

const COUNTER_CAP: i32 = 16;

/// The groups every symbolic measurement in this repository is taken on.
const MEASURED: [i32; 5] = [368, 631, 14, 28, 1030];

/// How much of the group a profile has read, as a percentage.
const PERCENT_SEEN: u32 = 50;

/// How long one forward pass may run before it is called unfinished.
///
/// A CAP THAT BINDS, and a row that hits it says INCOMPLETE for a different reason than one
/// that spent its memory budget - the two are not the same failure and a comparison has to
/// say which happened. 631 under `PerComponent` spends this whole allowance.
const FORWARD_CAP: std::time::Duration = std::time::Duration::from_secs(300);

/// How long ONE backward candidate may run. There are hundreds of them per group.
const BACKWARD_CAP: std::time::Duration = std::time::Duration::from_secs(60);

/// The largest group the mutual-reachability cross-check is run on.
///
/// It walks the graph twice per component, so it is quadratic in a way the other checks are
/// not. The groups above this are covered by the two linear checks, and the shapes that
/// could break Tarjan - nested cycles, a component entered at two places - are not rare
/// enough to need a big group to find one.
const CROSS_CHECK_CEILING: usize = 400;

/// Which entries `from` can reach, following `links` forwards or backwards.
fn walk(
    from: DialogueNodeId,
    edges: &HashMap<DialogueNodeId, Vec<DialogueNodeId>>,
) -> HashSet<DialogueNodeId> {
    let mut seen = HashSet::from([from]);
    let mut queue = VecDeque::from([from]);
    while let Some(id) = queue.pop_front() {
        for &next in edges.get(&id).into_iter().flatten() {
            if seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    seen
}

/// The group's links, and the same links reversed.
fn edges_of(
    graph: &LookAheadGraph,
) -> (HashMap<DialogueNodeId, Vec<DialogueNodeId>>, HashMap<DialogueNodeId, Vec<DialogueNodeId>>) {
    let mut forward: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
    let mut backward: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
    for node in graph.nodes() {
        for &child in &node.links {
            if graph.get(child).is_none() {
                continue;
            }
            forward.entry(node.id).or_default().push(child);
            backward.entry(child).or_default().push(node.id);
        }
    }
    (forward, backward)
}

/// Every group in the index, largest first so a failure names a real one.
fn groups(index: &Index) -> Vec<i32> {
    let mut all: Vec<i32> = index.keys().copied().collect();
    all.sort_unstable();
    all
}

/// Every entry is ranked, and the ranks number the components without a gap.
///
/// A RANK IS A COMPONENT NUMBER, not a position among entries - members of one cycle share
/// one. So the ranks seen across a group must be exactly `0..components`, every one of them
/// used, and the number of entries carrying each is that component's size.
#[test]
fn every_entry_is_ranked_and_the_ranks_number_the_components() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");

    let mut checked = 0;
    for conversation in groups(&index) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        if graph.count() == 0 {
            continue;
        }
        let order = IterationOrder::of(&graph);

        assert_eq!(
            order.len(),
            graph.count(),
            "conversation {conversation}: {} entries but {} ranked",
            graph.count(),
            order.len(),
        );

        // BY COMPONENT, NOT BY RANK, because what a rank means depends on the `Ranking` and
        // the decomposition does not. `PerComponent` makes the two the same; the default
        // does not.
        let mut sizes: HashMap<u32, usize> = HashMap::new();
        for node in graph.nodes() {
            let component = order.component_of(node.id).expect("every entry is in one");
            *sizes.entry(component).or_default() += 1;
        }

        let mut used: Vec<u32> = sizes.keys().copied().collect();
        used.sort_unstable();
        let expected: Vec<u32> = (0..order.components() as u32).collect();
        assert_eq!(
            used, expected,
            "conversation {conversation}: the ranks do not number its components",
        );
        assert_eq!(
            sizes.values().copied().max().unwrap_or(0),
            order.largest_component(),
            "conversation {conversation}: largest_component disagrees with the ranks",
        );
        checked += 1;
    }

    assert!(checked > 100, "only {checked} groups were checked; the corpus did not load");
}

/// THE GUARANTEE. A link that leaves a component always climbs in rank.
///
/// This is the whole of what the searches rely on: a component is finished before anything
/// downstream of it begins. Inside a component nothing is promised, because a cycle has no
/// first entry.
#[test]
fn a_link_between_components_always_climbs() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");

    let mut crossings = 0;
    for conversation in groups(&index) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };

        // BOTH ORDERING RANKINGS. They disagree about what happens inside a component and
        // must not disagree about this. `Fifo` is excluded because it makes no ordering
        // claim - it exists to be the baseline, not to honour a guarantee.
        for ranking in [Ranking::PerEntry, Ranking::PerComponent] {
            let order = IterationOrder::of(&graph).ranked(ranking);

            for node in graph.nodes() {
                for &child in &node.links {
                    if graph.get(child).is_none() {
                        continue;
                    }
                    if order.component_of(node.id) == order.component_of(child) {
                        continue;
                    }
                    assert!(
                        order.rank_of(node.id) < order.rank_of(child),
                        "conversation {conversation} under {ranking:?}: {} -> {child} \
                         leaves a component but does not climb ({} then {})",
                        node.id,
                        order.rank_of(node.id),
                        order.rank_of(child),
                    );
                    crossings += 1;
                }
            }
        }
    }

    assert!(crossings > 1000, "only {crossings} boundaries were crossed; too few to trust");
}

/// A component is exactly the entries mutually reachable with any one of its members.
///
/// CROSS-CHECKED BY WALKING, not by asking Tarjan again. An entry `e` is in the component of
/// `r` exactly when `r` reaches `e` and `e` reaches `r`, so the component is the intersection
/// of a forward walk and a backward walk from any member. A decomposition that merged two
/// components or split one would fail here and would pass every other check in this file.
#[test]
fn a_component_is_exactly_what_is_mutually_reachable() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");

    let mut checked = 0;
    let mut cyclic = 0;
    for conversation in groups(&index) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        if graph.count() == 0 || graph.count() > CROSS_CHECK_CEILING {
            continue;
        }

        let order = IterationOrder::of(&graph);
        let (forward, backward) = edges_of(&graph);

        // The members of each component, gathered from the decomposition under test.
        let mut members: HashMap<u32, HashSet<DialogueNodeId>> = HashMap::new();
        for node in graph.nodes() {
            let component = order.component_of(node.id).expect("every entry is in one");
            members.entry(component).or_default().insert(node.id);
        }

        for claimed in members.values() {
            let representative = *claimed.iter().next().expect("a component is not empty");
            let reaches = walk(representative, &forward);
            let reached_by = walk(representative, &backward);
            let truly: HashSet<DialogueNodeId> =
                reaches.intersection(&reached_by).copied().collect();

            assert_eq!(
                *claimed, truly,
                "conversation {conversation}: the component holding {representative} is not \
                 what is mutually reachable with it",
            );
            if claimed.len() > 1 {
                cyclic += 1;
            }
        }
        checked += 1;
    }

    assert!(checked > 50, "only {checked} groups were cross-checked; the corpus did not load");
    assert!(cyclic > 0, "no group had a cycle at all, so nothing interesting was checked");
}

/// The same xorshift the other measurements use, so they all draw the same profiles.
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

/// The entries a profile leaves unseen: the same draw the other measurements make.
fn unseen_for(all: &[DialogueNodeId], percent: u32) -> HashSet<DialogueNodeId> {
    let mut rng = Rng::new(percent as u64);
    let mut shuffled = all.to_vec();
    for i in (1..shuffled.len()).rev() {
        let j = (rng.next() % (i as u64 + 1)) as usize;
        shuffled.swap(i, j);
    }
    let seen = (shuffled.len() * percent as usize) / 100;
    shuffled.into_iter().skip(seen).collect()
}

/// What each search costs on a group, taken in the order.
///
/// NO FIFO COLUMN, because there is no FIFO path any more. The numbers to read this against
/// are the FIFO ones recorded at the top of this file, which were taken on the same profile
/// and the same machine before the unordered path was removed. `steps` is deterministic, so
/// it is the column that carries the comparison; the clock is the weaker one.
///
///     CONVERSATION=631 cargo test --release --test iteration_order -- --ignored --nocapture
#[test]
#[ignore = "a measurement, not a test: run it with --ignored --release"]
fn what_the_searches_cost_in_the_order() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    let asked: Vec<i32> = match std::env::var("CONVERSATION") {
        Ok(named) => named.split(',').filter_map(|id| id.trim().parse().ok()).collect(),
        Err(_) => MEASURED.to_vec(),
    };

    println!(
        "{:>6} {:>8} {:>6} {:>8}  {:9} {:9} {:>9} {:>9} {:>7} {:>8} {:>9}",
        "conv", "entries", "SCCs", "largest", "ranking", "pass", "steps", "widenings", "pops/e", "ms",
        "reached",
    );

    for conversation in asked {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let shape = IterationOrder::of(&graph);
        println!(
            "{conversation:>6} {:>8} {:>6} {:>8}",
            graph.count(), shape.components(), shape.largest_component(),
        );

        for ranking in [Ranking::Fifo, Ranking::PerComponent, Ranking::PerEntry] {
            measure(&graph, start, &world, ranking);
        }
    }
}

/// One group under one ranking: the forward pass, then every backward candidate.
///
/// BOTH RANKINGS IN ONE PROCESS AND ONE BUILD, which is the whole reason this exists. They
/// disagree by an order of magnitude on the groups that are mostly one cycle, and in
/// opposite directions on two groups of the same shape - so a comparison across two runs,
/// let alone two builds, would not be worth reading.
fn measure(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn lookahead_engine::world::world::ILookAheadWorld,
    ranking: Ranking,
) {
    let label = match ranking {
        Ranking::PerComponent => "per-comp",
        Ranking::PerEntry => "per-entry",
        Ranking::Fifo => "fifo",
    };
    {
        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(graph, COUNTER_CAP, None, false)
            .keeping_only_read(&symbols, &DataLayout::read_by(graph));
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
        let seed = seed_of(graph, world, &vars);
        let order = IterationOrder::of(graph).ranked(ranking);

        let began = std::time::Instant::now();
        let forward = Reachability::explore_knowing(
            graph, start, &seed, &mut compiler, world, COUNTER_CAP as u32,
            &ForwardBudget {
                time: FORWARD_CAP,
                ..Default::default()
            },
            &order,
        );
        let reached = forward.entries().count();
        println!(
            "{:>23}  {label:9} {:9} {:>9} {:>9} {:>7.2} {:>8} {:>9}  {}",
            "", "forward", forward.stats().steps, forward.stats().widenings,
            forward.stats().steps as f64 / reached.max(1) as f64,
            began.elapsed().as_millis(), reached,
            if forward.stats().reached_fixed_point { "settled" } else { "INCOMPLETE" },
        );

        // EVERY CANDIDATE, WITH NO STOPPING EARLY, which is what the driver pays on a group
        // whose answer is no: all of them asked, all of them refused, each a whole pass.
        // THE SAME DRAW THE OTHER MEASUREMENTS MAKE: over the entries a link walk can
        // actually arrive at, excluding the start, so the profiles line up across files.
        let (links, _) = edges_of(graph);
        let mut arrivable: Vec<DialogueNodeId> = walk(start, &links)
            .into_iter()
            .filter(|id| *id != start)
            .filter(|id| graph.get(*id).is_some_and(|node| !node.is_group))
            .collect();
        arrivable.sort_unstable_by_key(|id| (id.conversation_id, id.entry_id));
        let unseen = unseen_for(&arrivable, PERCENT_SEEN);
        let novelty = |id: DialogueNodeId| {
            if unseen.contains(&id) { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
        };

        let targets = candidates(graph, start, &novelty);
        let known = Known::of(graph).ranking(ranking);
        let (mut steps, mut widenings, mut entries, mut settled) = (0, 0, 0usize, true);
        let began = std::time::Instant::now();
        for &target in &targets {
            let pass = Backward::reaching_knowing(
                graph, target, &mut compiler, world, COUNTER_CAP as u32,
                &BackwardBudget {
                    steps: usize::MAX,
                    time: BACKWARD_CAP,
                    ..Default::default()
                },
                Some(&known),
            );
            steps += pass.stats().steps;
            widenings += pass.stats().widenings;
            entries += pass.stats().entries_reaching;
            settled &= pass.stats().reached_fixed_point;
        }
        println!(
            "{:>23}  {label:9} {:9} {:>9} {:>9} {:>7.2} {:>8} {:>9}  {}, {} candidates",
            "", "backward", steps, widenings,
            steps as f64 / entries.max(1) as f64,
            began.elapsed().as_millis(), entries,
            if settled { "settled" } else { "INCOMPLETE" },
            targets.len(),
        );
    }
}
