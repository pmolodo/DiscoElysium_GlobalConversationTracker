// SPDX-License-Identifier: MIT
use serde::{Deserialize, Serialize};
use std::fmt;
use std::hash::{Hash, Hasher};

/// Identifies one dialogue entry: the conversation it belongs to, and its id within that conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialogueNodeId {
    pub conversation_id: i32,
    pub entry_id: i32,
}

impl DialogueNodeId {
    pub const fn new(conversation_id: i32, entry_id: i32) -> Self {
        Self {
            conversation_id,
            entry_id,
        }
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

/// What ended a search before it explored everything reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum LookAheadLimit {
    None = 0,
    States = 1,
    Time = 2,
    /// The search frontier grew past what it was allowed to hold.
    ///
    /// The limit that governs by default, because it is the one that means the same thing
    /// in every conversation. A state carries one slot per tracked variable in its group, so
    /// a budget counted in STATES buys between 136 and 455 megabytes depending on which
    /// conversation the player is standing in - measured - and a
    /// number that elastic protects nothing in particular. See de-e23q.
    Memory = 3,

    /// The MACHINE could not supply what the search asked for, inside its budget.
    ///
    /// A DIFFERENT THING FROM [`Self::Memory`], and they want opposite responses.
    /// `Memory` is the ceiling the CALLER set: the search behaved, and a player who wants
    /// more markers can raise it. This is the allocator saying no, which says nothing at
    /// all about the algorithm and cannot be fixed by turning a dial - it wants the memory
    /// freed, or a bigger machine.
    ///
    /// The alternative to reporting it is not a wrong answer, it is NO PROCESS: an
    /// infallible allocation that fails reaches `handle_alloc_error`, which aborts, and the
    /// engine runs inside the game. See `tests/out_of_memory.rs` for what is and is not
    /// covered.
    NoMemory = 4,
}

/// Which outcome of a rolled start a search explores.
///
/// A white or red check is the one node that can be entered in two ways: the roll passes
/// and its success flag is set, or it fails and - for a red check - its failure flag is.
/// A search told which of those it is exploring answers about that outcome alone, which is
/// what lets the mod draw a check's two halves apart - see de-8hh2.6.
///
/// HERE RATHER THAN BESIDE ONE SEARCH, because more than one thing asks the question: the
/// entry step, the backward driver and the refusal walk in front of them all take it.
/// It names the ROLL rather than an index into anything a particular search builds.
///
/// A start that does not roll leaves exactly one way in, and that way is its `Pass`; its
/// `Fail` is empty, because there is no failure to explore. The bridge asks for branches
/// only where the start is a white or red check, so that case is a definition rather than
/// a situation - but it is the definition that keeps `Fail` from quietly meaning `Pass`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartBranch {
    /// Both, when the start rolls. What an ordinary search wants: the answer is the best
    /// anything reachable can offer, and which side of a roll it lay on does not change it.
    Either,

    /// The roll passed.
    Pass,

    /// The roll failed. EMPTY WHERE THERE IS NO SUCH BRANCH - a red check with no failure
    /// flag has nowhere to fail to, and the honest answer is that the search found nothing
    /// rather than that it explored the pass branch twice.
    Fail,
}
