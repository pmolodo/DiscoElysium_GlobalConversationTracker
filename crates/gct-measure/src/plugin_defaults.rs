// SPDX-License-Identifier: MIT
//! The look-ahead budgets the plugin ships, for the offline runs that stand in for it.
//!
//! The numbers are the engine's - `lookahead_engine::shipped_budgets`, which the plugin's
//! `Config.Bind` defaults are generated from too - so a run meant to say what a player waits for
//! asks under exactly what a player gets. See the "one algorithm by default" rule in `CLAUDE.md`.

pub use lookahead_engine::shipped_budgets::{
    MEMORY_BUDGET_MB, MENU_TIME_BUDGET_MS, TIME_BUDGET_MS,
};

/// The three budgets a request carries, as one value to put on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budgets {
    /// What one option is allowed, in milliseconds; 0 is no limit.
    pub time_budget_ms: u64,
    /// What the whole menu is allowed, in milliseconds; 0 is no limit.
    pub menu_time_budget_ms: u64,
    /// What one option's search may hold, in megabytes.
    pub memory_budget_mb: usize,
}

impl Budgets {
    /// What a player who never opened the config file gets.
    pub const SHIPPED: Self = Self {
        time_budget_ms: TIME_BUDGET_MS,
        menu_time_budget_ms: MENU_TIME_BUDGET_MS,
        memory_budget_mb: MEMORY_BUDGET_MB,
    };

    /// `request`, asked under these budgets.
    pub fn apply(
        &self,
        request: lookahead_engine::bridge::LookAheadRequest,
    ) -> lookahead_engine::bridge::LookAheadRequest {
        lookahead_engine::bridge::LookAheadRequest {
            time_budget_ms: self.time_budget_ms,
            menu_time_budget_ms: self.menu_time_budget_ms,
            memory_budget_mb: self.memory_budget_mb,
            ..request
        }
    }
}
