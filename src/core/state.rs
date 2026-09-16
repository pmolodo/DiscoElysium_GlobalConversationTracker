// SPDX-License-Identifier: MIT
use ahash::AHasher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::core::types::DialogueNodeId;

/// Prefix constants for different slot types
pub const ITEM_PREFIX: &str = "item:";
/// A thought the player has gained, which is what `IsTHCPresent` asks about.
///
/// GAINED, not internalised. The game keeps three sets - `gainedThoughts`,
/// `cookingEffects` and `fixedEffects` - and `Sunshine.Dialogue.THCLuaFunctions` exposes
/// a reader for each. Only the first is reachable from dialogue: the Lua surface has
/// exactly one thought writer, `GainThought`, and it adds to `gainedThoughts` and nothing
/// else. Internalising is the player spending a cabinet slot and hours of game time, and
/// forgetting costs a skill point, so `IsTHCCooking` and `IsTHCFixed` are constants for
/// the length of any search and get no slot.
pub const THOUGHT_PREFIX: &str = "thought:";
/// How badly a skill is damaged, by `SkillType` name - see `core::damage`.
pub const DAMAGE_PREFIX: &str = "damage:";
/// What dialogue has done to the party during a search - see `core::party`.
pub const PARTY_PREFIX: &str = "party:";
/// A worn or held item dialogue has taken away, by item name - see `core::equipment`.
pub const UNEQUIPPED_PREFIX: &str = "unequipped:";
pub const ONCE_PREFIX: &str = "once:";
pub const SEEN_PREFIX: &str = "seen:";

/// The prefixes a slot's name carries when the slot is not a dialogue variable.
pub const NOT_A_VARIABLE: [&str; 7] = [
    ITEM_PREFIX,
    THOUGHT_PREFIX,
    DAMAGE_PREFIX,
    PARTY_PREFIX,
    UNEQUIPPED_PREFIX,
    ONCE_PREFIX,
    SEEN_PREFIX,
];

/// Whether a slot of this name holds a dialogue variable, rather than an item, a
/// thought or the engine's own bookkeeping.
pub fn names_a_variable(name: &str) -> bool {
    !NOT_A_VARIABLE.iter().any(|prefix| name.starts_with(prefix))
}

/// A dialogue variable the search may read from the world.
///
/// ONLY A SYMBOL TABLE HANDS THESE OUT, through [`StateSymbols::variable_ref`], and a table
/// holds only the variables its group reads - which is the list the plugin is asked to
/// answer. [`crate::world::ILookAheadWorld::get_variable`] takes one of these rather than a
/// name, so reading a variable nobody asked for does not compile: there is no way to name
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VariableRef<'t> {
    id: usize,
    name: &'t str,
}

impl<'t> VariableRef<'t> {
    /// Where the variable sits in its table, which is where its answer sits in the plugin's.
    pub fn id(self) -> usize {
        self.id
    }

    pub fn name(self) -> &'t str {
        self.name
    }
}

/// Maps every named thing the search can read or write onto a slot index, and names the
/// dialogue variables it can read from the world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSymbols {
    indices: HashMap<String, usize, ahash::RandomState>,
    names: Vec<String>,
    /// The dialogue variables the search may read from the world, sorted, which is the order
    /// their ids number them in.
    variables: Vec<String>,
    /// Each of those variables' ids, by name.
    variable_ids: HashMap<String, usize, ahash::RandomState>,
}

impl StateSymbols {
    pub fn new() -> Self {
        Self {
            indices: HashMap::default(),
            names: Vec::new(),
            variables: Vec::new(),
            variable_ids: HashMap::default(),
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

    pub fn thought(&mut self, name: &str) -> usize {
        self.intern(format!("{THOUGHT_PREFIX}{name}"))
    }

    pub fn damage(&mut self, skill: &str) -> usize {
        self.intern(format!("{DAMAGE_PREFIX}{skill}"))
    }

    /// The slot saying dialogue has taken `item` away, and with it out of any equipment slot.
    pub fn unequipped(&mut self, item: &str) -> usize {
        self.intern(format!("{UNEQUIPPED_PREFIX}{item}"))
    }

    /// The slot saying Kim has been taken out of the party during the search.
    pub fn kim_removed(&mut self) -> usize {
        self.intern(crate::core::party::KIM_REMOVED_SLOT.to_string())
    }

    pub fn once(&mut self, node: DialogueNodeId) -> usize {
        self.intern(format!(
            "{ONCE_PREFIX}{}:{}",
            node.conversation_id, node.entry_id
        ))
    }

    pub fn seen(&mut self, node: DialogueNodeId) -> usize {
        self.intern(format!(
            "{SEEN_PREFIX}{}:{}",
            node.conversation_id, node.entry_id
        ))
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.indices.get(name).copied()
    }

    pub fn name_of(&self, index: usize) -> Option<&str> {
        self.names.get(index).map(|s| s.as_str())
    }

    /// The variable of this name, where the group reads one.
    pub fn variable_ref(&self, name: &str) -> Option<VariableRef<'_>> {
        let id = *self.variable_ids.get(name)?;
        Some(VariableRef {
            id,
            name: &self.variables[id],
        })
    }

    /// Every variable the group reads, in id order.
    pub fn variables(&self) -> &[String] {
        &self.variables
    }

    /// Fixes the variables the group reads.
    ///
    /// CRATE-PRIVATE, because this is the one way a name becomes readable from the world: the
    /// graph declares what its slots and guards read when it is built, and nothing outside
    /// the engine may add to that.
    pub(crate) fn declare_variables(&mut self, names: impl IntoIterator<Item = String>) {
        let mut variables: Vec<String> = names.into_iter().collect();
        variables.sort();
        variables.dedup();

        self.variable_ids = variables
            .iter()
            .enumerate()
            .map(|(id, name)| (name.clone(), id))
            .collect();
        self.variables = variables;
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

        // The variables are the group's, not the slots', so they survive a narrower layout.
        kept.variables = self.variables.clone();
        kept.variable_ids = self.variable_ids.clone();

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
        }
        .rehashed()
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
        }
        .rehashed()
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
        }
        .rehashed()
    }

    pub fn with_changes(&self, changes: &[(usize, i32)], money: i32, day_minutes: i32) -> Self {
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
        }
        .rehashed()
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
            if v != 0
                && let Some(name) = symbols.name_of(i)
            {
                parts.push(format!("{name}={v}"));
            }
        }
        parts.join(", ")
    }
}

impl PartialEq for LookAheadState {
    fn eq(&self, other: &Self) -> bool {
        if self.hash != other.hash
            || self.money != other.money
            || self.day_minutes != other.day_minutes
        {
            return false;
        }
        let shared = self.slots.len().min(other.slots.len());
        if self.slots[..shared] != other.slots[..shared] {
            return false;
        }
        // Check tails are zero
        self.slots[shared..].iter().all(|&v| v == 0)
            && other.slots[shared..].iter().all(|&v| v == 0)
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

/// The state a search starts in: what the world says about every slot the graph has.
///
/// HERE RATHER THAN IN A SEARCH, because it is a fact about the world and the graph and
/// not about any way of walking them. Every search that starts anywhere starts here.
///
/// Seeding is not a detail. A symbolic run seeded with every data state explores paths that
/// need an item the player does not have and reports entries no real search can reach; two
/// searches only agree if they start together.
/// What a slot holds for a variable the world answers `value`: 1 for true, the number for a
/// number, and 0 for false, text or a value nobody could read.
pub fn slot_value_of(value: &crate::core::guard_value::GuardValue) -> i32 {
    use crate::core::guard_value::GuardValueKind;
    match value.kind() {
        GuardValueKind::Boolean if value.boolean() => 1,
        GuardValueKind::Number => value.number() as i32,
        _ => 0,
    }
}

pub fn seed_state(
    graph: &crate::graph::LookAheadGraph,
    world: &dyn crate::world::ILookAheadWorld,
) -> LookAheadState {
    let symbols = graph.symbols();
    let mut state = LookAheadState::empty(symbols.count(), world.money(), world.day_minutes());

    for slot in 0..symbols.count() {
        if let Some(name) = symbols.name_of(slot) {
            if let Some(stripped) = name.strip_prefix("item:") {
                if world.initially_has_item(stripped) {
                    state = state.with(slot, 1);
                }
            } else if let Some(stripped) = name.strip_prefix("thought:") {
                if world.initially_has_thought(stripped) {
                    state = state.with(slot, 1);
                }
            } else if let Some(skill) = name.strip_prefix(DAMAGE_PREFIX) {
                // THE AMOUNT, as a positive count, where the game keeps a negative modifier.
                // Unread reads as undamaged - there is no unknown a slot can hold.
                let damage = world
                    .initial_damage(skill)
                    .map_or(0, crate::core::damage::amount_of);
                if damage > 0 {
                    state = state.with(slot, damage);
                }
            } else if names_a_variable(name) {
                let variable = symbols.variable_ref(name).unwrap_or_else(|| {
                    panic!("slot '{name}' is a variable the group does not declare")
                });
                let value = slot_value_of(&world.get_variable(variable));
                if value != 0 {
                    state = state.with(slot, value);
                }
            }
        }
    }

    // WHAT THE SAVE HAS ALREADY SHOWN, which two slots read. A `seen:` slot closes an entry
    // that shuts once seen. A `once:` slot says an entry's one-time effects have fired, and an
    // entry the save has shown has fired them in the game's eyes: `GenericLuaFunctions.Once`
    // is
    //
    //     if (SunshineNode.IsSeen(ConversationLogger.LastDialogueEntry)) return 0.0;
    //
    // with `LastDialogueIsSeen` captured before the entry is marked displayed, and
    // `CostOptionNode.HandleEntry` skips a `CostOnce` charge on the same test. So a once-only
    // increment, reputation change, payment or charge on a shown entry does nothing.
    for node in graph.nodes() {
        if !world.is_seen(node.id) {
            continue;
        }
        for slot in [node.seen_slot, node.once_slot] {
            if slot >= 0 {
                state = state.with(slot as usize, 1);
            }
        }
    }

    state
}
