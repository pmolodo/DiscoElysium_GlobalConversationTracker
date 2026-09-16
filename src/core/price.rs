// SPDX-License-Identifier: MIT
//! What a priced entry (`ClickCost`) actually costs, which in hardcore mode is not always its
//! `ClickCost`.
//!
//! ## The game
//!
//! `CostOptionNode.GetCost`, from the pre-final-cut export; Final Cut's export has the body
//! stripped, and Cpp2IL's ISIL dump of the shipped build makes the same calls:
//!
//! ```text
//! int num = Field.LookupInt(entry.fields, "ClickCost");
//! InventoryItem inventoryItem = FindItemInResponse(entry);
//! if (inventoryItem == null) return num;
//! if (IsHealingItem(inventoryItem.GetDisplayName()))
//!     num = (int)((float)num * GameModeController.HealingItemCostMultiplier);
//! else if (IsDrugItem(inventoryItem.GetDisplayName()))
//!     num = (int)((float)num * GameModeController.DrugItemCostMultiplier);
//! return num;
//! ```
//!
//! with
//!
//! ```text
//! healingItems = { "Nosaphed", "Drouamine", "Magnesium", "Hypnogamma" };
//! drugItems = { "Alcohol \"Commodore Red\"", "Alcohol \"Potent Pilsner\"", "Smokes \"Astra\"" };
//! ```
//!
//! `HaveMoney`, `IsHidden` and the charge in `HandleEntry` all go through `GetCost`, so the
//! scaled price is the one the option is disabled, hidden and paid by.
//!
//! Each multiplier is a field of the current mode's `GameModeData`. Final Cut's
//! `Normal Mode.asset` and `Hardcore Mode.asset` hold:
//!
//! ```text
//!                  healingItemCostMultiplier  drugItemCostMultiplier
//! Normal Mode      1                          1
//! Hardcore Mode    2                          2
//! ```
//!
//! ## Which item an entry buys
//!
//! `FindItemInResponse` walks the entry's own conversation from the priced entry and stops at
//! the first entry whose script names an item. It evaluates each entry's condition as it goes,
//! and skips one whose condition is false - so where the walk meets a guard, which item it
//! finds can depend on the world. [`crate::index::price`] replays the walk when the graph is
//! built.
//!
//! ## What the engine does
//!
//! Each priced entry carries the [`PriceScale`] its walk finds, and a graph paired with a world
//! is priced for that world's mode by [`crate::graph::LookAheadGraph::price_for`].
//!
//! AN ENTRY WHOSE WALK MEETS A GUARD IS PRICED AT ITS `ClickCost`. No multiplier is below one,
//! so the unscaled price is never above what the game asks: the engine may show a purchase the
//! game refuses, never refuse one the game allows. That is the same direction every Unknown
//! errs in.
//!
//! Of the 84 priced entries, nine reach a scaled item with no guard on the walk: the
//! pharmacy's four healing items (conversation 475) and the drinks and smokes sold in
//! conversations 552 and 902. Three more - conversation 28's room at 20 real, offered three
//! ways - reach a Commodore Red only past more than twenty guards on Garte's hub, and are held
//! unscaled. `tests/hardcore_prices.rs` pins the nine.

/// Which of the two multipliers a priced entry is scaled by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PriceScale {
    /// `GameModeController.HealingItemCostMultiplier`.
    Healing,
    /// `GameModeController.DrugItemCostMultiplier`.
    Drug,
}

/// `CostOptionNode.healingItems`, by the item names a script uses rather than the display
/// names the game compares - `item_names.jsonl` pairs them.
const HEALING_ITEMS: [&str; 4] = ["nosaphed", "drouamine", "magnesium", "hypnogamma"];

/// `CostOptionNode.drugItems`, the same way.
const DRUG_ITEMS: [&str; 3] = [
    "drug_alcohol_commodore_red",
    "drug_alcohol_potent_pilsner",
    "drug_smokes_astra",
];

/// Both multipliers in `Hardcore Mode.asset`; `Normal Mode.asset` has 1 for both.
const HARDCORE_HEALING_MULTIPLIER: f32 = 2.0;
const HARDCORE_DRUG_MULTIPLIER: f32 = 2.0;

impl PriceScale {
    /// The scale buying `item` puts on a price, if any.
    pub fn of_item(item: &str) -> Option<Self> {
        if HEALING_ITEMS.contains(&item) {
            Some(Self::Healing)
        } else if DRUG_ITEMS.contains(&item) {
            Some(Self::Drug)
        } else {
            None
        }
    }

    /// The multiplier in the given mode.
    fn multiplier(self, hardcore: bool) -> f32 {
        match (self, hardcore) {
            (_, false) => 1.0,
            (Self::Healing, true) => HARDCORE_HEALING_MULTIPLIER,
            (Self::Drug, true) => HARDCORE_DRUG_MULTIPLIER,
        }
    }
}

/// What the game charges for an entry with this `ClickCost` and scale.
///
/// `(int)((float)num * multiplier)`: a single-precision product, truncated toward zero.
pub fn price(click_cost: i32, scale: Option<PriceScale>, hardcore: bool) -> i32 {
    match scale {
        Some(scale) => (click_cost as f32 * scale.multiplier(hardcore)) as i32,
        None => click_cost,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardcore_doubles_healing_and_drug_prices_and_nothing_else() {
        assert_eq!(price(90, Some(PriceScale::Healing), true), 180);
        assert_eq!(price(290, Some(PriceScale::Drug), true), 580);
        assert_eq!(price(90, Some(PriceScale::Healing), false), 90);
        assert_eq!(price(5000, None, true), 5000);
    }

    #[test]
    fn items_are_named_as_scripts_name_them() {
        assert_eq!(PriceScale::of_item("nosaphed"), Some(PriceScale::Healing));
        assert_eq!(
            PriceScale::of_item("drug_smokes_astra"),
            Some(PriceScale::Drug)
        );
        assert_eq!(PriceScale::of_item("drug_alcohol_pale_aged_vodka"), None);
        assert_eq!(PriceScale::of_item("magnesium_based_lifeform"), None);
    }
}
