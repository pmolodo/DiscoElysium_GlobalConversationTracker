// SPDX-License-Identifier: MIT
//! What the player is wearing and holding, which is what `CheckEquipped` asks about.
//!
//! ## The game's definition
//!
//! From the pre-final-cut export, `Sunshine.Dialogue.InventoryLuaFunctions` and
//! `Sunshine.Metric.InventoryViewData` (Assets/Scripts/Assembly-CSharp). Final Cut's bodies
//! are stripped; its `EquipmentSlotType` and the members below are declared identically, and
//! the bodies are taken to be unchanged.
//!
//! ```text
//! public static bool CheckEquipped(string itemName)
//! {
//!     try
//!     {
//!         return InventoryViewData.Singleton.IsEquipped(itemName);
//!     }
//!     catch (Exception ex)
//!     {
//!         Debug.LogError("CheckEquipped(\"" + itemName + "\"): " + ex);
//!     }
//!     return false;
//! }
//!
//! public bool IsEquipped(string itemName)
//! {
//!     InventoryItem byName = SingletonComponent<InventoryItemList>.Singleton.GetByName(itemName);
//!     if (byName == null)
//!     {
//!         Debug.Log("Invalid item: " + itemName);
//!         return false;
//!     }
//!     if (byName.type.Equals(ItemType.HELD))
//!     {
//!         if (!itemName.Equals(GetEquipped(EquipmentSlotType.HELDLEFT)))
//!         {
//!             return itemName.Equals(GetEquipped(EquipmentSlotType.HELDRIGHT));
//!         }
//!         return true;
//!     }
//!     return itemName.Equals(GetEquipped(byName.type));
//! }
//!
//! public string GetEquipped(EquipmentSlotType slotType)
//! {
//!     if (equipment.ContainsKey(slotType))
//!     {
//!         return equipment[slotType];
//!     }
//!     return null;
//! }
//! ```
//!
//! ## Why this asks every slot rather than the item's own
//!
//! The game looks in ONE slot - the item's type's, or either hand for a held item - so
//! emulating it literally needs each item's type, which is database data the engine does not
//! carry. Every slot is asked instead, and an item is equipped when any slot holds it. The two
//! agree, because the only way into a slot is `Equip`, which files an item under the slot its
//! type maps to: an item cannot be sitting in another type's slot for the literal reading to
//! miss. The name the game cannot find answers false in both - `GetByName` returns null in the
//! game, and no slot can hold an item that does not exist here. That covers the corpus's own
//! `CheckEquipped("jacket_rcm ")`, trailing space and all.

/// Every slot the game keeps equipment in, by `EquipmentSlotType` name, in the enum's order.
///
/// `_ERROR`, the enum's last value, is what an unmappable type converts to and never holds
/// anything, so it is not asked.
pub const SLOTS: [&str; 12] = [
    "ARMOR",
    "COAT",
    "GLASSES",
    "GLOVES",
    "HAT",
    "JACKET",
    "NECK",
    "PANTS",
    "SHIRT",
    "SHOES",
    "HELDLEFT",
    "HELDRIGHT",
];

/// The query this module answers.
pub const CHECK_EQUIPPED: &str = "CheckEquipped";

/// Whether `item` is in any slot, given what each slot holds.
///
/// `in_slot` answers a slot's item - empty for an empty slot - or `None` where nobody could
/// read it. A slot holding the item settles the answer whatever else is unread; otherwise an
/// unread slot leaves it unknowable, since the item may be the one in it.
pub fn is_equipped<'a>(item: &str, in_slot: impl Fn(&str) -> Option<&'a str>) -> Option<bool> {
    let mut all_read = true;
    for slot in SLOTS {
        match in_slot(slot) {
            Some(held) if held == item => return Some(true),
            Some(_) => {}
            None => all_read = false,
        }
    }

    all_read.then_some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_in_any_slot_is_equipped() {
        let answer = is_equipped("neck_tie", |slot| {
            Some(if slot == "NECK" { "neck_tie" } else { "" })
        });
        assert_eq!(answer, Some(true));
    }

    #[test]
    fn an_item_in_no_slot_is_not_equipped() {
        let answer = is_equipped("neck_tie", |slot| {
            Some(if slot == "HAT" { "hat_faln" } else { "" })
        });
        assert_eq!(answer, Some(false));
    }

    #[test]
    fn an_unread_slot_leaves_a_missing_item_unknowable() {
        let answer = is_equipped("neck_tie", |slot| (slot != "NECK").then_some(""));
        assert_eq!(answer, None);
    }

    #[test]
    fn an_unread_slot_does_not_unmake_an_item_found_elsewhere() {
        let answer = is_equipped("chaincutters", |slot| match slot {
            "HELDLEFT" => Some("chaincutters"),
            "NECK" => None,
            _ => Some(""),
        });
        assert_eq!(answer, Some(true));
    }
}
