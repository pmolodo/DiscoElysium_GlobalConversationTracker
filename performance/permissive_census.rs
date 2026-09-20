// SPDX-License-Identifier: MIT
//! What is unreachable in EVERY possible save, rather than in one of them.
//!
//! ## The question, and why the census already taken does not answer it
//!
//! A reachability verdict is about ONE WORLD. The layout is built from a save, the seed pins
//! every slot to that save's values, and the guard compiler answers from it - so "entry 436:14
//! is unreachable" means unreachable from that save, and a different save can reach it. The
//! money ceiling alone was enough to move an answer from 4 unreachable candidates to 10 or
//! more on group 436.
//!
//! That makes the verdict uncacheable and unshippable: nothing can be told to a player about
//! an entry on the strength of a measurement save.
//!
//! LOOSENING THE WORLD CAN ONLY ADD PATHS. So an entry still proved unreachable against a
//! world that constrains NOTHING is unreachable from every save there is - a sound
//! under-approximation rather than a guess, and the only part of the census that is
//! cacheable. This measures how big that part is, which decides whether de-a6ws is worth
//! building at all: if a permissive census finds almost nothing, structural reachability is
//! already the whole cacheable story.
//!
//! ## What the permissive arm actually changes, and why each piece is needed
//!
//! Three places read the world, and all three have to be loosened together - loosening one
//! and not the others gives a world that is permissive about some questions and specific
//! about others, which is not a world any save could be.
//!
//! 1. THE GUARD COMPILER GETS NO WORLD AT ALL. `GuardCompiler` without one returns
//!    `undecided` for every question about a variable, an item, a thought or a query, and
//!    `may_be_true` lets an undecided gate through. That IS the permissive reading, and it
//!    falls out of the existing code rather than needing a new mode.
//! 2. THE SEED IS `vars.top()` rather than `seed_of`. Every slot free, money included.
//!    `seed_of`'s own doc warns that leaving money free "starts the search rich AND poor at
//!    once, which undoes the whole of `affordable`" - and here that is the point, because a
//!    permissive world is one where the player may have any amount.
//! 3. THE LAYOUT DROPS NO COUNTERS. `DataLayout::dropping_redundant_counters` folds a
//!    CONSTANT taken from the save - the counter's value less the sites the save records as
//!    shown - into the guards that read it. Sound for one world, meaningless for all of
//!    them, since the outside contribution is exactly what a permissive world leaves free.
//!    So the permissive layout is `for_group_entered_at_under` minus that one step.
//!
//! ## What is NOT permissive here, and it is the honest limit of the number
//!
//! THE CLOCK IS PINNED. `ActionImage::for_world` reads `day_minutes` and `day_counter` off
//! the world, and a permissive world has no honest answer - `at_clock`'s doc says a reading
//! that is not one number for the whole set leaves the slot FORGOTTEN, which is what is
//! wanted, but `Backward` builds the image through `for_world` and there is no way in. So an
//! entry reachable only after the clock has moved past a threshold can still be called dead,
//! which is the one direction that matters.
//!
//! SO IT IS TESTED RATHER THAN DISCLAIMED. `--at-minute` and `--at-day` move the reading, and
//! the `clock` column flags every group that can move it itself. A count that changes between
//! two readings was decided by the hour; one that does not was decided by the content. The
//! single dead verdict this found, on group 786, is the same verdict at day 1 midnight and at
//! day 4 in the evening, so it is not an artefact of either.
//!
//! `initially_has_item` and `initially_has_thought` return a plain `bool` with no way to say
//! "maybe", which looked like the same problem and is not: with no world in the compiler and
//! no `seed_of`, nothing in this arm ever calls them. They are answered `false` and it does
//! not reach the answer.
//!
//! ## How to run it
//!
//! ```text
//! tools/run-logged.sh --kind analysis cargo permissive-census -- \
//!   cargo run --release --example permissive_census
//! ```
//!
//! It is `analysis` rather than `performance`: what comes out is a dataset about the
//! database, not a timing to compare against another timing, so it pays no cold-run tax.
//!
//! ## WHAT IT ANSWERED, and the answer is no
//!
//! Eight groups, forty candidates apiece, ten seconds a target:
//!
//! ```text
//!  group   cands  asked dead:save undec:save dead:every undec:every  clock
//!      7      32     32         0          0          0           0      -
//!     16    2311     40        28          9          0          13      -
//!    436      30     30        17          0          0           0      -
//!    850     974     40        13          0          0           0  moves
//!    786    1041     40        32          0          1           0  moves
//!    617     224     40        27          0          0           0      -
//!   1093    1204     40        23          0          0           0      -
//!   1203    1058     40        30          0          0           0      -
//!
//! candidates asked about   302
//! proved dead from the save 170
//! PROVED DEAD IN EVERY SAVE   1
//! ```
//!
//! 170 OF 302 AGAINST ONE SAVE, ONE AGAINST ALL OF THEM. More than half of what the census
//! calls unreachable is the save talking, and what survives a world that says nothing is a
//! single entry in group 786. Even reading the permissive column as the floor it is - group
//! 16's 13 undecided targets could in principle all be dead - the ceiling is fourteen.
//!
//! SO THE IDEA IS ANSWERED NEGATIVE. There is no worthwhile cache of "unreachable in every
//! save" to ship, because there is almost nothing in it; edge-only structural reachability,
//! which is already cacheable and already computed, is the whole shippable story. The thing
//! that makes a verdict useful is exactly the thing that makes it uncacheable, and this
//! measures how completely: almost entirely.
//!
//! This is kept rather than deleted because it is the evidence for that no, and because the
//! same rig answers the middle road de-a6ws also proposed - a cache keyed on the projection
//! onto the variables a group's guards actually read, which is a different and still open
//! question.

use lookahead_engine::core::guard_value::GuardValue;
use lookahead_engine::core::state::VariableRef;
use lookahead_engine::core::types::{DialogueNodeId, Ternary};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::backward::{Backward, Budget, SettledPass};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated::on_its_own_thread;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::world::ILookAheadWorld;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "options.rs"]
mod options;

#[path = "seen_profile.rs"]
mod seen_profile;

const COUNTER_CAP: i32 = 16;

/// A spread rather than the expensive list, and DELIBERATELY WITHOUT the heavy groups.
///
/// This is the cheap measurement that decides whether the expensive one is worth building,
/// so it must finish. 14 and 368 do not settle at six gigabytes and ten minutes on ONE
/// target, and this asks about many - naming them here would mean the run never reports
/// anything at all. 436 is in it because it is the group whose money ceiling moved an answer
/// from 4 unreachable to 10, which is the observation the whole idea came from. Name them on
/// the command line when there is a reason to wait.
const SPREAD: [i32; 8] = [7, 16, 436, 850, 786, 617, 1093, 1203];

/// What this driver takes. With no group named it uses the spread above.
#[derive(clap::Parser)]
#[command(about = "What is unreachable in every save, not just in the measurement one.")]
struct Options {
    #[command(flatten)]
    groups: options::Groups,

    /// How many of a group's candidates to ask about, spread over its own order.
    ///
    /// A pass per candidate is the cost here, and group 16 has 2,311 of them. A sample says
    /// what fraction is dead, which is the question, and the whole set says the same thing
    /// for a great deal more money.
    #[arg(long, default_value_t = 40)]
    candidates: usize,

    /// How long one target's backward pass may run before it gives up undecided.
    ///
    /// The default pass allows two minutes, which is right for a search that must answer and
    /// wrong for a census that would rather report an undecided row than stop. Undecided is
    /// counted and printed, so a budget too small shows up as a column rather than as a
    /// wrong number.
    #[arg(long, default_value_t = 10)]
    seconds: u64,

    /// What time it is for the permissive world, in minutes past midnight.
    ///
    /// The clock is the one thing this arm cannot leave free, so run twice and compare: a
    /// count that moves between two readings was decided by the hour rather than by the
    /// content, and a dead verdict that survives both is not an artefact of either.
    #[arg(long, default_value_t = 0)]
    at_minute: i32,

    /// Which day it is for the permissive world. The same caveat as `--at-minute`.
    #[arg(long, default_value_t = 1)]
    at_day: i32,
}

/// A world that constrains NOTHING, for the arm that asks about every save at once.
///
/// NOT AN EMPTY SAVE, which is a different thing and the trap the task warned about: a save
/// with nothing set is a world where every flag is FALSE, as specific as any other. Every
/// answer here is the one that refuses to decide, so a guard reading any of them stays
/// satisfiable and the search is free to walk through it.
///
/// The clock is the exception, and the module doc says what it costs. It is a FIELD rather
/// than a constant so that the cost can be measured instead of assumed: run the same groups
/// at two readings, and any row whose count moves was decided by the clock rather than by the
/// content. A number that survives every reading is one the pinning did not manufacture.
struct EverySave {
    minute: i32,
    day: i32,
}

impl ILookAheadWorld for EverySave {
    /// Zero, which is not a claim about the purse. Money is read from here only by
    /// `DataLayout::money_ceiling`, which takes it as a LOWER bound on how wide the register
    /// must be and widens it to the dearest price in the group plus everything the group can
    /// earn. The seed then leaves the register free, so every amount up to that ceiling is
    /// in play and no price is refused for want of a balance.
    fn money(&self) -> i32 {
        0
    }

    /// The one place this arm is not permissive - see the module doc and `--at-minute`.
    fn day_minutes(&self) -> i32 {
        self.minute
    }

    fn day_counter(&self) -> i32 {
        self.day
    }

    /// Unlocked, because a locked clock is a claim that time cannot pass.
    fn is_clock_locked(&self) -> bool {
        false
    }

    fn get_variable(&self, _variable: VariableRef<'_>) -> GuardValue {
        GuardValue::unknown()
    }

    /// Never reached in this arm - the compiler has no world and the seed is not built from
    /// one, which are the only two callers. See the module doc.
    fn initially_has_item(&self, _name: &str) -> bool {
        false
    }

    fn initially_has_thought(&self, _name: &str) -> bool {
        false
    }

    fn query(&self, _name: &str, _arguments: &[GuardValue]) -> GuardValue {
        GuardValue::unknown()
    }

    /// Unknown, so `never_displays` is never true and no passive check is failed in advance
    /// on behalf of a character sheet this world does not have.
    fn check_passes(&self, _node: DialogueNodeId) -> Ternary {
        Ternary::Unknown
    }

    /// Nothing has been seen, which for this arm is a formality: the seed leaves every
    /// `seen:` slot free, so what is recorded here never constrains anything.
    fn is_seen(&self, _node: DialogueNodeId) -> bool {
        false
    }

    /// Every red check may pass. A thought can force them all to fail, and a world that
    /// stands for every save must include the ones without it.
    fn red_check_may_pass(&self, _node: DialogueNodeId) -> bool {
        true
    }
}

/// The layout the permissive arm searches under: everything a specialised one has except the
/// counter substitution, which is the only step that folds a save's value into a guard.
fn permissive_layout(graph: &LookAheadGraph, world: &dyn ILookAheadWorld) -> DataLayout {
    DataLayout::for_graph(
        graph,
        COUNTER_CAP,
        DataLayout::money_ceiling(graph, world.money()),
        false,
    )
    .keeping_only_read(graph.symbols(), &DataLayout::read_by(graph))
    .in_variable_order(graph, Default::default())
}

/// What one arm made of the candidates it was given.
///
/// THREE OUTCOMES RATHER THAN TWO, and the third is the whole reason this is a struct. A
/// backward pass that spends its budget without settling has NOT proved anything: its sets
/// are a lower bound, so `reachable_from` says no for a target it simply did not get to.
/// Counting those as dead would report unreachable content that is merely unfinished
/// business - the one direction that matters, since the point of the permissive arm is a
/// sound under-approximation.
#[derive(Default)]
struct Verdicts {
    /// Reached, which is a proof and needs no fixed point.
    alive: usize,
    /// Not reached AND the fixed point was complete: a proof of the negative.
    dead: usize,
    /// Not reached and the pass gave up first. Says nothing either way.
    undecided: usize,
    /// Whether the diagram manager ran out of room on any target.
    out_of_room: bool,
}

/// What each arm made of `candidates`, searching from `start` under one world.
///
/// The seed is what the search holds on arrival at the start: the save's state in the
/// specialised arm, and everything at once in the permissive one.
fn verdicts_under(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    candidates: &[DialogueNodeId],
    layout: &DataLayout,
    world: &dyn ILookAheadWorld,
    permissive: bool,
    seconds: u64,
) -> Option<Verdicts> {
    // BUILT HERE RATHER THAN PASSED IN. `Budget` carries its progress hook in an `Rc`, so it
    // is not `Send` and cannot cross into the thread each arm runs on; the number of seconds
    // is, and the budget is one line to rebuild from it.
    let budget = Budget {
        time: std::time::Duration::from_secs(seconds),
        ..Budget::default()
    };
    let symbols = graph.symbols().clone();
    let vars = DataVars::new(layout, &symbols, DiagramBudget::over_a_group());
    let mut compiler = GuardCompiler::new(&vars);
    if !permissive {
        compiler = compiler
            .with_world(world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
    }

    let seed = if permissive {
        vars.top()
    } else {
        seed_of(graph, world, &vars)?
    };

    let mut found = Verdicts::default();
    for target in candidates {
        let backward = Backward::reaching_within(
            graph,
            *target,
            &mut compiler,
            world,
            COUNTER_CAP as u32,
            &budget,
        );
        let stats = backward.stats();
        found.out_of_room |= stats.out_of_memory;
        if backward.reachable_from(start, &seed) {
            found.alive += 1;
        } else if stats.reached_fixed_point && !stats.out_of_memory {
            found.dead += 1;
        } else {
            found.undecided += 1;
        }
    }
    Some(found)
}

/// At most `most` of `candidates`, spread evenly over the order they arrive in.
///
/// EVENLY RATHER THAN THE FIRST N, because `candidates` comes back deepest-first and the
/// deepest entries are not a sample of anything - they are the end of the graph, and the end
/// of the graph is where unreachable entries live. Taking the first N would report a
/// fraction dead that is far above the group's own.
fn spread_over(candidates: &[DialogueNodeId], most: usize) -> Vec<DialogueNodeId> {
    if candidates.len() <= most || most == 0 {
        return candidates.to_vec();
    }
    let step = candidates.len() / most;
    candidates
        .iter()
        .copied()
        .step_by(step.max(1))
        .take(most)
        .collect()
}

/// Whether the group can MOVE the clock, which is where a pinned one is an approximation.
///
/// A group that never passes time is at one reading for the whole search, so pinning it
/// costs nothing there; a group that does has entries this arm may call unreachable for want
/// of a later hour. The column exists so that exposure is counted rather than assumed.
fn moves_the_clock(graph: &LookAheadGraph) -> bool {
    DataLayout::group_passes_time(graph)
}

fn main() {
    let asked = <Options as clap::Parser>::parse();
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let save = common::measurement_save();
    let every = EverySave {
        minute: asked.at_minute,
        day: asked.at_day,
    };

    // Spelled out rather than formatted from the column names: the widths below are what
    // line the rows up, and a header built from the same widths reads as though it were
    // derived from them when it is only agreeing with them.
    println!(" group   cands  asked dead:save undec:save dead:every undec:every  clock  note");

    let (mut asked_total, mut dead_save, mut dead_every) = (0usize, 0usize, 0usize);
    for conversation in asked.groups.or(&SPREAD) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let all = seen_profile::candidates(&graph, start);
        if all.is_empty() {
            println!("{conversation:>6} {:>7}  nothing to ask about", 0);
            continue;
        }
        let candidates = spread_over(&all, asked.candidates);

        let specialised = DataLayout::for_group(&graph, &save, COUNTER_CAP);
        let permissive = permissive_layout(&graph, &every);
        let clock = moves_the_clock(&graph);

        // A THREAD PER ARM, with each manager built inside it - de-fpax. The counts that
        // come back are plain data; the sets they were read off do not leave.
        let under_save = on_its_own_thread(|| {
            verdicts_under(
                &graph,
                start,
                &candidates,
                &specialised,
                &save,
                false,
                asked.seconds,
            )
        });
        let under_every = on_its_own_thread(|| {
            verdicts_under(
                &graph,
                start,
                &candidates,
                &permissive,
                &every,
                true,
                asked.seconds,
            )
        });

        let note = match (&under_save, &under_every) {
            (None, _) => "no seed: the manager filled".to_string(),
            (_, None) => "permissive arm gave no seed".to_string(),
            (Some(a), Some(b)) if a.out_of_room || b.out_of_room => {
                "OUT OF ROOM - dead counts are floors".to_string()
            }
            _ => String::new(),
        };
        let save_seen = under_save.unwrap_or_default();
        let every_seen = under_every.unwrap_or_default();

        asked_total += candidates.len();
        dead_save += save_seen.dead;
        dead_every += every_seen.dead;
        println!(
            "{conversation:>6} {:>7} {:>6} {:>9} {:>10} {:>10} {:>11} {:>6}  {note}",
            all.len(),
            candidates.len(),
            save_seen.dead,
            save_seen.undecided,
            every_seen.dead,
            every_seen.undecided,
            if clock { "moves" } else { "-" }
        );
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }

    println!();
    println!("candidates asked about         {asked_total}");
    println!("proved dead from the save      {dead_save}");
    println!("PROVED DEAD IN EVERY SAVE      {dead_every}");
    println!();
    println!(
        "The second number is the cacheable one, and it is a FLOOR: an undecided target is a\n\
         pass that ran out of budget, not a target shown reachable. If it is near zero,\n\
         structural reachability is already the whole shippable story and de-a6ws is not\n\
         worth building; if it is a useful fraction of the first, it is content no save can\n\
         reach, and a marker on it never comes off."
    );
}
