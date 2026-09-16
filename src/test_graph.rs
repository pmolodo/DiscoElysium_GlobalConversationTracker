// SPDX-License-Identifier: MIT
//! Builds small graphs, written the way the database writes them.
//!
//! The Rust counterpart of the C# test suite's `GraphBuilder`. Guards and actions are
//! given as the raw condition and script TEXT rather than as pre-built objects, so the
//! tests exercise the parsers on the same syntax the game ships and a fixture can be
//! pasted straight out of the asset.

use std::collections::HashMap;

use crate::core::price::PriceScale;
use crate::core::state::StateSymbols;
use crate::core::types::{DialogueCheckKind, DialogueNodeId};
use crate::graph::LookAheadGraph;
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
    pub price_scale: Option<PriceScale>,
    pub cost_once: bool,
    /// Any other entry fields, as the database spells them - a check's `SkillType`, a passive
    /// check's `Actor` - read by the same helpers the real builder uses.
    pub fields: HashMap<String, String>,
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
            price_scale: None,
            cost_once: false,
            fields: HashMap::new(),
        }
    }

    /// An entry field the database carries, as it spells it.
    pub fn field(mut self, name: &str, value: &str) -> Self {
        self.fields.insert(name.to_string(), value.to_string());
        self
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

    /// A price scaled in hardcore mode, as buying a healing or drug item is.
    pub fn price_scale(mut self, scale: PriceScale) -> Self {
        self.price_scale = Some(scale);
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

    /// Adds one entry, and hands the builder back for the next.
    ///
    /// NOT `std::ops::Add`, which is what the lint suggests: this is a builder step read as
    /// "add this entry", chained over dozens of fixtures, and `builder + entry` would say
    /// the same thing less plainly.
    #[allow(clippy::should_implement_trait)]
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
            let mut actions = parse_actions(entry.script.as_deref().unwrap_or(""), &mut symbols);
            actions.extend(crate::index::passive_success_actions(
                &entry.fields,
                entry.kind,
                &mut symbols,
            ));
            let failure_actions =
                crate::index::check_failure_actions(&entry.fields, entry.kind, &mut symbols);

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

            let mut built = LookAheadNode {
                is_group: entry.is_group,
                kind: entry.kind,
                guard,
                skill_moves: crate::core::skill_movers::SkillMoves::of(
                    actions.iter().chain(&failure_actions),
                    &symbols,
                ),
                damageable_skill: crate::index::damageable_skill(&entry.fields, entry.kind),
                actions,
                failure_actions,
                links,
                cost: entry.cost,
                click_cost: entry.cost,
                price_scale: entry.price_scale,
                cost_once: entry.cost_once,
                flag_slot,
                failed_flag_slot,
                boolean_only: entry.boolean_only,
                seen_slot,
                ..LookAheadNode::new(id)
            };
            built.player = entry.player;
            nodes.push(built);
        }

        let graph =
            LookAheadGraph::new(nodes, symbols).expect("a fixture should have distinct ids");
        let symbols = graph.symbols().clone();
        (graph, symbols)
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
