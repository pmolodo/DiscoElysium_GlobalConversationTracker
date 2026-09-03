// SPDX-License-Identifier: MIT
use std::fmt;
use serde::{Deserialize, Serialize};

use crate::core::types::{DialogueNodeId, DialogueCheckKind};
use crate::core::guard::GuardExpression;
use crate::core::action::DialogueAction;
use crate::core::state::StateSymbols;

/// One dialogue entry as the look-ahead needs it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookAheadNode {
    pub id: DialogueNodeId,
    pub is_group: bool,
    pub kind: DialogueCheckKind,
    pub guard: GuardExpression,
    pub actions: Vec<DialogueAction>,
    pub links: Vec<DialogueNodeId>,
    pub cost: i32,
    pub cost_once: bool,
    pub hidden_when_unaffordable: bool,
    pub flag_slot: i32,          // -1 if none
    pub failed_flag_slot: i32,   // -1 if none
    pub boolean_only: bool,
    pub seen_slot: i32,          // -1 if none
}

impl LookAheadNode {
    pub fn new(
        id: DialogueNodeId,
        is_group: bool,
        kind: DialogueCheckKind,
        guard: GuardExpression,
        actions: Vec<DialogueAction>,
        links: Vec<DialogueNodeId>,
        cost: i32,
        cost_once: bool,
        hidden_when_unaffordable: bool,
        flag_slot: i32,
        failed_flag_slot: i32,
        boolean_only: bool,
        seen_slot: i32,
    ) -> Self {
        Self {
            id,
            is_group,
            kind,
            guard,
            actions,
            links,
            cost,
            cost_once,
            hidden_when_unaffordable,
            flag_slot,
            failed_flag_slot,
            boolean_only,
            seen_slot,
        }
    }

    pub fn closes_once_seen(&self) -> bool {
        self.kind == DialogueCheckKind::Fake
            || (self.kind == DialogueCheckKind::KimSwitch && !self.boolean_only)
    }

    pub fn is_cost_option(&self) -> bool {
        self.cost > 0
    }

    pub fn is_rolled(&self) -> bool {
        self.kind.is_rolled()
    }
}

impl fmt::Display for LookAheadNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind_str = if self.is_group {
            "group".to_string()
        } else if self.kind == DialogueCheckKind::None {
            "node".to_string()
        } else {
            format!("{:?}", self.kind).to_lowercase()
        };
        let price = if self.is_cost_option() { format!(", cost {}", self.cost) } else { String::new() };
        write!(f, "{} {}{}", kind_str, self.id, price)
    }
}
