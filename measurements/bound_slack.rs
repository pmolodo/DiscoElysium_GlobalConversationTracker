// SPDX-License-Identifier: MIT
//! Where the structural bound's slack is, and what refusals and dominance can take off it.
//!
//! The branch and bound in `symbolic::menu` skips a target when its bound cannot beat the
//! best distance proven this round, and on conversation 761 it skips nothing: the bound
//! reads 8 where the layered pass proves 23 to 25. de-0jsf.13 measured that gap and
//! de-0jsf.15 asks whether closing it is a matter of walking a smaller graph.
//!
//! `novelty_search::choice_bounds` consults no guard at all. It walks `node.links`,
//! respecting only the cut set, so every route the dialogue refuses for reasons that do not
//! depend on the data state is still in it.
//!
//! THE BOUND MUST STAY A LOWER BOUND, so only a node NO state can enter may be refused -
//! never one that is merely shut in the state the search starts in, since a route may open
//! it on the way. Three such refusals, all of which the symbolic passes already make and
//! the structural walk does not:
//!
//! - a compiled guard whose `may_be_true` is unsatisfiable, which is what a guard on a
//!   world constant the world reports false compiles to, and what a contradiction compiles
//!   to;
//! - a passive check the world fails, which is `reachability::never_displays`;
//! - a hidden `Test`, for which `Backward::pre_enter` returns the empty set unconditionally.
//!
//! ## What it prints
//!
//! One block per group: how many entries each refusal catches, then one row per unread
//! target. `bound` is what the walk gives today and `refusing` what it gives over the
//! smaller graph. `doms` is the target's strict dominators and `to open` how many of those
//! are gates the seed shuts but some state opens; `route` is the length of one shortest
//! structural route and `shut on` how many gates lie along it. `detour` is the bound the
//! dominance composition gives, `-` where a gate's guard reads something the layout does not
//! track, `shut` where nothing in the group writes what it reads.
//!
//! ## What it said, 2026-09-10: NOTHING, on every heavy group
//!
//! The seven heavy groups, eight options, ten unread, the player's 256 MB. The refusal set
//! is tiny and it lies nowhere near the short routes, so every bound is unmoved:
//!
//! ```text
//!   conv  entries  refused   bounds now   bounds refusing
//!    761     2263       23         8, 9              8, 9
//!    631     2857       22       10, 11            10, 11
//!    640     2445        8            -                 -
//!    368     3192       48            -                 -
//!     14     2252        7            -                 -
//!   1030     1003       10            -                 -
//!     16     2332       21            -                 -
//! ```
//!
//! NOT ONE TARGET MOVED BY ONE CHOICE, on any group. Twenty-three refusals out of 2,263
//! entries on 761, and the ten unread candidates read 8 and 9 either way against a true
//! distance of 23 to 25.
//!
//! WHAT THAT SETTLES. The bound's slack is not routes the world has already shut. It is
//! routes shut by guards on variables the dialogue itself writes, and those cannot be
//! refused here: a route may set what it reads on the way, so removing the edge would make
//! the walk report a distance ABOVE the truth and the branch and bound would cut a target
//! that should have won. 761 is the shape at its worst - content that sits eight choices
//! away through links and twenty-four away in fact, because a variable has to be set first.
//!
//! Tightening the bound therefore needs an analysis that reasons about ORDER - what must be
//! set before an entry opens, and what setting it costs - rather than about which entries
//! are shut from the start. See de-0jsf.15.
//!
//! ## And the order analysis, measured the same day: SOUND, AND WORTH TWO
//!
//! Dominance is what makes an order analysis cheap. Where `t` lies on every route to `u`,
//! every route is `s..w..t..u` for some `w` that opens `t`, so the distance is at least
//! `d(s,w) + d(t,u)` - and because `t` dominates `u`, `d(t,u)` is `d(s,u) - d(s,t)` out of
//! the walk's own distances, so nothing is walked twice. `detour_charge` is that line.
//!
//! DOMINANCE ALONE CANNOT HELP AND IT IS WORTH SAYING WHY. Where `t` dominates `u`,
//! `d(s,u)` ALREADY EQUALS `d(s,t) + d(t,u)`: the shortest route splits at `t`, and
//! concatenating the two shortest halves is a route, so neither inequality is strict.
//! Decomposing at a choke point reproduces the number the walk already had. Only a segment
//! carrying a fact the walk does not have - a gate that must be opened first, or a distance
//! an earlier round proved - can move anything.
//!
//! ```text
//!   conv   bound   with the detour   true      dominators   of those, gates
//!    761    8, 9           10, 11   23-25           26-33             1 or 2
//!    631  10, 11                -      20           53-58                  1
//!    640    7- 9             7- 9       -           38-48                  4
//!    368   20-21            21-22       -          97-101                  1
//!     14   11-12            12-13       -           50-55             1 or 2
//! ```
//!
//! TWO CHOICES, AGAINST A GAP OF SIXTEEN. 761's targets go from 8 to 10 where the truth is
//! 24, so round one still cannot skip a thing, and nothing about the group changes. 631 gets
//! nothing at all: its one gate reads a variable the layout does not track, so the openers
//! cannot be enumerated and no charge may be made.
//!
//! The data says why, and it is a fact about how this dialogue is written rather than about
//! the analysis. A target has thirty to a hundred strict dominators and only one to four of
//! them are gates the seed shuts - and the nearest thing that opens such a gate sits a
//! choice or two away, because a conversation puts the line that sets a flag beside the line
//! that reads it. There is no long forced detour to charge for.
//!
//! ## How to run it
//!
//! ```text
//! DEGCT_CONVERSATION=761 \
//!   tools/run-logged.sh cargo bound-slack -- cargo run --release --example bound_slack
//! ```
//!
//! `DEGCT_STARTS`, `DEGCT_UNSEEN` and `DEGCT_BUDGET_MB` mean what they mean in
//! `menu_matrix`, so a reading here lines up with a row there.

use std::collections::HashSet;

use lookahead_engine::bridge::{SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::guard::GuardExpression;
use lookahead_engine::core::types::{DialogueCheckKind, DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::backward::Position;
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::dominators::Dominators;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::novelty_search::{Where, choice_bounds};
use lookahead_engine::symbolic::reachability::{never_displays, seed_of};
use lookahead_engine::symbolic::vars::DataVars;
use oxidd::BooleanFunction;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

const CONVERSATIONS: [i32; 7] = [761, 631, 640, 368, 14, 1030, 16];
const BUDGET_MB: usize = 256;
const STARTS: usize = 8;
const UNSEEN: usize = 10;
const COUNTER_CAP: i32 = 16;

/// Why a node can never be entered, in the order the cheapest test comes first.
#[derive(Default)]
struct Refused {
    hidden_test: usize,
    passive_fails: usize,
    guard_empty: usize,
}

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");
    let budget = DiagramBudget::new(from_env("BUDGET_MB", BUDGET_MB) * 1024 * 1024);
    let starts_wanted = from_env("STARTS", STARTS);
    let unseen_wanted = from_env("UNSEEN", UNSEEN);

    for conversation in numbers("CONVERSATION", &CONVERSATIONS) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            eprintln!("conversation {conversation}: no group builds from it; skipping.");
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            eprintln!("conversation {conversation}: no entry 0; skipping.");
            continue;
        }
        let Some(profile) = MenuProfile::of(&graph, root, unseen_wanted, starts_wanted) else {
            println!("conversation {conversation}: no menu");
            continue;
        };
        isolated::on_its_own_thread(|| {
            report(conversation, &graph, &profile, budget);
            Some(())
        });
    }
}

fn report(conversation: i32, graph: &LookAheadGraph, profile: &MenuProfile, budget: DiagramBudget) {
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
    let Some(vars) = DataVars::try_new(&layout, &symbols, budget) else {
        eprintln!("conversation {conversation}: no room for the variables; skipping.");
        return;
    };
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(&world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, &world, &vars).expect("room for a seed");

    let mut refused = Refused::default();
    let mut shut = HashSet::new();
    for node in graph.nodes() {
        if node.is_group {
            continue;
        }
        if node.kind == DialogueCheckKind::Test {
            refused.hidden_test += 1;
            shut.insert(node.id);
            continue;
        }
        if never_displays(node, &world) {
            refused.passive_fails += 1;
            shut.insert(node.id);
            continue;
        }
        // ONLY AN EMPTY MAY-BE-TRUE, which says no data state at all admits this node. A
        // guard merely false in the seed is not this and must not be refused: a route can
        // set what it reads on the way.
        let compiled = compiler.compile_for(node.id, &node.guard);
        if !compiled.may_be_true.satisfiable() {
            refused.guard_empty += 1;
            shut.insert(node.id);
        }
    }

    let entries = graph.nodes().filter(|n| !n.is_group).count();
    println!(
        "\n== conversation {conversation}: {entries} entries, {} refused",
        shut.len()
    );
    println!(
        "   hidden tests {}   passive the world fails {}   guard empty in every state {}",
        refused.hidden_test, refused.passive_fails, refused.guard_empty
    );

    let novelty = profile.novelty();
    let mut targets: Vec<_> = graph
        .nodes()
        .filter(|n| !n.is_group && novelty(n.id) > Novelty::SeenThisGame)
        .map(|n| n.id)
        .collect();
    targets.sort_by_key(|id| (id.conversation_id, id.entry_id));

    // SHUT ON ARRIVAL BUT NOT SHUT ALWAYS, which is the only kind of guard a detour bound
    // can charge for. A guard no state admits is already in `shut` above and refusing it
    // costs the walk nothing; a guard the seed admits asks nothing of the route. What is
    // left is a guard the route has to OPEN, and the entry carrying one is where a
    // dominator becomes worth something.
    let mut needs_opening = HashSet::new();
    for node in graph.nodes() {
        if node.is_group || shut.contains(&node.id) {
            continue;
        }
        let compiled = compiler.compile_for(node.id, &node.guard);
        if compiled
            .may_be_true
            .and(&seed)
            .is_ok_and(|open| !open.satisfiable())
        {
            needs_opening.insert(node.id);
        }
    }

    let empty = HashSet::new();
    let mut now = std::collections::HashMap::<DialogueNodeId, usize>::new();
    let mut tighter = std::collections::HashMap::<DialogueNodeId, usize>::new();
    let mut chains = std::collections::HashMap::<DialogueNodeId, Vec<DialogueNodeId>>::new();
    let mut routes = std::collections::HashMap::<DialogueNodeId, Vec<DialogueNodeId>>::new();
    let mut detours = std::collections::HashMap::<DialogueNodeId, usize>::new();
    for &start in &profile.starts {
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
        for (id, distance) in choice_bounds(graph, &position, &empty) {
            now.entry(id)
                .and_modify(|d| *d = (*d).min(distance))
                .or_insert(distance);
        }
        for (id, distance) in choice_bounds(graph, &position, &shut) {
            tighter
                .entry(id)
                .and_modify(|d| *d = (*d).min(distance))
                .or_insert(distance);
        }
        // PER OPTION, because dominance is a fact about paths FROM somewhere and each
        // option is a different somewhere. A target is bounded by the option nearest it, so
        // the chain worth keeping is the one belonging to that option.
        let tree = Dominators::of(graph, &position.entries);
        let reached = choice_bounds(graph, &position, &empty);
        for &target in &targets {
            let Some(&here) = reached.get(&target) else {
                continue;
            };
            if now.get(&target) != Some(&here) {
                continue;
            }
            let chain: Vec<_> = tree.above(target).collect();
            let best = chain
                .iter()
                .filter(|t| needs_opening.contains(t))
                .filter_map(|&gate| detour_charge(graph, &vars, &reached, gate, target))
                .max();
            chains.insert(target, chain);
            routes.insert(target, shortest_route(graph, &position, &reached, target));
            if let Some(charge) = best {
                detours.insert(target, charge);
            }
        }
    }

    println!(
        "   entries whose guard the seed shuts but some state opens: {}",
        needs_opening.len()
    );
    println!(
        "   {:>14}  {:>6}  {:>8}  {:>5}  {:>7}  {:>6}  {:>7}  {:>6}",
        "target", "bound", "refusing", "doms", "to open", "route", "shut on", "detour"
    );
    for id in targets {
        let before = now.get(&id).map(|d| d.to_string()).unwrap_or("-".into());
        let after = tighter
            .get(&id)
            .map(|d| d.to_string())
            .unwrap_or("-".into());
        let chain = chains.get(&id).cloned().unwrap_or_default();
        let route = routes.get(&id).cloned().unwrap_or_default();
        println!(
            "   {:>9}:{:<4}  {:>6}  {:>8}  {:>5}  {:>7}  {:>6}  {:>7}  {:>6}",
            id.conversation_id,
            id.entry_id,
            before,
            after,
            chain.len(),
            chain.iter().filter(|t| needs_opening.contains(t)).count(),
            route.len(),
            route.iter().filter(|t| needs_opening.contains(t)).count(),
            match detours.get(&id) {
                None => "-".to_string(),
                Some(&charge) if charge == usize::MAX => "shut".to_string(),
                Some(charge) => charge.to_string(),
            },
        );
    }
}

/// What a dominator the seed shuts is worth as a charge on the distance to `target`.
///
/// THE WHOLE COMPOSITION IN ONE LINE. `t` lies on every route to `u`, so a route is
/// s..w..t..u for some `w` that opens `t`, and its length is at least `d(s,w) + d(t,u)`.
/// Because `t` dominates `u`, `d(t,u)` is just `d(s,u) - d(s,t)` out of the distances
/// already in hand, so nothing has to be walked again. The charge beats the plain bound
/// exactly where the nearest opener is farther off than `t` itself.
///
/// `None` where the guard reads anything the layout does not track, since then the openers
/// cannot be enumerated and nothing may be assumed about them. `Some(usize::MAX)` where the
/// guard reads only tracked slots and NOTHING in the group writes them: the gate cannot be
/// opened at all, so the target is unreachable rather than far.
fn detour_charge(
    graph: &LookAheadGraph,
    vars: &DataVars<'_>,
    distances: &std::collections::HashMap<DialogueNodeId, usize>,
    gate: DialogueNodeId,
    target: DialogueNodeId,
) -> Option<usize> {
    let node = graph.get(gate)?;
    let mut slots = HashSet::new();
    for part in node.guard.nodes() {
        if let GuardExpression::Variable(name) = part.expression() {
            slots.insert(vars.slot_of(name)?);
        }
    }
    if slots.is_empty() {
        return None;
    }
    let opener = graph
        .nodes()
        .filter(|n| {
            n.actions.iter().any(|a| {
                a.writes_slot() && usize::try_from(a.slot()).is_ok_and(|s| slots.contains(&s))
            })
        })
        .filter_map(|n| distances.get(&n.id).copied())
        .min();
    let (Some(&whole), Some(&upto)) = (distances.get(&target), distances.get(&gate)) else {
        return None;
    };
    match opener {
        None => Some(usize::MAX),
        Some(reach) => Some(reach + whole.saturating_sub(upto)),
    }
}

/// The entries on one shortest structural route from `position` to `target`, target last.
///
/// WALKED BACK OUT OF `choice_bounds`'s OWN DISTANCES rather than measured again: a parent
/// lies on a shortest route exactly where its distance plus what leaving it charges equals
/// the child's. Only the charge is restated here, and it is one expression; re-deriving the
/// distances would be a second answer to a question that already has one.
fn shortest_route(
    graph: &LookAheadGraph,
    position: &Position,
    distances: &std::collections::HashMap<DialogueNodeId, usize>,
    target: DialogueNodeId,
) -> Vec<DialogueNodeId> {
    let mut parents: std::collections::HashMap<DialogueNodeId, Vec<DialogueNodeId>> =
        std::collections::HashMap::new();
    for node in graph.nodes() {
        for &child in &node.links {
            parents.entry(child).or_default().push(node.id);
        }
    }
    let mut route = vec![target];
    let mut at = target;
    let mut guard = 0;
    while !position.entries.contains(&at) && guard < graph.count() {
        guard += 1;
        let Some(&here) = distances.get(&at) else {
            break;
        };
        let Some(&step) = parents.get(&at).into_iter().flatten().find(|parent| {
            let charged = graph
                .get(**parent)
                .is_some_and(|n| n.choice && **parent != position.option);
            distances
                .get(*parent)
                .is_some_and(|&d| d + usize::from(charged) == here)
        }) else {
            break;
        };
        route.push(step);
        at = step;
    }
    route.reverse();
    route
}

fn from_env(name: &str, fallback: usize) -> usize {
    lookahead_engine::core::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(fallback)
}

fn numbers(name: &str, fallback: &[i32]) -> Vec<i32> {
    match lookahead_engine::core::env::var(name) {
        Ok(named) => named
            .split(',')
            .map(str::trim)
            .filter(|piece| !piece.is_empty())
            .map(|piece| {
                piece
                    .parse()
                    .unwrap_or_else(|_| panic!("{name}={piece:?} is not a number"))
            })
            .collect(),
        Err(_) => fallback.to_vec(),
    }
}
