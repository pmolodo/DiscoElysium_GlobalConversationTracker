// SPDX-License-Identifier: MIT
//! What the player is wearing and holding, which is what `CheckEquipped` and the clothing
//! questions ask about.
//!
//! Every question here is answered from what each equipment slot holds, which the plugin
//! reads with `InventoryViewData.GetEquipped` and the offline fixture reads from a save's
//! `inventoryViewState.equipment`. [`slots_read_by`] says which slots a question needs, and
//! [`answer`] answers it.
//!
//! ## The game's definitions
//!
//! From the pre-final-cut export (Assets/Scripts/Assembly-CSharp):
//! `Sunshine.Dialogue.InventoryLuaFunctions`, `Sunshine.Dialogue.ClothingLuaFunctions`,
//! `TequilaClothing` and `Sunshine.Metric.InventoryViewData`. Final Cut's bodies are stripped;
//! its `EquipmentSlotType` and the members below are declared identically, and the bodies are
//! taken to be unchanged.
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
//! public static bool WeirdClothing()
//! {
//!     if (HasShirt() && HasPants())
//!     {
//!         return !HasShoes();
//!     }
//!     return true;
//! }
//!
//! public static bool HasHat()     { return TequilaClothing.HatEquipped(); }
//! public static bool HasNecktie() { return TequilaClothing.NeckEquipped(); }
//! public static bool HasJacket()  { return TequilaClothing.JacketEquipped(); }
//! public static bool HasShirt()   { return TequilaClothing.ShirtEquipped(); }
//! public static bool HasPants()   { return TequilaClothing.PantsEquipped(); }
//! public static bool HasShoes()   { return TequilaClothing.ShoesEquipped(); }
//!
//! // TequilaClothing, one per slot, all of this shape:
//! public static bool HatEquipped()
//! {
//!     return InventoryViewData.Singleton.IsEquipped(EquipmentSlotType.HAT);
//! }
//!
//! // InventoryViewData:
//! public bool IsEquipped(EquipmentSlotType slotType)
//! {
//!     if (equipment.ContainsKey(slotType))
//!     {
//!         return !string.IsNullOrEmpty(equipment[slotType]);
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
//! ## Why `CheckEquipped` asks every slot rather than the item's own
//!
//! The game looks in ONE slot - the item's type's, or either hand for a held item - so
//! emulating it literally needs each item's type, which is database data the engine does not
//! carry. Every slot is asked instead, and an item is equipped when any slot holds it. The two
//! agree, because the only way into a slot is `Equip`, which files an item under the slot its
//! type maps to: an item cannot be sitting in another type's slot for the literal reading to
//! miss. The name the game cannot find answers false in both - `GetByName` returns null in the
//! game, and no slot can hold an item that does not exist here. That covers the corpus's own
//! `CheckEquipped("jacket_rcm ")`, trailing space and all.
//!
//! `HasNecktie` and `HasPants` are asked by no guard in the shipped corpus. They are here
//! because `WeirdClothing` is built from `HasPants`, and the six share one shape.

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

/// Whether an item is in any slot.
const CHECK_EQUIPPED: &str = "CheckEquipped";

/// Missing a shirt or trousers, or barefoot.
const WEIRD_CLOTHING: &str = "WeirdClothing";

/// The slots `WeirdClothing` reads, through `HasShirt`, `HasPants` and `HasShoes`.
const WEIRD_CLOTHING_SLOTS: [&str; 3] = ["SHIRT", "PANTS", "SHOES"];

/// Each clothing question that asks whether one slot is filled, with its slot.
static SLOT_FILLED: [(&str, &str); 6] = [
    ("HasHat", "HAT"),
    ("HasNecktie", "NECK"),
    ("HasJacket", "JACKET"),
    ("HasShirt", "SHIRT"),
    ("HasPants", "PANTS"),
    ("HasShoes", "SHOES"),
];

/// The slot a one-slot clothing question asks about.
fn slot_filled_by(name: &str) -> Option<&'static str> {
    SLOT_FILLED
        .iter()
        .find(|(query, _)| *query == name)
        .map(|(_, slot)| *slot)
}

/// The slots `name` reads, or nothing where it is not a question this module answers.
pub fn slots_read_by(name: &str) -> &'static [&'static str] {
    match name {
        CHECK_EQUIPPED => &SLOTS,
        WEIRD_CLOTHING => &WEIRD_CLOTHING_SLOTS,
        _ => SLOT_FILLED
            .iter()
            .find(|(query, _)| *query == name)
            .map_or(&[], |(_, slot)| std::slice::from_ref(slot)),
    }
}

/// The answer to `name`, given its text argument and what each slot holds.
///
/// `in_slot` answers a slot's item - empty for an empty slot - or `None` where nobody could
/// read it. The outer `None` means `name` is not one of these questions; the inner one means
/// the answer is not knowable from what was read.
pub fn answer<'a>(
    name: &str,
    argument: Option<&str>,
    in_slot: impl Fn(&str) -> Option<&'a str>,
) -> Option<Option<bool>> {
    let filled = |slot: &str| in_slot(slot).map(|held| !held.is_empty());

    if name == CHECK_EQUIPPED {
        return Some(argument.and_then(|item| is_equipped(item, &in_slot)));
    }

    if name == WEIRD_CLOTHING {
        let [shirt, pants, shoes] = WEIRD_CLOTHING_SLOTS.map(filled);
        return Some(weird_clothing(shirt, pants, shoes));
    }

    slot_filled_by(name).map(filled)
}

/// Whether `item` is in any slot.
///
/// A slot holding the item settles the answer whatever else is unread; otherwise an unread
/// slot leaves it unknowable, since the item may be the one in it.
fn is_equipped<'a>(item: &str, in_slot: impl Fn(&str) -> Option<&'a str>) -> Option<bool> {
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

/// `WeirdClothing`, from whether each of its three slots is filled.
///
/// TRUE WHEN A SHIRT OR TROUSERS ARE MISSING, whatever the shoes - the `&&` stops at the
/// first false, so an unread slot beside a known empty one still answers. Only with both
/// worn does it become whether the player is barefoot.
fn weird_clothing(shirt: Option<bool>, pants: Option<bool>, shoes: Option<bool>) -> Option<bool> {
    match (shirt, pants) {
        (Some(false), _) | (_, Some(false)) => Some(true),
        (Some(true), Some(true)) => shoes.map(|worn| !worn),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A world where every slot is read and `worn` names what is in each filled one.
    fn wearing<'a>(worn: &'a [(&str, &'a str)]) -> impl Fn(&str) -> Option<&'a str> {
        move |slot| {
            Some(
                worn.iter()
                    .find(|(filled, _)| *filled == slot)
                    .map_or("", |(_, item)| *item),
            )
        }
    }

    #[test]
    fn an_item_in_any_slot_is_equipped() {
        let answer = answer(
            "CheckEquipped",
            Some("neck_tie"),
            wearing(&[("NECK", "neck_tie")]),
        );
        assert_eq!(answer, Some(Some(true)));
    }

    #[test]
    fn an_item_in_no_slot_is_not_equipped() {
        let answer = answer(
            "CheckEquipped",
            Some("neck_tie"),
            wearing(&[("HAT", "hat_faln")]),
        );
        assert_eq!(answer, Some(Some(false)));
    }

    #[test]
    fn an_unread_slot_leaves_a_missing_item_unknowable() {
        let answer = answer("CheckEquipped", Some("neck_tie"), |slot| {
            (slot != "NECK").then_some("")
        });
        assert_eq!(answer, Some(None));
    }

    #[test]
    fn an_unread_slot_does_not_unmake_an_item_found_elsewhere() {
        let answer = answer("CheckEquipped", Some("chaincutters"), |slot| match slot {
            "HELDLEFT" => Some("chaincutters"),
            "NECK" => None,
            _ => Some(""),
        });
        assert_eq!(answer, Some(Some(true)));
    }

    #[test]
    fn a_clothing_question_asks_whether_its_slot_is_filled() {
        let dressed = wearing(&[("HAT", "hat_faln")]);
        assert_eq!(answer("HasHat", None, &dressed), Some(Some(true)));
        assert_eq!(answer("HasShoes", None, &dressed), Some(Some(false)));
        assert_eq!(slots_read_by("HasJacket"), &["JACKET"]);
    }

    #[test]
    fn weird_clothing_is_true_without_a_shirt_whatever_the_shoes() {
        let shirtless = wearing(&[("PANTS", "pants_faln"), ("SHOES", "shoes_faln")]);
        assert_eq!(answer("WeirdClothing", None, shirtless), Some(Some(true)));
    }

    #[test]
    fn weird_clothing_with_shirt_and_trousers_is_whether_barefoot() {
        let shod = wearing(&[
            ("SHIRT", "shirt_faln"),
            ("PANTS", "pants_faln"),
            ("SHOES", "shoes_faln"),
        ]);
        assert_eq!(answer("WeirdClothing", None, shod), Some(Some(false)));

        let barefoot = wearing(&[("SHIRT", "shirt_faln"), ("PANTS", "pants_faln")]);
        assert_eq!(answer("WeirdClothing", None, barefoot), Some(Some(true)));
    }

    #[test]
    fn weird_clothing_is_settled_by_a_missing_shirt_even_with_other_slots_unread() {
        let answer = answer("WeirdClothing", None, |slot| {
            (slot == "SHIRT").then_some("")
        });
        assert_eq!(answer, Some(Some(true)));
    }

    #[test]
    fn a_question_this_module_does_not_own_is_left_alone() {
        assert_eq!(answer("IsKimHere", None, wearing(&[])), None);
        assert!(slots_read_by("IsKimHere").is_empty());
    }
}
