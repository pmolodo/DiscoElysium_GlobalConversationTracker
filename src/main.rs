// SPDX-License-Identifier: MIT
use std::collections::HashSet;
use std::path::PathBuf;
use clap::Parser;

use lookahead_engine::bridge::{
    answer, LookAheadAnswer, LookAheadRequest, NodeRef, WorldSnapshot,
};
use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::world::test_world::TestWorld;
use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions, StartBranch};

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

    /// Memory budget in megabytes, or 0 for the engine's own default
    ///
    /// The dial the player has, so the dial this tool has. It was a state budget until
    /// de-7z0f: a count of search states costs between 136 and 455 megabytes depending on
    /// which conversation it is counted in, which makes it a limit nobody can set
    /// meaningfully.
    #[arg(long, default_value = "0")]
    memory_budget_mb: usize,

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

    /// Report where each outcome of this entry's roll leads, and crawl nothing.
    ///
    /// What an in-game fixture needs and cannot read off the index by hand: both
    /// branches of a check link into the same group, and which of its children are
    /// live is decided by guards on the check's own flag. Naming the entry here asks
    /// the engine the same question the mod's Pass / Fail line is answering.
    #[arg(long)]
    branches_of: Option<i32>,

    /// Entries a global state fixture records, as conv:entry, for --branches-of.
    ///
    /// The middle rung: recorded somewhere, so unseen THIS game rather than unseen
    /// anywhere. Everything the graph holds that is named by neither this nor
    /// --seen-here is unseen in any game, which is what an empty fixture means.
    #[arg(long, value_delimiter = ',')]
    recorded: Vec<String>,

    /// Entries the save has already displayed, as conv:entry, for --branches-of.
    #[arg(long, value_delimiter = ',')]
    seen_here: Vec<String>,
}

/// Reads a conv:entry pair, which is how the report writes a node and how a person
/// reading that report would type one back.
fn node_ref(text: &str) -> anyhow::Result<NodeRef> {
    let (conversation, entry) = text
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("'{text}' is not a conv:entry pair"))?;

    Ok(NodeRef { conversation: conversation.trim().parse()?, entry: entry.trim().parse()? })
}

/// A novelty as the mod's colours name it, since that is what a fixture is arranging.
fn rung(novelty: i32) -> &'static str {
    match novelty {
        2 => "orange (unseen in any game)",
        1 => "red (unseen this game)",
        _ => "dark red (already seen this game)",
    }
}

/// One half of the line the mod would draw: the word's colour, then its asterisk.
fn half(branch: &LookAheadAnswer) -> String {
    let asterisk = if !branch.complete {
        " with a grey '*?' - its search gave up".to_string()
    } else if branch.best > branch.destination {
        format!(" with an asterisk in {}", rung(branch.best))
    } else {
        String::new()
    };

    format!("{}{}", rung(branch.destination), asterisk)
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
    let default_options = LookAheadOptions::default();
    let engine = LookAheadEngine::new(LookAheadOptions {
        memory_budget: if args.memory_budget_mb == 0 {
            default_options.memory_budget
        } else {
            args.memory_budget_mb * 1024 * 1024
        },
        time_budget: std::time::Duration::from_millis(args.time_budget_ms),
        collect_trace: args.trace,
        ..Default::default()
    });

    if let Some(entry_id) = args.branches_of {
        let start = DialogueNodeId::new(args.conversation_id, entry_id);
        for branch in [StartBranch::Pass, StartBranch::Fail] {
            let destinations = engine.branch_destinations(&graph, start, &world, branch);
            let names: Vec<String> =
                destinations.iter().map(|id| format!("{}", id)).collect();
            println!(
                "{:?} leads to {}",
                branch,
                if names.is_empty() { "nowhere".to_string() } else { names.join(", ") }
            );
        }

        // The whole Pass / Fail line, from the same call the mod makes, so a fixture can
        // be checked before it costs an in-game run rather than after.
        let recorded: HashSet<NodeRef> = args
            .recorded
            .iter()
            .map(|text| node_ref(text))
            .collect::<anyhow::Result<_>>()?;
        let seen_here: HashSet<NodeRef> = args
            .seen_here
            .iter()
            .map(|text| node_ref(text))
            .collect::<anyhow::Result<_>>()?;

        let request = LookAheadRequest {
            conversation: args.conversation_id,
            starts: vec![NodeRef::from(start)],
            unseen_any_game: graph
                .nodes()
                .map(|node| NodeRef::from(node.id))
                .filter(|node| !recorded.contains(node) && !seen_here.contains(node))
                .collect(),
            unseen_this_game: recorded
                .iter()
                .copied()
                .filter(|node| !seen_here.contains(node))
                .collect(),
            memory_budget_mb: args.memory_budget_mb,
            time_budget_ms: args.time_budget_ms,
            world: WorldSnapshot {
                day_minutes: args.day_minutes,
                day_counter: args.day_counter,
                ..Default::default()
            },
            ..Default::default()
        };

        let response = answer(&index, None, &request);
        if let Some(error) = response.error {
            anyhow::bail!("the bridge refused the request: {error}");
        }

        // ONE ANSWER PER OUTCOME, so the report is per answer. An entry that does not
        // roll comes back once, naming no outcome, which is what says it has one.
        for option in &response.answers {
            match option.branch.as_deref() {
                Some(outcome) => println!(
                    "{}: {}",
                    if outcome == "pass" { "Pass" } else { "Fail" },
                    half(option),
                ),
                None => println!(
                    "{}:{} came back naming no outcome, so it does not roll",
                    option.start.conversation, option.start.entry
                ),
            }
        }

        return Ok(());
    }

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
