// SPDX-License-Identifier: MIT
//! The shared scenario definition, as the offline executors read it.
//!
//! `testing/scenarios/suites.json` is the definition `tools/GameHarness` builds its in-game
//! runs from. These are the same rows, typed for the tests that execute them without a
//! game - so a scenario is written once and neither side can come to describe a run the
//! other is not doing.
//!
//! Its own module because more than one test binary reads it: `scenario_suites.rs` runs the
//! markers and the offline claims, and `all_seen.rs` asks the one question about the
//! all-seen rows that the shared executor cannot.

use serde::Deserialize;

use super::repo_root;

/// Where the definition lives, under the repository root.
pub const TABLE: &str = "testing/scenarios/suites.json";

#[derive(Debug, Deserialize)]
pub struct Table {
    pub suites: Vec<Suite>,
}

impl Table {
    /// One suite by name.
    ///
    /// # Panics
    ///
    /// If there is no such suite. A test naming one that has been renamed away would
    /// otherwise pass over its own subject in silence.
    pub fn suite(&self, name: &str) -> &Suite {
        self.suites
            .iter()
            .find(|suite| suite.suite == name)
            .unwrap_or_else(|| {
                let names: Vec<&str> =
                    self.suites.iter().map(|suite| suite.suite.as_str()).collect();
                panic!("no suite '{name}' in {TABLE}; there are: {}", names.join(", "))
            })
    }
}

/// One suite: what to stage once, and the scenarios that share the staging.
#[derive(Debug, Deserialize)]
pub struct Suite {
    pub suite: String,
    /// The global state to stage, by file name under the scenarios folder.
    pub state: String,
    /// The state budget to run at, or 0 for no such limit. Suite-wide, as it is in game.
    #[serde(default, rename = "stateBudget")]
    pub state_budget: usize,
    /// The claim only a run without a game can make, where the suite has one.
    #[serde(default)]
    pub offline: Option<OfflineClaim>,
    pub scenarios: Vec<Scenario>,
}

/// A claim about EVERY entry in a group, which only an executor with no menu can make.
///
/// The mirror of the `inGame` key, and there for the same reason. An in-game run sees the
/// handful of options a menu composed; this sees the conversation. Where a suite's real
/// subject is a rule rather than a menu - "no option that is itself unread anywhere is ever
/// crawled" - asking it of the whole group is the stronger form AND the honest one, since
/// the menu-wide marker policies say nothing that can be checked without a menu.
#[derive(Debug, Deserialize)]
pub struct OfflineClaim {
    /// Which claim, by a name the offline executor knows.
    pub claim: String,
    /// Why it holds, and why it is worth asking that way. For a reader of the definition.
    #[serde(default)]
    #[allow(dead_code)]
    pub why: String,
}

/// One save, and what the menu it opens must look like.
#[derive(Debug, Deserialize)]
pub struct Scenario {
    pub save: String,
    pub conversation: i32,
    pub what: String,
    #[serde(default)]
    pub money: Option<i32>,
    #[serde(default, rename = "dayMinutes")]
    pub day_minutes: Option<i32>,
    /// How much the scenario claims about the markers: named, noneAnywhere or ignored.
    #[serde(default = "named")]
    pub markers: String,
    #[serde(default)]
    pub options: Vec<OptionRow>,
}

fn named() -> String {
    "named".to_string()
}

/// What one option must carry. `OptionRow` because `Option` is taken and this is a row.
#[derive(Debug, Deserialize)]
pub struct OptionRow {
    pub entry: i32,
    pub marker: String,
    pub why: String,
}

/// The definition, read.
///
/// # Panics
///
/// If it is missing or will not parse. Both mean the definition the in-game harness builds
/// its runs from is not there, which is not something a test should pass over.
pub fn table() -> Table {
    let path = repo_root().join(TABLE);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} does not read: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not a scenario table: {error}", path.display()))
}
