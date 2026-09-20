// SPDX-License-Identifier: MIT
// run-log-kind: analysis
//! A group as graphviz, so a graph can be LOOKED at, plus what its slots are.
//!
//! ## Why this exists
//!
//! What goes wrong in a graph is a shape - an entry something still links to after it should
//! have gone, a chain that runs through a choice, a tail two routes disagree about. None of that
//! is legible from a list of ids or a count of nodes, and all of it is obvious in a picture.
//!
//! ## NOTHING IS ELIDED
//!
//! An entry's label carries its guard written out and every action it holds, naming the slot each
//! one touches - not "guard" and "3 act". A summary tells you a guard exists, which is the one
//! thing already obvious from the fact that you are reading the entry at all; what a reader wants
//! is WHICH condition, and against WHICH slot, because that is what decides whether two entries
//! that look alike behave alike. Labels therefore run long, and a graph too large to read is
//! what `--survey` is for.
//!
//! ## What a picture cannot say
//!
//! Who else touches a slot. A label says entry 12 raises slot 7; it cannot say that entry 3 is
//! the one reading it, because that fact belongs to no single node. So the `.md` beside the
//! `.dot` lists every slot in the group with its number, its name, its kind, and the entries that
//! write and read it.
//!
//! ## Picking something small enough to read
//!
//! `--survey` lists groups worth looking at: under `--at-most` entries and holding at least two
//! menus, a menu being a parent that offers more than one player line. A group of four thousand
//! entries is a picture of nothing.
//!
//! ## Turning one into a picture
//!
//! `tools/render-dot.py` runs graphviz over the `.dot` files this writes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use lookahead_engine::core::action::DialogueAction;
use lookahead_engine::core::guard::{Guard, GuardExpression};
use lookahead_engine::core::guard_value::GuardValueKind;
use lookahead_engine::core::state::{NOT_A_VARIABLE, StateSymbols};
use lookahead_engine::core::types::{DialogueCheckKind, Ternary};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::graph::node::LookAheadNode;
use lookahead_engine::index::{Index, build_group_graph, read_index};

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "options.rs"]
mod options;

/// Everything this driver takes.
#[derive(clap::Parser, Debug)]
#[command(
    about = "A group as graphviz, with its slots, guards and actions written out. With no group \
             named, a survey of the ones small enough to read.",
    long_about = None
)]
struct Options {
    #[command(flatten)]
    groups: options::Groups,

    /// Where the .dot and .md files go
    #[arg(long, default_value = "analysis/outputs/graphs")]
    out: PathBuf,

    /// List the groups worth drawing and write nothing
    #[arg(long)]
    survey: bool,

    /// The largest group a survey will suggest
    #[arg(long = "at-most", default_value_t = 50)]
    at_most: usize,
}

fn main() {
    let asked = <Options as clap::Parser>::parse();
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the index reads");

    if asked.survey || asked.groups.conversations.is_empty() {
        survey(&index, asked.at_most);
        return;
    }
    for conversation in &asked.groups.conversations {
        draw(&index, *conversation, &asked.out);
    }
}

/// Groups small enough to look at that still offer more than one menu.
fn survey(index: &Index, at_most: usize) {
    println!(
        "{:>6} {:>8} {:>7} {:>7}",
        "conv", "entries", "menus", "slots"
    );
    println!("  a menu is a parent offering more than one player line");
    let mut starts: Vec<i32> = index.keys().copied().collect();
    starts.sort_unstable();
    for start in starts {
        let Ok((graph, _)) = build_group_graph(index, start) else {
            continue;
        };
        let entries = graph.nodes().count();
        if entries > at_most {
            continue;
        }
        if menus_of(&graph) < 2 {
            continue;
        }
        println!(
            "{start:>6} {entries:>8} {:>7} {:>7}",
            menus_of(&graph),
            graph.symbols().count()
        );
    }
}

/// How many menus a group holds: parents that offer more than one player line.
fn menus_of(graph: &LookAheadGraph) -> usize {
    graph
        .nodes()
        .filter(|node| {
            node.links
                .iter()
                .filter(|link| graph.get(**link).is_some_and(|child| child.choice))
                .count()
                > 1
        })
        .count()
}

/// Writes one group: the picture and the slot summary beside it.
fn draw(index: &Index, conversation: i32, out: &Path) {
    let Ok((graph, _)) = build_group_graph(index, conversation) else {
        eprintln!("conversation {conversation}: no group builds from it");
        return;
    };
    if let Err(problem) = std::fs::create_dir_all(out) {
        eprintln!("cannot write to {}: {problem}", out.display());
        return;
    }

    let picture = out.join(format!("{conversation}.dot"));
    let summary = out.join(format!("{conversation}-slots.md"));
    if let Err(problem) = std::fs::write(&picture, dot(&graph, conversation)) {
        eprintln!("cannot write {}: {problem}", picture.display());
        return;
    }
    if let Err(problem) = std::fs::write(&summary, slots_md(&graph, conversation)) {
        eprintln!("cannot write {}: {problem}", summary.display());
        return;
    }
    println!(
        "conversation {conversation}: {} entries, {} slots\n  {}\n  {}",
        graph.nodes().count(),
        graph.symbols().count(),
        picture.display(),
        summary.display(),
    );
}

/// One graph as graphviz.
fn dot(graph: &LookAheadGraph, conversation: i32) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "digraph conversation{conversation} {{");
    let _ = writeln!(out, "  rankdir=TB;");
    let _ = writeln!(out, "  node [fontname=\"monospace\" fontsize=9];");
    let mut ids: Vec<_> = graph.nodes().map(|node| node.id).collect();
    ids.sort_unstable_by_key(|id| (id.conversation_id, id.entry_id));
    for id in &ids {
        let Some(node) = graph.get(*id) else { continue };
        let _ = writeln!(
            out,
            "  \"{}\" [{} label=\"{}\"];",
            node.id,
            shape_of(node),
            label_of(node, graph.symbols())
        );
    }
    for id in &ids {
        let Some(node) = graph.get(*id) else { continue };
        for link in &node.links {
            let _ = writeln!(out, "  \"{}\" -> \"{}\";", node.id, link);
        }
    }
    let _ = writeln!(out, "}}");
    out
}

/// A choice is a box, a group a folder, a check a corner-cut box, anything else an ellipse.
///
/// A CHECK IS NOT A DIAMOND, which is what a flow chart would draw and what a label of one line
/// could live in. Graphviz fits a label to a shape's inscribed area, and a diamond's is a
/// quarter of its box, so an entry carrying a guard and three actions grows a diamond wider than
/// the picture and still prints its text across the border. `diagonals` cuts the corners of a
/// box that is sized properly, which says the same thing and holds the label.
fn shape_of(node: &LookAheadNode) -> &'static str {
    if node.is_group {
        return "shape=folder";
    }
    if node.kind != DialogueCheckKind::None {
        return "shape=box style=diagonals";
    }
    match node.choice {
        true => "shape=box",
        false => "shape=ellipse",
    }
}

/// Everything one entry is, as the lines of its label.
fn label_of(node: &LookAheadNode, symbols: &StateSymbols) -> String {
    let mut lines = vec![
        [node.id.entry_id.to_string()]
            .into_iter()
            .chain(what_it_is(node))
            .collect::<Vec<_>>()
            .join(" "),
    ];
    if let Some(guard) = guard_text(&node.guard) {
        lines.push(format!("if {guard}"));
    }
    for (what, slot) in slots_of(node) {
        lines.push(format!("{what} {}", named(slot, symbols)));
    }
    for action in &node.actions {
        lines.push(format!("do {}", action_text(action, symbols)));
    }
    // A CHECK'S OWN ACTIONS AND ITS FAILING BRANCH'S ARE DIFFERENT LISTS, and which list an
    // action is in decides whether it fires on the route being read. Merged into one they are
    // a picture of an entry no play ever meets.
    for action in &node.failure_actions {
        lines.push(format!("fail {}", action_text(action, symbols)));
    }
    lines
        .iter()
        .map(|line| escaped(line))
        .collect::<Vec<_>>()
        .join("\\l")
        + "\\l"
}

/// What sort of entry it is, in the words the engine uses for it.
fn what_it_is(node: &LookAheadNode) -> Vec<String> {
    let mut marks = Vec::new();
    if node.is_group {
        marks.push("group".to_string());
    }
    if node.choice {
        marks.push("choice".to_string());
    } else if node.player {
        marks.push("player".to_string());
    }
    if node.kind != DialogueCheckKind::None {
        marks.push(format!("{:?}", node.kind));
    }
    if node.cost > 0 {
        marks.push(format!("cost {}", node.cost));
    }
    if node.cost_once {
        marks.push("cost once".to_string());
    }
    if node.hidden_when_unaffordable {
        marks.push("hidden when poor".to_string());
    }
    if node.holds_the_screen {
        marks.push("holds".to_string());
    }
    marks
}

/// The slots the entry itself carries, by what each one is for.
fn slots_of(node: &LookAheadNode) -> Vec<(&'static str, i32)> {
    [
        ("once", node.once_slot),
        ("seen", node.seen_slot),
        ("flag", node.flag_slot),
        ("failed flag", node.failed_flag_slot),
    ]
    .into_iter()
    .filter(|(_, slot)| *slot >= 0)
    .collect()
}

/// One action, with every slot it touches named.
///
/// The action's own [`std::fmt::Display`] spells each kind - money, the clock, an assignment
/// that a set slot refuses - and knows no symbol table, so what is added here is the NAMES, and
/// only where the action has a slot to name.
fn action_text(action: &DialogueAction, symbols: &StateSymbols) -> String {
    let mut text = format!("{action}");
    if action.writes_slot() && action.slot() >= 0 {
        let _ = write!(text, " [{}]", named(action.slot(), symbols));
    }
    if let Some(unless) = action.unless() {
        let _ = write!(text, " [unless {}]", named(unless as i32, symbols));
    }
    text
}

/// A slot as its number and its name, which is how every line here spells one.
fn named(slot: i32, symbols: &StateSymbols) -> String {
    let name = usize::try_from(slot)
        .ok()
        .and_then(|slot| symbols.name_of(slot))
        .unwrap_or("?");
    format!("#{slot} {name}")
}

/// The guard, written out, or `None` for one that lets everything through.
fn guard_text(guard: &Guard) -> Option<String> {
    let open = matches!(
        guard.expression(),
        GuardExpression::Literal(value) if value.as_condition() == Ternary::True
    );
    match open {
        true => None,
        false => Some(format!("{guard}")),
    }
}

/// A label graphviz will take: the quote and the backslash are its own, and a guard is full of
/// both.
fn escaped(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Every slot the group holds, and who touches it.
fn slots_md(graph: &LookAheadGraph, conversation: i32) -> String {
    let symbols = graph.symbols();
    let mut writers: BTreeMap<usize, BTreeSet<i32>> = BTreeMap::new();
    let mut readers: BTreeMap<usize, BTreeSet<i32>> = BTreeMap::new();
    for node in graph.nodes() {
        let entry = node.id.entry_id;
        for (_, slot) in slots_of(node) {
            if let Ok(slot) = usize::try_from(slot) {
                writers.entry(slot).or_default().insert(entry);
            }
        }
        for action in node.all_actions() {
            if action.writes_slot()
                && let Ok(slot) = usize::try_from(action.slot())
            {
                writers.entry(slot).or_default().insert(entry);
            }
            // THE SLOT THAT STOPS AN ACTION IS READ BY IT, which is the one case where an
            // action appears on this side of the table.
            if let Some(unless) = action.unless() {
                readers.entry(unless).or_default().insert(entry);
            }
        }
        for slot in read_by(&node.guard, symbols) {
            readers.entry(slot).or_default().insert(entry);
        }
    }

    let mut out = format!("# Conversation {conversation}: slots\n\n");
    let _ = writeln!(
        out,
        "{} slots over {} entries.\n",
        symbols.count(),
        graph.nodes().count()
    );
    let _ = writeln!(out, "| # | name | kind | written by | read by |");
    let _ = writeln!(out, "|--:|------|------|------------|---------|");
    for slot in 0..symbols.count() {
        let name = symbols.name_of(slot).unwrap_or("?");
        let _ = writeln!(
            out,
            "| {slot} | `{name}` | {} | {} | {} |",
            kind_of(name),
            entries(writers.get(&slot)),
            entries(readers.get(&slot)),
        );
    }
    let _ = write!(out, "{SLOT_SUMMARY_LIMITS}");
    out
}

/// What the table can and cannot see, said where the table is read.
///
/// READING IS BY NAME, which is exact wherever a guard names what it asks about and blind
/// wherever it does not - so the limit is written down beside the answer rather than left for a
/// reader to discover from an empty column.
const SLOT_SUMMARY_LIMITS: &str = "\n\
A slot is WRITTEN BY an entry whose action assigns it, and by the entry that carries it: a\n\
once, seen or check flag is raised by the engine at its own entry.\n\
\n\
A slot is READ BY an entry whose guard names it - a dialogue variable by its own name, an\n\
item, thought or damage slot by the subject a query names - or by an action that refuses to\n\
fire while it is set. A question that reads a slot WITHOUT naming its subject, such as one\n\
asking whether any equipment slot is filled at all, reads more than this column can say.\n";

/// What sort of thing a slot holds, from the prefix its name carries.
fn kind_of(name: &str) -> &str {
    NOT_A_VARIABLE
        .iter()
        .find(|prefix| name.starts_with(**prefix))
        .map(|prefix| prefix.trim_end_matches(':'))
        .unwrap_or("variable")
}

/// A set of entries for one cell of the table.
fn entries(which: Option<&BTreeSet<i32>>) -> String {
    match which {
        None => "-".to_string(),
        Some(entries) => entries
            .iter()
            .map(|entry| entry.to_string())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// The slots a guard reads, by the names it mentions.
///
/// A SWEEP RATHER THAN A WALK, because what is wanted is every variable and every query the
/// guard holds, and none of that depends on the shape it holds them in - see
/// [`Guard::nodes`].
fn read_by(guard: &Guard, symbols: &StateSymbols) -> BTreeSet<usize> {
    let mut slots = BTreeSet::new();
    let mut found = |name: &str| {
        if let Some(slot) = symbols.find(name) {
            slots.insert(slot);
        }
    };
    for node in guard.nodes() {
        match node.expression() {
            GuardExpression::Variable(name) => found(name),
            GuardExpression::Call(name, arguments) => {
                // A REPUTATION QUERY NAMES NO VARIABLE and reads a whole range of them, so it
                // is asked what it reads rather than read off its arguments.
                for variable in lookahead_engine::core::reputation::variables_read_by(name) {
                    found(&variable);
                }
                // WHAT A QUERY NAMES IS ITS SUBJECT, and a subject's slot is that name behind
                // one of the engine's prefixes. Probing every prefix costs a lookup each and
                // needs no table of which query asks about what.
                for argument in arguments.iter() {
                    let GuardExpression::Literal(value) = argument.expression() else {
                        continue;
                    };
                    if value.kind() != GuardValueKind::Text {
                        continue;
                    }
                    for prefix in NOT_A_VARIABLE {
                        found(&format!("{prefix}{}", value.text()));
                    }
                }
            }
            _ => {}
        }
    }
    slots
}
