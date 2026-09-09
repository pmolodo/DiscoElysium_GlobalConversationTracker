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
//! 2. IT DID NOT MEET. `BackwardStats::met_at` set means the pass stopped early against what
//!    the search holds where it BEGINS, so what it holds is a proof for that seed and a
//!    partial fixed point for any other. A memo is asked about seeds it has never seen.
//!
//! Both are properties of the pass rather than of how it was asked for, which is why this
//! can run the search exactly as it ships and read the answer off afterwards.
//!
//! ## What this runs
//!
//! The same walk `candidate_recurrence` counts - shared as `menu_walk`, so the ceiling and
//! this are about the same asks and the two numbers may be multiplied - and then, at each
//! menu, the passes the driver would actually run. Per ask: whether it settled, whether it
//! met, how long it took and how many diagram nodes its sets hold.
//!
//! ONE MANAGER FOR THE WALK, built inside a thread of its own - de-fpax - because that is
//! what a session gives it: a menu after menu in one group against one store.
//!
//! ## What is faithful here and what is not
//!
//! FAITHFUL: the shipped budget (`answer::Budget::default`), the player's 256 MB, the
//! shipped layout, and the dominance rule that refuses most of a candidate list for free.
//!
//! NOT: the driver stops at its first proof, and this asks about every candidate of every
//! option, because what a memo holds is verdicts and not searches. So this is the
//! all-refusals population, in the same direction and for the same reason `menu_walk::minimal`
//! is.
//!
//! AN OPTION THAT NEVER SEARCHES IS NOT ASKED ABOUT. Where nothing better than the floor is
//! link-reachable the bridge begins no search, and where the START ALREADY CARRIES the class
//! being hunted the driver answers before touching a diagram. Both are counted and reported
//! rather than quietly dropped, because an ask that never reaches the backward driver is not
//! an ask a memo could serve either, and it is part of the same subtraction the dominance
//! rule started.
//!
//! ## What it said, 2026-09-09: the memo is worth MORE than it looked, and for a reason
//!
//! Nine groups, twenty menus each, at ninety-five and fifty per cent seen:
//!
//! ```text
//!   asks in the ceiling                    9882
//!   of those, repeats                      8276  83.7%
//!   asks that reached the driver           9559  96.7%
//!   passes that settled                    5498  57.5%
//!   passes that met                        4061  42.5%
//!   cacheable - settled, did not meet      5498  57.5%
//!   repeats a memo would answer            5615  67.8%
//!   passes a memo would replace            5615  58.7%
//!
//!   one cacheable pass, ms                 19.3
//!   every pass, ms                       153736
//! ```
//!
//! 5,615 repeat asks at 19.3 ms is 108 SECONDS OF THE 154 these walks spend on passes. That
//! is the number de-znov.3 is worth, and it is the ceiling multiplied by the share of asks
//! that end in a verdict worth keeping, which is what de-znov.2 came for.
//!
//! ALMOST EVERYTHING NOW REACHES THE DRIVER - 9,559 of 9,882, against a third of that when
//! an option could be answered by a second search running ahead of it. What is left out is
//! the handful of options whose START already carries the class being hunted, and they are
//! concentrated in the fifty-per-cent walks where plenty is unread.
//!
//! ## Two fifths of the passes MEET, and that is the interesting half
//!
//! Not one pass met when a second search answered the easy options first. Now 4,061 of
//! 9,559 do. The reason is structural rather than a regression: the options that used to be
//! answered early are exactly the ones from which something novel is REACHABLE, so their
//! passes meet what the search holds at its start and stop having proved it. A met pass is
//! a proof for one seed and not a fixed point anyone may keep - see `symbolic::memo` - so
//! it is not cacheable, and the cacheable share falls from everything to 57.5 per cent.
//!
//! THE SHARE FELL AND THE COUNT ROSE, which is the reading that matters: 5,498 cacheable
//! passes against 3,361, because the population it is a share OF nearly tripled. de-znov.2's
//! plausible failure - that the asks which recur are the cheap ones that meet early, while
//! the ones that cost never settle - is now half true and does not close the issue: a memo
//! still answers 67.8 per cent of the repeats.
//!
//! AND THE SPREAD PER GROUP IS WIDE, which no total shows. Read `of ran` in the per-walk
//! table: conversation 14 at ninety-five per cent seen has every one of its 953 passes
//! cacheable and 90.8 per cent of its asks answerable from a memo, while 640 at the same
//! profile is at 9.2 per cent and 631 at fifty per cent is at 7.6. A design that assumed
//! the average would be wrong about most groups in both directions.
//!
//! ## WHAT IT COSTS IS MEMORY, WHICH IS THE HALF TO WORRY ABOUT
//!
//! Conversation 14's walk left 5.5 million diagram nodes in the manager, and at
//! `DiagramBudget::BYTES_PER_NODE` that is most of the player's 256 MB, for a store holding
//! one walk's guards and passes. A memo keeps a subset of that ALIVE across requests where
//! today it is dropped with the query, so de-znov.3's design question is not whether the
//! verdicts are worth keeping - they are, and by more than before - but how many may be
//! kept at once.
//!
//! ## How to run it
//!
//! ```text
//! DEGCT_RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo cacheable-asks -- \
//!   cargo run --release --example cacheable_asks
//! ```
//!
//! `CONVERSATION` picks the groups, `PROFILES` the percentages, `MENUS` how many menus to
//! take from each group, `EACH_MS` what one pass may spend and `BUDGET_MB` the manager's
//! allowance.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use lookahead_engine::bridge::{SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::answer;
use lookahead_engine::symbolic::backward::{Backward, Budget as BackwardBudget};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::known::GroupShape;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;

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
    let each = Duration::from_millis(from_env(
        "EACH_MS",
        answer::Budget::default().each.as_millis() as usize,
    ) as u64);

    println!(
        "WHAT A MEMO BETWEEN REQUESTS COULD HOLD. The walk `candidate_recurrence` counts, \
         with the\npasses the driver would actually run at each menu.\n"
    );
    println!(
        "A CACHEABLE PASS IS ONE THAT SETTLED AND DID NOT MEET. An unsettled pass holds a \
         subset\nof what it would have held; a met one holds a proof for the seed it met \
         against.\n"
    );
    println!(
        "{} MB, {menus_wanted} menus a walk, {} ms a pass\n",
        budget.memory() / (1024 * 1024),
        each.as_millis(),
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
            let Some(ran) = arm(&graph, &menus, budget, each) else {
                eprintln!("conversation {conversation}: no room for the manager; skipping.");
                continue;
            };

            // A LINE AS EACH WALK LANDS, because every table here is printed at the end and
            // a heavy group is minutes of silence otherwise. It says what the walk cost and
            // what it found, which is enough to follow a run rather than wait one out.
            println!(
                "  ... {conversation} at {percent}pc-seen: {} menus, {} asks ran of {}, \
                 {} cacheable, {} saved, {:.0} ms",
                ceiling.menus, ran.asks, ceiling.asked, ran.cacheable, ran.saved, ran.millis,
            );
            every.push(Walked {
                conversation,
                percent,
                ceiling,
                ran,
            });
        }
    }

    if every.is_empty() {
        println!("nothing walked.");
        return;
    }

    per_walk(&every);
    reached(&every);
    what_a_pass_costs(&every);
    the_answer(&every);
}

/// One group under one profile: the ceiling, and what the walk spent reaching it.
struct Walked {
    conversation: i32,
    percent: u32,
    ceiling: Recurrence,
    ran: Arm,
}

/// What one walk found.
#[derive(Default)]
struct Arm {
    /// Asks that never reached the backward driver, and why.
    ///
    /// `no_hunt` is an option with nothing better than the floor link-reachable, which the
    /// bridge refuses before a diagram is touched; `at_the_start` is one whose start already
    /// carries the class being hunted, which the driver answers without searching. Neither
    /// is an ask a memo could serve, and both are part of the same subtraction the dominance
    /// rule started - so they are reported rather than dropped.
    no_hunt: usize,
    at_the_start: usize,
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
    /// EVERYTHING, not just what a memo would keep: the compiled guards and every pass's
    /// sets share this one store. A memo would hold a subset, and this says what the subset
    /// is inside.
    manager_nodes: usize,
}

impl Arm {
    fn add(&mut self, other: &Arm) {
        self.no_hunt += other.no_hunt;
        self.at_the_start += other.at_the_start;
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

/// One walk's passes.
///
/// A THREAD OF ITS OWN WITH THE MANAGER INSIDE IT - de-fpax, and `symbolic::isolated` for
/// why the manager cannot be handed in.
fn arm(
    graph: &LookAheadGraph,
    menus: &[Menu],
    budget: DiagramBudget,
    each: Duration,
) -> Option<Arm> {
    isolated::on_its_own_thread(|| {
        let symbols = graph.symbols().clone();
        let world = SnapshotWorld::declaring(
            WorldSnapshot {
                day_minutes: 720,
                day_counter: 1,
                ..Default::default()
            },
            None,
        );
        let layout = DataLayout::for_group(graph, &world, COUNTER_CAP);
        let vars = DataVars::try_new(&layout, &symbols, budget)?;
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
        let seed = seed_of(graph, &world, &vars).expect("room for a seed");
        let shape = GroupShape::of(graph);

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

                // THE START ANSWERS FOR ITSELF where it already carries the class being
                // hunted, which the driver checks before it touches a diagram. It asks
                // about nothing, so counting its targets as asks would inflate the ceiling.
                if novelty(start) == hunting {
                    counted.at_the_start += targets.len();
                    continue;
                }

                let known = shape
                    .known_from(graph, start)
                    // WHAT THE SEARCH HOLDS ARRIVING AT ITS START. `Where::of` is what the
                    // driver calls, and for a start entered either way - which every option
                    // of a walked menu is - it yields exactly this pair.
                    .from(start, &seed);

                let pass = BackwardBudget {
                    steps: usize::MAX,
                    time: each,
                    ..Default::default()
                };
                for target in targets {
                    let began = Instant::now();
                    let backward = Backward::reaching_knowing(
                        graph,
                        target,
                        &mut compiler,
                        &world,
                        COUNTER_CAP as u32,
                        &pass,
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
/// option: what is hunted is decided per start, so the passes have to be taken a start at
/// a time. An arm
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

/// Every walk, one row each: the ceiling, and how much of it survives.
fn per_walk(every: &[Walked]) {
    println!("PER WALK. `asked` and `repeat` are the ceiling; `saved` is the repeats a memo");
    println!("would have answered, which needs the first ask to have settled without meeting.\n");
    println!(
        "{:>6}  {:>10}  {:>6}  {:>7}  {:>7}  {:>7}  {:>7}  {:>7}  {:>9}  {:>7}",
        "conv",
        "profile",
        "menus",
        "asked",
        "repeat",
        "ran",
        "cacheab",
        "saved",
        "of repeat",
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
            walked.ran.asks,
            walked.ran.cacheable,
            walked.ran.saved,
            share(walked.ran.saved, walked.ceiling.repeat),
            share(walked.ran.saved, walked.ran.asks),
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
    println!("`no hunt` is an option refused before a diagram is touched, and `at start` one");
    println!("whose start already carries the class being hunted.\n");
    println!(
        "{:>6}  {:>10}  {:>8}  {:>8}  {:>9}  {:>7}  {:>8}",
        "conv", "profile", "asked", "no hunt", "at start", "ran", "options",
    );
    for walked in every {
        let arm = &walked.ran;
        println!(
            "{:>6}  {:>10}  {:>8}  {:>8}  {:>9}  {:>7}  {:>8}",
            walked.conversation,
            format!("{}pc-seen", walked.percent),
            walked.ceiling.asked,
            arm.no_hunt,
            arm.at_the_start,
            arm.asks,
            arm.options,
        );
    }
}

/// What one cacheable pass costs, in the two currencies a memo spends.
fn what_a_pass_costs(every: &[Walked]) {
    println!("\nWHAT ONE CACHEABLE PASS COSTS, which is what a memo would be filled from.");
    println!("The nodes matter as much as the milliseconds: a memo holds BDD sets in the");
    println!("player's 256 MB, so a saving that costs the manager is not a saving.\n");
    println!(
        "{:>6}  {:>10}  {:>8}  {:>9}  {:>11}  {:>11}  {:>13}",
        "conv", "profile", "cacheab", "ms each", "nodes each", "worst pass", "manager end",
    );
    for walked in every {
        let arm = &walked.ran;
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
         what the\nwhole walk actually left in the store - every guard and every pass - and a\n\
         memo holds a subset of that rather than the sum."
    );
}

/// The three numbers de-znov.3 lives or dies on.
fn the_answer(every: &[Walked]) {
    let mut ceiling = Recurrence::default();
    let mut ran = Arm::default();
    for walked in every {
        ceiling.add(&walked.ceiling);
        ran.add(&walked.ran);
    }

    println!("\nOVER EVERY WALK:\n");
    counted("menus walked", ceiling.menus);
    counted("asks in the ceiling", ceiling.asked);
    part("of those, repeats", ceiling.repeat, ceiling.asked);
    part("asks that reached the driver", ran.asks, ceiling.asked);
    part("passes that settled", ran.settled, ran.asks);
    part("passes that met", ran.met, ran.asks);
    part("cacheable - settled, did not meet", ran.cacheable, ran.asks);
    part("repeats a memo would answer", ran.saved, ceiling.repeat);
    part("passes a memo would replace", ran.saved, ran.asks);
    println!();
    millis("one cacheable pass, ms", ran.each_cacheable_ms());
    counted(
        "one cacheable pass, nodes",
        ran.each_cacheable_nodes().round() as usize,
    );
    counted("every cacheable pass summed", ran.cacheable_nodes);
    counted("the worst manager at walk's end", ran.manager_nodes);
    println!();
    millis("every pass, ms", ran.millis);

    println!(
        "\nTHE MEMO IS WORTH {:.0} MS OF THE {:.0} MS these walks spend on passes - the {} \
         repeat\nasks it would have answered, at what one of them cost. That is the ceiling \
         multiplied by\nthe share of asks that end in a verdict worth keeping, which is what \
         de-znov.2 came for.",
        ran.saved as f64 * ran.each_cacheable_ms(),
        ran.millis,
        ran.saved,
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
    match lookahead_engine::core::env::var("PROFILES") {
        Ok(text) => text
            .split(',')
            .filter_map(|part| part.trim().parse().ok())
            .collect(),
        Err(_) => PERCENTS.to_vec(),
    }
}

fn from_env(name: &str, fallback: usize) -> usize {
    lookahead_engine::core::env::var(name)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(fallback)
}

fn numbers(name: &str, fallback: &[i32]) -> Vec<i32> {
    match lookahead_engine::core::env::var(name) {
        Ok(text) => text
            .split(',')
            .filter_map(|part| part.trim().parse().ok())
            .collect(),
        Err(_) => fallback.to_vec(),
    }
}
