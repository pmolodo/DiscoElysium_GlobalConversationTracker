// SPDX-License-Identifier: MIT
//! Whether the player holds anything in an item group, which is what `CheckItemGroup` asks.
//!
//! ## The game's definition
//!
//! From the pre-final-cut export (Assets/Scripts/Assembly-CSharp):
//! `Sunshine.Dialogue.InventoryLuaFunctions`, `Sunshine.Inventory` and `ItemUtil`. Final Cut's
//! bodies are stripped and taken to be unchanged.
//!
//! ```text
//! public static bool CheckItemGroup(string group)
//! {
//!     return Inventory.CheckItemGroup(group);
//! }
//!
//! public static bool CheckItemGroup(string group)
//! {
//!     foreach (InventoryItem gainedItem in SingletonComponent<World>.Singleton.you.items.gainedItems)
//!     {
//!         if (ItemUtil.GetItemGroup(gainedItem.group) == group)
//!         {
//!             return true;
//!         }
//!     }
//!     return false;
//! }
//!
//! public static string[] itemGroup = new string[7] { "none", "alcohol", "smokes", "ghb", "speed", "pyrholidon", "tare" };
//!
//! public static string GetItemGroup(ItemGroup iGroup)
//! {
//!     return itemGroup[(int)iGroup];
//! }
//! ```
//!
//! ## Why it is answered per member
//!
//! The game walks what is held and asks each item its group. The engine cannot walk what is
//! held, because what is held changes as a search runs: `GainItem` and `LoseItem` write an
//! `item:` slot. So the question is turned round - the group's MEMBERS are read from the
//! database, and the group is held when any member is, each member answered the way
//! `CheckItem` is: from its slot where the group moves it, and from what the player held when
//! the crawl started where it does not. Both halves are data the plugin reads rather than
//! runs - see `DataKind::ItemsInGroup` and `DataKind::HeldItemsInGroup`.

/// The query this module answers.
pub const CHECK_ITEM_GROUP: &str = "CheckItemGroup";

/// Whether any member of a group is held.
///
/// `members` is the group's items, or `None` where they could not be read. `tracked` answers a
/// member the search moves - its slot's value - and `None` for one it does not; `initially`
/// answers the rest from the starting inventory, or `None` where that could not be read.
///
/// A member known to be held settles the answer. Otherwise any member that could not be
/// answered leaves it unknowable, since it may be the one held.
pub fn any_held(
    members: Option<&[String]>,
    tracked: impl Fn(&str) -> Option<bool>,
    initially: impl Fn(&str) -> Option<bool>,
) -> Option<bool> {
    let members = members?;
    let mut all_answered = true;
    for item in members {
        match tracked(item).or_else(|| initially(item)) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => all_answered = false,
        }
    }

    all_answered.then_some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn booze() -> Vec<String> {
        vec!["pilsner".to_string(), "vodka".to_string()]
    }

    #[test]
    fn a_member_held_from_the_start_holds_the_group() {
        let members = booze();
        let answer = any_held(Some(&members), |_| None, |item| Some(item == "vodka"));
        assert_eq!(answer, Some(true));
    }

    #[test]
    fn a_member_the_search_gained_holds_the_group() {
        let members = booze();
        let answer = any_held(
            Some(&members),
            |item| (item == "pilsner").then_some(true),
            |_| Some(false),
        );
        assert_eq!(answer, Some(true));
    }

    #[test]
    fn a_member_the_search_lost_does_not_hold_it() {
        let members = booze();
        let answer = any_held(
            Some(&members),
            |item| (item == "vodka").then_some(false),
            |item| Some(item == "vodka"),
        );
        assert_eq!(answer, Some(false));
    }

    #[test]
    fn unread_members_leave_it_unknowable() {
        assert_eq!(any_held(None, |_| None, |_| Some(false)), None);
        let members = booze();
        assert_eq!(any_held(Some(&members), |_| None, |_| None), None);
    }

    #[test]
    fn a_group_with_no_members_is_never_held() {
        assert_eq!(any_held(Some(&[]), |_| None, |_| None), Some(false));
    }
}
