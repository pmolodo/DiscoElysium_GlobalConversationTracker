// SPDX-License-Identifier: MIT
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::core::action::DialogueAction;
use crate::core::guard::Guard;
use crate::core::price::PriceScale;
use crate::core::skill_movers::SkillMoves;

fn settled_by_default() -> bool {
    true
}
use crate::core::types::{DialogueCheckKind, DialogueNodeId};

/// One dialogue entry as the look-ahead needs it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookAheadNode {
    pub id: DialogueNodeId,
    pub is_group: bool,
    pub player: bool,
    /// A player line offered alongside another player line.
    pub choice: bool,
    /// Whether this entry's sequence keeps its line on screen, so leaving it costs a continue
    /// whatever is behind it - see [`crate::index::sequence_holds_the_screen`].
    pub holds_the_screen: bool,
    pub kind: DialogueCheckKind,
    pub guard: Guard,
    pub actions: Vec<DialogueAction>,
    /// What a rolled or fake check's FAILING branch does beyond recording the failure, applied
    /// after the failure flag - see [`crate::core::thought_effects`]. Empty for anything else.
    #[serde(default)]
    pub failure_actions: Vec<DialogueAction>,
    /// What this entry's actions do to skill values - see [`crate::core::skill_movers`].
    #[serde(default)]
    pub skill_moves: SkillMoves,
    /// Whether a passive check's outcome is the world's answer, or Unknown because the group
    /// can change what is worn - set by [`crate::graph::LookAheadGraph::fit`].
    #[serde(default = "settled_by_default")]
    pub check_settled: bool,
    pub links: Vec<DialogueNodeId>,
    /// The price a search charges and checks the purse against: [`Self::click_cost`] as
    /// [`crate::graph::LookAheadGraph::fit`] last priced it for a game mode.
    pub cost: i32,
    /// The entry's own `ClickCost`, before any game mode scales it.
    pub click_cost: i32,
    /// Which hardcore multiplier the price takes, if any - see [`crate::core::price`].
    pub price_scale: Option<PriceScale>,
    pub cost_once: bool,
    pub hidden_when_unaffordable: bool,
    pub flag_slot: i32,        // -1 if none
    pub failed_flag_slot: i32, // -1 if none
    pub boolean_only: bool,
    pub seen_slot: i32, // -1 if none
    /// The slot recording that this entry's once-only effects have fired, or -1.
    ///
    /// Two different things share it, because they ask the same question - has this
    /// entry already had its one-time effect: a `cost_once` charge, and any action
    /// marked `once`. A node needing neither has no slot, which is most of them.
    ///
    /// Assigned by [`crate::graph::LookAheadGraph::new`] rather than passed to
    /// [`LookAheadNode::new`], because interning a name mutates the symbol table and the
    /// graph is what owns it. Doing it there is what lets the table be FROZEN before a
    /// search starts: the search only ever reads slots, never creates them.
    pub once_slot: i32, // -1 if none
}

impl LookAheadNode {
    /// An ordinary entry with nothing on it: no guard, no actions, no links, no cost, no check
    /// and no slots.
    ///
    /// A STARTING POINT FOR A STRUCT UPDATE rather than a constructor that takes every field.
    /// The fields are public and most entries leave most of them alone, so a caller names what
    /// it sets - `LookAheadNode { guard, links, ..LookAheadNode::new(id) }` - and a list of
    /// thirteen positional arguments, most of them `false` or `-1`, stops being a place to put
    /// one in the wrong slot.
    pub fn new(id: DialogueNodeId) -> Self {
        Self {
            id,
            is_group: false,
            player: false,
            choice: false,
            holds_the_screen: false,
            kind: DialogueCheckKind::None,
            guard: Guard::always_true(),
            actions: Vec::new(),
            failure_actions: Vec::new(),
            skill_moves: SkillMoves::default(),
            check_settled: true,
            links: Vec::new(),
            cost: 0,
            click_cost: 0,
            price_scale: None,
            cost_once: false,
            hidden_when_unaffordable: false,
            flag_slot: -1,
            failed_flag_slot: -1,
            boolean_only: false,
            seen_slot: -1,
            once_slot: -1,
        }
    }

    /// Whether this entry has a one-time effect worth a slot to remember.
    ///
    /// Asked once, when the graph is built. Interning a slot for every node instead
    /// would widen the state vector by one bit per entry - 4,514 of them in the largest
    /// conversation group - to answer a question almost none of them ask.
    pub fn needs_once_slot(&self) -> bool {
        self.cost_once || self.actions.iter().any(|action| action.is_once())
    }

    /// Every action the entry can take, on entering and on a failing branch alike - for what
    /// asks which slots, thoughts or money an entry can touch rather than when.
    pub fn all_actions(&self) -> impl Iterator<Item = &DialogueAction> {
        self.actions.iter().chain(&self.failure_actions)
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
        let price = if self.is_cost_option() {
            format!(", cost {}", self.cost)
        } else {
            String::new()
        };
        write!(f, "{} {}{}", kind_str, self.id, price)
    }
}
