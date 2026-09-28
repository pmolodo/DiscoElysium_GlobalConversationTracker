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
                let names: Vec<&str> = self
                    .suites
                    .iter()
                    .map(|suite| suite.suite.as_str())
                    .collect();
                panic!(
                    "no suite '{name}' in {TABLE}; there are: {}",
                    names.join(", ")
                )
            })
    }
}

/// One suite: what to stage once, and the scenarios that share the staging.
#[derive(Debug, Deserialize)]
pub struct Suite {
    pub suite: String,
    /// Why this suite is not being run, where it is not.
    ///
    /// A SENTENCE RATHER THAN A FLAG, and it is not optional for a disabled suite: a
    /// definition that is switched off without saying why is one nobody can decide to
    /// switch back on. Both executors skip it and both say the sentence, so a run that is
    /// missing a claim reports which claim and on whose authority.
    ///
    /// Meant to be temporary, and paired with a task. A suite that stays off is a claim
    /// nobody is making any more, which wants deleting rather than disabling.
    #[serde(default)]
    pub disabled: Option<String>,
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
/// searched" - asking it of the whole group is the stronger form AND the honest one, since
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
    #[serde(default)]
    pub money: Option<i32>,
    #[serde(default, rename = "dayMinutes")]
    pub day_minutes: Option<i32>,
    /// Whether the profile has finished a hardcore game, which no save records. Unnamed, an
    /// offline run answers `fixtures::HARDCORE_PLAYTHROUGH_COMPLETED`.
    #[serde(default, rename = "hardcorePlaythroughCompleted")]
    pub hardcore_playthrough_completed: Option<bool>,
    /// How much the scenario claims about the markers: named, noneAnywhere or ignored.
    #[serde(default = "named")]
    pub markers: String,
    /// How much it claims about its rolled checks' Pass / Fail lines: everyCheck, noneAnywhere
    /// or ignored. Under everyCheck, every rolled check on every stop is held to `pass` and
    /// `fail`.
    #[serde(default = "ignored")]
    pub branches: String,
    /// What the word "Pass" must be drawn as, under everyCheck.
    #[serde(default)]
    pub pass: Option<BranchHalfRow>,
    /// What the word "Fail" must be drawn as, under everyCheck.
    #[serde(default)]
    pub fail: Option<BranchHalfRow>,
    /// The menus this scenario is held to, in the order its walk reaches them.
    ///
    /// ONE SHAPE RATHER THAN TWO. A scenario of one menu writes one stop; a scenario that
    /// passes the same ground twice writes two, the second pressing on from where the first
    /// was checked. Nothing about a menu is said outside this list.
    pub stops: Vec<StopRow>,
}

/// One menu a scenario is held to: what to press to reach it, and what it must look like.
///
/// A scenario's own inputs and options are its FIRST stop, and `stops` carries the rest, so
/// both executors work from a list rather than from two shapes.
pub struct Stop<'a> {
    /// What this stop is for, for a failure to name.
    pub what: &'a str,
    /// What to press from the conversation's START to this menu, earlier stops included.
    pub inputs: Option<Vec<lookahead_engine::walkthrough::Input>>,
    /// What each named option must carry here.
    pub options: &'a [OptionRow],
}

impl Scenario {
    /// Every menu this scenario is held to, in the order it reaches them.
    ///
    /// EACH ONE'S INPUTS ARE WHOLE, counted from the conversation's start: a stop presses on
    /// from where the last check happened, and the offline executor walks from the start every
    /// time, so what it walks is every earlier stop's inputs followed by this one's. In game
    /// the conversation carries on in place instead, which is the same walk.
    ///
    /// # Panics
    ///
    /// If an input is not one. A row that misspells one describes a walk neither executor can
    /// make.
    pub fn stops(&self) -> Vec<Stop<'_>> {
        let parse = |inputs: &Option<Vec<String>>| {
            inputs.as_ref().map(|inputs| {
                inputs
                    .iter()
                    .map(|input| {
                        input
                            .parse()
                            .unwrap_or_else(|error| panic!("{}: {error}", self.save))
                    })
                    .collect::<Vec<_>>()
            })
        };

        let mut pressed: Option<Vec<lookahead_engine::walkthrough::Input>> = None;
        let mut stops = Vec::new();
        for (at, stop) in self.stops.iter().enumerate() {
            // A STOP PRESSES ON, so what reaches it is everything pressed so far. The first
            // names what reaches it from the conversation's start, and a stop after one that
            // named no inputs presses on from the first menu, which is where pressing "enter"
            // to it left the conversation.
            let inputs = if at == 0 {
                parse(&stop.inputs)
            } else {
                let mut so_far = pressed.clone().unwrap_or_default();
                so_far.extend(parse(&stop.inputs).unwrap_or_default());
                Some(so_far)
            };
            pressed = inputs.clone();
            stops.push(Stop {
                what: &stop.what,
                inputs,
                options: &stop.options,
            });
        }
        stops
    }
}

/// One more menu along a scenario's walk, as the definition spells it.
#[derive(Debug, Deserialize)]
pub struct StopRow {
    pub what: String,
    /// What to press on from the last check to this menu.
    #[serde(default)]
    pub inputs: Option<Vec<String>>,
    #[serde(default)]
    pub options: Vec<OptionRow>,
}

fn named() -> String {
    "named".to_string()
}

fn ignored() -> String {
    "ignored".to_string()
}

/// One word of a rolled check's Pass / Fail line: the colour it is drawn in, which is where
/// that outcome lands, and the marker after it, which is what lies beyond.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct BranchHalfRow {
    /// orange, red or darkRed.
    pub colour: String,
    /// orange, red or gaveUp, and absent for no marker.
    #[serde(default)]
    pub marker: Option<String>,
}

impl std::fmt::Display for BranchHalfRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.marker {
            Some(marker) => write!(f, "{} with {marker}", self.colour),
            None => write!(f, "{}", self.colour),
        }
    }
}

/// What one option must carry. `OptionRow` because `Option` is taken and this is a row.
#[derive(Debug, Deserialize)]
pub struct OptionRow {
    pub entry: i32,
    pub marker: String,
    pub why: String,
}

/// The header this reader is written for.
///
/// CHECKED RATHER THAN ASSUMED, and the cheap mistake it catches is real: the two tables
/// are the same shape of document, so one read as the other parses, deserialises into
/// something, and quietly describes nothing the reader wanted.
const FORMAT: lookahead_engine::formats::header::Expected =
    lookahead_engine::formats::header::Expected {
        format: "scenario-suites",
        version: 1,
    };
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

    let document: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not JSON: {error}", path.display()));
    FORMAT
        .check_document(&document)
        .unwrap_or_else(|fault| panic!("{}: {fault}", path.display()));

    serde_json::from_value(document)
        .unwrap_or_else(|error| panic!("{} is not a scenario table: {error}", path.display()))
}
