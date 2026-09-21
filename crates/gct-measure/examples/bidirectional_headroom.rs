// SPDX-License-Identifier: MIT
//! What a FORWARD front costs per layer, against the backward one the search already walks.
//!
//! A backward search walks one direction only: from the target back towards the options,
//! one choice-layer at a time. On conversation 761 that is twenty-four layers and the
//! manager grows about 1.5x per layer, 912 thousand nodes to 64 million, which is the whole
//! cost of the group (de-0jsf.13).
//!
//! MEET IN THE MIDDLE IS THE OBVIOUS ANSWER TO A CURVE LIKE THAT. If a front from the
//! options and a front from the target each walked half the depth and met, each side would
//! pay 1.5^12 rather than 1.5^24 - and the distance still comes out exactly, as the forward
//! layer plus the backward layer at the meeting point.
//!
//! WHETHER IT HOLDS IS AN EMPIRICAL QUESTION AND THIS IS IT. The argument assumes the
//! forward front grows like the backward one. It need not: the two directions carry
//! different sets over the same graph, and a forward front that blows up faster would meet
//! sooner in layers and later in nodes, which is no saving at all. So this walks the forward
//! front and prints what it costs per layer, to be read against the backward trace taken
//! under the same profile and the same six gigabytes.
//!
//! ## What the walk is
//!
//! The same unit and the same charge as `seen_state_search::choice_bounds`: leaving a choice
//! that is not the option itself costs one, so every
//! route out of it belongs to the next layer, and the option the player stands on is free.
//!
//! The eight options' fronts are UNIONED rather than kept apart, which is what
//! a backward search effectively races: it stops at the first position it meets, so the
//! question is when ANY option's front arrives, not which.
//!
//! `Reachability::entry_states` is the forward step - what entering a node from a set of
//! states hands on - and it is the mirror of the `pre_enter` the backward pass uses. A node's
//! cumulative set grows the way the backward one does, and only the DELTA travels.
//!
//! ## What it prints
//!
//! One row per layer: entries whose set has been touched, entries carried into the next
//! layer, manager nodes, and elapsed milliseconds. `at target` says whether the front has
//! reached the named target yet, which is the forward-only distance when it first turns yes.
//!
//! ## What it said, 2026-09-11: IT HOLDS, and by a factor of thirty
//!
//! 761, eight options, ten unread, six gigabytes, target 1168:266 - the target whose single
//! backward pass costs 67 seconds and 64 million nodes. The forward front grows at about the
//! same 1.5x per layer the backward one does, so meeting is worth what the curve says:
//!
//! ```text
//!    split     forward    backward   sum nodes   sum ms
//!    10+14   1,039,387   3,127,189      4.17 M    2,097
//!    11+13   1,622,842   2,036,087      3.66 M    1,743
//!    12+12   2,522,480   1,435,096      3.96 M    1,937
//!    13+11   3,891,922   1,136,281      5.03 M    2,687
//!
//!    backward alone to layer 24        63.80 M   60,429
//! ```
//!
//! SEVENTEEN TIMES FEWER NODES AND THIRTY-FIVE TIMES LESS TIME, and 3.66 million nodes is a
//! figure the player's 256 MB can hold where 64 million is not. The group stops being
//! unanswerable and becomes ordinary.
//!
//! THE OPTIMUM IS NOT QUITE THE MIDDLE, and that is the useful detail. The forward front
//! grows faster than the backward one - 2.5 million against 1.4 at layer twelve - so the
//! cheapest meeting has the backward side going DEEPER, at eleven and thirteen. The curve is
//! flat around it, 3.66 against 3.96 a layer either way, so a scheduler that simply expands
//! whichever side is currently smaller lands within ten per cent of the best split without
//! being told where it is.
//!
//! DIRECTION STILL MATTERS, which is what makes that scheduler worth having rather than
//! merely tidy. Forward ALONE is far worse than backward alone: it reaches the target at
//! layer 23 having built 132.8 million nodes, against the backward pass's 56.3 million at the
//! same depth. A fixed rule that picked the forward side would lose; one that watches which
//! side is growing cannot.
//!
//! ## IT WAS BUILT, AND THEN REMOVED. This file is what is left of it
//!
//! The search this measurement argued for was written, verified and deleted, all of which is
//! worth knowing before anyone argues for it again.
//!
//! WHAT IT ACHIEVED, on 761 at six gigabytes: 10,846 ms and 10.1 million diagram nodes,
//! against 138,545 and 92.3 million for the one-way walk, with every option settled and
//! `tests/menu_oracle.rs` agreeing exactly - so the distances and winners were right, not
//! approximated. It answered that group from about 288 MB, where nothing had answered it at
//! any budget.
//!
//! TWO THINGS IT TAUGHT, both intrinsic to meeting rather than to this implementation:
//!
//! - THE MEETING ENTRY'S OWN CHARGE IS PAID BY NEITHER SIDE. The metric charges entries, and
//!   each side charges one only when it LEAVES it, so a forward layer has paid for the
//!   choices strictly before the meeting and a backward layer for those strictly after. The
//!   first version returned a distance one short.
//! - A UNIONED FORWARD FRONT'S EXEMPTION BELONGS TO THE SEEDING, not to the option entries.
//!   Exempting every option by identity makes a route that loops back through a SIBLING
//!   option free of its charge, which is exactly the hub shape the group is.
//!
//! WHY IT WENT ANYWAY. de-0jsf.20's hybrid answers the whole game in 11.9 seconds by asking a
//! cheaper question, and reaches the exact marking on 25 menus of 395 - none of them 761, 631
//! or 640. So the group this was built for stopped reaching it, and the groups that still
//! could are the two where it measured three times SLOWER: 631 at 1,936 ms against 573, and
//! 640 at 833 against 255. A faster search nothing fast needs is not worth the code.
//!
//! See de-0jsf.16 for the build and de-0jsf.27 for the removal. This measurement still runs,
//! and is the thing to run first if the question ever comes back.
//!
//! ## What this measurement does NOT establish
//!
//! - THE MEET TEST IS NOT COSTED. Conjoining a forward set with a backward set at every
//!   entry, every layer, to ask whether they intersect is work this does not do, and the
//!   sums above do not include it.
//! - THE FIRST MEET NEED NOT BE THE NEAREST once the two sides advance by different amounts.
//!   A bidirectional search that wants the true minimum carries on until the frontier depths
//!   sum past the best distance already found. de-0jsf.12 already settled for the first meet
//!   within a layer, so this is the same trade rather than a new one.
//! - THE FRONTS ARE ASSUMED TO MEET near the middle. They must, since the route exists and
//!   is twenty-four layers long, but WHERE their states intersect is not the same question as
//!   where their entries do, and only the entries were walked here.
//! - The two traces come from different processes with different baselines - a fresh manager
//!   here against one that had already done setup and a worklist pass there - so the sums are
//!   indicative and the GROWTH CURVES are the solid part.
//! - The forward walk races all eight starts where the backward pass raced only the options
//!   still hunting, which is why forward-only reports 23 against the backward pass's 24. That
//!   makes these forward figures conservative rather than flattering.
//!
//! ## How to run it
//!
//! ```text
//! tools/run-logged.sh cargo bidirectional -- \
//!   cargo run --release --example bidirectional_headroom -- \
//!   --conversation 761 --budget-mb 6144 --target 1168:266
//! ```
//!
//! `--layers` caps the walk, `--starts` and `--unseen` mean what they mean in
//! `menu_matrix`, so a reading here lines up with a row there.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use lookahead_engine::bridge::{GameWorld, WorldRawData};
use lookahead_engine::core::types::{DialogueNodeId, StartBranch};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::reachability::{Reachability, seed_of};
use lookahead_engine::symbolic::seen_state_search::Where;
use lookahead_engine::symbolic::vars::DataVars;
use oxidd::BooleanFunction;
use oxidd::bdd::BDDFunction;

use gct_measure::common;

use gct_measure::options;

use gct_measure::menu_profile;
use menu_profile::MenuProfile;

const CONVERSATION: i32 = 761;
const BUDGET_MB: usize = 6144;
const STARTS: usize = 8;
const UNSEEN: usize = 10;
const COUNTER_CAP: i32 = 16;
const LAYERS: usize = 26;

/// What this driver takes.
///
/// HANDED DOWN RATHER THAN READ WHERE IT IS WANTED: `walk` needs the layer cap and
/// `wanted_target` needs the target, and both sit well below `main`. One parameter carries
/// either, and the next option to arrive changes no signature at all.
#[derive(clap::Parser)]
#[command(about = "How much headroom a bidirectional search has, layer by layer, on one group.")]
struct Options {
    #[command(flatten)]
    groups: options::Groups,
    #[command(flatten)]
    starts: options::Starts<STARTS>,
    #[command(flatten)]
    unseen: options::Unseen<UNSEEN>,
    #[command(flatten)]
    budget: options::Budget<BUDGET_MB>,
    /// How many layers to walk before stopping
    #[arg(long, value_name = "N", default_value_t = LAYERS)]
    layers: usize,
    /// Which entry to watch for, as `conversation:entry`; the profile's first unread otherwise
    #[arg(long, value_name = "CONV:ENTRY")]
    target: Option<String>,
}

fn main() {
    let asked = <Options as clap::Parser>::parse();
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");
    let budget = DiagramBudget::new(asked.budget.bytes());
    let conversation = asked
        .groups
        .conversations
        .first()
        .copied()
        .unwrap_or(CONVERSATION);
    let Ok((graph, _)) = build_group_graph(&index, conversation) else {
        eprintln!("conversation {conversation}: no group builds from it.");
        return;
    };
    let root = DialogueNodeId::new(conversation, 0);
    let Some(profile) = MenuProfile::of(&graph, root, asked.unseen.unseen, asked.starts.starts)
    else {
        eprintln!("conversation {conversation}: no menu.");
        return;
    };
    isolated::on_its_own_thread(|| {
        walk(conversation, &graph, &profile, budget, &asked);
        Some(())
    });
}

fn walk(
    conversation: i32,
    graph: &LookAheadGraph,
    profile: &MenuProfile,
    budget: DiagramBudget,
    asked: &Options,
) {
    let symbols = graph.symbols().clone();
    let world = GameWorld::declaring(
        WorldRawData {
            day_minutes: 720,
            day_counter: 1,
            ..Default::default()
        },
        common::declared(),
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
    let target = wanted_target(graph, profile, asked);

    // LAYER ZERO IS WHERE THE PLAYER STANDS: every option's entries, holding what entering
    // that option leaves. Unioned across the options, because the backward pass races them
    // and stops at the first one it meets rather than telling them apart.
    let mut reached: HashMap<DialogueNodeId, BDDFunction> = HashMap::new();
    let mut here: HashMap<DialogueNodeId, BDDFunction> = HashMap::new();
    let mut options = HashSet::new();
    for &start in &profile.starts {
        options.insert(start);
        let position = Where::of(
            graph,
            start,
            StartBranch::Either,
            &seed,
            &mut compiler,
            &world,
            COUNTER_CAP as u32,
        )
        .position(start);
        for &entry in &position.entries {
            add(&mut here, entry, &position.holding);
        }
    }

    let began = Instant::now();
    println!(
        "\n== conversation {conversation}, forward front from {} options, target {}:{}",
        profile.starts.len(),
        target.conversation_id,
        target.entry_id
    );
    println!(
        "   {:>5}  {:>8}  {:>8}  {:>12}  {:>9}  {:>9}",
        "layer", "touched", "carried", "nodes", "ms", "at target"
    );

    let cap = asked.layers;
    for layer in 0..cap {
        if here.is_empty() {
            println!("   front died at layer {layer}");
            return;
        }
        // A stable order keeps diagram allocation and measurements repeatable.
        let mut frontier: Vec<_> = std::mem::take(&mut here).into_iter().collect();
        frontier.sort_by_key(|(id, _)| (id.conversation_id, id.entry_id));

        let mut queue: Vec<_> = frontier;
        let mut touched_this_layer = 0usize;
        // The zero-charge part of a layer closes first: leaving a node that is not a
        // charged choice keeps the states in THIS layer, so it goes back on the queue.
        while let Some((id, arriving)) = queue.pop() {
            let Some(fresh) = widen(&mut reached, &vars, id, &arriving) else {
                continue;
            };
            touched_this_layer += 1;
            let Some(node) = graph.get(id) else { continue };
            let Some(onward) = Reachability::entry_states(
                graph,
                id,
                StartBranch::Either,
                &fresh,
                &mut compiler,
                &world,
                COUNTER_CAP as u32,
            ) else {
                println!("   out of nodes at layer {layer}");
                return;
            };
            if !onward.satisfiable() {
                continue;
            }
            // LEAVING A CHOICE COSTS ONE, and the option the player stands on is free -
            // the same charge `choice_bounds` makes.
            let charged = node.choice && !options.contains(&id);
            for &child in &node.links {
                if graph.get(child).is_none() {
                    continue;
                }
                if charged {
                    add(&mut here, child, &onward);
                } else {
                    queue.push((child, onward.clone()));
                }
            }
        }

        let at_target = reached
            .get(&target)
            .is_some_and(|states| states.satisfiable());
        println!(
            "   {:>5}  {:>8}  {:>8}  {:>12}  {:>9}  {:>9}",
            layer,
            touched_this_layer,
            here.len(),
            vars.node_count(),
            began.elapsed().as_millis(),
            if at_target { "YES" } else { "no" }
        );
        if at_target {
            println!("   forward-only distance to the target: {layer}");
            return;
        }
    }
}

/// Adds `states` to what is pending at `id`.
fn add(
    pending: &mut HashMap<DialogueNodeId, BDDFunction>,
    id: DialogueNodeId,
    states: &BDDFunction,
) {
    match pending.remove(&id) {
        Some(already) => {
            if let Ok(joined) = already.or(states) {
                pending.insert(id, joined);
            }
        }
        None => {
            pending.insert(id, states.clone());
        }
    }
}

/// Adds `arriving` to what is known at `id`, and says what part of it was new.
///
/// The mirror of `Backward::widen`, and for the same reason: what travels is the DELTA, so
/// a node revisited with states it has already forwarded sends nothing on.
fn widen(
    reached: &mut HashMap<DialogueNodeId, BDDFunction>,
    vars: &DataVars<'_>,
    id: DialogueNodeId,
    arriving: &BDDFunction,
) -> Option<BDDFunction> {
    if !arriving.satisfiable() {
        return None;
    }
    let known = reached.get(&id).cloned().unwrap_or_else(|| vars.bottom());
    let fresh = known.not().ok().and_then(|gap| arriving.and(&gap).ok())?;
    if !fresh.satisfiable() {
        return None;
    }
    let widened = known.or(arriving).ok()?;
    reached.insert(id, widened);
    Some(fresh)
}

/// The target to watch for, named as `conversation:entry` or the profile's first unread.
fn wanted_target(graph: &LookAheadGraph, profile: &MenuProfile, asked: &Options) -> DialogueNodeId {
    if let Some(named) = asked.target.as_deref() {
        let (conversation, entry) = named
            .split_once(':')
            .expect("--target is conversation:entry");
        return DialogueNodeId::new(
            conversation.trim().parse().expect("a conversation number"),
            entry.trim().parse().expect("an entry number"),
        );
    }
    // WHAT THE PROFILE CALLS UNREAD ANYWHERE, asked of the profile rather than of a seen state.
    // A seen state needs a world as well - see `world::seen_state` - and this wants the
    // profile's own assertion, which is the set it was built around.
    graph
        .nodes()
        .filter(|n| !n.is_group && profile.unseen.contains(&n.id))
        .map(|n| n.id)
        .min_by_key(|id| (id.conversation_id, id.entry_id))
        .expect("the profile marks something unread")
}
