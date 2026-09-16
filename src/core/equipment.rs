// SPDX-License-Identifier: MIT
//! What the player is wearing and holding, which is what `CheckEquipped`, the clothing
//! questions and the held-group questions ask about.
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
//! public static bool CheckEquippedGroup(string group)
//! {
//!     return InventoryViewData.Singleton.IsGroupEquipped(group);
//! }
//!
//! public static bool CheckHeldLeftGroup(string group)
//! {
//!     return InventoryViewData.Singleton.IsGroupHeldLeft(group);
//! }
//!
//! public static bool CheckHeldRightGroup(string group)
//! {
//!     return InventoryViewData.Singleton.IsGroupHeldRight(group);
//! }
//!
//! public bool IsGroupEquipped(string groupName)
//! {
//!     if (!IsGroupHeld(groupName))
//!     {
//!         if (GetEquipped(groupName) == null)
//!         {
//!             return false;
//!         }
//!         return true;
//!     }
//!     return true;
//! }
//!
//! public bool IsGroupHeld(string groupName)
//! {
//!     if (!IsGroupHeldLeft(groupName))
//!     {
//!         return IsGroupHeldRight(groupName);
//!     }
//!     return true;
//! }
//!
//! public bool IsGroupHeldRight(string groupName)
//! {
//!     InventoryItem itemInSlot = GetItemInSlot(EquipmentSlotType.HELDRIGHT);
//!     if (itemInSlot != null && ItemUtil.GetGroupByName(groupName) == itemInSlot.group)
//!     {
//!         return true;
//!     }
//!     return false;
//! }
//!
//! public string GetEquipped(string itemTypeValue)
//! {
//!     EquipmentSlotType slotByType = ItemUtil.GetSlotByType(itemTypeValue);
//!     return GetEquipped(slotByType);
//! }
//!
//! // ItemUtil: both upper-case the name and fall back to _ERROR for anything the enum lacks.
//! public static EquipmentSlotType GetSlotByType(string itemType) { ... }
//! public static ItemGroup GetGroupByName(string itemGroup) { ... }
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
//! ## The group questions take two vocabularies
//!
//! `CheckEquippedGroup` is asked of `alcohol` and `smokes`, which are item groups, and of
//! `jacket`, `hat` and `gloves`, which are not - they are slots. The body answers both: a hand
//! holding an item of that group, or the slot that name maps to being filled. An item's group
//! is database data, so the members of the group come from `core::item_group`'s data read
//! rather than from a table here.
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

/// Whether anything worn or held is in a group, or a slot of that name is filled.
const CHECK_EQUIPPED_GROUP: &str = "CheckEquippedGroup";

/// Each question about the group of what one hand holds, with the hand's slot.
///
/// `CheckHeldLeftGroup` is asked by no guard in the shipped corpus; it is here because
/// `CheckEquippedGroup` is built from both hands and the two share one shape.
static HELD_GROUP: [(&str, &str); 2] = [
    ("CheckHeldLeftGroup", "HELDLEFT"),
    ("CheckHeldRightGroup", "HELDRIGHT"),
];

/// The game's `ItemGroup` names, lowercased as `ItemUtil.itemGroup` spells them - what
/// `ItemUtil.GetGroupByName` accepts once uppercased. `_ERROR`, the enum's last value, is what
/// every other name converts to and no item carries.
const ITEM_GROUPS: [&str; 7] = [
    "none",
    "alcohol",
    "smokes",
    "ghb",
    "speed",
    "pyrholidon",
    "tare",
];

/// The hand slot a held-group question asks about.
fn hand_of(name: &str) -> Option<&'static str> {
    HELD_GROUP
        .iter()
        .find(|(query, _)| *query == name)
        .map(|(_, slot)| *slot)
}

/// The item group an argument names, as `ItemUtil.GetGroupByName` resolves it - uppercased and
/// matched against the enum - or `None` for a name that resolves to `_ERROR`.
fn item_group_named(argument: &str) -> Option<&'static str> {
    ITEM_GROUPS
        .iter()
        .find(|group| group.eq_ignore_ascii_case(argument))
        .copied()
}

/// The equipment slot an argument names, as `ItemUtil.GetSlotByType(string)` resolves it, or
/// `None` for a name that resolves to `_ERROR`.
fn slot_named(argument: &str) -> Option<&'static str> {
    SLOTS
        .iter()
        .find(|slot| slot.eq_ignore_ascii_case(argument))
        .copied()
}

/// The slots `name` reads with this argument, or nothing where it is not a question this
/// module answers.
pub fn slots_read_by(name: &str, argument: Option<&str>) -> Vec<&'static str> {
    if let Some(hand) = hand_of(name) {
        return vec![hand];
    }

    match name {
        CHECK_EQUIPPED => SLOTS.to_vec(),
        WEIRD_CLOTHING => WEIRD_CLOTHING_SLOTS.to_vec(),
        CHECK_EQUIPPED_GROUP => {
            let mut slots = vec!["HELDLEFT", "HELDRIGHT"];
            slots.extend(argument.and_then(slot_named));
            slots
        }
        _ => slot_filled_by(name).into_iter().collect(),
    }
}

/// The item group whose members `name` needs with this argument, if any.
pub fn group_read_by(name: &str, argument: Option<&str>) -> Option<&'static str> {
    if name == CHECK_EQUIPPED_GROUP || hand_of(name).is_some() {
        argument.and_then(item_group_named)
    } else {
        None
    }
}

/// The answer to `name`, given its text argument, what each slot holds and each group's
/// members.
///
/// `in_slot` answers a slot's item - empty for an empty slot - or `None` where nobody could
/// read it; `members` answers an item group's items, or `None` likewise. The outer `None` means
/// `name` is not one of these questions; the inner one means the answer is not knowable from
/// what was read.
pub fn answer<'a>(
    name: &str,
    argument: Option<&str>,
    in_slot: impl Fn(&str) -> Option<&'a str>,
    members: impl Fn(&str) -> Option<Vec<String>>,
) -> Option<Option<bool>> {
    let filled = |slot: &str| in_slot(slot).map(|held| !held.is_empty());
    let held_in_group = |hand: &str| -> Option<bool> {
        let item = in_slot(hand)?;
        let Some(group) = argument.and_then(item_group_named) else {
            // A name that is no group resolves to `_ERROR`, which no item carries.
            return Some(false);
        };
        if item.is_empty() {
            return Some(false);
        }
        Some(members(group)?.iter().any(|member| member == item))
    };

    if name == CHECK_EQUIPPED {
        return Some(argument.and_then(|item| is_equipped(item, &in_slot)));
    }

    if name == WEIRD_CLOTHING {
        let [shirt, pants, shoes] = WEIRD_CLOTHING_SLOTS.map(filled);
        return Some(weird_clothing(shirt, pants, shoes));
    }

    if let Some(hand) = hand_of(name) {
        return Some(argument.and_then(|_| held_in_group(hand)));
    }

    if name == CHECK_EQUIPPED_GROUP {
        let Some(argument) = argument else {
            return Some(None);
        };
        // `GetEquipped(groupName)` asks the slot the name maps to, and `_ERROR` is a slot
        // nothing is ever filed under.
        let worn = slot_named(argument).map_or(Some(false), filled);
        return Some(any_of([
            held_in_group("HELDLEFT"),
            held_in_group("HELDRIGHT"),
            worn,
        ]));
    }

    slot_filled_by(name).map(filled)
}

/// `a || b || c`, where any of them may be unknowable: a known true settles it, and an unknown
/// otherwise leaves it unknown.
fn any_of<const N: usize>(parts: [Option<bool>; N]) -> Option<bool> {
    if parts.contains(&Some(true)) {
        return Some(true);
    }
    parts.iter().all(Option::is_some).then_some(false)
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

    /// [`super::answer`] in a world that knows no group's members.
    fn answer<'a>(
        name: &str,
        argument: Option<&str>,
        in_slot: impl Fn(&str) -> Option<&'a str>,
    ) -> Option<Option<bool>> {
        super::answer(name, argument, in_slot, |_| None)
    }

    /// The alcohol group, as the database files it.
    fn booze(group: &str) -> Option<Vec<String>> {
        (group == "alcohol").then(|| vec!["drug_alcohol_potent_pilsner".to_string()])
    }

    #[test]
    fn a_hand_holding_a_member_holds_the_group() {
        let holding = wearing(&[("HELDRIGHT", "drug_alcohol_potent_pilsner")]);
        assert_eq!(
            super::answer("CheckHeldRightGroup", Some("alcohol"), &holding, booze),
            Some(Some(true))
        );
        assert_eq!(
            super::answer("CheckHeldLeftGroup", Some("alcohol"), &holding, booze),
            Some(Some(false))
        );
        assert_eq!(
            super::answer("CheckHeldRightGroup", Some("smokes"), &holding, |_| Some(
                Vec::new()
            )),
            Some(Some(false))
        );
    }

    #[test]
    fn a_held_item_of_unknown_group_membership_is_unknowable() {
        let holding = wearing(&[("HELDRIGHT", "drug_alcohol_potent_pilsner")]);
        assert_eq!(
            super::answer("CheckHeldRightGroup", Some("alcohol"), &holding, |_| None),
            Some(None)
        );
        // An empty hand needs no membership at all.
        assert_eq!(
            super::answer("CheckHeldRightGroup", Some("alcohol"), wearing(&[]), |_| {
                None
            }),
            Some(Some(false))
        );
    }

    #[test]
    fn equipped_group_takes_a_slot_name_as_well_as_a_group() {
        let jacketed = wearing(&[("JACKET", "jacket_suede")]);
        assert_eq!(
            super::answer("CheckEquippedGroup", Some("jacket"), &jacketed, |_| None),
            Some(Some(true))
        );
        assert_eq!(
            super::answer("CheckEquippedGroup", Some("hat"), &jacketed, |_| None),
            Some(Some(false))
        );
        let drinking = wearing(&[("HELDLEFT", "drug_alcohol_potent_pilsner")]);
        assert_eq!(
            super::answer("CheckEquippedGroup", Some("alcohol"), &drinking, booze),
            Some(Some(true))
        );
        assert_eq!(
            slots_read_by("CheckEquippedGroup", Some("jacket")),
            vec!["HELDLEFT", "HELDRIGHT", "JACKET"]
        );
        assert_eq!(group_read_by("CheckEquippedGroup", Some("jacket")), None);
        assert_eq!(
            group_read_by("CheckEquippedGroup", Some("alcohol")),
            Some("alcohol")
        );
    }

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
        assert_eq!(slots_read_by("HasJacket", None), vec!["JACKET"]);
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
        assert!(slots_read_by("IsKimHere", None).is_empty());
    }
}
