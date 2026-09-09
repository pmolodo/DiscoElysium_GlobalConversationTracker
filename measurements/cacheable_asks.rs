// SPDX-License-Identifier: MIT
//! Of the asks that recur between menus, how many could a memo actually hold - and what
//! does holding one cost?
//!
//! ## The gate on de-znov.3, and why de-znov.1's number is not enough on its own
//!
//! `candidate_recurrence`'s walk arm counted how many targets a later menu asks about that
//! an earlier menu of the same walk already asked about: 31 of 32 at ninety-five per cent
//! seen. That is a CEILING on what a memo between requests could ever be worth, and it is
//! about the LISTS. It did not ask what any of those asks cost or how they ended, and only
//! some endings are worth keeping.
//!
//! ## What has to be true of an ask for a memo to serve it
//!
//! 1. THE PASS SETTLED. `BackwardStats::reached_fixed_point`. An unsettled pass proves
//!    nothing and holds a subset of what it would have held; the driver already refuses to
//!    remember one within a request, for the same reason.
//! 2. IT DID NOT MEET. `BackwardStats::met_at` set means the pass stopped early against
//!    forward sets DERIVED FROM ONE SEED, so what it holds is a proof for that seed and a
//!    partial fixed point for any other. A memo is asked about seeds it has never seen.
//! 3. IT IS NO DEARER UNPRUNED. `Known::restricted` narrows every pre-image with forward
//!    sets that were seeded, so a pass that is to outlive its seed has to be built with
//!    pruning OFF. What that costs is the whole question - it may be free on the groups
//!    whose forward slice does not settle, since `Known` narrows nothing without a settled
//!    run, and it may be the entire saving on the ones where it does.
//!
//! ## What this runs
//!
//! The same walk `candidate_recurrence` counts - shared as `menu_walk`, so the ceiling and
//! this are about the same asks and the two numbers may be multiplied - and then, at each
//! menu, the passes the driver would actually run. Per ask: whether it settled, whether it
//! met, how long it took and how many diagram nodes its sets hold.
//!
//! TWICE OVER THE SAME WALK, once pruned and once not, which answers item 3 in one run
//! against one world rather than in two runs against different draws. The walk is a function
//! of the graph and the profile alone, so both arms ask exactly the same questions in
//! exactly the same order.
//!
//! ONE MANAGER PER ARM, built inside a thread of its own - de-fpax - so neither arm inherits
//! the other's node store and the second is not flattered by the first's cache.
//!
//! ## What is faithful here and what is not
//!
//! FAITHFUL: the shipped budget (`portfolio::Budget::default`), the player's 256 MB, the
//! shipped layout, the forward slice at its fifty milliseconds with the halt condition the
//! driver gives it, and the dominance rule that refuses most of a candidate list for free.
//!
//! NOT: the driver stops at its first proof, and this asks about every candidate of every
//! option, because what a memo holds is verdicts and not searches. So this is the
//! all-refusals population, in the same direction and for the same reason `menu_walk::minimal`
//! is.
//!
//! AN OPTION THE SLICE ANSWERS FOR IS NOT ASKED ABOUT. Where the forward slice halts, the
//! driver reports and no backward pass runs at all; where nothing better than the floor is
//! link-reachable, no search is begun. Both are counted and reported rather than quietly
//! dropped, because an ask that never reaches the backward driver is not an ask a memo could
//! serve either, and it is part of the same subtraction the dominance rule started.
//!
//! ## What it said, 2026-09-09: EVERY PASS THAT RUNS IS CACHEABLE, and two thirds recur
//!
//! Nine groups, twenty menus each, at ninety-five and fifty per cent seen:
//!
//! ```text
//!   asks in the ceiling                    9882
//!   of those, repeats                      8276  83.7%
//!   asks that reached the driver           3361  34.0%
//!   passes that settled                    3361  100.0%
//!   passes that met                           0  0.0%
//!   cacheable - settled, did not meet      3361  100.0%
//!   passes a memo would replace            2305  68.6%
//! ```
//!
//! THE FEARED ANSWER DID NOT HAPPEN. de-znov.2 named the plausible failure - that the asks
//! which recur are the ones that meet early, the cheap ones, while the ones that cost never
//! settle - and not one of 3361 passes met, nor one failed to settle. The reason is
//! structural rather than lucky: the forward slice halts on any option from which something
//! of the hunted class is reachable, so what reaches the backward driver at all is exactly
//! the population where every ask is a refusal. That is the assumption `menu_walk::minimal`
//! states, and the slice enforces it.
//!
//! AND THE CEILING IS A THIRD OF WHAT IT LOOKED. Only 3361 of 9882 asks reach the driver,
//! because the slice answers the other two thirds outright. So de-znov.1's 83.7 per cent
//! repeats is worth 27.9 per cent of the ceiling - but 68.6 per cent of the passes actually
//! spent, which is the number a design is decided on. Read `of ran`, not `of repeat`.
//!
//! ## PRUNING COSTS NOTHING HERE, and for a reason worth knowing
//!
//! ```text
//!   every pass unpruned, ms             75151.5
//!   every pass pruned, ms               74115.4
//!   unpruned settled                       3361  100.0%
//!   pruned settled                         3361  100.0%
//! ```
//!
//! Within noise, and identical settle rates - so item 3 above is answered yes: a pass built
//! to outlive its seed is no dearer than the one that ships. Not because pruning is cheap
//! but because it never applies: the forward slice settled in 2 of 720 options, and `Known`
//! narrows nothing without a settled run, so on this population the two arms are one arm.
//!
//! THAT IS ABOUT THESE NINE GROUPS AND NOT ABOUT THE GAME. They are the heavy ones, chosen
//! for being heavy; `measurements/settles_within.rs` puts 119 of 120 ORDINARY groups inside
//! the same fifty milliseconds. So this says the memo is free where the passes are dear, and
//! says nothing yet about what it would cost where pruning does bite.
//!
//! ## WHAT IT COSTS IS MEMORY, WHICH IS THE HALF TO WORRY ABOUT
//!
//! ```text
//!   one cacheable pass, ms                 22.4
//!   one cacheable pass, nodes             29956
//!   the worst manager at walk's end     4900581
//! ```
//!
//! 2305 passes at 22.4 ms is 51.5 of the 75.2 seconds these walks spend, so the saving is
//! real and large. The other column is the constraint: conversation 14's walk left 4.9
//! million diagram nodes in the manager, and at `DiagramBudget::BYTES_PER_NODE` that is most
//! of the player's 256 MB, for a store holding one walk's guards, slices and passes. A memo
//! keeps a subset of that ALIVE across requests where today it is dropped with the query, so
//! de-znov.3's design question is not whether the verdicts are worth keeping - they are - but
//! how many may be kept at once.
//!
//! ## How to run it
//!
//! ```text
//! RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo cacheable-asks -- \
//!   cargo run --release --example cacheable_asks
//! ```
//!
//! `CONVERSATION` picks the groups, `PROFILES` the percentages, `MENUS` how many menus to
//! take from each group, `EACH_MS` what one pass may spend and `BUDGET_MB` the manager's
//! allowance.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use lookahead_engine::bridge::{SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::backward::{Backward, Budget as BackwardBudget};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::known::GroupShape;
use lookahead_engine::symbolic::portfolio;
use lookahead_engine::symbolic::reachability::{seed_of, Budget as ForwardBudget, Reachability};
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::world::world::ILookAheadWorld;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "seen_profile.rs"]
mod seen_profile;

#[path = "menu_walk.rs"]
mod menu_walk;
use menu_walk::{Menu, Recurrence};

/// The groups to ask about: the same nine `candidate_recurrence` walks, so the ceiling this
/// takes a fraction of is the ceiling that was measured.
const GROUPS: [i32; 9] = [362, 368, 631, 14, 28, 1030, 825, 587, 640];

/// The percentages to walk under.
///
/// TWO, AND THE LATE ONE FIRST. de-znov.1 found the recurrence moves with how much is left
/// unread - 1.17 asks each at five per cent seen against 21.24 at ninety-five - so the
/// regime where a memo could pay is the late one, and a sweep of all seven would spend most
/// of its passes in the regime that already answered no.
const PERCENTS: [u32; 2] = [95, 50];

/// How many menus to take from one walk.
///
/// TWENTY, where the structural arm takes forty. Every menu here is several bounded fixed
/// points rather than a walk over a link graph, so the sample is sized to what a run can
/// afford; `MENUS` raises it.
///
/// AND NOT MUCH FEWER, which four menus of conversation 362 showed: they left 42 passes
/// about 42 distinct targets, so the repeat population - the whole subject - was empty. The
/// walk has to be long enough to come back to something.
const MENUS: usize = 20;

/// The player's allowance, because the question is what a player's engine would hold.
const BUDGET_MB: usize = 256;

const COUNTER_CAP: i32 = 16;

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);
    let menus_wanted = from_env("MENUS", MENUS);
    let each = Duration::from_millis(
        from_env("EACH_MS", portfolio::Budget::default().each.as_millis() as usize) as u64,
    );

    println!(
        "WHAT A MEMO BETWEEN REQUESTS COULD HOLD. The walk `candidate_recurrence` counts, \
         with the\npasses the driver would actually run at each menu - once with pruning on, \
         which is what\nships, and once off, which is what a pass has to be built with to \
         outlive its seed.\n"
    );
    println!(
        "A CACHEABLE PASS IS ONE THAT SETTLED AND DID NOT MEET. An unsettled pass holds a \
         subset\nof what it would have held; a met one holds a proof for the seed it met \
         against.\n"
    );
    println!(
        "{} MB, {menus_wanted} menus a walk, {} ms a pass, forward slice {} ms\n",
        budget.memory() / (1024 * 1024),
        each.as_millis(),
        portfolio::Budget::default().forwards.as_millis(),
    );

    let mut every: Vec<Walked> = Vec::new();

    for conversation in numbers("CONVERSATION", &GROUPS) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            eprintln!("conversation {conversation}'s group does not build; skipping.");
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            continue;
        }
        let pool = seen_profile::candidates(&graph, root);

        for percent in percents() {
            // BUILT ONCE AND HANDED TO BOTH ARMS. Two walks would be the same walk - it is a
            // function of the graph and the draw - but "would be" is what a shared plan
            // turns into "is", and the pruned-against-unpruned column is only readable if
            // the two arms asked the same questions.
            let menus = menu_walk::menus(
                &graph,
                root,
                seen_profile::percent_unseen(&pool, percent),
                menus_wanted,
            );
            if menus.is_empty() {
                continue;
            }

            let ceiling = Recurrence::of(&menus);
            // PRUNING OFF FIRST, so the arm whose cost decides de-znov.3 is the one that
            // cannot have been warmed by the other. The managers are separate either way;
            // the machine's own caches are not.
            let Some(off) = arm(&graph, &menus, budget, each, false) else {
                eprintln!("conversation {conversation}: no room for the manager; skipping.");
                continue;
            };
            let Some(on) = arm(&graph, &menus, budget, each, true) else { continue };

            // A LINE AS EACH WALK LANDS, because every table here is printed at the end and
            // a heavy group is minutes of silence otherwise. It says what the walk cost and
            // what it found, which is enough to follow a run rather than wait one out.
            println!(
                "  ... {conversation} at {percent}pc-seen: {} menus, {} asks ran of {}, \
                 {} cacheable, {} saved, {:.0} ms off / {:.0} ms on",
                ceiling.menus,
                off.asks,
                ceiling.asked,
                off.cacheable,
                off.saved,
                off.millis,
                on.millis,
            );
            every.push(Walked { conversation, percent, ceiling, off, on });
        }
    }

    if every.is_empty() {
        println!("nothing walked.");
        return;
    }

    per_walk(&every);
    reached(&every);
    what_a_pass_costs(&every);
    pruned_against_not(&every);
    the_answer(&every);
}

/// One group under one profile: the ceiling, and what the two arms spent reaching it.
struct Walked {
    conversation: i32,
    percent: u32,
    ceiling: Recurrence,
    /// Pruning off - the arm a cacheable pass would have to be built in.
    off: Arm,
    /// Pruning on - what ships.
    on: Arm,
}

/// What one arm of one walk found.
#[derive(Default)]
struct Arm {
    /// Asks that never reached the backward driver, and why.
    ///
    /// `no_hunt` is an option with nothing better than the floor link-reachable, which the
    /// bridge refuses before a diagram is touched; `halted` is one the forward slice
    /// answered outright. Neither is an ask a memo could serve, and both are part of the
    /// same subtraction the dominance rule started - so they are reported rather than
    /// dropped.
    no_hunt: usize,
    halted: usize,
    /// Options whose forward slice SETTLED, which is the only thing that licenses pruning.
    settled_slice: usize,
    options: usize,
    /// Passes actually run.
    asks: usize,
    settled: usize,
    met: usize,
    /// Settled, and did not meet: what a memo could hold.
    cacheable: usize,
    /// Of the repeat asks - a target an earlier menu of this walk already asked about - how
    /// many a memo would have answered, which needs the FIRST ask to have been cacheable.
    repeat: usize,
    saved: usize,
    millis: f64,
    /// Milliseconds spent on the cacheable passes alone, which is what a memo saves per hit.
    cacheable_millis: f64,
    /// Diagram nodes across a cacheable pass's sets, summed and at its worst.
    ///
    /// SUMMED IS AN UPPER BOUND AND A LOOSE ONE. `BackwardStats::diagram_nodes` counts with
    /// sharing inside one pass's sets and not between passes, and every pass of a walk is
    /// built over ONE manager out of largely the same guards - so adding them up counts the
    /// shared subgraphs once per pass that touches them. It is the right number for what one
    /// pass costs and the wrong one for what a memo holds; [`Arm::manager_nodes`] is the
    /// second.
    cacheable_nodes: usize,
    largest_nodes: usize,
    /// What the manager holds when the walk ends, which is the honest ceiling on all of it.
    ///
    /// EVERYTHING, not just what a memo would keep: the compiled guards, the forward slices
    /// and every pass's sets share this one store. A memo would hold a subset, and this says
    /// what the subset is inside.
    manager_nodes: usize,
}

impl Arm {
    fn add(&mut self, other: &Arm) {
        self.no_hunt += other.no_hunt;
        self.halted += other.halted;
        self.settled_slice += other.settled_slice;
        self.options += other.options;
        self.asks += other.asks;
        self.settled += other.settled;
        self.met += other.met;
        self.cacheable += other.cacheable;
        self.repeat += other.repeat;
        self.saved += other.saved;
        self.millis += other.millis;
        self.cacheable_millis += other.cacheable_millis;
        self.cacheable_nodes += other.cacheable_nodes;
        self.largest_nodes = self.largest_nodes.max(other.largest_nodes);
        // THE WORST ONE MANAGER HELD, not the sum: each walk runs in a manager of its own,
        // so adding them would name a store that never existed.
        self.manager_nodes = self.manager_nodes.max(other.manager_nodes);
    }

    /// What one cacheable pass cost, in milliseconds.
    fn each_cacheable_ms(&self) -> f64 {
        divide(self.cacheable_millis, self.cacheable)
    }

    /// What one cacheable pass holds, in diagram nodes.
    fn each_cacheable_nodes(&self) -> f64 {
        divide(self.cacheable_nodes as f64, self.cacheable)
    }
}

/// One walk's passes, under one setting of pruning.
///
/// A THREAD OF ITS OWN WITH THE MANAGER INSIDE IT - de-fpax, and `symbolic::isolated` for
/// why the manager cannot be handed in.
fn arm(
    graph: &LookAheadGraph,
    menus: &[Menu],
    budget: DiagramBudget,
    each: Duration,
    pruning: bool,
) -> Option<Arm> {
    isolated::on_its_own_thread(|| {
        let symbols = graph.symbols().clone();
        let world = SnapshotWorld::declaring(
            WorldSnapshot { day_minutes: 720, day_counter: 1, ..Default::default() },
            None,
        );
        let layout = DataLayout::for_group(graph, &world, COUNTER_CAP);
        let vars = DataVars::try_new(&layout, &symbols, budget)?;
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
        let seed = seed_of(graph, &world, &vars).expect("room for a seed");
        let shape = GroupShape::of(graph);
        let shipped = portfolio::Budget::default();

        // WHICH MENU FIRST ASKED ABOUT EACH TARGET, and whether that first pass was one a
        // memo could have kept. A repeat ask is only SAVED where the ask that paid for it
        // ended in a verdict worth holding.
        let mut cacheable_first: HashMap<DialogueNodeId, bool> = HashMap::new();
        let mut counted = Arm::default();

        for menu in menus {
            let unseen = &menu.unseen;
            let novelty = |id: DialogueNodeId| {
                if unseen.contains(&id) {
                    Novelty::UnseenAnyGame
                } else {
                    Novelty::SeenThisGame
                }
            };

            for start in options(menu) {
                let targets: Vec<DialogueNodeId> = menu
                    .asks
                    .iter()
                    .filter(|(option, _)| *option == start)
                    .map(|(_, target)| *target)
                    .collect();
                counted.options += 1;

                // NOTHING WORTH HUNTING, so the bridge never begins a search from here.
                // `class_worth_hunting` walks the links and refuses before a diagram is
                // touched, and an option refused there asks about nothing.
                let hunting = graph.best_linked_class(start, novelty);
                let Some(hunting) = hunting.filter(|best| *best > Novelty::SeenThisGame) else {
                    counted.no_hunt += targets.len();
                    continue;
                };

                let forward = slice(
                    graph, start, &seed, &mut compiler, &world, hunting, &novelty, &shipped,
                    &shape,
                );
                if forward.stats().halted_at.is_some() {
                    // THE SLICE ANSWERED, so no backward pass runs and there is no verdict
                    // to remember. This is the shipped path's own short circuit, not a
                    // shortcut taken here.
                    counted.halted += targets.len();
                    continue;
                }
                if forward.stats().reached_fixed_point {
                    counted.settled_slice += 1;
                }

                let known = shape
                    .known_from(graph, start)
                    // WHAT THE SEARCH HOLDS ARRIVING AT ITS START. `Where::of` is what the
                    // driver calls, and for a start entered either way - which every option
                    // of a walked menu is - it yields exactly this pair.
                    .from(start, &seed)
                    .with_forward(&forward)
                    .pruning(pruning);

                let pass = BackwardBudget { steps: usize::MAX, time: each, ..Default::default() };
                for target in targets {
                    let began = Instant::now();
                    let backward = Backward::reaching_knowing(
                        graph, target, &mut compiler, &world, COUNTER_CAP as u32, &pass,
                        Some(&known),
                    );
                    let took = began.elapsed().as_secs_f64() * 1000.0;
                    let stats = backward.stats();
                    let settled = stats.reached_fixed_point;
                    let met = stats.met_at.is_some();
                    let keepable = settled && !met;

                    counted.asks += 1;
                    counted.millis += took;
                    counted.settled += usize::from(settled);
                    counted.met += usize::from(met);
                    counted.largest_nodes = counted.largest_nodes.max(stats.diagram_nodes);
                    if keepable {
                        counted.cacheable += 1;
                        counted.cacheable_millis += took;
                        counted.cacheable_nodes += stats.diagram_nodes;
                    }

                    match cacheable_first.get(&target) {
                        // A LATER MENU ASKING AGAIN, which is the whole population de-znov.1
                        // counted. It is a saving only where the first ask left something
                        // worth keeping.
                        Some(first) => {
                            counted.repeat += 1;
                            counted.saved += usize::from(*first);
                        }
                        None => {
                            cacheable_first.insert(target, keepable);
                        }
                    }
                }
            }
        }

        counted.manager_nodes = vars.node_count();
        Some(counted)
    })
}

/// The options that ask about anything, in the order the menu puts them.
///
/// Here rather than on [`Menu`] because only an arm that RUNS the asks groups them by
/// option: a slice is per start, so the passes have to be taken a start at a time. An arm
/// that only counts asks reads the pairs straight.
fn options(menu: &Menu) -> Vec<DialogueNodeId> {
    let mut found = Vec::new();
    for (start, _) in &menu.asks {
        if !found.contains(start) {
            found.push(*start);
        }
    }
    found
}

/// The forward slice the driver runs before the backward half, at the shipped settings.
///
/// The halt condition is the driver's own: the class the caller established is the best one
/// link-reachable, so a pass that stops on one has answered and nothing else needs to run.
#[allow(clippy::too_many_arguments)]
fn slice<'a, F>(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    seed: &oxidd::bdd::BDDFunction,
    compiler: &mut GuardCompiler<'a>,
    world: &dyn ILookAheadWorld,
    hunting: Novelty,
    novelty: &F,
    shipped: &portfolio::Budget,
    shape: &GroupShape,
) -> Reachability<'a>
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    // THE SET RATHER THAN THE CLOSURE, because `halt_on` outlives this call and cannot
    // borrow the novelty function.
    let quarry: HashSet<DialogueNodeId> =
        graph.nodes().map(|node| node.id).filter(|id| novelty(*id) == hunting).collect();

    Reachability::explore_branch_knowing(
        graph,
        start,
        StartBranch::Either,
        seed,
        compiler,
        world,
        COUNTER_CAP as u32,
        &ForwardBudget {
            time: shipped.forwards,
            memory: shipped.slice_memory,
            steps: shipped.slice_steps,
            halt_on: Some(Box::new(move |id| quarry.contains(&id))),
            ..Default::default()
        },
        shape.order(),
    )
}

/// Every walk, one row each: the ceiling, and how much of it survives.
fn per_walk(every: &[Walked]) {
    println!("PER WALK. `asked` and `repeat` are the ceiling; `saved` is the repeats a memo");
    println!("would have answered, which needs the first ask to have settled without meeting.\n");
    println!(
        "{:>6}  {:>10}  {:>6}  {:>7}  {:>7}  {:>7}  {:>7}  {:>7}  {:>9}  {:>7}",
        "conv", "profile", "menus", "asked", "repeat", "ran", "cacheab", "saved", "of repeat",
        "of ran",
    );
    for walked in every {
        println!(
            "{:>6}  {:>10}  {:>6}  {:>7}  {:>7}  {:>7}  {:>7}  {:>7}  {:>9}  {:>7}",
            walked.conversation,
            format!("{}pc-seen", walked.percent),
            walked.ceiling.menus,
            walked.ceiling.asked,
            walked.ceiling.repeat,
            walked.off.asks,
            walked.off.cacheable,
            walked.off.saved,
            share(walked.off.saved, walked.ceiling.repeat),
            share(walked.off.saved, walked.off.asks),
        );
    }
    println!(
        "\n`of ran` IS THE ONE TO READ. `of repeat` measures the memo against a ceiling most \
         of\nwhose asks never reach the backward driver at all - see the next table - so it \
         reports\na small fraction of a number that was never available. `of ran` is the share \
         of the\npasses actually spent that a memo would have answered instead."
    );
}

/// How much of the ceiling never reaches the backward driver at all.
fn reached(every: &[Walked]) {
    println!("\nWHAT NEVER REACHES THE BACKWARD DRIVER, which is not a memo's to serve:");
    println!("`no hunt` is an option refused before a diagram is touched; `halted` is one the");
    println!("forward slice answered outright; `slice` is options whose slice SETTLED, which is");
    println!("the only thing that licenses pruning.\n");
    println!(
        "{:>6}  {:>10}  {:>8}  {:>8}  {:>8}  {:>7}  {:>13}",
        "conv", "profile", "asked", "no hunt", "halted", "ran", "slice settled",
    );
    for walked in every {
        let arm = &walked.off;
        println!(
            "{:>6}  {:>10}  {:>8}  {:>8}  {:>8}  {:>7}  {:>5} of {:>5}",
            walked.conversation,
            format!("{}pc-seen", walked.percent),
            walked.ceiling.asked,
            arm.no_hunt,
            arm.halted,
            arm.asks,
            arm.settled_slice,
            arm.options,
        );
    }
}

/// What one cacheable pass costs, in the two currencies a memo spends.
fn what_a_pass_costs(every: &[Walked]) {
    println!("\nWHAT ONE CACHEABLE PASS COSTS, unpruned - the arm a memo would be filled from.");
    println!("The nodes matter as much as the milliseconds: a memo holds BDD sets in the");
    println!("player's 256 MB, so a saving that costs the manager is not a saving.\n");
    println!(
        "{:>6}  {:>10}  {:>8}  {:>9}  {:>11}  {:>11}  {:>13}",
        "conv", "profile", "cacheab", "ms each", "nodes each", "worst pass", "manager end",
    );
    for walked in every {
        let arm = &walked.off;
        println!(
            "{:>6}  {:>10}  {:>8}  {:>9.1}  {:>11.0}  {:>11}  {:>13}",
            walked.conversation,
            format!("{}pc-seen", walked.percent),
            arm.cacheable,
            arm.each_cacheable_ms(),
            arm.each_cacheable_nodes(),
            arm.largest_nodes,
            arm.manager_nodes,
        );
    }
    println!(
        "\n`nodes each` COUNTS SHARING INSIDE ONE PASS AND NOT BETWEEN THEM, so summing it \
         over a\nwalk counts a shared subgraph once per pass that touches it. `manager end` is \
         what the\nwhole walk actually left in the store - every guard, every slice and every \
         pass - and a\nmemo holds a subset of that rather than the sum."
    );
}

/// The same asks with pruning on and off, which is the third thing a memo needs to be true.
fn pruned_against_not(every: &[Walked]) {
    println!("\nPRUNED AGAINST UNPRUNED, on the same asks in the same order. A memo has to be");
    println!("filled unpruned, so this is what a cacheable pass costs OVER what ships. Where the");
    println!("slice does not settle, `Known` narrows nothing and the two arms are one arm.\n");
    println!(
        "{:>6}  {:>10}  {:>7}  {:>10}  {:>10}  {:>8}  {:>12}  {:>12}",
        "conv", "profile", "asks", "off ms", "on ms", "dearer", "off settled", "on settled",
    );
    for walked in every {
        let (off, on) = (&walked.off, &walked.on);
        println!(
            "{:>6}  {:>10}  {:>7}  {:>10.1}  {:>10.1}  {:>7.2}x  {:>12}  {:>12}",
            walked.conversation,
            format!("{}pc-seen", walked.percent),
            off.asks,
            off.millis,
            on.millis,
            off.millis / on.millis.max(f64::MIN_POSITIVE),
            share(off.settled, off.asks),
            share(on.settled, on.asks),
        );
    }
}

/// The three numbers de-znov.3 lives or dies on.
fn the_answer(every: &[Walked]) {
    let mut ceiling = Recurrence::default();
    let mut off = Arm::default();
    let mut on = Arm::default();
    for walked in every {
        ceiling.add(&walked.ceiling);
        off.add(&walked.off);
        on.add(&walked.on);
    }

    println!("\nOVER EVERY WALK:\n");
    counted("menus walked", ceiling.menus);
    counted("asks in the ceiling", ceiling.asked);
    part("of those, repeats", ceiling.repeat, ceiling.asked);
    part("asks that reached the driver", off.asks, ceiling.asked);
    part("passes that settled", off.settled, off.asks);
    part("passes that met", off.met, off.asks);
    part("cacheable - settled, did not meet", off.cacheable, off.asks);
    part("repeats a memo would answer", off.saved, ceiling.repeat);
    part("passes a memo would replace", off.saved, off.asks);
    println!();
    millis("one cacheable pass, ms", off.each_cacheable_ms());
    counted("one cacheable pass, nodes", off.each_cacheable_nodes().round() as usize);
    counted("every cacheable pass summed", off.cacheable_nodes);
    counted("the worst manager at walk's end", off.manager_nodes);
    println!();
    millis("every pass unpruned, ms", off.millis);
    millis("every pass pruned, ms", on.millis);
    part("unpruned settled", off.settled, off.asks);
    part("pruned settled", on.settled, on.asks);

    println!(
        "\nTHE MEMO IS WORTH {:.0} MS OF THE {:.0} MS these walks spend on passes - the {} \
         repeat\nasks it would have answered, at what one of them cost. That is the ceiling \
         multiplied by\nthe share of asks that end in a verdict worth keeping, which is what \
         de-znov.2 came for.",
        off.saved as f64 * off.each_cacheable_ms(),
        off.millis,
        off.saved,
    );
    println!(
        "\nA CACHEABLE SHARE NEAR ZERO CLOSES de-znov.3, and the plausible way to get one is \
         for\nthe asks that recur to be the ones that meet early - the cheap ones - while the \
         ones\nthat cost never settle."
    );
}

/// How wide the summary's labels are, said once so the rows line up without restating it.
const LABEL: usize = 33;

/// A summary line: a label and a number.
fn counted(label: &str, of: usize) {
    println!("  {label:<LABEL$}{of:>10}");
}

/// The same, and what the number is a share OF - which is the column that makes it mean
/// something, since nearly every count here is a fraction of a different denominator.
fn part(label: &str, of: usize, whole: usize) {
    println!("  {label:<LABEL$}{of:>10}  {}", share(of, whole));
}

/// The same for a duration, which wants a decimal where a count does not.
fn millis(label: &str, of: f64) {
    println!("  {label:<LABEL$}{of:>10.1}");
}

fn divide(total: f64, count: usize) -> f64 {
    if count == 0 {
        return 0.0;
    }
    total / count as f64
}

fn share(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "-".to_string();
    }
    format!("{:.1}%", part as f64 * 100.0 / whole as f64)
}

fn percents() -> Vec<u32> {
    match std::env::var("PROFILES") {
        Ok(text) => text.split(',').filter_map(|part| part.trim().parse().ok()).collect(),
        Err(_) => PERCENTS.to_vec(),
    }
}

fn from_env(name: &str, fallback: usize) -> usize {
    std::env::var(name).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(fallback)
}

fn numbers(name: &str, fallback: &[i32]) -> Vec<i32> {
    match std::env::var(name) {
        Ok(text) => text.split(',').filter_map(|part| part.trim().parse().ok()).collect(),
        Err(_) => fallback.to_vec(),
    }
}
