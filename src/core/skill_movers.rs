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
//!   database does not carry.
//! - DAMAGE. `DamageVolition`, `HealVolition` and the endurance forms move the `DAMAGE` modifier
//!   of Volition or Endurance.
//!
//! ## What the engine does
//!
//! The plugin evaluates each passive check once, against the character as the request finds
//! it. Where a group can change what is worn, the graph is fitted to answer every passive check
//! Unknown instead (`LookAheadNode::check_settled`), which carries both outcomes - more markers
//! than earned, never fewer. A lost item counts only if the world has it on: an item not worn has
//! no bonus to take away. Such groups are rare, and the bonus sizes are not known, so nothing
//! narrower is possible.
//!
//! DAMAGE IS HELD at the plugin's answer. Treating every Volition or Endurance check as Unknown in
//! a group that damages the skill over-marks the most visited conversations - Kim's, group 29,
//! damages Volition unconditionally, and a blow of one rarely crosses a check's threshold. Doing it
//! exactly needs each check's margin from the plugin (de-70eo.18).

use serde::{Deserialize, Serialize};

use crate::core::action::DialogueAction;
use crate::core::state::{ITEM_PREFIX, StateSymbols, UNEQUIPPED_PREFIX};

/// What one entry's actions can do to what is worn, named before a group drops the slots nothing
/// reads - an unread `unequipped:` slot still takes an item off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMoves {
    /// Items the entry takes away, which unequips them if worn.
    pub lost_items: Vec<String>,
    /// Whether the entry gains an item that puts itself on.
    pub puts_on: bool,
}

impl SkillMoves {
    /// What `actions` do to what is worn, read off the slots they write.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outfit_is_what_puts_itself_on() {
        assert!(is_autoequip("neck_tie"));
        assert!(!is_autoequip("hat_mullen"));
    }
}
