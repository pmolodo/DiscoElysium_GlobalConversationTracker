// SPDX-License-Identifier: MIT
use std::path::PathBuf;
use clap::Parser;

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::world::test_world::TestWorld;
use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions};

#[derive(Parser, Debug)]
#[command(name = "lookahead-offline")]
#[command(about = "Offline look-ahead analysis for Disco Elysium conversations")]
struct Args {
    /// Conversation group ID to analyze
    #[arg(short, long)]
    conversation_id: i32,

    /// Path to conversation data JSON file
    #[arg(short, long, default_value = "conversations.json")]
    input: PathBuf,

    /// State budget (max states to explore)
    #[arg(long, default_value = "200000")]
    state_budget: usize,

    /// Time budget in milliseconds
    #[arg(long, default_value = "1000")]
    time_budget_ms: u64,

    /// Collect trace of hottest nodes
    #[arg(long)]
    trace: bool,

    /// Output results as JSON
    #[arg(long)]
    json: bool,

    /// Starting money in centimes
    #[arg(long, default_value = "0")]
    money: i32,

    /// Day minutes (0-1439)
    #[arg(long, default_value = "720")]
    day_minutes: i32,

    /// Day counter
    #[arg(long, default_value = "1")]
    day_counter: i32,

    /// Clock locked
    #[arg(long)]
    clock_locked: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let index = read_index(&args.input)?;

    // The whole reachable GROUP, not the one conversation. Links cross conversation
    // boundaries, and clipping at the boundary would throw away most of the region a
    // crawl can actually walk - crawling from WHIRLING / LENA INTRO's 511 entries
    // reaches 1,844 nodes across three conversations.
    let (graph, group) = build_group_graph(&index, args.conversation_id)
        .map_err(anyhow::Error::msg)?;
    println!(
        "Built graph: {} nodes, {} slots, over {} conversation(s)",
        graph.count(), graph.symbols().count(), group.len()
    );

    // Create world
    let world = TestWorld::new()
        .with_money(args.money)
        .with_day_minutes(args.day_minutes)
        .with_day_counter(args.day_counter)
        .with_clock_locked(args.clock_locked);

    // Run look-ahead on each option node (non-group, has outgoing links)
    let engine = LookAheadEngine::new(LookAheadOptions {
        state_budget: args.state_budget,
        time_budget: std::time::Duration::from_millis(args.time_budget_ms),
        collect_trace: args.trace,
        ..Default::default()
    });

    let mut results = Vec::new();
    let mut skipped = 0usize;

    // Simplified novelty: unseen if not in the conversation being crawled.
    let novelty = |id: DialogueNodeId| {
        if id.conversation_id == args.conversation_id {
            Novelty::SeenThisGame
        } else {
            Novelty::UnseenAnyGame
        }
    };

    for node in graph.nodes() {
        if node.is_group || node.links.is_empty() {
            continue; // Skip groups and terminal nodes
        }

        // The same question the plugin asks before it builds any crawl state, asked here
        // for the same reason: if nothing outranks this option, no walk can produce a
        // marker. Asking it keeps this tool and the game agreeing about which options are
        // worth crawling - without it the tool reports crawls, and costs, that the game
        // never pays.
        //
        // Asked of what this option can REACH rather than of the whole loaded group. A
        // group runs to 16,558 entries while a start reaches 1,144 on average, so the
        // group version answers yes for a great many options whose own corner of the
        // graph holds nothing worth finding.
        if !LookAheadEngine::reaches_potential_improvement(
            &graph,
            node.id,
            novelty(node.id),
            novelty,
        ) {
            skipped += 1;
            continue;
        }

        let result = engine.evaluate(&graph, node.id, &world, novelty);
        results.push((node.id, result));
    }

    // The graph holds its nodes in a hash map, so iteration order varies between runs.
    // Sort before reporting: this is a tool whose output people diff against a previous
    // run, and a shuffled list would look like a change every time.
    results.sort_by_key(|(id, _)| (id.conversation_id, id.entry_id));
    eprintln!(
        "{} option(s) crawled, {} skipped with no novelty headroom",
        results.len(),
        skipped
    );

    // Output
    if args.json {
        let output: Vec<serde_json::Value> = results.iter().map(|(id, r)| {
            serde_json::json!({
                "node": format!("{}", id),
                "best": format!("{:?}", r.best),
                "states_explored": r.states_explored,
                "nodes_reached": r.nodes_reached,
                "budget_exhausted": r.budget_exhausted(),
                "stopped_by": format!("{:?}", r.stopped_by),
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for (id, result) in results {
            println!("{} -> {}", id, result);
            if args.trace {
                if let Some(trace) = &result.trace {
                    for h in &trace.hottest_nodes {
                        println!("  {}", h);
                    }
                }
            }
        }
    }

    Ok(())
}
