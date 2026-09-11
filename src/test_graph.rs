// SPDX-License-Identifier: MIT
//! Builds small graphs, written the way the database writes them.
//!
//! The Rust counterpart of the C# test suite's `GraphBuilder`. Guards and actions are
//! given as the raw condition and script TEXT rather than as pre-built objects, so the
//! tests exercise the parsers on the same syntax the game ships and a fixture can be
//! pasted straight out of the asset.

use crate::core::state::StateSymbols;
use crate::core::types::{DialogueCheckKind, DialogueNodeId};
use crate::graph::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::parser::action_parser::parse_actions;
use crate::parser::guard_parser::parse_guard;

/// The conversation small fixtures live in.
pub const DEFAULT_CONVERSATION: i32 = 1;

/// An entry id in the default conversation.
pub fn node(id: i32) -> DialogueNodeId {
    DialogueNodeId::new(DEFAULT_CONVERSATION, id)
}

/// How one entry is described. Everything but the id has a default.
pub struct Entry {
    pub id: i32,
    pub guard: Option<String>,
    pub script: Option<String>,
    pub links: Vec<i32>,
    pub is_group: bool,
    pub player: bool,
    pub kind: DialogueCheckKind,
    pub flag: Option<String>,
    pub boolean_only: bool,
    pub cost: i32,
    pub cost_once: bool,
}

impl Entry {
    pub fn new(id: i32) -> Self {
        Self {
            id,
            guard: None,
            script: None,
            links: Vec::new(),
            is_group: false,
            player: false,
            kind: DialogueCheckKind::None,
            flag: None,
            boolean_only: false,
            cost: 0,
            cost_once: false,
        }
    }

    pub fn guard(mut self, text: &str) -> Self {
        self.guard = Some(text.to_string());
        self
    }

    pub fn script(mut self, text: &str) -> Self {
        self.script = Some(text.to_string());
        self
    }

    pub fn links(mut self, links: &[i32]) -> Self {
        self.links = links.to_vec();
        self
    }

    pub fn group(mut self) -> Self {
        self.is_group = true;
        self
    }

    pub fn player(mut self) -> Self {
        self.player = true;
        self
    }

    pub fn kind(mut self, kind: DialogueCheckKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn flag(mut self, name: &str) -> Self {
        self.flag = Some(name.to_string());
        self
    }

    pub fn boolean_only(mut self) -> Self {
        self.boolean_only = true;
        self
    }

    pub fn cost(mut self, cost: i32) -> Self {
        self.cost = cost;
        self
    }

    pub fn cost_once(mut self) -> Self {
        self.cost_once = true;
        self
    }
}

/// Builds a graph from entries, interning symbols as the real builder does.
pub struct GraphBuilder {
    entries: Vec<Entry>,
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add(mut self, entry: Entry) -> Self {
        self.entries.push(entry);
        self
    }

    /// The graph, and a copy of the symbol table so a test can look slots up.
    ///
    /// Both, because the graph takes the table by value - and a test that needs to name a
    /// slot cannot get at it afterwards.
    pub fn build_with_symbols(self) -> (LookAheadGraph, StateSymbols) {
        let mut symbols = StateSymbols::new();
        let mut nodes = Vec::with_capacity(self.entries.len());

        for entry in self.entries {
            let id = node(entry.id);
            let guard = parse_guard(entry.guard.as_deref().unwrap_or(""))
                .expect("a fixture's guard should parse");
            let actions = parse_actions(entry.script.as_deref().unwrap_or(""), &mut symbols);

            // A rolled check's success and failure flags, matching the real builder.
            let (flag_slot, failed_flag_slot) = match &entry.flag {
                Some(flag) if !flag.is_empty() => (
                    symbols.variable(flag) as i32,
                    symbols.variable(&format!("{flag}_failed")) as i32,
                ),
                _ => (-1, -1),
            };

            let closes_once_seen = entry.kind == DialogueCheckKind::Fake
                || (entry.kind == DialogueCheckKind::KimSwitch && !entry.boolean_only);
            let seen_slot = if closes_once_seen {
                symbols.seen(id) as i32
            } else {
                -1
            };

            let links: Vec<DialogueNodeId> = entry.links.iter().map(|l| node(*l)).collect();

            let mut built = LookAheadNode::new(
                id,
                entry.is_group,
                entry.kind,
                guard,
                actions,
                links,
                entry.cost,
                entry.cost_once,
                false,
                flag_slot,
                failed_flag_slot,
                entry.boolean_only,
                seen_slot,
            );
            built.player = entry.player;
            nodes.push(built);
        }

        let snapshot = symbols.clone();
        (
            LookAheadGraph::new(nodes, symbols).expect("a fixture should have distinct ids"),
            snapshot,
        )
    }

    pub fn build(self) -> LookAheadGraph {
        self.build_with_symbols().0
    }
}

impl Default for GraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}
