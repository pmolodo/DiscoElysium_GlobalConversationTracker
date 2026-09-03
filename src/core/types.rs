// SPDX-License-Identifier: MIT
use std::fmt;
use std::hash::{Hash, Hasher};
use serde::{Deserialize, Serialize};

/// Identifies one dialogue entry: the conversation it belongs to, and its id within that conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialogueNodeId {
    pub conversation_id: i32,
    pub entry_id: i32,
}

impl DialogueNodeId {
    pub const fn new(conversation_id: i32, entry_id: i32) -> Self {
        Self { conversation_id, entry_id }
    }
}

impl fmt::Display for DialogueNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.conversation_id, self.entry_id)
    }
}

impl Hash for DialogueNodeId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.conversation_id.hash(state);
        self.entry_id.hash(state);
    }
}

/// How new a dialogue entry is to the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum Novelty {
    SeenThisGame = 0,
    UnseenThisGame = 1,
    UnseenAnyGame = 2,
}

/// Which special node type an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum DialogueCheckKind {
    None = 0,
    Passive = 1,
    Red = 2,
    White = 3,
    Fake = 4,
    Test = 5,
    KimSwitch = 6,
}

impl DialogueCheckKind {
    pub fn is_rolled(self) -> bool {
        matches!(self, Self::Red | Self::White)
    }
}

/// Three-valued logic for guard evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Ternary {
    False = 0,
    True = 1,
    Unknown = 2,
}

impl Ternary {
    pub fn can_pass(self) -> bool {
        self != Self::False
    }
}

pub fn ternary_not(value: Ternary) -> Ternary {
    match value {
        Ternary::True => Ternary::False,
        Ternary::False => Ternary::True,
        Ternary::Unknown => Ternary::Unknown,
    }
}

pub fn ternary_and(left: Ternary, right: Ternary) -> Ternary {
    if left == Ternary::False || right == Ternary::False {
        Ternary::False
    } else if left == Ternary::True && right == Ternary::True {
        Ternary::True
    } else {
        Ternary::Unknown
    }
}

pub fn ternary_or(left: Ternary, right: Ternary) -> Ternary {
    if left == Ternary::True || right == Ternary::True {
        Ternary::True
    } else if left == Ternary::False && right == Ternary::False {
        Ternary::False
    } else {
        Ternary::Unknown
    }
}

/// What ended a crawl before it explored everything reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum LookAheadLimit {
    None = 0,
    States = 1,
    Time = 2,
}
