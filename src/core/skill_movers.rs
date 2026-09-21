// SPDX-License-Identifier: MIT
//! Which dialogue actions can move the skill a passive check compares, and so unsettle its
//! outcome for the length of a search.
//!
//! ## The game
//!
//! `PassiveNode.CheckSuccess` (pre-final-cut export) compares `CharacterSheet.GetSkillValue` - the
//! skill's `value` - with the check's threshold, and `Modifiable.Recalc` sums that value over
//! every modifier the skill carries:
//!
//! ```text
//! foreach (Modifier modifier in modifiers)
//! {
//!     value += modifier.Amount;
//!     if (modifier.type == ModifierType.DAMAGE) damageValue += amount; else maximumValue += amount;
//!     ...
//! }
//! ```
//!
//! So two kinds of action move it in dialogue:
//!
//! - EQUIPMENT. A worn item's bonuses are modifiers. `LoseItem` unequips what it deletes
//!   (`Inventory.DeleteItem`), and `GainItem` equips an `autoequip` item
//!   (`Inventory.HandlePickedUpItem`). Which skills an item moves IS in the dialogue database,
//!   as the prose a player reads - `MediumTextValue`, "+1 Rhetoric: The heroic deeds (of
//!   others)". Seventy items carry one and sixty-four state a signed bonus. Nothing here reads
//!   it yet; de-sr1u.2 extracts it and de-sr1u.3 narrows the unsettling below by it.
//! - DAMAGE. `DamageVolition`, `HealVolition` and the endurance forms move the `DAMAGE` modifier
//!   of Volition or Endurance.
//!
//! ## What the engine does
//!
//! The plugin evaluates each passive check once, against the character as the request finds
//! it. Where a group can change what is worn, the graph is fitted to answer every passive check
//! Unknown instead (`LookAheadNode::check_settled`), which carries both outcomes - more markers
//! than earned, never fewer. A lost item counts only if the world has it on: an item not worn has
//! no bonus to take away. Such groups are rare, and nothing here knows which skill a garment
//! moves, so nothing narrower is possible YET - see the note above, which says where that is.
//!
//! DAMAGE unsettles a check only where it can cross the check's MARGIN, which the plugin sends for
//! Volition and Endurance passives ([`crate::world::ILookAheadWorld::check_margin`]): the skill
//! value plus the check's bonus, minus its threshold. A passing check (margin zero or more) flips
//! if the group's damage to the skill can exceed its margin; a failing one if the group's healing
//! can make up the shortfall. The totals add every blow or heal in the group that fires in the
//! world - a conditional one only while its thought is fixed; a blow on an entry that can be entered again round a cycle, or a heal of everything,
//! has no bound. `DamageValue` clamps a blow at the skill's value and a heal stops at its maximum,
//! which only ever makes the real change smaller. Marking every damaged skill's checks Unknown
//! instead would over-mark the most visited conversations - Kim's, group 29, damages Volition
//! unconditionally, and a blow of one rarely crosses a threshold.

use serde::{Deserialize, Serialize};

use crate::core::action::{DialogueAction, DialogueActionKind};
use crate::core::state::{DAMAGE_PREFIX, ITEM_PREFIX, StateSymbols, UNEQUIPPED_PREFIX};

/// One blow or heal an entry deals a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageMove {
    /// The skill, by `SkillType` name.
    pub skill: String,
    /// How much damage it adds - negative for a heal - or `None` for a heal of all of it.
    pub amount: Option<i32>,
    /// Whether it fires only the first time its entry is entered.
    pub once: bool,
    /// The thought it needs fixed to fire at all, if it is conditional.
    pub fixed_thought: Option<String>,
}

/// What one entry's actions can do to skill values, named before a group drops the slots nothing
/// reads - an unread `unequipped:` or `damage:` slot still moves a skill.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMoves {
    /// Items the entry takes away, which unequips them if worn.
    pub lost_items: Vec<String>,
    /// Whether the entry gains an item that puts itself on.
    pub puts_on: bool,
    /// The blows and heals the entry deals.
    pub damage: Vec<DamageMove>,
}

impl SkillMoves {
    /// What `actions` do to skill values, read off the slots they write.
    pub fn of<'a>(
        actions: impl IntoIterator<Item = &'a DialogueAction>,
        symbols: &StateSymbols,
    ) -> Self {
        let mut moves = Self::default();
        for action in actions {
            if !action.writes_slot() {
                continue;
            }
            let Some(name) = usize::try_from(action.slot())
                .ok()
                .and_then(|slot| symbols.name_of(slot))
            else {
                continue;
            };
            if let Some(item) = name.strip_prefix(UNEQUIPPED_PREFIX) {
                moves.lost_items.push(item.to_string());
            } else if let Some(item) = name.strip_prefix(ITEM_PREFIX) {
                moves.puts_on |= action.value() > 0 && is_autoequip(item);
            } else if let Some(skill) = name.strip_prefix(DAMAGE_PREFIX) {
                moves.damage.push(DamageMove {
                    skill: skill.to_string(),
                    amount: (action.kind() == DialogueActionKind::Increment)
                        .then_some(action.value()),
                    once: action.is_once(),
                    fixed_thought: action.fixed_thought().map(str::to_string),
                });
            }
        }
        moves
    }
}

/// How far a group can move one skill's value, each way: `None` where it has no bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageReach {
    pub damage: Option<i64>,
    pub healing: Option<i64>,
}

impl Default for DamageReach {
    fn default() -> Self {
        Self {
            damage: Some(0),
            healing: Some(0),
        }
    }
}

impl DamageReach {
    /// Adds one blow or heal, dealt from an entry that can be entered again or not.
    pub fn add(&mut self, damage: &DamageMove, repeatable: bool) {
        let add = |total: &mut Option<i64>, amount: i32| {
            *total = match (*total, repeatable) {
                (Some(sum), false) => Some(sum + i64::from(amount)),
                _ => None,
            };
        };
        match damage.amount {
            Some(amount) if amount > 0 => add(&mut self.damage, amount),
            Some(amount) => add(&mut self.healing, -amount),
            None => self.healing = None,
        }
    }

    /// Whether a check with this margin can flip, on a skill already carrying `damaged` points of
    /// damage - `None` where that is not known.
    ///
    /// A heal only undoes damage (`HealValue` stops at the maximum), so the value can never rise
    /// above where it starts by more than the damage already there - whatever is dealt and
    /// healed on the way.
    pub fn can_flip(&self, margin: i32, damaged: Option<i64>) -> bool {
        let margin = i64::from(margin);
        if margin >= 0 {
            return self.damage.is_none_or(|damage| damage > margin);
        }
        let rise = match (self.healing, damaged) {
            (Some(healing), Some(damaged)) => Some(healing.min(damaged)),
            (healing, None) => healing,
            (None, damaged) => damaged,
        };
        rise.is_none_or(|rise| rise >= -margin)
    }
}

/// The items `GainItem` equips on pickup: the database's `autoequip` items, all of them the
/// outfit the game starts in.
const AUTOEQUIP_ITEMS: [&str; 7] = [
    "jacket_suede",
    "neck_tie",
    "pants_bellbottom",
    "shirt_dress_disco",
    "shoes_snakeskin",
    "shoes_snakeskin_left",
    "shoes_snakeskin_right",
];

/// Whether gaining `item` puts it on.
pub fn is_autoequip(item: &str) -> bool {
    AUTOEQUIP_ITEMS.contains(&item)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_flips_only_where_the_group_can_cross_its_margin() {
        let blow = |amount| DamageMove {
            skill: "VOLITION".to_string(),
            amount: Some(amount),
            once: false,
            fixed_thought: None,
        };
        let unknown = None;
        let mut reach = DamageReach::default();
        reach.add(&blow(2), false);
        assert!(
            reach.can_flip(1, unknown),
            "two damage takes a margin of one below zero"
        );
        assert!(
            !reach.can_flip(2, unknown),
            "and leaves a margin of two at zero, still clearing"
        );
        assert!(
            !reach.can_flip(-1, unknown),
            "no healing, so a failing check stays failed"
        );

        reach.add(&blow(-3), false);
        assert!(reach.can_flip(-3, unknown));
        assert!(!reach.can_flip(-4, unknown));
        assert!(
            !reach.can_flip(-3, Some(2)),
            "a heal of three lifts an undamaged-but-two skill by only two"
        );
        assert!(reach.can_flip(-3, Some(3)));

        reach.add(&blow(1), true);
        assert!(
            reach.can_flip(100, unknown),
            "a blow round a cycle has no bound"
        );
    }

    #[test]
    fn the_outfit_is_what_puts_itself_on() {
        assert!(is_autoequip("neck_tie"));
        assert!(!is_autoequip("hat_mullen"));
    }
}
