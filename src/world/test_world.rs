// SPDX-License-Identifier: MIT
use std::collections::HashMap;
use crate::core::types::{DialogueNodeId, Ternary};
use crate::core::guard_value::GuardValue;
use crate::world::world::ILookAheadWorld;

/// Test implementation of ILookAheadWorld for unit tests.
#[derive(Debug, Clone, Default)]
pub struct TestWorld {
    pub money: i32,
    pub day_minutes: i32,
    pub day_counter: i32,
    pub clock_locked: bool,
    pub variables: HashMap<String, GuardValue>,
    pub items: HashMap<String, bool>,
    pub tasks: HashMap<String, bool>,
    pub check_results: HashMap<DialogueNodeId, Ternary>,
    pub seen: HashMap<DialogueNodeId, bool>,
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

    pub fn set_variable(mut self, name: &str, value: GuardValue) -> Self {
        self.variables.insert(name.to_string(), value);
        self
    }

    pub fn set_item(mut self, name: &str, has: bool) -> Self {
        self.items.insert(name.to_string(), has);
        self
    }

    pub fn set_task(mut self, name: &str, active: bool) -> Self {
        self.tasks.insert(name.to_string(), active);
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
}

impl ILookAheadWorld for TestWorld {
    fn money(&self) -> i32 { self.money }
    fn day_minutes(&self) -> i32 { self.day_minutes }
    fn day_counter(&self) -> i32 { self.day_counter }
    fn is_clock_locked(&self) -> bool { self.clock_locked }
    fn get_variable(&self, name: &str) -> GuardValue {
        self.variables.get(name).cloned().unwrap_or(GuardValue::unknown())
    }
    fn has_item(&self, name: &str) -> bool { self.items.get(name).copied().unwrap_or(false) }
    fn is_task_active(&self, name: &str) -> bool { self.tasks.get(name).copied().unwrap_or(false) }
    fn query(&self, name: &str, _arguments: &[GuardValue]) -> GuardValue {
        GuardValue::unknown()
    }
    fn check_passes(&self, node: DialogueNodeId) -> Ternary {
        self.check_results.get(&node).copied().unwrap_or(Ternary::Unknown)
    }
    fn is_seen(&self, node: DialogueNodeId) -> bool {
        self.seen.get(&node).copied().unwrap_or(false)
    }
}
