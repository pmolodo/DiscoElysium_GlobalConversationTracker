// SPDX-License-Identifier: MIT
use clap::Parser;
use std::collections::HashSet;
use std::path::PathBuf;

use lookahead_engine::bridge::{LookAheadAnswer, LookAheadRequest, NodeRef, WorldSnapshot, answer};
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::core::types::StartBranch;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::world::test_world::TestWorld;

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

    /// Time budget in milliseconds, per option
    #[arg(long, default_value = "1000")]
    time_budget_ms: u64,

    /// Time budget in milliseconds for the whole request, or 0 for no such limit
    ///
    /// ZERO WHERE THE PLUGIN SHIPS THREE SECONDS, and the difference is what the two are
    /// asked. The plugin asks about a menu - a dozen options a player is waiting on - and
    /// three seconds is what it will make them wait. This asks about every entry in a
    /// conversation at once, thousands of them in one request, so the same wall would answer
    /// the first few and give up on the rest. Set it to reproduce a menu's wall deliberately.
    #[arg(long, default_value = "0")]
    menu_time_budget_ms: u64,

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

    /// Report where each outcome of this entry's roll leads, and search nothing.
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

    Ok(NodeRef {
        conversation: conversation.trim().parse()?,
        entry: entry.trim().parse()?,
    })
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

/// The entries one outcome of a rolled start opens, guards and costs considered.
///
/// See `novelty_search::Where::destinations`, which is what does the work. Built here
/// rather than exposed from the bridge because this is a tool's convenience: the bridge
/// computes the same set for its baseline and does not need to hand it out.
fn branch_destinations(
    graph: &lookahead_engine::graph::LookAheadGraph,
    world: &TestWorld,
    start: DialogueNodeId,
    branch: StartBranch,
) -> Vec<DialogueNodeId> {
    use lookahead_engine::symbolic::budget::DiagramBudget;
    use lookahead_engine::symbolic::data_layout::DataLayout;
    use lookahead_engine::symbolic::guard_formula::GuardCompiler;
    use lookahead_engine::symbolic::novelty_search::Where;
    use lookahead_engine::symbolic::reachability::seed_of;
    use lookahead_engine::symbolic::vars::DataVars;

    const COUNTER_CAP: i32 = 16;

    let symbols = graph.symbols().clone();
    let layout = DataLayout::for_group(graph, world, COUNTER_CAP);
    let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
    let mut compiler = GuardCompiler::new(&vars).with_world(world);
    let seed = seed_of(graph, world, &vars).expect("room for a seed");

    let mut from = Where::of(
        graph,
        start,
        branch,
        &seed,
        &mut compiler,
        world,
        COUNTER_CAP as u32,
    );
    from.destinations(graph, &mut compiler, world, COUNTER_CAP as u32)
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let index = read_index(&args.input)?;

    // The whole reachable GROUP, not the one conversation. Links cross conversation
    // boundaries, and clipping at the boundary would throw away most of the region a
    // search can actually walk - searching from WHIRLING / LENA INTRO's 511 entries
    // reaches 1,844 nodes across three conversations.
    let (graph, group) =
        build_group_graph(&index, args.conversation_id).map_err(anyhow::Error::msg)?;
    println!(
        "Built graph: {} nodes, {} slots, over {} conversation(s)",
        graph.count(),
        graph.symbols().count(),
        group.len()
    );

    // Create world
    let world = TestWorld::new()
        .with_money(args.money)
        .with_day_minutes(args.day_minutes)
        .with_day_counter(args.day_counter)
        .with_clock_locked(args.clock_locked);

    if let Some(entry_id) = args.branches_of {
        let start = DialogueNodeId::new(args.conversation_id, entry_id);
        for branch in [StartBranch::Pass, StartBranch::Fail] {
            let destinations = branch_destinations(&graph, &world, start, branch);
            let names: Vec<String> = destinations.iter().map(|id| format!("{}", id)).collect();
            println!(
                "{:?} leads to {}",
                branch,
                if names.is_empty() {
                    "nowhere".to_string()
                } else {
                    names.join(", ")
                }
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
            menu_time_budget_ms: args.menu_time_budget_ms,
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

    // THE SAME CALL THE MOD MAKES, for every option at once. This loop used to build its
    // own engine, ask its own refusal question and read its own result type, which meant a
    // tool that could disagree with the game about which options are worth searching and
    // what one costs. `answer` is what the game asks, so what comes out here is what a
    // player would see.
    //
    // Simplified novelty: unseen if not in the conversation being asked about.
    let mut starts: Vec<NodeRef> = graph
        .nodes()
        .filter(|node| !node.is_group && !node.links.is_empty())
        .map(|node| NodeRef {
            conversation: node.id.conversation_id,
            entry: node.id.entry_id,
        })
        .collect();

    // The graph holds its nodes in a hash map, so iteration order varies between runs.
    // Sort before asking: this is a tool whose output people diff against a previous run,
    // and a shuffled list would look like a change every time.
    starts.sort_by_key(|start| (start.conversation, start.entry));

    let unseen: HashSet<NodeRef> = graph
        .nodes()
        .filter(|node| node.id.conversation_id != args.conversation_id)
        .map(|node| NodeRef {
            conversation: node.id.conversation_id,
            entry: node.id.entry_id,
        })
        .collect();

    let response = answer(
        &index,
        None,
        &LookAheadRequest {
            conversation: args.conversation_id,
            starts,
            unseen_any_game: unseen.into_iter().collect(),
            unseen_this_game: Default::default(),
            state_budget: 0,
            time_budget_ms: args.time_budget_ms,
            menu_time_budget_ms: args.menu_time_budget_ms,
            memory_budget_mb: args.memory_budget_mb,
            encountered: Vec::new(),
            world: WorldSnapshot::default(),
        },
    );

    if let Some(error) = &response.error {
        anyhow::bail!("{error}");
    }

    let settled = response.answers.iter().filter(|a| a.complete).count();
    eprintln!("{} answer(s), {} settled", response.answers.len(), settled,);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&response.answers)?);
    } else {
        for answer in &response.answers {
            println!(
                "{}:{}{} -> best {}, {}{}",
                answer.start.conversation,
                answer.start.entry,
                answer
                    .branch
                    .as_deref()
                    .map(|b| format!(" ({b})"))
                    .unwrap_or_default(),
                answer.best,
                if answer.complete {
                    "settled"
                } else {
                    "a lower bound"
                },
                answer
                    .witness
                    .map(|w| format!(", proved by {}:{}", w.conversation, w.entry))
                    .unwrap_or_default(),
            );
        }
    }

    Ok(())
}
