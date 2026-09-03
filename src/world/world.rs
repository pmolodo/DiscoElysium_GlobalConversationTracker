// SPDX-License-Identifier: MIT
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

/// What a crawl consults that outlives any one state: the symbol table and the world.
///
/// Deliberately holds NO state. An earlier version stored the state being evaluated and
/// had a `bind` method, which cannot be made to typecheck: the stored reference took the
/// same lifetime as the symbols and the world, so binding one of the crawl's own
/// short-lived states required it to outlive the whole search. Handing out a short-lived
/// [`BoundContext`] instead lets each state be borrowed for exactly the guard evaluation
/// that reads it, and leaves this shareable as `&CrawlContext`.
pub struct CrawlContext<'w> {
    pub symbols: &'w StateSymbols,
    pub world: &'w dyn ILookAheadWorld,
}

impl<'w> CrawlContext<'w> {
    pub fn new(symbols: &'w StateSymbols, world: &'w dyn ILookAheadWorld) -> Self {
        Self { symbols, world }
    }

    /// A view that answers guards from `state` where the crawl tracks a slot, and from
    /// the world otherwise.
    pub fn bound<'s>(&self, state: &'s LookAheadState) -> BoundContext<'s>
    where
        'w: 's,
    {
        BoundContext { symbols: self.symbols, world: self.world, state: Some(state) }
    }

    /// A view with no state behind it, for the seeding pass that runs before the first
    /// state exists.
    pub fn unbound(&self) -> BoundContext<'_> {
        BoundContext { symbols: self.symbols, world: self.world, state: None }
    }
}

/// A [`CrawlContext`] looking at one particular state, for the length of one evaluation.
pub struct BoundContext<'s> {
    pub symbols: &'s StateSymbols,
    pub world: &'s dyn ILookAheadWorld,
    state: Option<&'s LookAheadState>,
}

impl IGuardContext for BoundContext<'_> {
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
                    if let Some(text) = arguments.get(0)
                        .filter(|v| v.kind() == GuardValueKind::Text)
                        .map(|v| v.text())
                    {
                        if let Some(slot) = self.symbols.find(&format!("item:{}", text)) {
                            return GuardValue::from_boolean(state.is_set(slot));
                        }
                    }
                }
                self.world.query(name, arguments)
            }
            "IsTaskActive" => {
                if let Some(state) = self.state {
                    if let Some(text) = arguments.get(0)
                        .filter(|v| v.kind() == GuardValueKind::Text)
                        .map(|v| v.text())
                    {
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
