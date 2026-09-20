// SPDX-License-Identifier: MIT
//! What does it cost to prove ONE target unreachable?
//!
//! ## The question
//!
//! From the template save, conversation 761's menu costs seconds and settles nothing, while a
//! greedy walk over the same group finishes in EIGHTEEN MILLISECONDS:
//! `performance/greedy_playthrough.rs` plays 37 legs, shows 44 entries of 2,263, and stops
//! EXHAUSTED. See de-y04p.
//!
//! WHAT "EXHAUSTED" DOES AND DOES NOT SAY, because the difference is the whole question. It says
//! THAT WALK could reach nothing unshown from where it stood or from a restart. It does NOT say
//! the conversation is closed whatever route is taken: a playthrough makes choices, and a choice
//! can strand content another route would have reached. So a cheap walk finishing is not a proof
//! that there is nothing to find, and the search is not merely re-proving what the walk proved.
//!
//! WHAT IS ACTUALLY PROVED, and it is narrower: a backward pass that reaches a FIXED POINT has
//! shown its target unreachable from the options it was given, in this world - over every route,
//! which a walk cannot claim. That is the thing worth timing here.
//!
//! IT IS NOT THE LINK PREFILTER EITHER. A target no uncut link route reaches is dropped before
//! any diagram is touched: `symbolic::menu` intersects the hunted class with what the options
//! reach along links with the hub cut in place, since guards can only remove a route, never make
//! one.
//!
//! So this times one target at a time, which nothing else does: the matrix reports what a whole
//! menu cost and `onward_or_back` asks per OPTION.
//!
//! ## What it prints
//!
//! One row per target, dearest first: whether the pass met what the search already knew (a
//! proof the target IS reachable), whether it reached a fixed point (a proof it is NOT), and
//! what the pass spent getting there.
//!
//! ```text
//!   target     met   fixed   steps   widenings   entries   nodes   largest      ms
//! ```
//!
//! A target that is REACHED stops the pass early and is cheap. One that is not has to be
//! proved unreachable, and the fixed point is the whole cost. The totals say which kind 761 is.
//!
//! ## How to run it
//!
//! ```text
//! tools/run-logged.sh --kind analysis cargo target-cost -- \
//!   cargo run --release --example target_cost -- --conversation 761
//! ```
//!
//! `--unseen` says how many of the structurally deepest entries count as targets, which is
//! what makes this the link-deepest-X profile; `--each-ms` bounds one pass, and a pass that
//! runs out says so in `fixed` rather than being dropped. `--save` names the save the world
//! is built from, and the default is the one 761's expensive arm was measured against.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use lookahead_engine::bridge::SnapshotWorld;
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::build_group_graph;
use lookahead_engine::symbolic::backward::{Backward, Budget as PassBudget};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::known::GroupShape;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::search::Search;
use lookahead_engine::symbolic::seen_state_search::Where;
use lookahead_engine::symbolic::vars::DataVars;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "options.rs"]
mod options;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

#[path = "prepared.rs"]
mod prepared;
use prepared::Shipped;

#[path = "save_world.rs"]
mod save_world;

/// The group the question is about.
const CONVERSATIONS: [i32; 1] = [761];

/// How many of the structurally deepest entries count as unread.
const UNSEEN: usize = 5;

/// How wide the menu is.
const STARTS: usize = 8;

/// The most one target's pass may run for. Generous: a pass that is stopped says so, and a
/// stopped pass is a cost this measurement wants to see rather than hide.
const EACH_MS: u64 = 30_000;

/// What the plugin gives a menu, so a row here is what a player's request pays.
const BUDGET_MB: usize = 256;

const COUNTER_CAP: i32 = 16;

/// What this driver takes.
///
/// HANDED DOWN to `ask`, which reads the per-pass ration twice and sits below `main`.
#[derive(clap::Parser)]
#[command(about = "What each target of a menu costs to answer, one row apiece.")]
struct Options {
    #[command(flatten)]
    groups: options::Groups,
    #[command(flatten)]
    starts: options::Starts<STARTS>,
    #[command(flatten)]
    unseen: options::Unseen<UNSEEN>,
    #[command(flatten)]
    budget: options::Budget<BUDGET_MB>,
    /// Which save the walked profile is built from
    #[arg(long, value_name = "NAME", default_value = save_world::TEMPLATE)]
    save: String,
    /// What one pass is allowed, in milliseconds
    #[arg(long = "each-ms", value_name = "MS", default_value_t = EACH_MS)]
    each_ms: u64,
    #[command(flatten)]
    caching: prepared::Caching,
}

fn main() {
    let asked = <Options as clap::Parser>::parse();
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let shipped = Shipped::at(path, asked.caching);
    let budget = DiagramBudget::new(asked.budget.bytes());
    let save = asked.save.clone();

    for conversation in asked.groups.or(&CONVERSATIONS) {
        let Ok((graph, _)) = build_group_graph(shipped.index(), conversation) else {
            eprintln!("conversation {conversation}: no group builds from it; skipping.");
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        let Some(profile) = MenuProfile::of(&graph, root, asked.unseen.unseen, asked.starts.starts)
        else {
            println!("conversation {conversation}: no menu");
            continue;
        };
        isolated::on_its_own_thread(|| {
            ask(
                conversation,
                &graph,
                &profile,
                &shipped,
                &save,
                budget,
                &asked,
            );
            Some(())
        });
    }
}

/// Every target of `profile`, asked one at a time.
fn ask(
    conversation: i32,
    graph: &LookAheadGraph,
    profile: &MenuProfile,
    shipped: &Shipped,
    save: &str,
    budget: DiagramBudget,
    asked: &Options,
) {
    let symbols = graph.symbols().clone();
    // A WORLD THAT ANSWERS, which is the whole point: a default world decides nothing, so every
    // guard is passable and no target is ever proved unreachable - the case this is not about.
    let world = SnapshotWorld::declaring(
        save_world::of_save(graph, conversation, shipped, save),
        save_world::declared(),
    );
    let layout = DataLayout::for_group(graph, &world, COUNTER_CAP);
    let Some(vars) = DataVars::try_new(&layout, &symbols, budget) else {
        eprintln!("conversation {conversation}: no room for the variables.");
        return;
    };
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(&world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, &world, &vars).expect("room for a seed");
    let shape = GroupShape::of(graph);

    // WHAT THE MENU KNOWS BEFORE IT ASKS ABOUT ANY ONE TARGET: every option's own states at its
    // own entries, which is what a pass is allowed to meet. Built once, as `symbolic::menu`
    // builds it once for the whole worklist pass rather than per target.
    let mut known = shape.known_from(graph, profile.starts[0]);
    for &start in &profile.starts {
        let position = Where::of(
            graph,
            start,
            lookahead_engine::core::types::StartBranch::Either,
            &seed,
            &mut compiler,
            &world,
            COUNTER_CAP as u32,
        )
        .position(start);
        for &entry in &position.entries {
            known = known.from(entry, &position.holding);
        }
    }

    // NOTHING CUT, which is what the gate asks: `symbolic::menu` passes the options REFUSED so
    // far, and at the first pass that is empty. Cutting every option instead removes every route
    // into the group and the pass proves nothing in no time - which is what this measured on its
    // first run, and is worth saying so nobody repeats it.
    let cut: HashSet<DialogueNodeId> = HashSet::new();
    let options: HashSet<DialogueNodeId> = profile.starts.iter().copied().collect();
    let positions: Vec<_> = profile
        .starts
        .iter()
        .map(|&start| {
            Where::of(
                graph,
                start,
                lookahead_engine::core::types::StartBranch::Either,
                &seed,
                &mut compiler,
                &world,
                COUNTER_CAP as u32,
            )
            .position(start)
        })
        .collect();
    let mut targets: Vec<DialogueNodeId> = profile.unseen.iter().copied().collect();
    targets.sort_unstable_by_key(|id| (id.conversation_id, id.entry_id));

    println!(
        "conversation {conversation}: {} target(s), {} option(s), world from {save}",
        targets.len(),
        options.len()
    );
    println!(
        "{:>12}  {:>5}  {:>5}  {:>8}  {:>9}  {:>8}  {:>10}  {:>9}  {:>8}",
        "target", "met", "fixed", "steps", "widenings", "entries", "nodes", "largest", "ms"
    );

    let mut rows = Vec::new();
    let mut distances = Vec::new();
    for target in targets {
        let began = Instant::now();
        let pass = Backward::reaching_any_knowing(
            Search {
                graph,
                compiler: &mut compiler,
                world: &world,
                counter_cap: COUNTER_CAP as u32,
            },
            &[target],
            &cut,
            &PassBudget {
                time: Duration::from_millis(asked.each_ms),
                steps: usize::MAX,
                ..Default::default()
            },
            Some(&known),
        );
        let stats = pass.stats();
        rows.push((
            target,
            stats.met_at.is_some(),
            stats.reached_fixed_point,
            stats.steps,
            stats.widenings,
            stats.entries_reaching,
            stats.diagram_nodes,
            stats.largest_set,
            began.elapsed().as_millis(),
        ));
        drop(pass);

        // AND THE SAME TARGET ASKED FOR A DISTANCE, which is what the EXACT marking asks once the
        // onward question has starred nothing. Reachability answers yes or no; this answers how
        // far, and it has to spread layer by layer until it meets an option or runs out - so the
        // two numbers side by side say whether the cost is in deciding the question or in
        // pricing the answer. See `symbolic::menu::mark_menu` and de-zxe0.
        let began = Instant::now();
        let near = Backward::nearest(
            Search {
                graph,
                compiler: &mut compiler,
                world: &world,
                counter_cap: COUNTER_CAP as u32,
            },
            target,
            &cut,
            &PassBudget {
                time: Duration::from_millis(asked.each_ms),
                steps: usize::MAX,
                ..Default::default()
            },
            &known,
            &positions,
        );
        distances.push((
            target,
            match near {
                lookahead_engine::symbolic::backward::Nearest::Found { distance, .. } => {
                    format!("{distance}")
                }
                lookahead_engine::symbolic::backward::Nearest::Unreachable => "none".to_string(),
                lookahead_engine::symbolic::backward::Nearest::Unfinished { out_of_memory } => {
                    if out_of_memory { "no room" } else { "gave up" }.to_string()
                }
            },
            began.elapsed().as_millis(),
        ));
    }

    rows.sort_by_key(|row| std::cmp::Reverse(row.8));
    for (target, met, fixed, steps, widenings, entries, nodes, largest, ms) in &rows {
        println!(
            "{:>12}  {:>5}  {:>5}  {:>8}  {:>9}  {:>8}  {:>10}  {:>9}  {:>8}",
            format!("{}:{}", target.conversation_id, target.entry_id),
            if *met { "yes" } else { "no" },
            if *fixed { "yes" } else { "no" },
            steps,
            widenings,
            entries,
            nodes,
            largest,
            ms,
        );
    }

    let shut = rows.iter().filter(|row| !row.1).count();
    let stopped = rows.iter().filter(|row| !row.1 && !row.2).count();
    println!(
        "\n{} of {} target(s) were not reached; {} of those did not even reach a fixed point",
        shut,
        rows.len(),
        stopped
    );
    println!(
        "reachability: {} ms, {} diagram nodes, {} steps, for all {} target(s)",
        rows.iter().map(|row| row.8).sum::<u128>(),
        rows.iter().map(|row| row.6).sum::<usize>(),
        rows.iter().map(|row| row.3).sum::<usize>(),
        rows.len(),
    );

    // THE SAME TARGETS, PRICED RATHER THAN DECIDED. `mark_menu` asks this once the onward
    // question has starred nothing, and the two totals beside each other are the answer to
    // where a menu that settles nothing spends its time.
    println!("\n{:>12}  {:>9}  {:>8}", "target", "distance", "ms");
    distances.sort_by_key(|row| std::cmp::Reverse(row.2));
    for (target, answer, ms) in &distances {
        println!(
            "{:>12}  {:>9}  {:>8}",
            format!("{}:{}", target.conversation_id, target.entry_id),
            answer,
            ms
        );
    }
    println!(
        "distance: {} ms for all {} target(s)",
        distances.iter().map(|row| row.2).sum::<u128>(),
        distances.len(),
    );
}
