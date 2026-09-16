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
//!   (`Inventory.HandlePickedUpItem`). Which skills an item moves is item data the dialogue
//!   database does not carry, so any such change unsettles every passive check.
//! - DAMAGE. `DamageVolition`, `HealVolition` and the endurance forms move the `DAMAGE` modifier
//!   of Volition or Endurance, and so the value a check on that skill compares.
//!
//! ## What the engine does
//!
//! The plugin evaluates each passive check once, against the character as the request finds
//! it. Where a group can move the checked skill, the graph is fitted to answer that check
//! Unknown instead (`LookAheadNode::check_settled`), which carries both outcomes - more markers
//! than earned, never fewer. A lost item unsettles checks only if the world has it on: an item
//! not worn has no bonus to take away.

use serde::{Deserialize, Serialize};

use crate::core::action::DialogueAction;
use crate::core::state::{DAMAGE_PREFIX, ITEM_PREFIX, StateSymbols, UNEQUIPPED_PREFIX};

/// What one entry's actions can do to skill values, named before a group drops the slots
/// nothing reads - an unread `unequipped:` or `damage:` slot still moves a skill.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMoves {
    /// Items the entry takes away, which unequips them if worn.
    pub lost_items: Vec<String>,
    /// Whether the entry gains an item that puts itself on.
    pub puts_on: bool,
    /// Skills whose damage the entry moves, by `SkillType` name.
    pub damaged_skills: Vec<String>,
}

impl SkillMoves {
    /// What `actions` do to skills, read off the slots they write.
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
                moves.damaged_skills.push(skill.to_string());
            }
        }
        moves
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

/// The skills damage moves, by the speaker's actor id in the shipped database - Volition is
/// actor 405 and Endurance 409, which `tests/shipped_index.rs` pins against the actor table.
pub const DAMAGEABLE_SKILL_ACTORS: [(&str, &str); 2] = [("405", "VOLITION"), ("409", "ENDURANCE")];

/// The damageable skill a passive check spoken by `actor` tests, if it tests one.
pub fn damageable_skill_of_actor(actor: &str) -> Option<&'static str> {
    DAMAGEABLE_SKILL_ACTORS
        .iter()
        .find(|(id, _)| *id == actor)
        .map(|(_, skill)| *skill)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outfit_is_what_puts_itself_on() {
        assert!(is_autoequip("neck_tie"));
        assert!(!is_autoequip("hat_mullen"));
        assert_eq!(damageable_skill_of_actor("405"), Some("VOLITION"));
        assert_eq!(damageable_skill_of_actor("399"), None);
    }
}
