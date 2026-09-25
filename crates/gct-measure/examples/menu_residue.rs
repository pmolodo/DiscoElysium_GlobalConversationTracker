// SPDX-License-Identifier: MIT
//! Does a MENU'S worth of searches survive the one thread the bridge gives them?
//!
//! ## The question, and why it is about the shipped path rather than the matrix
//!
//! de-8hh2.13 found the fault behind de-fpax: something accumulates PER THREAD inside the
//! diagram manager, and two searches' worth of it is enough to make a third overflow the
//! stack on conversation 28. The fix was one search, one thread -
//! [`lookahead_engine::symbolic::isolated`].
//!
//! `bridge::answer` takes that thread ONCE PER REQUEST and then runs every start on it. Its
//! own comment says so and gives the reason - the layout, the variables, the compiled guards
//! and the seed are facts about the GROUP, and a menu asks about a dozen options in it - so
//! a thread per start would rebuild all of that a dozen times.
//!
//! But a response menu is a dozen options, a rolled check is TWO starts, and de-8hh2.13's
//! number is three. If the accumulation is per thread and a request puts a dozen searches on
//! one, then the arrangement that fixed the measurements did not fix the game: it moved the
//! same pile from the process's main thread onto a fresh one, where a big enough menu fills
//! it just the same.
//!
//! That is what this asks, and it asks it through `bridge::answer` rather than through the
//! engines underneath, because the question is about the request the game actually makes.
//!
//! ## What it answered: NO, and for a reason that is not luck
//!
//! Forty-five runs - budgets of 256 MB, 1 GB and 6 GB, three, eight and twenty-four starts -
//! and none of them died; nor did one run at 384 starts, 146 of which paid for a real
//! search, which finished in 3.9 s. The reason is in `search_residue`'s table: what
//! accumulates is a SECOND MANAGER built on a thread, not a second search, and
//! `bridge::answer` builds exactly one inside its request thread. So the fault is out of
//! reach of a menu however long the menu is, and one thread per request is right as it
//! stands.
//!
//! `price` is the arm that says what the alternative would have cost, since rejecting it on
//! a guess would have been the same mistake in the other direction.
//!
//! ## How to read it
//!
//! ```text
//! cargo run --release --example menu_residue -- one-request
//! cargo run --release --example menu_residue -- one-each
//! ```
//!
//! THE SAME SEARCHES BOTH TIMES; only how many threads they are spread over differs.
//!
//! - `one-request` puts every start in ONE call, so they share one thread - what the game
//!   does with a menu.
//! - `one-each` makes one call per start, so each gets a thread of its own - what the
//!   measurements do since de-8hh2.13, and the arrangement known to survive.
//!
//! A run that dies takes the process with it - STATUS_STACK_OVERFLOW is not a panic and
//! there is nothing to catch - which is why the two arms are separate invocations rather
//! than two loops in one `main`. The arm that dies prints the start it died on; the arm that
//! lives prints every start and a closing line, so "no closing line" IS the failure.
//!
//! ## The profile
//!
//! ADVERSARIAL, and the same shape as the matrix's `deepest-1`: exactly one entry - the
//! structurally deepest reachable from the start - is unseen in any game, and everything
//! else is seen. Every start that can reach it therefore has something better than its own
//! class beyond it, so `class_worth_hunting` refuses none of them and every start pays for a
//! real search. A percentage profile would refuse most of them and measure nothing.
//!
//! The depth walk here is the matrix's `candidates` rule restated: reachable, not the start,
//! not a group. Restated rather than shared because an example cannot import another
//! example's helpers, and the rule is six lines.

use std::collections::{HashMap, HashSet, VecDeque};

use clap::ValueEnum;
use lookahead_engine::bridge::{GameWorld, LookAheadRequest, NodeRef, WorldRawData, answer};
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;

use gct_measure::common;

use gct_measure::options;
use gct_measure::plugin_defaults;

/// The conversation de-fpax dies on, which is the whole point of this measurement and so is
/// fixed rather than asked for.
const CONVERSATION: i32 = 28;

/// The counter cap every symbolic measurement in this repository uses.
const COUNTER_CAP: i32 = 16;

/// How many of the deepest entries are left unseen. `--unseen` moves it.
///
/// The matrix's adversarial profiles are `deepest-1`, `deepest-5` and `deepest-10`, and its
/// third row - the one that overflowed - is the last of those.
const UNSEEN: usize = 10;

/// What the request asks for as its time budget, in milliseconds. `--time-budget-ms` moves it.
///
/// WHAT A PLAYER GETS, the shipped per-option budget. Zero is no limit, as it is on the
/// plugin's own setting.
const TIME_BUDGET_MS: u64 = plugin_defaults::TIME_BUDGET_MS;

/// How many starts to ask about, which is a generous menu rather than a typical one.
///
/// A response menu is usually a handful and de-8hh2.13's number is THREE, so anything past
/// three is already past the point the fault appeared. Twenty-four is what a menu of twelve
/// costs when every option is a rolled check, since a check is two starts - the worst a real
/// menu can be rather than a number chosen to break something.
const STARTS: usize = 24;

/// The request's memory budget in megabytes. `--memory-budget-mb` moves it.
///
/// SIX GIGABYTES, the budget every matrix row is measured at, because the row that
/// overflowed was measured here and a probe at a smaller one would be asking about a
/// different search. What a PLAYER gets by default is 256 -
/// `DiagramBudget::DEFAULT_MEMORY_BUDGET` - which is the other number worth running at.
///
const MEMORY_BUDGET_MB: usize = 6 * 1024;

/// Which arrangement a run measures.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Arm {
    /// Every start in one call, sharing one thread - what a menu does
    OneRequest,
    /// One call per start, a thread each - the arrangement known to survive
    OneEach,
    /// What a thread per start would COST: the diagram side rebuilt, once per start
    Price,
}

/// What this driver takes.
#[derive(clap::Parser)]
#[command(about = "What a menu leaves behind, under each arrangement of its starts.")]
struct Options {
    /// Which arrangement to measure
    #[arg(value_enum)]
    arm: Arm,
    #[command(flatten)]
    starts: options::Starts<STARTS>,
    #[command(flatten)]
    unseen: options::Unseen<UNSEEN>,
    /// What the whole menu is allowed, in milliseconds; 0 is no wall
    #[arg(long = "time-budget-ms", value_name = "MS", default_value_t = TIME_BUDGET_MS)]
    time_budget_ms: u64,
    /// What the diagram manager may commit, in megabytes
    #[arg(long = "memory-budget-mb", value_name = "MB", default_value_t = MEMORY_BUDGET_MB)]
    memory_budget_mb: usize,
}

fn main() {
    let asked = <Options as clap::Parser>::parse();
    let arm = asked.arm;

    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");
    let Ok((graph, _)) = build_group_graph(&index, CONVERSATION) else {
        eprintln!("conversation {CONVERSATION}'s group does not build; skipping.");
        return;
    };

    let root = DialogueNodeId::new(CONVERSATION, 0);
    if graph.get(root).is_none() {
        eprintln!("no entry 0 in conversation {CONVERSATION}; skipping.");
        return;
    }

    let ranked = deepest_first(&graph, root);
    let unseen_count = asked.unseen.unseen;
    let unseen: Vec<NodeRef> = ranked
        .iter()
        .take(unseen_count)
        .map(|id| NodeRef::from(*id))
        .collect();
    if unseen.is_empty() {
        eprintln!("nothing reachable from {root:?}; skipping.");
        return;
    }

    // STARTS THAT CAN ACTUALLY REACH SOMETHING UNSEEN, and this is not a detail: a start
    // with nothing better beyond it is REFUSED before a diagram is touched -
    // `bridge::class_worth_hunting` - and a run made of refusals survives everything while
    // measuring nothing. The first cut of this probe took the shallowest entries in the
    // group and got twenty-four refusals and zero candidates, which reads in the closing
    // line exactly like a clean run.
    //
    // Shallowest FIRST among those that qualify, because that is what an option in a menu
    // is: an entry with the group's depth still in front of it.
    let reaching = can_reach(&graph, &unseen);
    let wanted = asked.starts.starts;
    let starts: Vec<NodeRef> = ranked
        .iter()
        .rev()
        .filter(|id| reaching.contains(*id))
        .map(|id| NodeRef::from(*id))
        .filter(|start| !unseen.contains(start))
        .take(wanted)
        .collect();
    assert!(
        !starts.is_empty(),
        "nothing in conversation {CONVERSATION}'s group reaches its {} deepest entries, so \
         every start would be refused and the run would measure nothing",
        unseen.len(),
    );

    let time_budget_ms = asked.time_budget_ms;
    let memory_budget_mb = asked.memory_budget_mb;

    println!(
        "conversation {CONVERSATION}: {} entries, {} starts, {} deepest unseen, \
         {memory_budget_mb} MB, {time_budget_ms} ms",
        graph.nodes().count(),
        starts.len(),
        unseen.len(),
    );
    println!(
        "arm: {}\n",
        arm.to_possible_value()
            .expect("every arm is a named value")
            .get_name()
    );

    // WITH A WALK, as the product asks, to whichever options the call carries - see
    // `hub::walk_to_menu`. Every request is built before the clock starts, since the walk
    // stands in for what the plugin records as a conversation plays.
    let ask = |starts: Vec<NodeRef>| LookAheadRequest {
        conversation: CONVERSATION,
        encountered: lookahead_engine::symbolic::hub::walk_to_menu(
            &graph,
            CONVERSATION,
            &starts
                .iter()
                .map(|start| DialogueNodeId::from(*start))
                .collect::<Vec<_>>(),
        )
        .into_iter()
        .map(NodeRef::from)
        .collect(),
        starts,
        // WHAT SOME PLAYTHROUGH SHOWED, which is everything the profile does not call unseen
        // anywhere - see `world::seen_state`.
        seen_any_game: graph
            .nodes()
            .map(|node| NodeRef::from(node.id))
            .filter(|node| !unseen.contains(node))
            .collect(),
        memory_budget_mb,
        time_budget_ms,
        world: WorldRawData {
            day_minutes: 720,
            day_counter: 1,
            ..Default::default()
        },
        ..Default::default()
    };

    let whole_menu = ask(starts.clone());
    let one_each: Vec<LookAheadRequest> = starts.iter().map(|start| ask(vec![*start])).collect();

    let began = std::time::Instant::now();
    let mut work = Work::default();

    if arm == Arm::Price {
        // WHAT A THREAD PER START WOULD COST, and it is NOT what `one-each` measures: that
        // arm goes through `bridge::answer`, which rebuilds the whole group GRAPH per call -
        // the guard and action parse for every entry - where a thread per start inside one
        // request would keep the graph and rebuild only the diagram side. So this times the
        // diagram side alone: the manager, the compiled guards and the seed, which is
        // everything `symbolic::isolated` says has to be built inside the thread.
        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(&graph, COUNTER_CAP, None, false)
            .keeping_only_read(&symbols, &DataLayout::read_by(&graph));
        let world = GameWorld::declaring(
            WorldRawData {
                day_minutes: 720,
                day_counter: 1,
                ..Default::default()
            },
            common::declared(),
        );
        let budget = DiagramBudget::new(memory_budget_mb * 1024 * 1024);

        println!(
            "rebuilding the diagram side {} times, {memory_budget_mb} MB each",
            starts.len(),
        );
        for n in 1..=starts.len() {
            let each = std::time::Instant::now();
            {
                let vars = DataVars::new(&layout, &symbols, budget);
                let compiler = GuardCompiler::new(&vars)
                    .with_world(&world)
                    .with_constant_clock(DataLayout::group_passes_time(&graph));
                let seed = seed_of(&graph, &world, &vars).expect("room for a seed");
                std::hint::black_box((&compiler, &seed));
            }
            println!(
                "  {n:>3}: {:>8.1} ms",
                each.elapsed().as_secs_f64() * 1000.0
            );
            flush();
        }

        println!(
            "\nA THREAD PER START would add this much to a menu of {} - {:.1}s in all.",
            starts.len(),
            began.elapsed().as_secs_f64(),
        );
        return;
    }

    if arm == Arm::OneRequest {
        // FLUSHED BEFORE THE CALL, because the call may not return: a stack overflow is not
        // a panic and nothing after it runs, so a line buffered here would be lost with the
        // process and the log would not say how far it got.
        println!("asking about all {} starts in one call...", starts.len());
        flush();

        let response = answer(&index, common::declared(), None, &whole_menu);
        assert!(response.error.is_none(), "{:?}", response.error);
        work.add(&response.answers);
    } else {
        for (n, (start, request)) in starts.iter().zip(&one_each).enumerate() {
            println!("  start {:>2}/{}: {start:?}", n + 1, starts.len());
            flush();

            let response = answer(&index, common::declared(), None, request);
            assert!(response.error.is_none(), "{start:?}: {:?}", response.error);
            work.add(&response.answers);
        }
    }

    // WHAT THE ANSWERS COST, and it is the first thing to read: an arm whose searches were
    // all REFUSED has survived nothing, because the refusal happens before a diagram is
    // touched. `class_worth_hunting` refuses a start with nothing better beyond it, and a
    // profile that leaves nothing to hunt refuses every one of them - which looks exactly
    // like a clean run in the closing line and is not one.
    println!(
        "\nanswers {}, of which {} asked a candidate; {} candidates asked in all",
        work.answers, work.searched, work.asked,
    );
    println!("stopped by: {}", work.stops());
    println!(
        "SURVIVED: {:.1}s, and the process is still here.",
        began.elapsed().as_secs_f64(),
    );
}

/// What a run's answers add up to, so a run that measured nothing says so.
#[derive(Default)]
struct Work {
    answers: usize,
    /// How many answers cost at least one candidate - the ones that actually searched.
    searched: usize,
    asked: usize,
    stopped: std::collections::BTreeMap<String, usize>,
}

impl Work {
    fn add(&mut self, answers: &[lookahead_engine::bridge::LookAheadAnswer]) {
        for found in answers {
            self.answers += 1;
            self.asked += found.nodes_reached;
            if found.nodes_reached > 0 {
                self.searched += 1;
            }
            *self.stopped.entry(found.stopped_by.clone()).or_default() += 1;
        }
    }

    fn stops(&self) -> String {
        self.stopped
            .iter()
            .map(|(why, n)| format!("{why} {n}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn flush() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

/// Every entry from which at least one of `targets` is link-reachable.
///
/// ONE WALK BACKWARDS rather than a forward walk per candidate: the question is asked of
/// every entry in the group, and 2,186 forward closures would be the probe's slowest part
/// by far. The links are followed in reverse, seeded with the targets themselves.
///
/// LINK-REACHABLE, which is deliberately the same approximation `class_worth_hunting` makes -
/// guards ignored, structure only. A start this says can reach a target is a start the
/// bridge will not refuse, which is the whole point of asking.
fn can_reach(graph: &LookAheadGraph, targets: &[NodeRef]) -> HashSet<DialogueNodeId> {
    let mut parents: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
    for node in graph.nodes() {
        for child in node.links.iter() {
            parents.entry(*child).or_default().push(node.id);
        }
    }

    let mut reaching: HashSet<DialogueNodeId> = HashSet::new();
    let mut queue: VecDeque<DialogueNodeId> = VecDeque::new();
    for target in targets {
        let id = DialogueNodeId::from(*target);
        if reaching.insert(id) {
            queue.push_back(id);
        }
    }

    while let Some(id) = queue.pop_front() {
        let Some(above) = parents.get(&id) else {
            continue;
        };
        for parent in above {
            if reaching.insert(*parent) {
                queue.push_back(*parent);
            }
        }
    }

    reaching
}

/// Every non-group entry reachable from `start`, deepest first, ties broken by id.
///
/// The matrix's `candidates` rule - see the module note on why it is restated rather than
/// shared. Sorted so a run repeats exactly on any machine.
fn deepest_first(graph: &LookAheadGraph, start: DialogueNodeId) -> Vec<DialogueNodeId> {
    let mut depth: HashMap<DialogueNodeId, usize> = HashMap::new();
    let mut queue: VecDeque<DialogueNodeId> = VecDeque::new();
    let mut seen: HashSet<DialogueNodeId> = HashSet::new();

    depth.insert(start, 0);
    seen.insert(start);
    queue.push_back(start);

    while let Some(id) = queue.pop_front() {
        let here = depth[&id];
        let Some(node) = graph.get(id) else { continue };
        for child in node.links.iter() {
            if seen.insert(*child) {
                depth.insert(*child, here + 1);
                queue.push_back(*child);
            }
        }
    }

    let mut all: Vec<(DialogueNodeId, usize)> = depth
        .into_iter()
        .filter(|(id, _)| *id != start)
        .filter(|(id, _)| graph.get(*id).is_some_and(|node| !node.is_group))
        .collect();

    all.sort_unstable_by_key(|(id, depth)| {
        (std::cmp::Reverse(*depth), id.conversation_id, id.entry_id)
    });
    all.into_iter().map(|(id, _)| id).collect()
}
