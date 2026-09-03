// SPDX-License-Identifier: MIT
use std::collections::HashMap;
use crate::core::types::{DialogueNodeId, Ternary};
use crate::core::guard_value::GuardValue;
use crate::core::guard::IGuardContext;
use crate::core::clock::ClockTime;
use crate::core::state::StateSymbols;
use crate::core::state::LookAheadState;

/// Everything outside the dialogue graph that the look-ahead needs to know.
pub trait ILookAheadWorld: Send + Sync {
    fn money(&self) -> i32;
    fn day_minutes(&self) -> i32;
    fn day_counter(&self) -> i32;
    fn is_clock_locked(&self) -> bool;
    fn get_variable(&self, name: &str) -> GuardValue;
    fn has_item(&self, name: &str) -> bool;
    fn is_task_active(&self, name: &str) -> bool;
    fn query(&self, name: &str, arguments: &[GuardValue]) -> GuardValue;
    fn check_passes(&self, node: DialogueNodeId) -> Ternary;
    fn is_seen(&self, node: DialogueNodeId) -> bool;
}

/// Context that answers guards from crawl state where tracked, world otherwise.
pub struct CrawlContext<'a> {
    pub symbols: &'a StateSymbols,
    pub world: &'a dyn ILookAheadWorld,
    state: Option<&'a LookAheadState>,
}

impl<'a> CrawlContext<'a> {
    pub fn new(symbols: &'a StateSymbols, world: &'a dyn ILookAheadWorld) -> Self {
        Self { symbols, world, state: None }
    }

    pub fn bind(&mut self, state: &'a LookAheadState) {
        self.state = Some(state);
    }

    fn get_slot_value(&self, slot: usize) -> i32 {
        self.state.map(|s| s.get(slot)).unwrap_or(0)
    }

    fn get_slot_bool(&self, slot: usize) -> bool {
        self.get_slot_value(slot) != 0
    }
}

impl IGuardContext for CrawlContext<'_> {
    fn get_variable(&self, name: &str) -> GuardValue {
        if let Some(slot) = self.symbols.find(name) {
            if let Some(state) = self.state {
                let value = state.get(slot);
                // Check if the world has this as a number
                let world_val = self.world.get_variable(name);
                if world_val.kind() == GuardValueKind::Number {
                    return GuardValue::from_number(value as f64);
                }
                return GuardValue::from_boolean(value != 0);
            }
        }
        self.world.get_variable(name)
    }

    fn query(&self, name: &str, arguments: &[GuardValue]) -> GuardValue {
        // Clock queries answered from crawl state
        if self.state.is_some() && ClockTime::owns(name) {
            let day_minutes = self.state.unwrap().day_minutes();
            let day_counter = self.world.day_counter();
            return ClockTime::answer(name, arguments, day_minutes, day_counter);
        }

        match name {
            "MoneyAmount" => {
                if let Some(state) = self.state {
                    GuardValue::from_number(state.money() as f64)
                } else {
                    self.world.query(name, arguments)
                }
            }
            "CheckItem" => {
                if let Some(state) = self.state {
                    if let Some(&GuardValue { kind: GuardValueKind::Text, ref text, .. }) = arguments.get(0) {
                        if let Some(slot) = self.symbols.find(&format!("item:{}", text)) {
                            return GuardValue::from_boolean(state.is_set(slot));
                        }
                    }
                }
                self.world.query(name, arguments)
            }
            "IsTaskActive" => {
                if let Some(state) = self.state {
                    if let Some(&GuardValue { kind: GuardValueKind::Text, ref text, .. }) = arguments.get(0) {
                        if let Some(slot) = self.symbols.find(&format!("task:{}", text)) {
                            return GuardValue::from_boolean(state.is_set(slot));
                        }
                    }
                }
                self.world.query(name, arguments)
            }
            _ => self.world.query(name, arguments),
        }
    }
}

use crate::core::guard_value::GuardValueKind;
