// SPDX-License-Identifier: MIT
//! Whether an inventory tab holds anything, which is what `HasPawnablesInInventory` asks.
//!
//! ## The game's definition
//!
//! From the pre-final-cut export (Assets/Scripts/Assembly-CSharp):
//! `Sunshine.Dialogue.InventoryLuaFunctions` and `TabbedSlotData`, which `InventoryViewData`
//! derives from. Final Cut's bodies are stripped and taken to be unchanged.
//!
//! ```text
//! public static bool HasPawnablesInInventory()
//! {
//!     return !InventoryViewData.Singleton.IsTabEmpty(ItemTabGroup.PAWNABLES);
//! }
//!
//! public bool IsTabEmpty(TabType tabType)
//! {
//!     if (tabContents.ContainsKey(tabType))
//!     {
//!         return tabContents[tabType].Count == 0;
//!     }
//!     return true;
//! }
//! ```
//!
//! THE TAB IS NOT AN ITEM GROUP. `ItemTabGroup` is the inventory screen's tabs - TOOLS,
//! CLOTHES, PAWNABLES, READING - and has nothing to do with the `ItemGroup` substance
//! categories `CheckItemGroup` asks about. The plugin reads the tab table directly and a save
//! records it as `inventoryViewState.inventory`, so no item-to-tab table is needed.
//!
//! Constant for a search: `GainItem` and `LoseItem` do fill and empty the tab, but the engine
//! does not know which tab an item goes in, so a group that gains a pawnable and then asks
//! reads the tab as it was when the crawl started. One guard in the whole database asks this.

/// Each question, with the tab it asks about.
const TAB_QUERIES: [(&str, &str); 1] = [("HasPawnablesInInventory", "PAWNABLES")];

/// The tab `name` asks about, or `None` where it is not one of these questions.
pub fn tab_read_by(name: &str) -> Option<&'static str> {
    TAB_QUERIES
        .iter()
        .find(|(query, _)| *query == name)
        .map(|(_, tab)| *tab)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pawnables_is_the_pawnables_tab() {
        assert_eq!(tab_read_by("HasPawnablesInInventory"), Some("PAWNABLES"));
        assert_eq!(tab_read_by("IsKimHere"), None);
    }
}
