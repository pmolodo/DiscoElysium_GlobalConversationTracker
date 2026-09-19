// SPDX-License-Identifier: MIT
//! Is a matrix row's `nodes` column a fact about the search, or about the process?
//!
//! ## What prompted it
//!
//! de-12wr.3. A control run of the performance matrix - the same binary, twice, over the
//! same twelve rows - gave identical verdicts, identical `by` and identical `asked`, and
//! moved the `nodes` column by up to 28 per cent. Every other column that moved is a clock,
//! which surprises nobody. `nodes` reads like a fact about the search.
//!
//! ## The experiment
//!
//! The same search, several times, IN ONE PROCESS, against a manager built fresh each time.
//! That separates the two explanations the issue names:
//!
//! WITHIN A PROCESS IDENTICAL, ACROSS PROCESSES NOT - the count depends on something the
//! process fixes once and the search then follows. `LookAheadGraph` holds its entries in a
//! `std::collections::HashMap`, whose `RandomState` is seeded per process, so `graph.nodes()`
//! yields a different order in every run; anything built by sweeping it inherits that order,
//! and the diagram nodes built along the way differ even where the answer does not.
//!
//! WITHIN A PROCESS ALSO MOVING - the manager is reclaiming on a schedule the search does
//! not control, and the count is a fact about when it was read.
//!
//! It also prints the first entries `graph.nodes()` yields, which is the order itself: two
//! runs of this file printing different ones is the per-process seed, visible directly.
//!
//! ## How to run it
//!
//! ```text
//! tools/run-logged.sh cargo nodes-repeat -- \
//!   cargo run --release --example nodes_repeat
//! ```
//!
//! `--conversation 1030` picks the group, `--rounds 5` how many times the search is run, and
//! `--unseen 1` how many of the deepest entries are unseen - `deepest-1` by default, which is
//! the profile the control run moved most on.

use std::collections::HashSet;

use lookahead_engine::bridge::{LookAheadRequest, NodeRef, answer_starts};
use lookahead_engine::core::types::{DialogueNodeId, SeenState};
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "options.rs"]
mod options;

#[path = "seen_profile.rs"]
mod seen_profile;
use seen_profile::candidates;

/// The group the control run moved most on.
const CONVERSATION: i32 = 1030;

/// How many times the same search is run.
const ROUNDS: usize = 5;

/// How many of the deepest entries are unseen; one is `deepest-1`.
const UNSEEN: usize = 1;

/// The counter cap a matrix row's layout is built with.
const COUNTER_CAP: i32 = 16;

/// What the in-game columns get, which is the player's own allowance.
const MEMORY: usize = 256 * 1024 * 1024;

/// What this driver takes. One group, since it repeats one group's search.
#[derive(clap::Parser)]
#[command(about = "Whether repeating one group's search reads the same node count each time.")]
struct Options {
    #[command(flatten)]
    groups: options::Groups,
    #[command(flatten)]
    unseen: options::Unseen<UNSEEN>,
    /// How many times to run the search
    #[arg(long, value_name = "N", default_value_t = ROUNDS)]
    rounds: usize,
}

fn main() {
    let asked = <Options as clap::Parser>::parse();
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    let conversation = asked
        .groups
        .conversations
        .first()
        .copied()
        .unwrap_or(CONVERSATION);
    let rounds = asked.rounds;
    let unseen_wanted = asked.unseen.unseen;

    let Ok((graph, _)) = build_group_graph(&index, conversation) else {
        eprintln!("conversation {conversation} builds no group; skipping.");
        return;
    };
    let start = DialogueNodeId::new(conversation, 0);
    if graph.get(start).is_none() {
        eprintln!("conversation {conversation} has no entry zero; skipping.");
        return;
    }

    // THE ORDER ITSELF, printed rather than inferred. Two runs of this file showing
    // different ids here is the per-process hash seed, with nothing else in the way.
    let order: Vec<String> = graph
        .nodes()
        .take(8)
        .map(|node| format!("{}:{}", node.id.conversation_id, node.id.entry_id))
        .collect();
    println!("conversation {conversation}, {} entries", graph.count());
    println!(
        "the first entries graph.nodes() yields: {}\n",
        order.join(" ")
    );

    let deepest = candidates(&graph, start);
    let unseen: HashSet<DialogueNodeId> = deepest.iter().take(unseen_wanted).copied().collect();
    let seen_state = |id: DialogueNodeId| {
        if unseen.contains(&id) {
            SeenState::UnseenAnyGame
        } else {
            SeenState::SeenThisGame
        }
    };

    // WHAT THE PLUGIN ASKS FOR, since the question is about a row of a matrix run and a
    // matrix row is answered at the player's own settings.
    let request = lookahead_engine::bridge::LookAheadRequest {
        time_budget_ms: 1000,
        memory_budget_mb: 256,
        ..Default::default()
    };
    let _budget = request.search_budget();

    println!(
        "{:>6}  {:>10}  {:>12}  {:>7}  {:>6}",
        "round", "verdict", "nodes", "asked", "ms"
    );

    let mut counts: Vec<usize> = Vec::new();
    for round in 1..=rounds {
        {
            // ON ITS OWN THREAD, exactly as a matrix column runs. Releasing a large diagram
            // walks it recursively, and the main thread's stack is not sized for it -
            // de-fpax. This measurement met that directly: the first cut ran on the main
            // thread, survived one process and died with STATUS_STACK_OVERFLOW in the next,
            // which is the very variation it exists to report.
            let (verdict, asked, nodes, took) =
                lookahead_engine::symbolic::isolated::on_its_own_thread(|| {
                    // A FRESH MANAGER EACH TIME, which is what a row gets. Reusing one
                    // would measure a warm store, and the question is what a row reports.
                    let layout = DataLayout::for_group(&graph, &world, COUNTER_CAP);
                    let vars =
                        DataVars::try_new(&layout, graph.symbols(), DiagramBudget::new(MEMORY))
                            .expect("room for a manager at the player's own allowance");
                    let mut compiler = GuardCompiler::new(&vars)
                        .with_world(&world)
                        .with_constant_clock(DataLayout::group_passes_time(&graph));
                    let seed = seed_of(&graph, &world, &vars).expect("room for a seed");

                    // A MENU OF ONE OPTION, which is the only kind of request there is. The
                    // question here is whether the nodes column is a fact about the search or
                    // about the process, and one option is enough to ask it.
                    // WITH A WALK, as the product asks: built before the clock starts, since it
                    // stands in for what the plugin records - see `hub::walk_to_menu`.
                    let request = LookAheadRequest {
                        conversation: start.conversation_id,
                        starts: vec![NodeRef::from(start)],
                        encountered: lookahead_engine::symbolic::hub::walk_to_menu(
                            &graph,
                            start.conversation_id,
                            &[start],
                        )
                        .into_iter()
                        .map(NodeRef::from)
                        .collect(),
                        ..Default::default()
                    };
                    let began = std::time::Instant::now();
                    let answers =
                        answer_starts(&graph, &world, &request, &seen_state, &mut compiler, &seed);
                    let took = began.elapsed();
                    let answer = answers.into_iter().next().expect("one start, one answer");
                    (
                        format!("{}", answer.best),
                        answer.nodes_reached,
                        vars.node_count(),
                        took,
                    )
                });

            counts.push(nodes);
            println!(
                "{round:>6}  {verdict:>10}  {nodes:>12}  {asked:>7}  {:>6.0}",
                took.as_secs_f64() * 1000.0,
            );
        }
    }

    println!();
    let low = counts.iter().copied().min().unwrap_or(0);
    let high = counts.iter().copied().max().unwrap_or(0);
    println!(
        "{low} to {high} nodes within this process, a spread of {:.1}%",
        if low == 0 {
            0.0
        } else {
            100.0 * (high - low) as f64 / low as f64
        },
    );
    println!(
        "\nRun this file twice. A spread of zero here beside two different totals between \
         the runs\nputs the move on the per-process hash seed rather than on the manager's \
         reclaiming.",
    );
}
