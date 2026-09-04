// SPDX-License-Identifier: MIT
//! Does the symbolic search reach the same entries as the explicit crawl?
//!
//! The question de-sze turns on, asked the only way that means anything: run both over
//! the same graph and the same world and compare. A symbolic engine that agrees with
//! nothing has established nothing, and one that disagrees with the explicit crawl is
//! answering a different question.
//!
//! ## What is compared, and what is not
//!
//! The set of ENTRIES reached, not the states at them. The two searches do not carry the
//! same thing - one holds `(entry, state)` pairs and the other one data-state set per
//! entry - so their state counts are not comparable by construction. The reachable entry
//! set is the thing both compute and the thing the look-ahead actually uses: a marker
//! depends on whether an unseen entry can be reached, not on how many ways.
//!
//! ## The direction a disagreement is allowed to run
//!
//! The symbolic side may reach MORE. Its guards let an undecided answer through, money is
//! not in its layout so no cost can be refused, and both are deliberate
//! over-approximations - a set that is too big loses precision, while one that is too
//! small loses markers. So the assertion is containment, not equality, and the surplus is
//! reported rather than tolerated silently.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions};
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::reachability::{Budget, Reachability};
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::world::world::ILookAheadWorld;

mod common;

const COUNTER_CAP: i32 = 16;
/// How many nodes the diagram manager may hold, derived from the shared allowance.
///
/// WAS A HAND-PICKED 2^22, and that quietly made this comparison unequal in the same way it
/// made the performance matrix unequal. The manager preallocates its capacity and refuses to
/// grow past it, so 4,194,304 nodes is a hard ceiling of about 134 MB - half what the
/// forward crawl gets. A row reading "NO ROOM" was then reporting the harness rationing the
/// diagram, not the diagram failing to fit, and the two are entirely different findings.
const NODE_CAPACITY: usize = COMPARISON_MEMORY / DataVars::NODE_BYTES;

/// The operation cache, kept at the quarter of the node capacity it was before.
const CACHE_CAPACITY: usize = NODE_CAPACITY / 4;

/// Small enough that the explicit crawl can exhaust them, which is what makes them usable
/// as an oracle. A conversation the explicit crawl gives up on proves nothing when the
/// symbolic side reaches more.
const CHECKABLE: [i32; 6] = [1123, 484, 1066, 1147, 949, 511];

/// What each engine is allowed for the comparison, in bytes.
///
/// The shipped default, given to BOTH sides. The comparison is the point of this file, and
/// a comparison needs a common unit: 200,000 states against 500,000 steps says nothing
/// about which search did more with the same room, because neither number is room. See
/// de-e23q and tests/crawl_memory.rs.
const COMPARISON_MEMORY: usize = lookahead_engine::engine::engine::DEFAULT_MEMORY_BUDGET;

/// And the same clock for both, for the same reason.
const COMPARISON_TIME: std::time::Duration = std::time::Duration::from_secs(60);

/// The groups that drive the cost.
///
/// 362 IS FIRST BECAUSE IT IS THE LARGEST AND WAS THE LAST ONE UNMEASURED. At 1,860
/// entries it is the biggest conversation in the game, and it was left out of every
/// measurement because the IN-GAME harness cannot open it - the conversation starts, the
/// mod logs it, and no menu is ever drawn (de-i60.17). That is a harness problem and not a
/// property of the data: nothing here needs a save, a scene or a menu, so the group builds
/// and crawls like any other.
///
/// Worth the place because entry count predicts cost almost not at all - the second
/// largest finishes in 55,889 states while the fourth does not finish in 1,600,000 - so
/// the largest of all was genuinely unknown. Measured 2026-09-04, and it is the hardest
/// group in the game by a distance: on the one-unseen-entry question BOTH engines give up,
/// the crawl exhausting 200,000 states in 316ms and the symbolic side timing out at 60s.
const EXPENSIVE: [i32; 6] = [362, 368, 631, 14, 28, 1030];

/// Which conversations this process should measure.
///
/// ONE PER PROCESS is the intended way to run these, driven by
/// `tools/measure-symbolic.sh`. They die in ways that take the whole process down - a
/// manager out of nodes, a stack overflow in a recursive diagram operation, a step that
/// runs minutes past its budget - and with several in one run the first crash destroys
/// every row after it. A run that measured 368, then overflowed on 631, reported nothing
/// at all for 14, 28 and 1030 and had to be started again from the beginning.
///
/// Answering the whole list when nothing is named keeps the test runnable on its own; the
/// script is what makes the results survivable.
fn conversations(default: &[i32]) -> Vec<i32> {
    match std::env::var("CONVERSATION") {
        Ok(named) => named
            .split(',')
            .filter_map(|id| id.trim().parse().ok())
            .collect(),
        Err(_) => default.to_vec(),
    }
}

/// Which entries are reachable from `start` by FOLLOWING LINKS ALONE, and how far.
///
/// Guards and actions ignored entirely. This is the loosest possible notion of reachable
/// and it is exactly why it is worth having: it is an upper bound that no stateful search
/// can exceed, so an entry it cannot find is unreachable for certain and a measurement
/// aimed at one proves nothing at all.
///
/// The C# tried this shape as a PREFILTER and it was measured and reverted - it prunes
/// only 0.5 to 4 per cent in the hub-connected case, because ignoring guards throws away
/// what makes dialogue reachability interesting. As a way of choosing a fair question to
/// ask, though, it is exactly right.
fn structurally_reachable(
    graph: &lookahead_engine::graph::graph::LookAheadGraph,
    start: DialogueNodeId,
) -> HashMap<DialogueNodeId, usize> {
    let mut depth = HashMap::new();
    let mut queue = std::collections::VecDeque::new();
    depth.insert(start, 0usize);
    queue.push_back(start);

    while let Some(id) = queue.pop_front() {
        let here = depth[&id];
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if graph.get(child).is_some() && !depth.contains_key(&child) {
                depth.insert(child, here + 1);
                queue.push_back(child);
            }
        }
    }

    depth
}

/// Every entry the explicit crawl reaches from `start`, and whether it ran out of budget.
fn explicit(
    graph: &lookahead_engine::graph::graph::LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    budget: usize,
) -> (HashSet<DialogueNodeId>, bool) {
    // Shared with the callback, which the engine requires to be 'static, so a borrow of a
    // local will not do.
    let reached: Arc<Mutex<HashSet<DialogueNodeId>>> = Arc::default();
    let sink = Arc::clone(&reached);

    let engine = LookAheadEngine::new(LookAheadOptions {
        state_budget: budget,
        time_budget: std::time::Duration::from_secs(60),
        counter_cap: COUNTER_CAP,
        // EVERY state, not a sample: the entry set is what is being collected, and
        // sampling would drop an entry that only one state reaches.
        state_sample_interval: 1,
        on_state_reached: Some(Box::new(move |node, _state, _count| {
            sink.lock().expect("the sink").insert(node);
        })),
        ..Default::default()
    });

    let result = engine.evaluate(graph, start, world, |_| Novelty::SeenThisGame);
    let entries = reached.lock().expect("the sink").clone();
    (entries, result.budget_exhausted())
}

#[test]
fn the_symbolic_search_reaches_what_the_explicit_crawl_reaches() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>9} {:>9} {:>8} {:>9} {:>7}",
        "conv", "entries", "explicit", "symbolic", "surplus", "bddnodes", "steps"
    );

    let mut compared = 0;

    for conversation in conversations(&CHECKABLE) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let (walked, exhausted) = explicit(&graph, start, &world, 400_000);
        if exhausted {
            println!("{conversation:>6}  the explicit crawl ran out of budget; skipped");
            continue;
        }

        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
        let symbols = graph.symbols().clone();
        let vars = DataVars::new(&layout, &symbols, NODE_CAPACITY, CACHE_CAPACITY);
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(&graph));

        // The same state the explicit crawl starts in, encoded - not every data state.
        // Starting from all of them would walk paths needing an item the player has not
        // got, and report entries the crawl cannot reach - a surplus that says nothing
        // about the encoding.
        let seed = lookahead_engine::symbolic::reachability::seed_of(&graph, &world, &vars);
        let found =
            Reachability::explore(&graph, start, &seed, &mut compiler, &world, COUNTER_CAP as u32);
        let symbolic: HashSet<DialogueNodeId> = found.entries().collect();

        let missed: Vec<&DialogueNodeId> = walked.difference(&symbolic).collect();
        let surplus = symbolic.difference(&walked).count();
        let stats = found.stats();

        println!(
            "{conversation:>6} {:>8} {:>9} {:>9} {:>8} {:>9} {:>7}",
            graph.count(),
            walked.len(),
            symbolic.len(),
            surplus,
            stats.diagram_nodes,
            stats.steps,
        );

        // A surplus is expected, but it should be explainable rather than mysterious.
        // The two known sources are an undecided guard let through and a cost check that
        // cannot be refused because money is not in the layout - and with a save holding
        // no money at all, the second is the one that bites: the crawl declines every
        // priced option and the symbolic search takes them all.
        if surplus > 0 {
            println!(
                "         {surplus} extra: {} guard fallbacks, {} cost checks undecidable",
                compiler.fallbacks(),
                stats.unaffordable_unknown,
            );
        }

        assert!(
            missed.is_empty(),
            "conversation {conversation}: the symbolic search MISSED {} entries the crawl \
             reached, which is the direction it is never allowed to be wrong in: {:?}",
            missed.len(),
            missed.iter().take(10).collect::<Vec<_>>(),
        );

        compared += 1;
    }

    assert!(compared > 0, "no conversation could be compared both ways");
}

/// The shape that actually costs: everything seen but one entry - can it be reached?
///
/// This is the question the look-ahead asks and the one the explicit crawl cannot answer
/// on the big groups. It burns its 200,000-state budget in about half a second and
/// returns nothing useful, because with nothing novel to find it has no reason to stop
/// early and simply enumerates until it is cut off.
///
/// Against a fresh save the question is trivial - everything is unseen, so the first
/// entry reached answers it - which is why the measurement has to be built the other way
/// round. Here one entry deep in the group is the only unseen one, and both engines are
/// asked whether it is reachable.
///
/// A slow answer beats a budget exhaustion that returns nothing, so the bar the symbolic
/// side has to clear is low: ANSWER AT ALL.
///
/// ## What it says on equal terms, 2026-09-04
///
/// Both engines given 256 MB and 60 seconds - and the memory really equal, which took two
/// goes: the diagram manager PREALLOCATES its node capacity and will not grow past it, so
/// the hand-picked 2^22 that stood here was a hard ceiling of about 134 MB, half what the
/// explicit crawl had. Derived from the budget instead:
///
/// ```text
///   conv  entries    vars     engine        ms  symbolic       ms
///    368     4724     263    gave up       394   gave up    60002
///    631     4514     331    gave up       381   gave up    60016
///     14     3594     250    gave up       411   NO ROOM    47972
///     28     2186     160    gave up       515     FOUND       50
///   1030     1476      92  not there         0 not there        0
///    362     1860     123    gave up       671   gave up    60000
/// ```
///
/// CONVERSATION 28 IS THE CASE FOR THE BACKWARD SEARCH, and the only one here: it answers
/// in 50 milliseconds a question the explicit crawl spends 515 giving up on. That is the
/// whole shape of the argument - a representation that shares structure can settle a
/// question an enumeration cannot reach - and it is now one measured instance rather than a
/// hope.
///
/// EVERYWHERE ELSE THE BACKWARD SEARCH IS WORSE, and ONE conversation fails for a different
/// reason than the rest: 14 fills the 256 MB and stops, while 368, 631 and 362 spend the
/// whole minute without either filling the budget or answering. A representation that does
/// not fit and one that is merely slow want different things done about them.
///
/// THAT DISTINCTION IS THE THING THE UNEQUAL CEILING WAS HIDING. At 134 MB, 631 also read
/// NO ROOM, and the earlier version of this note concluded that two conversations did not
/// fit. Given the room the explicit crawl gets, 631 turns out to fit and to be slow. One
/// artefact, one real result, and no way to tell them apart without giving both sides the
/// same allowance.
///
/// 1030 answers instantly on both sides because its whole reachable space is a few hundred
/// states - it is the control rather than a result.
#[test]
#[ignore = "a long measurement, not a test: run it with --ignored --release"]
fn finding_one_unseen_entry_in_a_group_that_is_otherwise_seen() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>7} {:>10} {:>9} {:>9} {:>8}",
        "conv", "entries", "vars", "engine", "ms", "symbolic", "ms"
    );

    for conversation in conversations(&EXPENSIVE) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        // The quarry has to be STRUCTURALLY REACHABLE or the question is a trick: an
        // entry no path leads to is unreachable whatever the guards say, and both engines
        // answering "not there" would be measuring nothing.
        //
        // The first attempt took the last entry the builder produced, which is an
        // arbitrary position in a hash-ordered walk and has no relation to the links at
        // all. Taking the DEEPEST structurally reachable entry instead makes the question
        // both fair and as hard as the group allows.
        let depths = structurally_reachable(&graph, start);
        // Deepest wins; the conversation and entry ids break ties so the choice is stable
        // across runs rather than hash-ordered.
        let Some((&quarry, &depth)) = depths
            .iter()
            .max_by_key(|(id, depth)| (**depth, id.conversation_id, id.entry_id))
        else {
            continue;
        };
        let novelty = move |id: DialogueNodeId| {
            if id == quarry { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
        };

        // The explicit crawl, asked exactly this.
        //
        // THE SAME ALLOWANCE THE SYMBOLIC SIDE GETS BELOW: the same bytes and the same
        // seconds. That is the only way the two verdicts in the table mean anything against
        // each other - a state count and a step count are not comparable quantities, and
        // holding one side to 200,000 states while the other ran unlimited was comparing
        // two searches given different amounts of room (de-e23q).
        let began = std::time::Instant::now();
        let explicit_answer = LookAheadEngine::new(LookAheadOptions {
            state_budget: usize::MAX,
            memory_budget: COMPARISON_MEMORY,
            time_budget: COMPARISON_TIME,
            counter_cap: COUNTER_CAP,
            ..Default::default()
        })
        .evaluate(&graph, start, &world, novelty);
        let explicit_ms = began.elapsed().as_millis();

        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false)
            .keeping_only_read(&symbols, &DataLayout::read_by(&graph));
        let vars = DataVars::new(&layout, &symbols, NODE_CAPACITY, CACHE_CAPACITY);
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(&graph));

        let seed = lookahead_engine::symbolic::reachability::seed_of(&graph, &world, &vars);
        let budget = Budget {
            // No step limit, because steps are not what is being rationed here.
            steps: usize::MAX,
            time: COMPARISON_TIME,
            memory: COMPARISON_MEMORY,
            report_every: 20_000,
            on_progress: None,
            // Stop the moment the quarry is reached - the whole point of the exercise.
            halt_on: Some(Box::new(move |id| id == quarry)),
        };

        let found = Reachability::explore_within(
            &graph, start, &seed, &mut compiler, &world, COUNTER_CAP as u32, &budget,
        );
        let stats = found.stats();

        let explicit_says = if explicit_answer.best == Novelty::UnseenAnyGame {
            "FOUND"
        } else if explicit_answer.budget_exhausted() {
            "gave up"
        } else {
            "not there"
        };
        let symbolic_says = if stats.halted_at.is_some() {
            "FOUND"
        } else if stats.out_of_memory {
            "NO ROOM"
        } else if stats.reached_fixed_point {
            "not there"
        } else {
            "gave up"
        };

        println!(
            "{conversation:>6} {:>8} {:>7} {explicit_says:>10} {explicit_ms:>9} \
             {symbolic_says:>9} {:>8}",
            graph.count(),
            layout.total_vars(),
            stats.elapsed.as_millis(),
        );
        println!(
            "         quarry {quarry} at link depth {depth}; {} of {} entries are \
             reachable by links alone",
            depths.len(),
            graph.count(),
        );

        // Both engines answering the same question must not contradict each other. The
        // symbolic side may say FOUND where the crawl gave up - that is the whole hope -
        // but if the crawl found it and the symbolic search says it is not there, the
        // symbolic search has missed a reachable entry, which is the one unforgivable
        // error.
        if explicit_says == "FOUND" {
            assert!(
                symbolic_says != "not there",
                "conversation {conversation}: the crawl reached {quarry} and the symbolic \
                 search reported it unreachable",
            );
        }
    }
}

/// What does the COMPLETE reachable set cost, for the conversations that drive the cost?
///
/// The question de-sze was opened to answer and the one that could not be asked before.
/// 631 and 14 are precisely the groups the explicit crawl cannot exhaust - it burns its
/// budget and returns nothing useful - so their complete sets had never been measured,
/// and could not be by enumerating states, because enumerating them is the thing that is
/// too expensive.
///
/// Symbolic reachability obtains one without enumerating it, so the number here is the
/// first honest answer to "what would holding all of it cost".
/// Run it deliberately: `cargo test --release --test symbolic_reachability -- --ignored
/// --nocapture --test-threads=1`.
///
/// Ignored by default because it is a MEASUREMENT rather than a test - it asserts nothing
/// and answers a question - and because it does not finish. The budget bounds the step
/// count and the wall clock between steps, but a single step late in a run can take
/// minutes on its own, so the cap is not a real ceiling. In a debug build it is worse
/// again; release is not optional here.
#[test]
#[ignore = "a long measurement, not a test: run it with --ignored --release"]
fn what_the_expensive_conversations_cost() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    println!(
        "{:>6} {:>8} {:>7} {:>9} {:>9} {:>9} {:>8} {:>7}",
        "conv", "entries", "vars", "reached", "bddnodes", "largest", "steps", "ms"
    );

    for conversation in conversations(&EXPENSIVE) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let symbols = graph.symbols().clone();
        let full = DataLayout::for_graph(&graph, COUNTER_CAP, None, false);
        // Drop every slot no guard in the group reads. Exact, not an approximation: a
        // group is closed under links, so a slot nothing in it reads cannot change which
        // entries are reachable however much the actions write to it.
        let layout = full.clone().keeping_only_read(&symbols, &DataLayout::read_by(&graph));
        println!(
            "         {conversation}: {} variables of {} carry anything a guard reads",
            layout.total_vars(),
            full.total_vars(),
        );
        let vars = DataVars::new(&layout, &symbols, NODE_CAPACITY, CACHE_CAPACITY);
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(&graph));

        let seed = lookahead_engine::symbolic::reachability::seed_of(&graph, &world, &vars);
        let budget = Budget {
            steps: 500_000,
            time: std::time::Duration::from_secs(120),
            memory: 0,
            report_every: 5_000,
            on_progress: Some(Box::new(move |steps, reached, held, largest| {
                println!(
                    "         ... {conversation}: {steps} steps, {reached} entries, \
                     {held} diagram nodes, largest set {largest}"
                );
            })),
            // This measurement wants the whole fixed point, so it stops for nothing.
            halt_on: None,
        };

        let found = Reachability::explore_within(
            &graph, start, &seed, &mut compiler, &world, COUNTER_CAP as u32, &budget,
        );
        let stats = found.stats();

        println!(
            "{conversation:>6} {:>8} {:>7} {:>9} {:>9} {:>9} {:>8} {:>7}  {}",
            graph.count(),
            layout.total_vars(),
            stats.entries_reached,
            stats.diagram_nodes,
            stats.largest_set,
            stats.steps,
            stats.elapsed.as_millis(),
            // The field that decides whether the rest of the row is an answer or a lower
            // bound on one.
            if stats.reached_fixed_point { "complete" } else { "OUT OF BUDGET" },
        );
        println!(
            "         guards {} compiled / {} fell back; {} cost checks undecidable, \
             {} actions ignored",
            compiler.compiled(),
            compiler.fallbacks(),
            stats.unaffordable_unknown,
            stats.actions_ignored,
        );
    }
}
