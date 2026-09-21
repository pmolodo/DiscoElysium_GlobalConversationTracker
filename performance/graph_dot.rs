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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use lookahead_engine::core::action::DialogueAction;
use lookahead_engine::core::guard::{Guard, GuardExpression};
use lookahead_engine::core::guard_value::GuardValueKind;
use lookahead_engine::core::state::{NOT_A_VARIABLE, StateSymbols};
use lookahead_engine::core::types::{DialogueCheckKind, DialogueNodeId, Ternary};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::graph::node::LookAheadNode;
use lookahead_engine::index::{Index, build_group_graph, read_index};

use gct_measure::common;

use gct_measure::options;

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

    /// Draw outwards from this entry instead of the one a play starts at
    #[arg(long, value_name = "ENTRY")]
    from: Option<i32>,

    /// Draw only the entries this many steps out, and no further
    #[arg(long, value_name = "STEPS")]
    within: Option<usize>,
}

/// Which part of a group is drawn, and from where.
///
/// A WHOLE GROUP IS OFTEN TOO MUCH FOR A VIEWER RATHER THAN FOR GRAPHVIZ: 761 lays out fine as a
/// picture and defeats an interactive previewer that draws it in a browser. Both halves of the
/// answer are the same breadth-first sweep the ranking already runs - start it somewhere else,
/// stop it sooner - so a neighbourhood costs no second notion of distance.
#[derive(Debug, Clone, Copy)]
struct Window {
    /// The entry the sweep starts at, and `None` for the one a play starts at.
    from: Option<i32>,
    /// How many steps out to draw, and `None` for all of them.
    within: Option<usize>,
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
    let window = Window {
        from: asked.from,
        within: asked.within,
    };
    for conversation in &asked.groups.conversations {
        draw(&index, *conversation, &asked.out, window);
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
fn draw(index: &Index, conversation: i32, out: &Path, window: Window) {
    let Ok((graph, _)) = build_group_graph(index, conversation) else {
        eprintln!("conversation {conversation}: no group builds from it");
        return;
    };
    let root = DialogueNodeId::new(conversation, window.from.unwrap_or(0));
    if graph.get(root).is_none() {
        eprintln!("conversation {conversation}: no entry {}", root.entry_id);
        return;
    }
    if let Err(problem) = std::fs::create_dir_all(out) {
        eprintln!("cannot write to {}: {problem}", out.display());
        return;
    }

    let mut depths = depths_from(&graph, root);
    // THE WHOLE GROUP MEANS THE WHOLE GROUP, including a conversation nothing links to from the
    // start. A window asked for a corner of it, so it gets only what its root reaches.
    if window.from.is_none() && window.within.is_none() {
        spread_to_the_rest(&graph, conversation, &mut depths);
    }
    let drawn: HashSet<DialogueNodeId> = depths
        .iter()
        .filter(|(_, depth)| window.within.is_none_or(|within| **depth <= within))
        .map(|(id, _)| *id)
        .collect();

    let picture = out.join(format!("{conversation}{}.dot", window.suffix()));
    // THE SLOT SUMMARY IS THE WHOLE GROUP'S even when the picture is a corner of it. Its column
    // answers WHO ELSE touches a slot, and an entry left out of the drawing is exactly the kind
    // of answer that is being looked for.
    let summary = out.join(format!("{conversation}-slots.md"));
    if let Err(problem) = std::fs::write(&picture, dot(&graph, conversation, &depths, &drawn)) {
        eprintln!("cannot write {}: {problem}", picture.display());
        return;
    }
    if let Err(problem) = std::fs::write(&summary, slots_md(&graph, conversation)) {
        eprintln!("cannot write {}: {problem}", summary.display());
        return;
    }
    println!(
        "conversation {conversation}: {} entries drawn of {}, {} slots\n  {}\n  {}",
        drawn.len(),
        graph.nodes().count(),
        graph.symbols().count(),
        picture.display(),
        summary.display(),
    );
}

impl Window {
    /// What the picture's name says about which part of the group it holds, and nothing for the
    /// whole of it.
    fn suffix(self) -> String {
        let mut suffix = String::new();
        if let Some(from) = self.from {
            let _ = write!(suffix, "-from{from}");
        }
        if let Some(within) = self.within {
            let _ = write!(suffix, "-within{within}");
        }
        suffix
    }
}

/// One graph as graphviz, laid out in layers away from the entry the sweep started at.
///
/// ## The layering is ours, not graphviz's
///
/// Left to itself `dot` infers a hierarchy from the edges, and a dialogue's edges do not
/// describe one: a hub every option loops back to has more arrows arriving than the start does,
/// so the picture gets drawn around the hub and the entry a play actually begins at ends up
/// somewhere in the middle. WE KNOW WHERE A PLAY BEGINS - it is entry 0 of the conversation
/// asked for - so the ranks are worked out here, by steps from that entry, and handed over as
/// `rank=same` groups.
///
/// ## What happens to an edge that does not descend
///
/// It is drawn dashed and grey, and told `constraint=false` so it takes no part in ranking.
/// Both halves matter: without the first a loop back to the hub is indistinguishable from the
/// dialogue moving forward, and without the second the edge fights the layer it was given and
/// drags its target up the picture. See `constraint` in the graphviz attributes.
///
/// ## An edge that leaves the picture
///
/// A neighbourhood is cut somewhere, and an entry on the cut links to entries that are not
/// drawn. Such an edge is dropped rather than drawn at nothing, and the entry it left is marked
/// so the cut is visible: an unmarked entry with no way out is the end of the dialogue, and the
/// two must not look alike.
fn dot(
    graph: &LookAheadGraph,
    conversation: i32,
    depths: &HashMap<DialogueNodeId, usize>,
    drawn: &HashSet<DialogueNodeId>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "digraph conversation{conversation} {{");
    let _ = writeln!(out, "  rankdir=TB;");
    // ORDERING KEEPS A MENU IN ITS OWN ORDER: the options leaving an entry are drawn left to
    // right as the entry lists them, rather than in whatever order avoids the most crossings.
    let _ = writeln!(out, "  ordering=out;");
    // WITH RANKS OF OUR OWN, which is what `newrank` is for.
    let _ = writeln!(out, "  newrank=true;");
    let _ = writeln!(out, "  ranksep=0.5;");
    let _ = writeln!(out, "  node [fontname=\"monospace\" fontsize=9];");
    let mut ids: Vec<DialogueNodeId> = drawn.iter().copied().collect();
    ids.sort_unstable_by_key(|id| (id.conversation_id, id.entry_id));
    for id in &ids {
        let Some(node) = graph.get(*id) else { continue };
        let leaves = node.links.iter().any(|link| !drawn.contains(link));
        let mut label = label_of(node, conversation, graph.symbols());
        if leaves {
            label.push_str("... beyond the picture\\l");
        }
        let _ = writeln!(
            out,
            "  \"{}\" [{} label=\"{label}\"];",
            node.id,
            shape_of(node),
        );
    }

    let mut layers: BTreeMap<usize, Vec<DialogueNodeId>> = BTreeMap::new();
    for id in &ids {
        if let Some(depth) = depths.get(id) {
            layers.entry(*depth).or_default().push(*id);
        }
    }
    for (depth, layer) in &layers {
        let _ = write!(out, "  {{ rank=same; /* {depth} steps */");
        for id in layer {
            let _ = write!(out, " \"{id}\";");
        }
        let _ = writeln!(out, " }}");
    }

    for id in &ids {
        let Some(node) = graph.get(*id) else { continue };
        for link in &node.links {
            if !drawn.contains(link) {
                continue;
            }
            let descends = match (depths.get(id), depths.get(link)) {
                (Some(from), Some(to)) => to > from,
                _ => true,
            };
            let attributes = match descends {
                true => "",
                false => " [constraint=false color=gray50 style=dashed]",
            };
            let _ = writeln!(out, "  \"{}\" -> \"{}\"{attributes};", node.id, link);
        }
    }
    let _ = writeln!(out, "}}");
    out
}

/// How many steps each entry is from the one a play starts at.
///
/// A BREADTH-FIRST SWEEP, so an entry reachable by a short route and a long one sits at the
/// short one's depth, which is where a reader looking for how soon something can happen expects
/// to find it.
///
/// WHAT IS NOT REACHED IS NOT HERE, which is what makes this the neighbourhood as well as the
/// ranking: an entry with no depth was not drawn, and the same map answers both questions.
fn depths_from(graph: &LookAheadGraph, root: DialogueNodeId) -> HashMap<DialogueNodeId, usize> {
    let mut depths = HashMap::from([(root, 0)]);
    sweep(graph, root, &mut depths);
    depths
}

/// Lays out what the sweep from the start never reached, each unreached entry seeded in turn
/// below everything already placed.
///
/// A GROUP IS SEVERAL CONVERSATIONS and only one of them was asked for, so the entries of the
/// others may not be reachable from its start at all. For a picture of the WHOLE group they
/// still belong in it, drawn below what does reach them rather than left out of the ranking. A
/// picture of a neighbourhood asks a narrower question and does not call this.
fn spread_to_the_rest(
    graph: &LookAheadGraph,
    conversation: i32,
    depths: &mut HashMap<DialogueNodeId, usize>,
) {
    let mut ids: Vec<DialogueNodeId> = graph.nodes().map(|node| node.id).collect();
    ids.sort_unstable_by_key(|id| {
        (
            id.conversation_id != conversation,
            id.conversation_id,
            id.entry_id,
        )
    });
    for seed in ids {
        if depths.contains_key(&seed) {
            continue;
        }
        let floor = depths
            .values()
            .copied()
            .max()
            .map_or(0, |deepest| deepest + 1);
        depths.insert(seed, floor);
        sweep(graph, seed, depths);
    }
}

/// A breadth-first sweep out from one entry, leaving every entry it reaches at its distance.
///
/// BREADTH FIRST, so an entry reachable by a short route and a long one sits at the short one's
/// depth, which is where a reader looking for how soon something can happen expects to find it.
/// An entry already carrying a depth keeps it, which is what stops a second sweep moving what
/// the first one placed.
fn sweep(
    graph: &LookAheadGraph,
    root: DialogueNodeId,
    depths: &mut HashMap<DialogueNodeId, usize>,
) {
    let mut pending = VecDeque::from([root]);
    while let Some(id) = pending.pop_front() {
        let depth = depths[&id];
        let Some(node) = graph.get(id) else { continue };
        for &link in &node.links {
            if graph.get(link).is_none() || depths.contains_key(&link) {
                continue;
            }
            depths.insert(link, depth + 1);
            pending.push_back(link);
        }
    }
}

/// A group is a folder, a check a corner-cut box, a choice a sharp box, anything else a rounded
/// one.
///
/// EVERY SHAPE HERE IS A RECTANGLE, which a flow chart's diamond for a check and oval for a line
/// of dialogue are not. Graphviz fits a label to a shape's INSCRIBED area, and a diamond's is a
/// quarter of its box while an ellipse's is under two thirds, so an entry carrying a guard and
/// three actions grows a diamond wider than the picture and still prints its text across the
/// border. A box holds what it is given at any length, so the kind is said with the border
/// instead: cut corners for a check, sharp for a choice, rounded for a line that just plays.
fn shape_of(node: &LookAheadNode) -> &'static str {
    if node.is_group {
        return "shape=folder";
    }
    if node.kind != DialogueCheckKind::None {
        return "shape=box style=diagonals";
    }
    match node.choice {
        true => "shape=box",
        false => "shape=box style=rounded",
    }
}

/// Everything one entry is, as the lines of its label.
///
/// AN ENTRY OF ANOTHER CONVERSATION SAYS SO. Entry numbers start again at 0 in each
/// conversation of a group, so a bare number is only unambiguous within the one asked for - and
/// a group can be eleven of them.
fn label_of(node: &LookAheadNode, conversation: i32, symbols: &StateSymbols) -> String {
    let spelled = match node.id.conversation_id == conversation {
        true => node.id.entry_id.to_string(),
        false => format!("{}:{}", node.id.conversation_id, node.id.entry_id),
    };
    let mut lines = vec![
        [spelled]
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
///
/// AN ENTRY IS NAMED BY ITS CONVERSATION AND ITS NUMBER, the way the picture names it. A GROUP
/// IS SEVERAL CONVERSATIONS - 640's is eleven - and entry numbers start again at 0 in each of
/// them, so a bare number here names one entry per conversation in the group and nothing in
/// particular. The first thing anyone does with this table is look an entry up in the picture.
fn slots_md(graph: &LookAheadGraph, conversation: i32) -> String {
    let symbols = graph.symbols();
    let mut writers: BTreeMap<usize, BTreeSet<(i32, i32)>> = BTreeMap::new();
    let mut readers: BTreeMap<usize, BTreeSet<(i32, i32)>> = BTreeMap::new();
    for node in graph.nodes() {
        let entry = (node.id.conversation_id, node.id.entry_id);
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

/// A set of entries for one cell of the table, each named as the picture names it.
fn entries(which: Option<&BTreeSet<(i32, i32)>>) -> String {
    match which {
        None => "-".to_string(),
        Some(entries) => entries
            .iter()
            .map(|(conversation, entry)| format!("{conversation}:{entry}"))
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
