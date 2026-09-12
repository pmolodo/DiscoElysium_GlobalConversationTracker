// SPDX-License-Identifier: MIT
//! Is the mod's state file the union of the saves it should cover?
//!
//! ## The rule, and why it is not equality
//!
//! With `Untouched < WasOffered < WasDisplayed`:
//!
//! ```text
//! global[e] >= max(save1[e], save2[e], ... saveN[e])   for every entry e
//! ```
//!
//! NOT SET EQUALITY, deliberately. The status a save records is the last value the game
//! wrote into that playthrough's table; the state file is merged monotonically across
//! playthroughs. So a global status sitting strictly HIGHER than every save is correct and
//! is reported as information. Only an entry the state failed to reach - a downgrade, or a
//! union member it never took in - is a failure.
//!
//! ## Why three saves and not two
//!
//! An entry that exists in only ONE save is the load-bearing evidence. It can be in the
//! state file only because the file carried it across, and when that save belongs to a
//! playthrough the current session never touched, the only path it could have taken is a
//! read off disk. The report counts those per save and says how many survived.
//!
//! ## What this is that the tests are not
//!
//! Every test in this repository runs over saves this repository wrote. This runs over a
//! real playthrough's, which is a question only a person with a played game can ask - and
//! the reason it is worth keeping a hand-run tool for.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gct_formats::expanded_save::OnDisk;
use gct_formats::global_state::{GlobalState, Status};
use gct_formats::lua_blob::{self, LuaTable, LuaValue, TABLE_NAMES};
use gct_formats::lua_sparse::{self, CONVERSATION_TABLE};
use gct_formats::{lua_parts, packed_save};

/// The per-conversation field holding the dialogue entry map.
const DIALOG_FIELD: &str = "Dialog";

/// The per-entry field holding the status name.
const SIM_STATUS_FIELD: &str = "SimStatus";

/// What a packed save's file is called.
const PACKED_SUFFIX: &str = ".ntwtf.zip";

/// And an expanded one's directory.
const EXPANDED_SUFFIX: &str = ".ntwtf";

/// How many differing entries a report names per category by default.
pub const DEFAULT_MAX_EXAMPLES: usize = 10;

/// Why a save or a state could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckFault {
    /// Nothing matches the name given.
    #[error(
        "no save matching '{0}' as a path, or as '{0}{PACKED_SUFFIX}' or '{0}{EXPANDED_SUFFIX}' \
         under {1}"
    )]
    NoSuchSave(String, String),
    /// A save is there and will not read.
    #[error("{0}: {1}")]
    Unreadable(String, String),
    /// A save holds no conversations, so it says nothing this can check.
    #[error("{0} has no {CONVERSATION_TABLE} table")]
    NoConversations(String),
    /// An entry names a status this build does not know.
    ///
    /// REFUSED RATHER THAN SKIPPED: an unknown status means an assumption about the save
    /// format is wrong, and dropping it would quietly weaken the check.
    #[error("{0}: conversation {1}, entry {2} has status '{3}', which is not one")]
    UnknownStatus(String, i32, i32, String),
    /// Fewer than two saves were named.
    #[error("at least two saves are needed; {0} were named")]
    TooFewSaves(usize),
}

/// One save being compared: how the report names it, and what it recorded.
#[derive(Debug, Clone)]
pub struct NamedSave {
    /// How the save is named in the printed report.
    pub label: String,
    /// The dialogue statuses that save records.
    pub state: GlobalState,
}

/// One entry whose global status differs from the saves' maximum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Difference {
    pub conversation: i32,
    pub entry: i32,
    /// The highest status any save records for it.
    pub expected_at_least: Status,
    /// What the state file records.
    pub actual: Status,
}

impl std::fmt::Display for Difference {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            out,
            "conversation {}, entry {}: saves say {}, global says {}",
            self.conversation,
            self.entry,
            self.expected_at_least.as_str(),
            self.actual.as_str(),
        )
    }
}

/// How many entries are unique to one save, and how many of those survived.
#[derive(Debug, Clone)]
pub struct Exclusive {
    /// The save these counts belong to.
    pub label: String,
    /// Entries above `Untouched` in this save and in no other.
    pub total: usize,
    /// Of `total`, how many the state file preserved.
    pub preserved: usize,
}

/// The comparison of a state file against the saves it should cover.
#[derive(Debug, Clone)]
pub struct Report {
    /// Entries where the state is BELOW the highest save. Non-empty means fail.
    pub downgrades: Vec<Difference>,
    /// Entries where it is above every save. Informational.
    pub above_all_saves: Vec<Difference>,
    /// Entries the state records that no save does. A large count means a stale file.
    pub in_global_only: Vec<Difference>,
    /// Per-save counts of entries unique to that save, in the order given.
    pub exclusives: Vec<Exclusive>,
    /// The union of every save's entries, which is what the rule is checked over.
    pub union_entries: usize,
}

impl Report {
    /// The whole point: no entry fell below the union.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.downgrades.is_empty()
    }
}

/// Compares a state file against two or more saves.
///
/// # Errors
///
/// Where fewer than two saves were named.
pub fn compare(global: &GlobalState, saves: &[NamedSave]) -> Result<Report, CheckFault> {
    if saves.len() < 2 {
        return Err(CheckFault::TooFewSaves(saves.len()));
    }

    // NO STATE STORES `Untouched`, so every key here is an entry that actually has to be
    // accounted for in the state file.
    let mut union: BTreeSet<(i32, i32)> = BTreeSet::new();
    for save in saves {
        for (conversation, entry) in entries_of(&save.state) {
            union.insert((conversation, entry));
        }
    }

    let mut report = Report {
        downgrades: Vec::new(),
        above_all_saves: Vec::new(),
        in_global_only: Vec::new(),
        exclusives: saves
            .iter()
            .map(|save| Exclusive {
                label: save.label.clone(),
                total: 0,
                preserved: 0,
            })
            .collect(),
        union_entries: union.len(),
    };

    for &(conversation, entry) in &union {
        let mut expected = Status::Untouched;
        let mut holders = 0;
        let mut last_holder = 0;

        for (at, save) in saves.iter().enumerate() {
            let status = save.state.status_of(conversation, entry);
            expected = expected.max(status);
            if status > Status::Untouched {
                holders += 1;
                last_holder = at;
            }
        }

        let actual = global.status_of(conversation, entry);
        let difference = Difference {
            conversation,
            entry,
            expected_at_least: expected,
            actual,
        };

        if actual < expected {
            report.downgrades.push(difference);
        } else if actual > expected {
            report.above_all_saves.push(difference);
        }

        if holders == 1 {
            report.exclusives[last_holder].total += 1;
            if actual >= expected {
                report.exclusives[last_holder].preserved += 1;
            }
        }
    }

    for (conversation, entry) in entries_of(global) {
        if !union.contains(&(conversation, entry)) {
            report.in_global_only.push(Difference {
                conversation,
                entry,
                expected_at_least: Status::Untouched,
                actual: global.status_of(conversation, entry),
            });
        }
    }

    Ok(report)
}

/// Every entry a state records above `Untouched`.
fn entries_of(state: &GlobalState) -> Vec<(i32, i32)> {
    let mut found = Vec::new();
    for conversation in state.conversations().collect::<Vec<_>>() {
        for status in [Status::WasOffered, Status::WasDisplayed] {
            for entry in state.entries_at(conversation, status) {
                found.push((conversation, entry));
            }
        }
    }

    found
}

/// The report as a person reads it, ending in a PASS or FAIL line.
#[must_use]
pub fn written(report: &Report, max_examples: usize) -> String {
    let mut out = String::new();
    let examples = |out: &mut String, differences: &[Difference]| {
        for difference in differences.iter().take(max_examples) {
            out.push_str(&format!("    {difference}\n"));
        }
        if differences.len() > max_examples {
            out.push_str(&format!(
                "    ... and {} more\n",
                differences.len() - max_examples,
            ));
        }
    };

    out.push_str(&format!(
        "Entries in the union of all saves : {}\n\n",
        report.union_entries,
    ));
    out.push_str("The evidence that every playthrough survived in one file:\n");
    for exclusive in &report.exclusives {
        out.push_str(&format!(
            "  Entries only in {}, preserved in global : {} of {}\n",
            exclusive.label, exclusive.preserved, exclusive.total,
        ));
    }

    out.push_str("\nInformational (not failures):\n");
    out.push_str(&format!(
        "  Global above every save : {}\n",
        report.above_all_saves.len(),
    ));
    examples(&mut out, &report.above_all_saves);
    out.push_str(&format!(
        "  Global only, in no save : {}\n",
        report.in_global_only.len(),
    ));
    examples(&mut out, &report.in_global_only);
    out.push('\n');

    if report.passed() {
        out.push_str("PASS: every entry in any save is at least as high in the global state.\n");
        return out;
    }

    out.push_str(&format!(
        "Downgraded or missing entries : {}\n",
        report.downgrades.len(),
    ));
    examples(&mut out, &report.downgrades);
    out.push_str(&format!(
        "\nFAIL: {} entry(s) sit lower in the global state than in a save.\n",
        report.downgrades.len(),
    ));
    out
}

/// A save's dialogue statuses, projected into the same type the state file is read as.
///
/// LOSSLESS FOR THIS PURPOSE. The state drops `Untouched` on merge, which is exactly what
/// is wanted: a real save holds around 113,000 entries of which only about 1,500 are above
/// it, and the state file never records `Untouched` either.
///
/// # Errors
///
/// Where the save will not read, holds no conversations, or names a status this build does
/// not know.
pub fn statuses_in_save(path: &Path) -> Result<GlobalState, CheckFault> {
    let shown = path.display().to_string();
    let conversations = conversations_in_save(path, &shown)?;

    let mut state = GlobalState::new();
    for (conversation, value) in numbered(&conversations) {
        let LuaValue::Table(held) = value else {
            continue;
        };
        let Some(LuaValue::Table(dialog)) = field(held, DIALOG_FIELD) else {
            continue;
        };

        for (entry, value) in numbered(dialog) {
            let LuaValue::Table(held) = value else {
                continue;
            };
            let Some(LuaValue::Text(named)) = field(held, SIM_STATUS_FIELD) else {
                continue;
            };

            let status = Status::parse(named).ok_or_else(|| {
                CheckFault::UnknownStatus(shown.clone(), conversation, entry, named.clone())
            })?;
            state.merge(conversation, entry, status);
        }
    }

    Ok(state)
}

/// Every entry of a table whose key is an id, from BOTH HALVES of it.
///
/// A Lua table keeps a list part whose keys are its own 1-based indices and are not stored,
/// and a dictionary part that is. Which half a conversation landed in is the blob's business
/// and not this one's, so both are walked.
fn numbered(table: &LuaTable) -> Vec<(i32, &LuaValue)> {
    let mut found: Vec<(i32, &LuaValue)> = table
        .list
        .iter()
        .enumerate()
        .filter_map(|(at, value)| i32::try_from(at + 1).ok().map(|id| (id, value)))
        .collect();

    found.extend(
        table
            .dict
            .iter()
            .filter_map(|(key, value)| id_of(key).map(|id| (id, value))),
    );

    found
}

/// One named field of a table, which is a dictionary key spelled as a string.
fn field<'a>(table: &'a LuaTable, name: &str) -> Option<&'a LuaValue> {
    table.get(&LuaValue::Text(name.to_string()))
}

/// A key as the id it may be, whether the blob wrote it as a number or as text.
fn id_of(key: &LuaValue) -> Option<i32> {
    match key {
        LuaValue::Int(whole) => Some(*whole),
        LuaValue::Text(text) => text.parse().ok(),
        _ => None,
    }
}

/// One save's `Conversation` table, whether the save is packed or expanded.
///
/// ONLY THAT TABLE IS DECODED. An expanded save's `Variable` table leaves out the variables
/// that only repeat the conversations, and putting those back needs an id map that is not
/// committed - so a reader that decoded all five would refuse a save it does not need to.
fn conversations_in_save(path: &Path, shown: &str) -> Result<LuaTable, CheckFault> {
    let at = TABLE_NAMES
        .iter()
        .position(|name| *name == CONVERSATION_TABLE)
        .expect("the conversations are one of the five");
    let unreadable =
        |why: &dyn std::fmt::Display| CheckFault::Unreadable(shown.to_string(), why.to_string());

    if path.is_dir() {
        let parts = lua_parts::read(&OnDisk, path).map_err(|why| unreadable(&why))?;
        return lua_sparse::decode_map(&parts.tables[at], CONVERSATION_TABLE, None)
            .map_err(|why| unreadable(&why));
    }

    let packed = packed_save::unpack(path).map_err(|why| unreadable(&why))?;
    let blob = lua_blob::read(&packed.lua).map_err(|why| unreadable(&why))?;
    match blob.tables.into_iter().nth(at) {
        Some(LuaValue::Table(table)) => Ok(table),
        _ => Err(CheckFault::NoConversations(shown.to_string())),
    }
}

/// The save a name means: a path if it is one, else a save of that name in the directory.
///
/// # Errors
///
/// Where nothing matches.
pub fn resolve_save(named: &str, directory: &Path) -> Result<PathBuf, CheckFault> {
    let given = Path::new(named);
    if given.exists() {
        return Ok(given.to_path_buf());
    }

    // A bare save name: prefer the packed archive, fall back to an expanded folder.
    for suffix in [PACKED_SUFFIX, EXPANDED_SUFFIX] {
        let candidate = directory.join(format!("{named}{suffix}"));
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(CheckFault::NoSuchSave(
        named.to_string(),
        directory.display().to_string(),
    ))
}

/// The saves in a directory, packed or expanded, by the name that names either.
#[must_use]
pub fn saves_in(directory: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };

    // A MAP RATHER THAN A LIST, because a save can be there in both shapes and is one save.
    let mut found: BTreeMap<String, ()> = BTreeMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stem = if entry.path().is_dir() {
            name.strip_suffix(EXPANDED_SUFFIX)
        } else {
            name.strip_suffix(PACKED_SUFFIX)
        };

        if let Some(stem) = stem {
            found.insert(stem.to_string(), ());
        }
    }

    found.into_keys().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save(label: &str, entries: &[(i32, i32, Status)]) -> NamedSave {
        let mut state = GlobalState::new();
        for &(conversation, entry, status) in entries {
            state.merge(conversation, entry, status);
        }

        NamedSave {
            label: label.to_string(),
            state,
        }
    }

    fn global(entries: &[(i32, i32, Status)]) -> GlobalState {
        let mut state = GlobalState::new();
        for &(conversation, entry, status) in entries {
            state.merge(conversation, entry, status);
        }

        state
    }

    #[test]
    fn a_state_holding_every_saves_highest_status_passes() {
        let saves = [
            save("one", &[(1, 2, Status::WasDisplayed)]),
            save("two", &[(1, 3, Status::WasOffered)]),
        ];
        let state = global(&[(1, 2, Status::WasDisplayed), (1, 3, Status::WasOffered)]);

        let report = compare(&state, &saves).expect("two saves are enough");

        assert!(report.passed(), "{:?}", report.downgrades);
        assert_eq!(report.union_entries, 2);
    }

    /// The failure this exists to catch.
    #[test]
    fn an_entry_the_state_never_took_in_is_a_downgrade() {
        let saves = [
            save("one", &[(1, 2, Status::WasDisplayed)]),
            save("two", &[(1, 3, Status::WasOffered)]),
        ];
        let state = global(&[(1, 2, Status::WasDisplayed)]);

        let report = compare(&state, &saves).expect("two saves are enough");

        assert!(!report.passed());
        assert_eq!(report.downgrades.len(), 1);
        assert_eq!(report.downgrades[0].entry, 3);
        assert!(written(&report, 10).contains("FAIL"));
    }

    /// A status recorded lower in the state than in a save, which is the other failure.
    #[test]
    fn a_status_lower_in_the_state_than_in_a_save_is_a_downgrade() {
        let saves = [
            save("one", &[(1, 2, Status::WasDisplayed)]),
            save("two", &[(1, 2, Status::WasOffered)]),
        ];
        let state = global(&[(1, 2, Status::WasOffered)]);

        let report = compare(&state, &saves).expect("two saves are enough");

        assert_eq!(report.downgrades.len(), 1);
        assert_eq!(report.downgrades[0].expected_at_least, Status::WasDisplayed);
    }

    /// Higher than every save is CORRECT, because the state is merged across playthroughs.
    #[test]
    fn a_state_above_every_save_is_information_and_not_a_failure() {
        let saves = [
            save("one", &[(1, 2, Status::WasOffered)]),
            save("two", &[(1, 2, Status::WasOffered)]),
        ];
        let state = global(&[(1, 2, Status::WasDisplayed)]);

        let report = compare(&state, &saves).expect("two saves are enough");

        assert!(report.passed());
        assert_eq!(report.above_all_saves.len(), 1);
        assert!(written(&report, 10).contains("PASS"));
    }

    /// The load-bearing evidence: an entry only one save has.
    #[test]
    fn an_entry_only_one_save_holds_is_counted_against_that_save() {
        let saves = [
            save(
                "one",
                &[(1, 2, Status::WasDisplayed), (9, 9, Status::WasOffered)],
            ),
            save("two", &[(1, 2, Status::WasDisplayed)]),
        ];
        let state = global(&[(1, 2, Status::WasDisplayed), (9, 9, Status::WasOffered)]);

        let report = compare(&state, &saves).expect("two saves are enough");

        assert_eq!(report.exclusives[0].total, 1);
        assert_eq!(report.exclusives[0].preserved, 1);
        assert_eq!(report.exclusives[1].total, 0);
    }

    #[test]
    fn a_state_entry_no_save_holds_is_listed_rather_than_only_counted() {
        let saves = [
            save("one", &[(1, 2, Status::WasDisplayed)]),
            save("two", &[(1, 2, Status::WasDisplayed)]),
        ];
        let state = global(&[(1, 2, Status::WasDisplayed), (4, 5, Status::WasOffered)]);

        let report = compare(&state, &saves).expect("two saves are enough");

        assert!(report.passed());
        assert_eq!(report.in_global_only.len(), 1);
        assert!(written(&report, 10).contains("conversation 4, entry 5"));
    }

    /// One save proves nothing about a union, so it is refused rather than reported on.
    #[test]
    fn fewer_than_two_saves_is_refused() {
        let one = [save("one", &[(1, 2, Status::WasDisplayed)])];

        assert!(matches!(
            compare(&global(&[]), &one),
            Err(CheckFault::TooFewSaves(1)),
        ));
    }

    #[test]
    fn a_report_names_only_so_many_examples_and_says_how_many_it_left_out() {
        let mut entries = Vec::new();
        for entry in 0..15 {
            entries.push((1, entry, Status::WasDisplayed));
        }
        let saves = [save("one", &entries), save("two", &entries)];

        let report = compare(&global(&[]), &saves).expect("two saves are enough");
        let text = written(&report, 3);

        assert_eq!(report.downgrades.len(), 15);
        assert!(text.contains("... and 12 more"), "{text}");
    }
}
