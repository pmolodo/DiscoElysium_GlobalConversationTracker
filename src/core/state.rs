// SPDX-License-Identifier: MIT
use std::collections::HashMap;
use ahash::AHasher;
use std::hash::{Hash, Hasher};
use serde::{Deserialize, Serialize};

use crate::core::types::DialogueNodeId;

/// Prefix constants for different slot types
pub const ITEM_PREFIX: &str = "item:";
pub const TASK_PREFIX: &str = "task:";
/// A thought the player has gained, which is what `IsTHCPresent` asks about.
///
/// GAINED, not internalised. The game keeps three sets - `gainedThoughts`,
/// `cookingEffects` and `fixedEffects` - and `Sunshine.Dialogue.THCLuaFunctions` exposes
/// a reader for each. Only the first is reachable from dialogue: the Lua surface has
/// exactly one thought writer, `GainThought`, and it adds to `gainedThoughts` and nothing
/// else. Internalising is the player spending a cabinet slot and hours of game time, and
/// forgetting costs a skill point, so `IsTHCCooking` and `IsTHCFixed` are constants for
/// the length of any crawl and get no slot.
pub const THOUGHT_PREFIX: &str = "thought:";
pub const ONCE_PREFIX: &str = "once:";
pub const SEEN_PREFIX: &str = "seen:";

/// Maps every named thing the crawl can read or write onto a slot index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSymbols {
    indices: HashMap<String, usize, ahash::RandomState>,
    names: Vec<String>,
}

impl StateSymbols {
    pub fn new() -> Self {
        Self {
            indices: HashMap::default(),
            names: Vec::new(),
        }
    }

    pub fn count(&self) -> usize {
        self.names.len()
    }

    fn intern(&mut self, name: String) -> usize {
        if let Some(&idx) = self.indices.get(&name) {
            return idx;
        }
        let idx = self.names.len();
        self.names.push(name.clone());
        self.indices.insert(name, idx);
        idx
    }

    pub fn variable(&mut self, name: &str) -> usize {
        self.intern(name.to_string())
    }

    pub fn item(&mut self, name: &str) -> usize {
        self.intern(format!("{ITEM_PREFIX}{name}"))
    }

    pub fn task(&mut self, name: &str) -> usize {
        self.intern(format!("{TASK_PREFIX}{name}"))
    }

    pub fn thought(&mut self, name: &str) -> usize {
        self.intern(format!("{THOUGHT_PREFIX}{name}"))
    }

    pub fn once(&mut self, node: DialogueNodeId) -> usize {
        self.intern(format!("{ONCE_PREFIX}{}:{}", node.conversation_id, node.entry_id))
    }

    pub fn seen(&mut self, node: DialogueNodeId) -> usize {
        self.intern(format!("{SEEN_PREFIX}{}:{}", node.conversation_id, node.entry_id))
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.indices.get(name).copied()
    }

    pub fn name_of(&self, index: usize) -> Option<&str> {
        self.names.get(index).map(|s| s.as_str())
    }

    /// The table with only the slots `keep` marks, plus the old-to-new index map.
    ///
    /// The map has one entry per slot of THIS table and holds `-1` where the slot has
    /// gone, so a caller renumbering slot indices baked into something else - a node's
    /// flag, seen and once slots, an action's target - can tell "moved to 0" from
    /// "dropped" without a second lookup. Kept slots keep their relative order, which
    /// keeps the numbering a deterministic function of the untrimmed one, and therefore
    /// keeps a symbolic variable order repeatable across runs.
    ///
    /// A slot shorter than `keep` covers is dropped: saying nothing about a slot is not
    /// the same as asking for it.
    pub fn retaining(&self, keep: &[bool]) -> (Self, Vec<i32>) {
        let mut kept = Self::new();
        let mut map = vec![-1i32; self.names.len()];

        for (index, name) in self.names.iter().enumerate() {
            if keep.get(index).copied().unwrap_or(false) {
                map[index] = kept.intern(name.clone()) as i32;
            }
        }

        (kept, map)
    }
}

impl Default for StateSymbols {
    fn default() -> Self {
        Self::new()
    }
}

/// Immutable state carried along a path: one int per slot, plus money and clock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookAheadState {
    slots: Vec<i32>,
    money: i32,
    day_minutes: i32,
    hash: u64,
}

impl LookAheadState {
    pub const MINUTES_IN_DAY: i32 = 1440;
    pub const PASS_TIME_MINUTES: i32 = 15;

    pub fn empty(slot_count: usize, money: i32, day_minutes: i32) -> Self {
        assert!(money >= 0, "money cannot be negative");
        let slots = vec![0; slot_count];
        let mut state = Self {
            slots,
            money,
            day_minutes: Self::wrap_minutes(day_minutes),
            hash: 0,
        };
        state.hash = state.compute_hash();
        state
    }

    fn compute_hash(&self) -> u64 {
        let mut hasher = AHasher::default();
        self.money.hash(&mut hasher);
        self.day_minutes.hash(&mut hasher);
        for (i, &v) in self.slots.iter().enumerate() {
            if v != 0 {
                i.hash(&mut hasher);
                v.hash(&mut hasher);
            }
        }
        hasher.finish()
    }

    pub fn wrap_minutes(m: i32) -> i32 {
        let m = m % Self::MINUTES_IN_DAY;
        if m < 0 { m + Self::MINUTES_IN_DAY } else { m }
    }

    pub fn money(&self) -> i32 {
        self.money
    }

    pub fn day_minutes(&self) -> i32 {
        self.day_minutes
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    pub fn get(&self, index: usize) -> i32 {
        self.slots.get(index).copied().unwrap_or(0)
    }

    pub fn is_set(&self, index: usize) -> bool {
        self.get(index) != 0
    }

    pub fn with(&self, index: usize, value: i32) -> Self {
        if self.get(index) == value {
            return self.clone();
        }
        let mut slots = self.slots.clone();
        if index >= slots.len() {
            slots.resize(index + 1, 0);
        }
        slots[index] = value;
        Self {
            slots,
            money: self.money,
            day_minutes: self.day_minutes,
            hash: 0, // will be computed
        }.rehashed()
    }

    pub fn with_money(&self, money: i32) -> Self {
        let money = money.max(0);
        if money == self.money {
            return self.clone();
        }
        Self {
            slots: self.slots.clone(),
            money,
            day_minutes: self.day_minutes,
            hash: 0,
        }.rehashed()
    }

    pub fn with_day_minutes(&self, day_minutes: i32) -> Self {
        let wrapped = Self::wrap_minutes(day_minutes);
        if wrapped == self.day_minutes {
            return self.clone();
        }
        Self {
            slots: self.slots.clone(),
            money: self.money,
            day_minutes: wrapped,
            hash: 0,
        }.rehashed()
    }

    pub fn with_changes(
        &self,
        changes: &[(usize, i32)],
        money: i32,
        day_minutes: i32,
    ) -> Self {
        let mut slots = self.slots.clone();
        let mut max_idx = slots.len();
        for &(idx, _) in changes {
            if idx >= max_idx {
                max_idx = idx + 1;
            }
        }
        if max_idx > slots.len() {
            slots.resize(max_idx, 0);
        }
        for &(idx, val) in changes {
            slots[idx] = val;
        }
        Self {
            slots,
            money: money.max(0),
            day_minutes: Self::wrap_minutes(day_minutes),
            hash: 0,
        }.rehashed()
    }

    fn rehashed(mut self) -> Self {
        self.hash = self.compute_hash();
        self
    }

    pub fn describe(&self, symbols: &StateSymbols) -> String {
        let mut parts = Vec::new();
        parts.push(format!("money={}", self.money));
        let hours = self.day_minutes / 60;
        let mins = self.day_minutes % 60;
        parts.push(format!("clock={hours:02}:{mins:02}"));
        for (i, &v) in self.slots.iter().enumerate() {
            if v != 0 {
                if let Some(name) = symbols.name_of(i) {
                    parts.push(format!("{name}={v}"));
                }
            }
        }
        parts.join(", ")
    }
}

impl PartialEq for LookAheadState {
    fn eq(&self, other: &Self) -> bool {
        if self.hash != other.hash || self.money != other.money || self.day_minutes != other.day_minutes {
            return false;
        }
        let shared = self.slots.len().min(other.slots.len());
        if self.slots[..shared] != other.slots[..shared] {
            return false;
        }
        // Check tails are zero
        self.slots[shared..].iter().all(|&v| v == 0) && other.slots[shared..].iter().all(|&v| v == 0)
    }
}

impl Eq for LookAheadState {}

impl Hash for LookAheadState {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl Default for LookAheadState {
    fn default() -> Self {
        Self::empty(0, 0, 0)
    }
}
