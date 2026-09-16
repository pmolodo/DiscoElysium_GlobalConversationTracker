// SPDX-License-Identifier: MIT
//! Which item a priced entry buys, found the way `CostOptionNode.FindItemInResponse` finds it.
//!
//! See [`crate::core::price`] for what the item does to the price. The walk, from the
//! pre-final-cut export:
//!
//! ```text
//! List<DialogueEntry> list = dialogueEntries.FindAll(e =>
//!     e.outgoingLinks.Find(l => l.destinationDialogueID == entry.id) != null);
//! hashSet.AddRange(list);
//! int num2 = 5; int num3 = 0; int num4 = 1;
//! stack.Push(entry);
//! while (num3 <= num2 && inventoryItem == null && stack.Count > 0)
//! {
//!     DialogueEntry dialogueEntry = stack.Pop();
//!     num4--;
//!     if (hashSet.Contains(dialogueEntry)) continue;
//!     if (num4 == 0) { num3++; num4 = stack.Count + dialogueEntry.outgoingLinks.Count; }
//!     if (!string.IsNullOrEmpty(dialogueEntry.conditionsString)
//!         && !Lua.IsTrue(dialogueEntry.conditionsString, ...)) continue;
//!     foreach link: push dialogueEntries.Find(e => e.id == link.destinationDialogueID) if found
//!     text = GetItemFromString(dialogueEntry.userScript);
//!     inventoryItem = InventoryItemList.GetByName(text);
//!     hashSet.Add(dialogueEntry);
//! }
//! ```
//!
//! and `GetItemFromString` takes the first quoted string after the first `GainItem` in the
//! script. Replayed as written, quirks included: a link is matched and followed by its entry id
//! alone, in this conversation, whatever conversation it names; and once a skipped entry takes
//! `num4` below zero the depth stops counting.
//!
//! Every `GainItem` argument in the corpus names an item, so a named item always ends the walk
//! as `GetByName` would.

use std::collections::{HashMap, HashSet};

use crate::core::price::PriceScale;
use crate::index::EntryRecord;

/// `num2`: how many times the depth may count before the walk gives up.
const DEPTH_LIMIT: usize = 5;

/// The call `GetItemFromString` looks for.
const GAIN_ITEM: &str = "GainItem";

/// The scale a priced entry is bought at, or None where it has none or where a guard on the walk
/// could change which item is found - see [`crate::core::price`] for why that errs the right
/// way.
pub fn scale_of(entries: &[EntryRecord], entry: &EntryRecord) -> Option<PriceScale> {
    let by_id: HashMap<i32, &EntryRecord> = entries.iter().map(|e| (e.id, e)).collect();
    let mut done: HashSet<i32> = entries
        .iter()
        .filter(|e| e.to.contains(&entry.id))
        .map(|e| e.id)
        .collect();

    let mut stack = vec![entry];
    let mut depth = 0;
    let mut remaining: i64 = 1;
    while depth <= DEPTH_LIMIT {
        let Some(current) = stack.pop() else {
            return None;
        };
        remaining -= 1;
        if done.contains(&current.id) {
            continue;
        }
        if remaining == 0 {
            depth += 1;
            remaining = (stack.len() + current.to.len()) as i64;
        }
        if !current.guard.is_empty() {
            return None;
        }
        stack.extend(current.to.iter().filter_map(|id| by_id.get(id).copied()));
        if let Some(item) = item_in(&current.script) {
            return PriceScale::of_item(item);
        }
        done.insert(current.id);
    }
    None
}

/// `GetItemFromString`'s item name, where the script has one.
fn item_in(script: &str) -> Option<&str> {
    let after = &script[script.find(GAIN_ITEM)?..];
    let start = after.find('"')? + 1;
    let length = after[start..].find('"')?;
    Some(&after[start..start + length])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i32, guard: &str, script: &str, to: &[i32]) -> EntryRecord {
        EntryRecord {
            id,
            group: false,
            guard: guard.to_string(),
            script: script.to_string(),
            to: to.to_vec(),
            to_conversation: Vec::new(),
            fields: HashMap::new(),
        }
    }

    #[test]
    fn a_purchase_is_scaled_by_the_item_its_walk_reaches() {
        let entries = vec![
            entry(0, "", "", &[1]),
            entry(1, "", "", &[2]),
            entry(2, "", r#"GainItem("nosaphed")"#, &[]),
        ];
        assert_eq!(scale_of(&entries, &entries[1]), Some(PriceScale::Healing));
    }

    #[test]
    fn the_first_item_ends_the_walk() {
        let entries = vec![
            entry(0, "", "", &[1]),
            entry(1, "", r#"GainItem("boombox")"#, &[2]),
            entry(2, "", r#"GainItem("drug_smokes_astra")"#, &[]),
        ];
        assert_eq!(scale_of(&entries, &entries[0]), None);
    }

    #[test]
    fn a_guard_on_the_walk_leaves_the_price_unscaled() {
        let entries = vec![
            entry(0, "", "", &[1]),
            entry(1, r#"Variable["x"]"#, "", &[2]),
            entry(2, "", r#"GainItem("drug_smokes_astra")"#, &[]),
        ];
        assert_eq!(scale_of(&entries, &entries[0]), None);
    }

    #[test]
    fn the_walk_does_not_go_back_through_an_entry_leading_to_the_priced_one() {
        let entries = vec![
            entry(0, "", r#"GainItem("hypnogamma")"#, &[1]),
            entry(1, "", "", &[0]),
        ];
        assert_eq!(scale_of(&entries, &entries[1]), None);
    }
}
