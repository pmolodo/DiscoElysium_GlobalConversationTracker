// SPDX-License-Identifier: MIT
use std::collections::HashMap;
use std::fmt;
use serde::{Deserialize, Serialize};

use crate::core::types::DialogueNodeId;
use crate::core::state::StateSymbols;
use crate::graph::node::LookAheadNode;

/// The dialogue entries the look-ahead can walk, indexed by id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookAheadGraph {
    nodes: HashMap<DialogueNodeId, LookAheadNode>,
    symbols: StateSymbols,
}

impl LookAheadGraph {
    /// Builds a graph, assigning the once slots and then freezing the symbol table.
    ///
    /// Interning happens HERE and nowhere later. A crawl reads slots by index and never
    /// creates one, which is what lets the symbol table be shared as `&StateSymbols`
    /// throughout the search, keeps the state vector's width fixed before the first
    /// state exists, and is a precondition for any symbolic encoding: a decision diagram
    /// has to fix its variable order up front, and cannot if a new variable can appear
    /// halfway through.
    pub fn new(nodes: Vec<LookAheadNode>, mut symbols: StateSymbols) -> Result<Self, String> {
        let mut map = HashMap::new();
        for mut node in nodes {
            if map.contains_key(&node.id) {
                return Err(format!("Duplicate dialogue entry {}", node.id));
            }
            if node.needs_once_slot() {
                node.once_slot = symbols.once(node.id) as i32;
            }
            map.insert(node.id, node);
        }
        Ok(Self { nodes: map, symbols })
    }

    pub fn symbols(&self) -> &StateSymbols {
        &self.symbols
    }

    pub fn count(&self) -> usize {
        self.nodes.len()
    }

    pub fn nodes(&self) -> impl Iterator<Item = &LookAheadNode> {
        self.nodes.values()
    }

    pub fn get(&self, id: DialogueNodeId) -> Option<&LookAheadNode> {
        self.nodes.get(&id)
    }

    pub fn get_mut(&mut self, id: DialogueNodeId) -> Option<&mut LookAheadNode> {
        self.nodes.get_mut(&id)
    }

    pub fn contains(&self, id: DialogueNodeId) -> bool {
        self.nodes.contains_key(&id)
    }
}

impl fmt::Display for LookAheadGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LookAheadGraph({} nodes, {} slots)", self.nodes.len(), self.symbols.count())
    }
}
