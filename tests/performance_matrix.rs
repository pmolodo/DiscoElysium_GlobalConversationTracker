// SPDX-License-Identifier: MIT
//! Both engines, six conversations, eleven profiles: the whole grid.
//!
//! The measurements this repository already has each ask one question well. This asks the
//! same question of every combination, because the thing that is actually wanted - a rule
//! for choosing FORWARD OR BACKWARD per option (de-a1wb) - cannot be drawn from a handful of
//! points. It needs a surface.
//!
//! ## The grid
//!
//! Six conversations - the five heaviest plus 362, the largest in the game - against eleven
//! profiles describing how much of the group the player has read:
//!
//! - everything seen, which the no-improvement shortcut should refuse outright;
//! - the deepest 1, 5 and 10 entries unseen, which are the deliberately hard cases;
//! - 95, 90, 75, 50, 25, 10 and 5 per cent seen, drawn at random, which are the shapes a
//!   real save actually has.
//!
//! ## Why "deepest" for the small counts and "random" for the percentages
//!
//! They are asking different things and the difference is the point.
//!
//! DEEPEST IS THE ADVERSARIAL CASE. An entry at the end of the longest chain is the one the
//! search reaches last, so seeding it and nothing else poses the hardest question the group
//! can pose. Depth here is by EDGE ANALYSIS ALONE - links followed, guards ignored - which
//! makes it an upper bound on reachability: an entry it cannot find is unreachable for
//! certain, so a quarry drawn from it is at least structurally fair.
//!
//! What it is NOT is a question the search can necessarily answer. Entries that deep are
//! often ones the guards shut, so these rows frequently read "explore everything, find
//! nothing" - and that is exactly the worst case for cost, which is what these rows are for.
//! A separate measurement, tests/unseen_falloff.rs, exists for the falloff CURVE and seeds
//! by reach order instead, because a flat "found nothing" series measures nothing about
//! falloff.
//!
//! RANDOM IS THE TYPICAL CASE. A save does not read a conversation depth-first; it reads
//! whatever the conversation led it to. Drawing uniformly from the structurally reachable
//! entries is the closest thing to a real profile that needs no real profile.
//!
//! The seed is the percentage, so a row is reproducible and two rows are not accidentally
//! the same draw.
//!
//! ## What the all-seen row is NOT
//!
//! It is not what the mod costs on a fully-read save. This calls `evaluate` directly, and
//! the no-improvement shortcut that makes that case free lives one level up, in the bridge's
//! `answer` - so these rows show the raw engine being asked a question the mod would never
//! put to it, and the numbers are large. On conversation 28 the row reads 222,400 states and
//! half a second; through the bridge the same profile costs zero states.
//!
//! Kept anyway, and deliberately: it is the CEILING for the group. Every other row on the
//! same conversation is a search that can stop early, so the all-seen row says how much
//! there was to explore in the first place, which is what makes the rest legible as
//! fractions of something.
//!
//! ## What the whole grid said at 256 MB, 2026-09-04
//!
//! SUPERSEDED, AND KEPT AS HISTORY. This run held both engines to 256 MB and 60 seconds -
//! the mod's shipping allowance - and de-e33h is the finding that this measures the ration
//! rather than the algorithm: most heavy rows end in gave-up or no-room having been stopped
//! by the ceiling. The measurement now runs at the shared six-gigabyte budget above, with a
//! cap that is meant not to fire. Every number below is from the old setting and should be
//! read as what a 256 MB ration does, not as what these searches cost.
//!
//! The adversarial rows - everything read, or all but the one, five or ten structurally
//! deepest entries - behave identically within a conversation, so one line stands for all
//! four:
//!
//! ```text
//!   conv  entries   forward                    backward
//!    368     4724   gave-up   400ms  170,870   gave-up   64s   7.5M nodes
//!    631     4514   gave-up   370ms  112,506   gave-up   65s   7.4M nodes
//!     14     3594   gave-up   390ms  133,089   NO ROOM   55s   8.4M nodes (the budget)
//!    362     1860   gave-up   660ms  377,017   gave-up   62s   2.1M nodes
//!     28     2186   gave-up   490ms  222,400   FOUND     50ms  180K nodes
//!   1030     1476   not-there   0ms      410   not-there  6ms  6.2K nodes
//! ```
//!
//! And every random profile, on every conversation, at every percentage:
//!
//! ```text
//!   forward: found, 0ms, 2-100 states       backward: found, 5-15ms, 30-55K nodes
//! ```
//!
//! ### The forward crawl wins almost everywhere, and it is not close
//!
//! On the profiles a real save actually has - any of the random percentages - the forward
//! crawl answers in under a millisecond and the backward one takes five to fifteen. Not a
//! disaster in either case, but there is no argument for the backward search there: it is
//! slower on every single row.
//!
//! On the adversarial profiles the forward crawl gives up in about four hundred
//! milliseconds and the backward one spends A MINUTE to give up as well. Five of the six
//! conversations end that way.
//!
//! ### Conversation 28 is the exception, and the whole case
//!
//! It ANSWERS where the forward crawl cannot: 50 milliseconds against 490 spent giving up.
//! That is what the backward search is for, and it is one conversation in six. Anything that
//! decides between the two engines per option (de-a1wb) has to find the 28-shaped groups
//! cheaply, because guessing wrong costs a minute.
//!
//! ### The verdicts changed once the two were really given the same room
//!
//! Worth recording because it was nearly missed. The manager PREALLOCATES its node capacity
//! and refuses to grow past it, and that capacity was a hand-picked 2^22 - about 134 MB,
//! half the forward crawl's allowance. On that setting 631 and 14 both read NO ROOM.
//!
//! Derive the capacity from the budget instead and they separate: 631 runs out of TIME at
//! 6.4 million nodes, and only 14 genuinely fails to fit, stopping at exactly the 8,388,608
//! nodes the budget allows. One of those is a search that is too slow and the other is a
//! representation that does not fit, and the earlier setting reported both as the second.
//!
//! ### The adversarial rows are all the same row
//!
//! Within a conversation, all-seen and deepest-1, -5 and -10 cost the forward crawl exactly
//! the same number of states - 170,870 on 368, four times over. The deepest entries by edge
//! analysis are the ones the guards shut, so seeding them changes nothing the search can
//! find and it explores the whole space regardless. That is the correct worst case and it is
//! what these rows are for; it is not a falloff curve, and tests/unseen_falloff.rs exists
//! because measuring one needs a different seeding entirely.
//!
//! ## Running it
//!
//! One conversation per process, because a diagram manager that runs out of nodes takes the
//! whole process with it and a crash in the fourth row should not cost the other five:
//!
//!     CONVERSATION=368 cargo test --release --test performance_matrix -- --ignored --nocapture
//!
//! The rows are printed as TAB-SEPARATED VALUES with a stable header, so a run can be piped
//! straight into a file and read by something else later - which is what de-raed asks for
//! when it says the logs should be kept for analysis.

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::state::StateSymbols;
use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::reachability::{seed_of, Budget, Reachability};
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::symbolic::budget::DiagramBudget;

mod common;

/// The six heaviest groups, 362 included.
const HEAVIEST: [i32; 6] = [362, 368, 631, 14, 28, 1030];

/// The same allowance for both engines, so the two columns can be read against each other.
///
/// THE MEASUREMENT ALLOWANCE, NOT THE SHIPPED ONE. This used to be
/// `DEFAULT_MEMORY_BUDGET`, which is a product decision about what a player's machine
/// should give a response menu - a fine ceiling to ship and the wrong one to measure
/// against, because a row that says "no room" then reports the ration rather than the
/// algorithm. What is wanted here is where the search actually stops, so it gets the
/// shared measurement budget and the shipped default is left alone.
///
/// Both engines take it: the crawl in bytes directly, the diagram through
/// [`DiagramBudget`], which turns it into a node capacity and a cache capacity. A hand-
/// picked capacity is what made this unequal before - 2^22 nodes is a hard ceiling of
/// about 134 MB, half what the crawl was allowed, and conversations 631 and 14 reported
/// "no room" at exactly 4,194,304 nodes, which was that ceiling and not the budget.
/// Override with `ROW_MEMORY_MB`, for the one conversation that wants more than the rest.
///
/// A run reported as a measurement should say which allowance it used, the same way it
/// should say which time cap - two rows given different budgets are not comparable, and
/// nothing in a TSV records the budget.
fn memory() -> usize {
    std::env::var("ROW_MEMORY_MB")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(|mb| mb * 1024 * 1024)
        .unwrap_or_else(|| DiagramBudget::measurement().memory())
}

fn budget() -> DiagramBudget {
    DiagramBudget::new(memory())
}

/// How long one engine may spend on one row.
///
/// TEN MINUTES, not the sixty seconds this used to be, and the reason is the budget above.
/// The matrix exists to find where a search actually stops and to answer "how long until
/// every scenario has a CONCRETE answer" - found or not-there rather than gave-up. A cap
/// that fires first answers neither: the row says `gave-up` having never come near
/// spending the memory, and raising the memory to six gigabytes buys nothing at all.
///
/// So the cap is meant to be the thing that does NOT stop a row, and it is here only
/// because a row that will never finish still has to end. Where it fires, the row's own
/// wall time says so and the verdict is `gave-up` as before.
///
/// Override with `ROW_SECONDS` to bound a run that has to fit in an afternoon; a run
/// reported as a measurement should say which cap it used.
const DEFAULT_ROW_SECONDS: u64 = 600;

fn row_time() -> std::time::Duration {
    let seconds = std::env::var("ROW_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(DEFAULT_ROW_SECONDS);
    std::time::Duration::from_secs(seconds)
}

/// How often a row should say where it has got to, or None to say nothing until it ends.
///
/// ## Why a row needs this at all
///
/// Because a heavy row is half an hour of silence. Both engines already had the hook - the
/// crawl reports on a clock, the fixed point every so many steps - and both were passed
/// None here, so a run that took an hour and twenty-seven minutes printed sixty-six lines
/// and nothing in between. There is no percentage to give: neither engine knows how much
/// is left, only how much it has spent. So progress is what it HAS spent, which is the
/// number that matters anyway, because spending the budget is how these rows end.
///
/// Off by default: the lines go into the row's log, and a run that is not being watched
/// does not want them.
fn progress_every() -> Option<std::time::Duration> {
    std::env::var("PROGRESS_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(std::time::Duration::from_secs)
}

/// How often the fixed point looks up from its work, against the five seconds it SPEAKS.
///
/// Two rates because they cost differently: looking up is a clock read and a memory
/// question, and is wanted often enough that the budget cannot be overspent by much;
/// gathering the line walks every entry's set for a node count, and is wanted only as often
/// as somebody can read it. The forward crawl needs no equivalent - it is already on a
/// clock of its own.
const CHECK_GAP: std::time::Duration = std::time::Duration::from_secs(1);

/// Bytes as gigabytes, for a line a person reads while waiting.
fn gb(bytes: usize) -> String {
    format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// A duration as m:ss, for the same reason.
fn mmss(elapsed: std::time::Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{}m{:02}s", seconds / 60, seconds % 60)
}

/// The prefix every progress line carries.
///
/// It must NOT start with a conversation number: tools/measure-matrix.sh picks the row out
/// of the log with `grep -E "^$conversation\b"`, and a progress line that matched would be
/// recorded as the row and the real one thrown away.
const PROGRESS: &str = "  ~";

/// The verdict for a row nothing was learned from, in both engines' columns.
///
/// LOUD, and not a word either engine can produce on its own, because the failure it
/// reports is not theirs: the machine could not supply the budget, so the row was never
/// run. A gap or a quiet `gave-up` here would read as a finding about the search.
const NOT_MEASURED: &str = "NOT-MEASURED";

const COUNTER_CAP: i32 = 16;



/// How much of a group a profile has read.
#[derive(Debug, Clone, Copy)]
enum Profile {
    /// Everything seen. Nothing to find, and the shortcut should say so without searching.
    AllSeen,
    /// The n structurally deepest entries unseen: the adversarial case.
    DeepestUnseen(usize),
    /// This percentage of entries seen, the rest unseen, drawn at random: the typical case.
    PercentSeen(u32),
}

impl Profile {
    fn label(self) -> String {
        match self {
            Profile::AllSeen => "all-seen".to_string(),
            Profile::DeepestUnseen(n) => format!("deepest-{n}"),
            Profile::PercentSeen(p) => format!("{p}pc-seen"),
        }
    }
}

const PROFILES: [Profile; 11] = [
    Profile::AllSeen,
    Profile::DeepestUnseen(1),
    Profile::DeepestUnseen(5),
    Profile::DeepestUnseen(10),
    Profile::PercentSeen(95),
    Profile::PercentSeen(90),
    Profile::PercentSeen(75),
    Profile::PercentSeen(50),
    Profile::PercentSeen(25),
    Profile::PercentSeen(10),
    Profile::PercentSeen(5),
];

fn conversations(default: &[i32]) -> Vec<i32> {
    match std::env::var("CONVERSATION") {
        Ok(named) => named.split(',').filter_map(|id| id.trim().parse().ok()).collect(),
        Err(_) => default.to_vec(),
    }
}

/// Which profiles this process should measure, by label.
///
/// ONE ROW PER PROCESS is the intended way to run this, driven by `tools/measure-matrix.sh`.
/// A row can take the whole process down - measured: conversation 28 with its five deepest
/// entries unseen overflows the stack inside a recursive diagram operation - and with every
/// row in one process the first crash destroys every row after it. The 28 run reported two
/// rows out of eleven and lost the rest.
///
/// Answering the whole list when nothing is named keeps the test runnable on its own; the
/// script is what makes the results survivable.
fn profiles() -> Vec<Profile> {
    match std::env::var("PROFILE") {
        Ok(named) => {
            let wanted: Vec<&str> = named.split(',').map(str::trim).collect();
            PROFILES
                .into_iter()
                .filter(|profile| wanted.contains(&profile.label().as_str()))
                .collect()
        }
        Err(_) => PROFILES.to_vec(),
    }
}

/// Which entries are reachable from `start` by following links alone, and how far.
///
/// Guards ignored entirely, which is what de-raed means by "determined by edge analysis
/// alone". It is the loosest notion of reachable there is, and that is why it is the right
/// one for choosing a question: an entry missing from it is unreachable for certain, so
/// seeding it would make a row meaningless.
fn structurally_reachable(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
) -> HashMap<DialogueNodeId, usize> {
    let mut depth = HashMap::new();
    let mut queue = VecDeque::new();
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

/// The entries a profile can be built from: reachable, not the start, and not groups.
///
/// GROUPS ARE EXCLUDED because the game never writes a group's SimStatus, so every group in
/// the database reads as never displayed and the crawl refuses to score one. Seeding a group
/// as unseen would add an entry that cannot end a search, which would quietly make a row
/// harder than it claims to be.
///
/// Returned deepest first, ties broken by id, so a run repeats exactly.
fn candidates(graph: &LookAheadGraph, start: DialogueNodeId) -> Vec<DialogueNodeId> {
    let depths = structurally_reachable(graph, start);
    let mut all: Vec<(DialogueNodeId, usize)> = depths
        .into_iter()
        .filter(|(id, _)| *id != start)
        .filter(|(id, _)| graph.get(*id).is_some_and(|node| !node.is_group))
        .collect();

    all.sort_unstable_by_key(|(id, depth)| {
        (std::cmp::Reverse(*depth), id.conversation_id, id.entry_id)
    });
    all.into_iter().map(|(id, _)| id).collect()
}

/// A small deterministic generator, so a row is the same row on every machine.
///
/// Written out rather than taken from a crate: what is wanted is repeatability across runs
/// and platforms, and a named algorithm with the arithmetic in view gives that without a
/// dependency whose version could change the draw underneath a recorded measurement.
/// This is xorshift64*, which is more than good enough for choosing which entries to mark.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // Zero is a fixed point of xorshift, so it can never be the state.
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

/// The entries a profile leaves unseen.
fn unseen_for(
    profile: Profile,
    candidates: &[DialogueNodeId],
) -> HashSet<DialogueNodeId> {
    match profile {
        Profile::AllSeen => HashSet::new(),
        Profile::DeepestUnseen(n) => candidates.iter().take(n).copied().collect(),
        Profile::PercentSeen(percent) => {
            // The seed IS the percentage, as de-raed asks: reproducible, and different for
            // every row so two rows are not accidentally the same draw.
            let mut rng = Rng::new(percent as u64);
            let mut shuffled = candidates.to_vec();

            // Fisher-Yates, so every subset of the right size is equally likely. Taking the
            // first n of a sorted list after a partial shuffle would not be.
            for i in (1..shuffled.len()).rev() {
                let j = (rng.next() % (i as u64 + 1)) as usize;
                shuffled.swap(i, j);
            }

            let seen = (shuffled.len() * percent as usize) / 100;
            shuffled.into_iter().skip(seen).collect()
        }
    }
}

/// What one engine did with one profile.
struct Row {
    verdict: &'static str,
    millis: u128,
    /// States for the forward crawl; diagram nodes actually held for the backward one.
    size: usize,
    /// The backward search's per-set sum, which is a different and larger number.
    ///
    /// Zero for the forward crawl, which has nothing analogous.
    set_sum: usize,
}

fn forward(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn lookahead_engine::world::world::ILookAheadWorld,
    unseen: &HashSet<DialogueNodeId>,
) -> Row {
    let novelty = |id: DialogueNodeId| {
        if unseen.contains(&id) { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
    };

    let began = std::time::Instant::now();
    let allowance = memory();
    let every = progress_every();
    let result = LookAheadEngine::new(LookAheadOptions {
        state_budget: usize::MAX,
        memory_budget: allowance,
        time_budget: row_time(),
        counter_cap: COUNTER_CAP,
        // The crawl's own clock decides when, so there is nothing to throttle here.
        progress_interval: every.unwrap_or_default(),
        on_progress: every.map(|_| {
            Box::new(move |_node, states: usize, reached: usize, bytes: usize, elapsed| {
                println!(
                    "{PROGRESS} fwd {:>7}  {states:>12} states  {reached:>6} reached  {} / {}",
                    mmss(elapsed),
                    gb(bytes),
                    gb(allowance),
                );
            }) as Box<dyn Fn(_, _, _, _, _) + Send + Sync>
        }),
        ..Default::default()
    })
    .evaluate(graph, start, world, novelty);

    Row {
        verdict: if result.best == Novelty::UnseenAnyGame {
            "found"
        } else if result.budget_exhausted() {
            "gave-up"
        } else {
            "not-there"
        },
        millis: began.elapsed().as_millis(),
        size: result.states_explored,
        set_sum: 0,
    }
}

fn backward(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn lookahead_engine::world::world::ILookAheadWorld,
    symbols: &StateSymbols,
    unseen: &HashSet<DialogueNodeId>,
) -> Row {
    let began = std::time::Instant::now();

    let layout = DataLayout::for_graph(graph, COUNTER_CAP, None, false)
        .keeping_only_read(symbols, &DataLayout::read_by(graph));
    // From the same allowance, so the diagram and the crawl are held to one number rather than two
    // that happen to agree.
    //
    // FALLIBLY, because the alternative is not a wrong number but a dead process: the
    // manager preallocates its node store and that allocation aborts. A None here means the
    // machine could not supply the budget, which is not a finding about the search - the
    // row is NOT MEASURED and wants running again with the memory free.
    let Some(vars) = DataVars::try_new(&layout, symbols, budget()) else {
        return Row {
            verdict: NOT_MEASURED,
            millis: began.elapsed().as_millis(),
            size: 0,
            set_sum: 0,
        };
    };
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(world)
        .with_constant_clock(DataLayout::group_passes_time(graph));

    let seed = seed_of(graph, world, &vars);
    let quarry: HashSet<DialogueNodeId> = unseen.clone();
    let allowance = memory();
    let every = progress_every();
    let bwd_budget = Budget {
        steps: usize::MAX,
        time: row_time(),
        memory: allowance,
        // A STARTING GUESS ONLY when progress is on: `check_gap` retunes it from here to
        // whatever holds a second, because no fixed step count can - twenty thousand steps
        // is a moment early on and minutes once the sets are large, and the minutes end is
        // exactly where somebody is watching. Checking every second also bounds the memory
        // overshoot in time rather than in steps, which can only make the no-room verdict
        // land closer to the budget it names.
        report_every: if every.is_some() { 500 } else { 20_000 },
        check_gap: if every.is_some() { CHECK_GAP } else { std::time::Duration::ZERO },
        // The measurement allowance is far larger than the shipped one, so the machine is
        // the real ceiling here and the guard matters more, not less.
        on_step: None,
        system_reserve: lookahead_engine::engine::system_memory::DEFAULT_RESERVE,
        report_gap: every.unwrap_or_default(),
        on_progress: every.map(|_| {
            Box::new(
                move |steps: usize,
                      reached: usize,
                      held: usize,
                      largest: usize,
                      bytes: usize| {
                    println!(
                        "{PROGRESS} bwd {:>7}  {steps:>10} steps  {reached:>6} reached  \
                         {held:>11} set nodes  largest {largest:>9}  {} / {}",
                        mmss(began.elapsed()),
                        gb(bytes),
                        gb(allowance),
                    );
                },
            ) as Box<dyn Fn(usize, usize, usize, usize, usize)>
        }),
        // The same early exit the forward crawl has: the question is whether ANY unseen
        // entry is reachable, not what the whole reachable set is.
        halt_on: Some(Box::new(move |id| quarry.contains(&id))),
    };

    let found = Reachability::explore_within(
        graph, start, &seed, &mut compiler, world, COUNTER_CAP as u32, &bwd_budget,
    );
    let stats = found.stats();

    Row {
        verdict: if stats.halted_at.is_some() {
            "found"
        } else if stats.out_of_system_memory {
            // THE MACHINE, not the budget. de-e33h asked for these to be told apart, and
            // this is the backward engine's half of it: the row is not a result and wants
            // running again with the memory free.
            "no-ram"
        } else if stats.out_of_memory {
            "no-room"
        } else if stats.reached_fixed_point {
            "not-there"
        } else {
            "gave-up"
        },
        millis: began.elapsed().as_millis(),
        // TWO DIFFERENT QUANTITIES, and they are both here because neither bounds the
        // other and one of them alone would mislead.
        //
        // `size` is what the MANAGER holds: every node allocated for anything - the
        // reachable sets, the compiled guards, the transition relations, the intermediate
        // results of every operation, and whatever has not been reclaimed yet. That is
        // memory in use, so it is what the budget watches.
        //
        // `set_sum` is the size of the ANSWER: each entry's set counted separately, so a
        // node shared between two entries is counted twice.
        //
        // On conversation 368 the sum is much the larger (19.3 million against a budget of
        // 8), because a great many entries hold big overlapping sets. On 28 the manager is
        // the larger (562,880 against 255,443), because the answer is small and most of the
        // allocation went on machinery. A column showing only one of them would suggest the
        // budget had failed to fire in the first case and that the search was cheap in the
        // second.
        size: vars.node_count(),
        set_sum: stats.diagram_nodes,
    }
}

#[test]
#[ignore = "a long measurement, not a test: run it with --ignored --release"]
fn both_engines_over_every_profile() {
    let Some(path) = common::conversation_index() else { return };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    // Tab separated with a stable header, so a run pipes straight into a file that
    // something else can read - see the note at the top about keeping the logs.
    //
    // SUPPRESSIBLE, because the driver script runs one row per process and wants one header
    // in the file rather than one per row.
    if std::env::var("NO_HEADER").is_err() {
        println!(
            "conv\tentries\tprofile\tunseen\tfwd_verdict\tfwd_ms\tfwd_states\t\
             bwd_verdict\tbwd_ms\tbwd_nodes\tbwd_setsum"
        );
    }

    for conversation in conversations(&HEAVIEST) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else { continue };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let symbols = graph.symbols().clone();
        let reachable = candidates(&graph, start);
        if reachable.is_empty() {
            continue;
        }

        for profile in profiles() {
            let unseen = unseen_for(profile, &reachable);

            // ASKED PER ROW, AND BEFORE ANYTHING IS SPENT. A machine that cannot supply the
            // budget makes the RUN invalid rather than the row a result - there is nothing
            // to record about a search that never happened - so the row says NOT-MEASURED
            // and wants running again when the memory is free. Per row rather than once at
            // the top because what else is running on the machine changes underneath a run
            // that takes hours.
            //
            // AND `backward` ASKS AGAIN, through DataVars::try_new, which is not
            // redundant. This decides the ROW - both engines are skipped, because a forward
            // verdict measured beside a backward one that never ran is half a row, and the
            // two are only comparable when they were rationed alike (de-e23q). That one is
            // the backstop for the race this check openly cannot close: another process can
            // take the memory between the answer here and the allocation there, and the
            // allocation aborts rather than failing.
            if !budget().can_be_supplied() {
                eprintln!(
                    "NOT MEASURED: {conversation} {} - this machine could not supply the \
                     {} MB budget. The row is not a result; run it again with the memory \
                     free.",
                    profile.label(),
                    memory() / (1024 * 1024),
                );
                println!(
                    "{conversation}\t{}\t{}\t{}\t{NOT_MEASURED}\t?\t?\t{NOT_MEASURED}\t?\t?\t?",
                    graph.count(),
                    profile.label(),
                    unseen.len(),
                );
                continue;
            }

            let fwd = forward(&graph, start, &world, &unseen);
            let bwd = backward(&graph, start, &world, &symbols, &unseen);

            println!(
                "{conversation}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                graph.count(),
                profile.label(),
                unseen.len(),
                fwd.verdict,
                fwd.millis,
                fwd.size,
                bwd.verdict,
                bwd.millis,
                bwd.size,
                bwd.set_sum,
            );
        }
    }
}
