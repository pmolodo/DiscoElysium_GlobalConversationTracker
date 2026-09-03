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
    pub fn new(nodes: Vec<LookAheadNode>, symbols: StateSymbols) -> Result<Self, String> {
        let mut map = HashMap::new();
        for node in nodes {
            if map.contains_key(&node.id) {
                return Err(format!("Duplicate dialogue entry {}", node.id));
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
