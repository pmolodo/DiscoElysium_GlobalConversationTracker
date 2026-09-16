// SPDX-License-Identifier: MIT
use crate::core::guard_value::GuardValue;
use crate::core::state::VariableRef;
use crate::core::types::{DialogueNodeId, Ternary};
use crate::world::ILookAheadWorld;
use std::collections::HashMap;

/// Test implementation of ILookAheadWorld for unit tests.
#[derive(Debug, Clone, Default)]
pub struct TestWorld {
    pub money: i32,
    pub day_minutes: i32,
    pub day_counter: i32,
    pub clock_locked: bool,
    /// Whether a thought forces every red check to fail.
    pub red_checks_fail: bool,
    pub variables: HashMap<String, GuardValue>,
    pub items: HashMap<String, bool>,
    /// Thoughts already in the cabinet, which is what `IsTHCPresent` asks about.
    pub thoughts: HashMap<String, bool>,
    /// Each skill's damage value, by `SkillType` name - negative where damaged.
    pub damage: HashMap<String, f64>,
    /// What each equipment slot holds, by `EquipmentSlotType` name.
    pub equipment: HashMap<String, String>,
    pub check_results: HashMap<DialogueNodeId, Ternary>,
    pub seen: HashMap<DialogueNodeId, bool>,
    /// Answers to named world queries, keyed by function name.
    ///
    /// The counterpart of the C# `OfflineWorld`'s `queries` block, which this had no
    /// equivalent of: every query answered unknown, so any guard asking one was
    /// undecidable. That is not a corner - `IsKimHere` alone is 691 of the roughly 1,200
    /// world queries the five biggest conversations' guards make, more than half.
    ///
    /// Only the name is keyed, not the arguments. Every query worth answering this way is
    /// one the search cannot change and that takes no argument - IsKimHere, IsCunoInParty
    /// and the rest of the party and character facts. The two that DO take an argument
    /// and that a search's own actions move - CheckItem and IsTHCPresent - are answered
    /// from `items` and `thoughts` instead.
    pub queries: HashMap<String, GuardValue>,
}

impl TestWorld {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_money(mut self, money: i32) -> Self {
        self.money = money;
        self
    }

    pub fn with_day_minutes(mut self, minutes: i32) -> Self {
        self.day_minutes = minutes;
        self
    }

    pub fn with_day_counter(mut self, day: i32) -> Self {
        self.day_counter = day;
        self
    }

    pub fn with_clock_locked(mut self, locked: bool) -> Self {
        self.clock_locked = locked;
        self
    }

    pub fn with_red_checks_failing(mut self, failing: bool) -> Self {
        self.red_checks_fail = failing;
        self
    }

    pub fn set_variable(mut self, name: &str, value: GuardValue) -> Self {
        self.variables.insert(name.to_string(), value);
        self
    }

    /// A variable's value by name, for a caller with no symbol table to ask through.
    pub fn variable(&self, name: &str) -> GuardValue {
        self.variables
            .get(name)
            .cloned()
            .unwrap_or(GuardValue::unknown())
    }

    pub fn set_item(mut self, name: &str, has: bool) -> Self {
        self.items.insert(name.to_string(), has);
        self
    }

    pub fn set_thought(mut self, name: &str, gained: bool) -> Self {
        self.thoughts.insert(name.to_string(), gained);
        self
    }

    pub fn set_damage(mut self, skill: &str, value: f64) -> Self {
        self.damage.insert(skill.to_string(), value);
        self
    }

    pub fn set_check_result(mut self, node: DialogueNodeId, result: Ternary) -> Self {
        self.check_results.insert(node, result);
        self
    }

    pub fn set_seen(mut self, node: DialogueNodeId, seen: bool) -> Self {
        self.seen.insert(node, seen);
        self
    }

    /// Puts `item` in an equipment slot.
    pub fn set_equipped(mut self, slot: &str, item: &str) -> Self {
        self.equipment.insert(slot.to_string(), item.to_string());
        self
    }

    /// Answers a named world query, such as `IsKimHere`.
    pub fn set_query(mut self, name: &str, value: GuardValue) -> Self {
        self.queries.insert(name.to_string(), value);
        self
    }

    /// Answers a named world query with a boolean.
    pub fn set_query_bool(self, name: &str, value: bool) -> Self {
        self.set_query(name, GuardValue::from_boolean(value))
    }
}

impl ILookAheadWorld for TestWorld {
    fn money(&self) -> i32 {
        self.money
    }
    fn day_minutes(&self) -> i32 {
        self.day_minutes
    }
    fn day_counter(&self) -> i32 {
        self.day_counter
    }
    fn is_clock_locked(&self) -> bool {
        self.clock_locked
    }
    fn get_variable(&self, variable: VariableRef<'_>) -> GuardValue {
        self.variable(variable.name())
    }
    fn initially_has_item(&self, name: &str) -> bool {
        self.items.get(name).copied().unwrap_or(false)
    }
    fn initially_has_thought(&self, name: &str) -> bool {
        self.thoughts.get(name).copied().unwrap_or(false)
    }
    fn initial_damage(&self, skill: &str) -> Option<f64> {
        self.damage.get(skill).copied()
    }
    fn item_in_slot(&self, slot: &str) -> Option<String> {
        Some(self.equipment.get(slot).cloned().unwrap_or_default())
    }
    fn query(&self, name: &str, _arguments: &[GuardValue]) -> GuardValue {
        self.queries
            .get(name)
            .cloned()
            .unwrap_or(GuardValue::unknown())
    }
    fn check_passes(&self, node: DialogueNodeId) -> Ternary {
        self.check_results
            .get(&node)
            .copied()
            .unwrap_or(Ternary::Unknown)
    }
    fn is_seen(&self, node: DialogueNodeId) -> bool {
        self.seen.get(&node).copied().unwrap_or(false)
    }
    fn red_check_may_pass(&self, _node: DialogueNodeId) -> bool {
        !self.red_checks_fail
    }
}
