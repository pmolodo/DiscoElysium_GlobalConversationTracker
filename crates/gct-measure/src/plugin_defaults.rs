// SPDX-License-Identifier: MIT
//! The look-ahead budgets the plugin ships, for the offline runs that stand in for it.
//!
//! The numbers are the plugin's: `Config.Bind` in `src/GlobalConversationTracker.Plugin/Plugin.cs`
//! declares each setting with its default, and that is what a player who never opened the config
//! file gets. A run meant to say what a player waits for has to ask under the same numbers - see
//! the "one algorithm by default" rule in `CLAUDE.md` - so they are named once here rather than
//! typed into each driver.
//!
//! A COPY, HELD TO THE ORIGINAL. The plugin is C# and nothing here can read its constants, so
//! the test below reads `Plugin.cs` itself and fails the moment a default there moves without
//! this file moving with it.

/// `LookAheadTimeBudgetMs`: the longest one option's look-ahead may run for.
pub const TIME_BUDGET_MS: u64 = 1000;

/// `LookAheadMenuTimeBudgetMs`: the longest a whole response menu's look-ahead may run for.
pub const MENU_TIME_BUDGET_MS: u64 = 3000;

/// `LookAheadMemoryBudgetMb`: the most memory one option's look-ahead may hold.
pub const MEMORY_BUDGET_MB: usize = 300;

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

#[cfg(test)]
mod tests {
    use super::*;

    /// The plugin's source, from this crate's folder.
    const PLUGIN: &str = "../../src/GlobalConversationTracker.Plugin/Plugin.cs";

    /// The default `Config.Bind` gives `setting`: the line after its quoted name.
    fn bound_default(source: &str, setting: &str) -> u64 {
        let quoted = format!("\"{setting}\",");
        let mut lines = source.lines().map(str::trim);
        lines
            .by_ref()
            .find(|line| *line == quoted)
            .unwrap_or_else(|| panic!("{PLUGIN} binds no setting named {setting}"));
        let default = lines
            .next()
            .unwrap_or_else(|| panic!("{PLUGIN} ends after binding {setting}"));
        default.trim_end_matches(',').parse().unwrap_or_else(|_| {
            panic!("{setting}'s default in {PLUGIN} is not a number: {default}")
        })
    }

    #[test]
    fn the_defaults_are_the_ones_the_plugin_binds() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(PLUGIN);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|fault| panic!("{}: {fault}", path.display()));

        assert_eq!(
            bound_default(&source, "LookAheadTimeBudgetMs"),
            TIME_BUDGET_MS
        );
        assert_eq!(
            bound_default(&source, "LookAheadMenuTimeBudgetMs"),
            MENU_TIME_BUDGET_MS
        );
        assert_eq!(
            bound_default(&source, "LookAheadMemoryBudgetMb"),
            MEMORY_BUDGET_MB as u64,
        );
    }
}
