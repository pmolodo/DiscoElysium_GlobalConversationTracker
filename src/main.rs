// SPDX-License-Identifier: MIT
use std::fs;
use std::path::PathBuf;
use clap::Parser;
use serde::{Deserialize, Serialize};

use lookahead_engine::core::types::{DialogueNodeId, Novelty, DialogueCheckKind};
use lookahead_engine::core::state::{StateSymbols, LookAheadState};
use lookahead_engine::core::action::DialogueAction;
use lookahead_engine::core::guard::GuardExpression;
use lookahead_engine::core::guard_value::GuardValue;
use lookahead_engine::parser::guard_parser::parse_guard;
use lookahead_engine::parser::action_parser::parse_actions;
use lookahead_engine::graph::node::LookAheadNode;
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::world::test_world::TestWorld;
use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions, LookAheadResult};

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

#[derive(Debug, Serialize, Deserialize)]
struct ConversationData {
    conversations: Vec<ConversationEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConversationEntry {
    conversation_id: i32,
    entries: Vec<DialogueEntryData>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DialogueEntryData {
    id: i32,
    is_group: bool,
    conditions_string: String,
    user_script: String,
    fields: Vec<FieldData>,
    outgoing_links: Vec<LinkData>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FieldData {
    name: String,
    value: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct LinkData {
    destination_conversation_id: i32,
    destination_dialogue_id: i32,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Load conversation data
    let data = fs::read_to_string(&args.input)?;
    let conv_data: ConversationData = serde_json::from_str(&data)?;

    // Find the conversation group (simplified - just use the one conversation)
    let Some(conv) = conv_data.conversations.iter().find(|c| c.conversation_id == args.conversation_id) else {
        eprintln!("Conversation {} not found", args.conversation_id);
        std::process::exit(1);
    };

    // Build graph
    let mut symbols = StateSymbols::new();
    let mut nodes = Vec::new();

    for entry in &conv.entries {
        // The conversation id comes from the conversation, not the entry: entries carry
        // only their own id, which restarts at 0 in every conversation.
        let node_id = DialogueNodeId::new(conv.conversation_id, entry.id);

        // Parse guard
        let guard = parse_guard(&entry.conditions_string).unwrap_or_else(|_| GuardExpression::always_true());

        // Parse actions
        let mut symbols_clone = symbols.clone();
        let actions = parse_actions(&entry.user_script, &mut symbols_clone);
        symbols = symbols_clone;

        // Determine kind from fields
        let kind = determine_kind(&entry.fields);
        let (cost, cost_once, hidden_when_unaffordable) = parse_cost(&entry.fields);
        let (flag_slot, failed_flag_slot) = parse_flags(&entry.fields, &mut symbols, kind);
        let boolean_only = entry.fields.iter().any(|f| f.name == "boolean_only" && f.value == "True");
        let seen_slot = if kind == DialogueCheckKind::Fake || (kind == DialogueCheckKind::KimSwitch && !boolean_only) {
            symbols.seen(node_id) as i32
        } else { -1 };

        let links: Vec<DialogueNodeId> = entry.outgoing_links.iter()
            .map(|l| DialogueNodeId::new(l.destination_conversation_id, l.destination_dialogue_id))
            .collect();

        nodes.push(LookAheadNode::new(
            node_id, entry.is_group, kind, guard, actions, links,
            cost, cost_once, hidden_when_unaffordable,
            flag_slot, failed_flag_slot, boolean_only, seen_slot
        ));
    }

    let graph = LookAheadGraph::new(nodes, symbols).map_err(anyhow::Error::msg)?;
    println!("Built graph: {} nodes, {} slots", graph.count(), graph.symbols().count());

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

    for node in graph.nodes() {
        if node.is_group || node.links.is_empty() {
            continue; // Skip groups and terminal nodes
        }

        let result = engine.evaluate(&graph, node.id, &world, |id| {
            // Simplified novelty: unseen if not in current conversation
            if id.conversation_id == args.conversation_id {
                Novelty::SeenThisGame
            } else {
                Novelty::UnseenAnyGame
            }
        });

        results.push((node.id, result));
    }

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

fn determine_kind(fields: &[FieldData]) -> DialogueCheckKind {
    for f in fields {
        match f.name.as_str() {
            "DifficultyPass" => return DialogueCheckKind::Passive,
            "DifficultyRed" => return DialogueCheckKind::Red,
            "DifficultyWhite" => return DialogueCheckKind::White,
            "DifficultyAtmo" => return DialogueCheckKind::Fake,
            "HiddenTest" => return DialogueCheckKind::Test,
            "kim_watch" => return DialogueCheckKind::KimSwitch,
            _ => {}
        }
    }
    DialogueCheckKind::None
}

fn parse_cost(fields: &[FieldData]) -> (i32, bool, bool) {
    let mut cost = 0;
    let mut cost_once = false;
    let mut hidden = false;
    for f in fields {
        match f.name.as_str() {
            "ClickCost" => cost = f.value.parse().unwrap_or(0),
            "CostOnce" => cost_once = f.value == "True",
            "HiddenNotEnough" => hidden = f.value == "True",
            _ => {}
        }
    }
    (cost, cost_once, hidden)
}

fn parse_flags(fields: &[FieldData], symbols: &mut StateSymbols, kind: DialogueCheckKind) -> (i32, i32) {
    if kind != DialogueCheckKind::Red && kind != DialogueCheckKind::White {
        return (-1, -1);
    }
    for f in fields {
        if f.name == "FlagName" && !f.value.is_empty() {
            let flag = symbols.variable(&f.value);
            let fail = symbols.variable(&format!("{}_failed", f.value));
            return (flag as i32, fail as i32);
        }
    }
    (-1, -1)
}
