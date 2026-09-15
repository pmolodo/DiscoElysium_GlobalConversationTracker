// SPDX-License-Identifier: MIT
//! Reading the scenario fixtures the in-game harness stages, without staging them.
//!
//! ## What a fixture is made of
//!
//! Two halves, and they are stored in different places because the game stores them in
//! different places. What OTHER saves have read is the mod's own global state file, staged
//! beside the profile and named by a suite. What THIS save has read is the game's per-save
//! SimStatus, which lives inside the save and nothing staged beside it can set - which is
//! why a scenario that needs an entry already read needs a save of its own.
//!
//! An offline executor has to assemble both to mean what the in-game run means, so it
//! reads them from the same two files the run loads rather than from a copy written down
//! beside the expectation. The copy is the thing that drifts.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use lookahead_engine::bridge::{NodeRef, NodeSet, WireValue};
use lookahead_engine::core::passive_check;
use lookahead_engine::core::types::Ternary;
use lookahead_engine::formats::global_state::{self, GlobalState, Status};
use lookahead_engine::formats::runs;
use serde::Deserialize;

use super::repo_root;

/// Where the committed scenario fixtures live.
pub fn scenarios() -> PathBuf {
    repo_root().join("testing").join("scenarios")
}

/// Every committed scenario save, by the name a scenario names it with.
///
/// # Panics
///
/// If the scenarios folder will not read, or holds none.
pub fn committed_saves() -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(scenarios())
        .unwrap_or_else(|why| panic!("{}: {why}", scenarios().display()))
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .strip_suffix(".ntwtf")
                .map(str::to_string)
        })
        .collect();

    assert!(!found.is_empty(), "no committed saves were found");
    found.sort();
    found
}

/// Where the base every scenario save eventually rests on lives.
///
/// Outside the scenarios folder, which is why a chain can walk out of it.
fn testing() -> PathBuf {
    repo_root().join("testing")
}

/// A save's folder, wherever it is.
///
/// A scenario names its save without a path - `afford-both` - and the folder is that plus
/// the game's extension. The chain then names its bases by relative path, so those are
/// resolved against the folder they were named in rather than looked up here.
fn folder_of(save: &str) -> PathBuf {
    let named = scenarios().join(format!("{save}.ntwtf"));
    if named.is_dir() {
        return named;
    }

    testing().join(format!("{save}.ntwtf"))
}

/// What a save folder's archive says it was built on top of.
#[derive(Debug, Deserialize)]
struct Archive {
    /// The next save down, by path relative to this folder. Absent at the bottom.
    #[serde(default)]
    base: Option<String>,
}

/// The save folders one save is made of, BASE-MOST FIRST.
///
/// A scenario save in this repository is a diff over a base, which is a diff over another,
/// down to `save_template` - so what a save holds is only knowable by walking the chain.
/// Reading the leaf alone was enough while the only question asked of a save was which
/// entries it had displayed, because no base records any; it is not enough for the
/// variables, which is exactly where the base does the work.
///
/// # Panics
///
/// If a folder is missing, its archive will not read, or the chain loops. A loop would
/// otherwise be an out-of-memory rather than a message.
fn chain(save: &str) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    let mut seen = HashSet::new();
    let mut current = folder_of(save);

    loop {
        assert!(
            current.is_dir(),
            "{save}'s chain reaches {}, which is not a save folder",
            current.display(),
        );
        assert!(
            seen.insert(current.clone()),
            "{save}'s chain of bases loops back to {}",
            current.display(),
        );

        folders.push(current.clone());

        let archive = current.join("_archive.json");
        let Some(base) = read_json::<Archive>(&archive).and_then(|a| a.base) else {
            break;
        };

        // Relative to the folder that named it, and normalised, because a chain that walks
        // out of the scenarios folder - as every one of them does, to save_template -
        // produces a path with `..` in it that no `is_dir` would answer for on its own.
        current = normalise(&current.join(base));
    }

    folders.reverse();
    folders
}

/// A path with its `.` and `..` steps applied, without touching the filesystem.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }

    out
}

/// One of a save folder's Lua parts, or None where the save does not change it.
///
/// The part files sit under `<save>.ntwtf.lua.parts`, named for the Lua table they carry.
fn part(folder: &Path, name: &str) -> Option<serde_json::Value> {
    let stem = folder.file_name()?.to_str()?;
    let path = folder.join(format!("{stem}.lua.parts")).join(name);
    read_json(&path)
}

/// A JSON document, or None where it is not there.
///
/// # Panics
///
/// If it is there and will not parse. A fixture that has become unreadable is a thing to
/// stop for, not to treat as absent.
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    if !path.exists() {
        return None;
    }

    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    Some(
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} will not parse: {e}", path.display())),
    )
}

/// The changes a part carries, whether it is a full table or a diff over one.
///
/// A base writes its table out whole and a diff writes only what it changes under
/// `_changes`. Both are read the same way here, since the merge that follows does not care
/// which it was - only the order does, and that is what [`chain`] fixes.
fn changes(document: &serde_json::Value) -> Option<&serde_json::Map<String, serde_json::Value>> {
    document.get("_changes").unwrap_or(document).as_object()
}

/// Keys a part carries that are about the FILE rather than about the world.
///
/// `_derived_simx` is the packer's own index into the entry tables; `_format` says which of
/// the two shapes above the document is in, and `_formatVersion` which version of it. None
/// is a dialogue variable, and a world that answered one as though it were would be
/// answering a question no guard asks.
///
/// `_base` says which file a diff is a diff of, and only a diff carries it - so it never
/// reaches the loop below, which reads a diff's `_changes` rather than its top level. It is
/// named anyway, because that is a fact about where it sits rather than a rule, and the
/// reader should not depend on it.
const NOT_A_VARIABLE: [&str; 4] = ["_format", "_formatVersion", "_base", "_derived_simx"];

/// Where the game keeps the rain, in the table it keeps every dialogue variable in.
const RAINING: &str = "auto.is_raining";

/// And the snow.
const SNOWING: &str = "auto.is_snowing";

/// The preset types that set each of them, as the weather table names them.
const RAIN: &str = "RAIN";
const SNOW: &str = "SNOW";

/// Every dialogue variable a save holds, with the bases it rests on merged in.
///
/// ## Why this is read and not declared
///
/// An ordinary option's guard asks about the world in a way a check's rarely does. 451:86 -
/// buy the Faln sneakers - is guarded on `jam.siileng_faln_sneakers == true`, and a
/// world that cannot answer it stops the search before it builds a single state: the engine
/// reports the floor over zero states, and the option draws nothing where the game draws
/// orange. The answer is in the save the in-game run loads, so it is read from there
/// rather than copied into the definition beside the expectation, for the same reason
/// [`read_in_save`] reads what a save has displayed.
///
/// ## What it cannot answer
///
/// Nothing that is not a dialogue variable. The Lua parts carry the variables and the
/// tables that DEFINE items and thoughts; what is held, thought and asked for is in the
/// save's second document, and [`holdings_in_save`] reads it.
///
/// # Panics
///
/// If the chain will not resolve, or a value is one no guard could hold.
pub fn variables_in_save(save: &str) -> HashMap<String, WireValue> {
    let mut variables: HashMap<String, WireValue> = HashMap::new();

    for folder in chain(save) {
        let Some(document) = part(&folder, "Variable.json") else {
            continue;
        };
        let Some(entries) = changes(&document) else {
            panic!("{}'s variables are not a table", folder.display());
        };

        for (name, value) in entries {
            if NOT_A_VARIABLE.contains(&name.as_str()) {
                continue;
            }

            match wire(value) {
                Some(answer) => {
                    variables.insert(name.clone(), answer);
                }
                // A TABLE OR A NULL, which no guard can compare against. Left unanswered
                // rather than guessed at, which reads as Unknown and is the permissive
                // answer - the same thing the plugin sends for a variable it could not
                // read.
                None => {
                    variables.remove(name);
                }
            }
        }
    }

    // THE WEATHER AS THE GAME LOADS IT, not as the save last wrote it. `WeatherController`
    // writes both weather variables from the preset's type whenever the weather changes, and
    // loading a save changes it - measured 2026-09-12, scene-raining answered `IsRaining()`
    // true mid-load and false by the time its menu went up. So a save whose variables and
    // preset disagree is read as written and loaded here the way the game loads it:
    // at-garte-kitchen was saved in the RAIN preset with `auto.is_raining` false.
    let weather = weather_of(save);
    for (variable, kind) in [(RAINING, RAIN), (SNOWING, SNOW)] {
        variables.insert(
            variable.to_string(),
            WireValue::Bool {
                value: weather == kind,
            },
        );
    }

    // THE CHECKS THIS SAVE HAS FAILED, locked the way the game locks them. The game keeps a
    // failed white check in its own failedWhiteChecksHolder rather than in any Lua variable,
    // and refuses the check for as long as it stays there; the engine closes a check whose
    // failure slot is set. Answering the slot is what makes a locked check neither an option
    // nor a way through to somewhere else.
    lookahead_engine::bridge::lock_failed_white_checks(
        &mut variables,
        failed_white_checks_in_save(save),
    );

    variables
}

/// The flags of every white check this save holds as failed.
///
/// # Panics
///
/// If the holder is not the shape the game writes. A lock misread as no lock opens a route the
/// game refuses, which is the failure this reading exists to prevent.
fn failed_white_checks_in_save(save: &str) -> HashSet<String> {
    let holder = world_state(save, "failedWhiteChecksHolder");
    let by_skill = holder["ChecksBySkill"]
        .as_object()
        .unwrap_or_else(|| panic!("{save}'s failedWhiteChecksHolder has no ChecksBySkill table"));

    by_skill
        .iter()
        .flat_map(|(skill, flags)| {
            flags
                .as_array()
                .unwrap_or_else(|| panic!("{save}'s failed {skill} checks are not a list"))
                .iter()
                .map(move |flag| {
                    flag.as_str()
                        .unwrap_or_else(|| panic!("{save}'s failed {skill} checks hold a non-name"))
                        .to_string()
                })
        })
        .collect()
}

/// One JSON value as the engine's wire vocabulary, or None where it is not one.
fn wire(value: &serde_json::Value) -> Option<WireValue> {
    match value {
        serde_json::Value::Bool(value) => Some(WireValue::Bool { value: *value }),
        serde_json::Value::Number(number) => {
            number.as_f64().map(|value| WireValue::Number { value })
        }
        serde_json::Value::String(value) => Some(WireValue::Text {
            value: value.clone(),
        }),
        _ => None,
    }
}

/// What some other save has read, per a staged global state file, over a whole group.
///
/// ## Why a group and not a conversation
///
/// A LOOK-AHEAD IS ASKED OF A GROUP, and a group is several conversations: the engine
/// loads everything reachable from the one that is open, and Joyce's runs to 2,857 entries
/// across many. Classifying only the open conversation's entries and letting the rest fall
/// through to "never seen anywhere" is not a small inaccuracy - it invents the top rung
/// almost everywhere, so a search that should be refused finds something to walk towards.
/// Measured: it made 2,629 of Joyce's 2,857 entries look worth searching under a fixture
/// that records every entry in the game.
///
/// A conversation the state says nothing about contributes nothing, which is the same
/// answer the mod's own state gives for it.
pub fn recorded_elsewhere_in_group(state_file: &str, conversations: &[i32]) -> HashSet<(i32, i32)> {
    let state = read_state(state_file);
    let mut recorded = HashSet::new();

    for conversation in conversations {
        recorded.extend(
            state
                .entries_at(*conversation, Status::WasDisplayed)
                .into_iter()
                .map(|entry| (*conversation, entry)),
        );
    }

    recorded
}

/// A staged global state fixture, resolved to what the mod would be handed.
///
/// RESOLVED, because a fixture may be written as a DIFF OF ANOTHER ONE rather than as a
/// whole state - the arrangement the saves have had from the start, and for the same
/// reason: the fixture that prompted it differed from the one beside it by one orb and two
/// entries, in 29 KB of otherwise identical bytes. `lookahead_engine::formats` is the one
/// definition of that, and the in-game run stages through the same code by way of the
/// engine host's `resolve` verb, so neither executor has a reader of its own to drift.
///
/// READ BY THE LIBRARY TOO, rather than by a struct declared here. A reader of its own
/// would check no header, so a fixture of the wrong kind would come back as a state that
/// records nothing - which fails no assertion and makes every scenario look like a fresh
/// playthrough.
///
/// # Panics
///
/// If it is missing, will not resolve, is not a global state, or holds a row that will not
/// read. The last is a warning to the mod, which must not throw a player's history away
/// over one bad row; here it is a fixture that has stopped saying what it meant.
fn read_state(state_file: &str) -> GlobalState {
    let path = scenarios().join(state_file);
    let document = lookahead_engine::formats::resolve::document(&path)
        .unwrap_or_else(|fault| panic!("{fault}"));
    let read = global_state::read_document(&document, &path.to_string_lossy())
        .unwrap_or_else(|fault| panic!("{fault}"));

    assert_eq!(
        read.skipped,
        0,
        "{} holds {} rows that will not read: {:?}",
        path.display(),
        read.skipped,
        read.warnings,
    );
    read.state
}

/// What a scenario save has already displayed, over a whole group.
///
/// READ FROM THE SAVE THE IN-GAME RUN LOADS, rather than copied into the definition beside
/// it. A row names its save and nothing more, so the two executors cannot come to disagree
/// about what that save holds - which they would the first time a save was edited and the
/// copy was not.
///
/// A scenario save is a diff over a base, and only the entries it CHANGES are written
/// down. THE WHOLE CHAIN IS WALKED, base-most first, and the last folder to say anything
/// about a conversation wins - which is what a diff means. It used to read the leaf alone,
/// on the documented grounds that no base in this repository records a displayed entry.
/// That was true, and it was an assumption where walking the chain is a fact; the chain has
/// to be resolved for the variables anyway (see [`variables_in_save`]), so there is nothing
/// left to buy by assuming it.
///
/// ONE WALK FOR EVERY CONVERSATION OF THE GROUP: the base writes out the whole Conversation
/// table, and re-reading it once per conversation would be re-parsing a megabyte per
/// question. See [`recorded_elsewhere_in_group`] for why the group rather than the open
/// conversation is what a look-ahead is asked about.
///
/// # Panics
///
/// If the save is not there, or a Conversation part will not parse.
pub fn read_in_save_group(save: &str, conversations: &[i32]) -> HashSet<(i32, i32)> {
    let mut displayed: HashSet<(i32, i32)> = HashSet::new();

    for folder in chain(save) {
        let Some(document) = part(&folder, "Conversation.json") else {
            continue;
        };
        let Some(entries) = changes(&document) else {
            continue;
        };

        for conversation in conversations {
            let Some(runs) = entries
                .get(&conversation.to_string())
                .and_then(|entry| entry.get("Dialog"))
                .and_then(|dialog| dialog.get("WasDisplayed"))
                .and_then(|runs| runs.as_str())
            else {
                continue;
            };

            // THE LAST FOLDER TO SAY ANYTHING WINS, per conversation, which is what a
            // diff means: a later save replacing the list replaces it, and says nothing
            // about the conversations it left alone.
            displayed.retain(|(had, _)| had != conversation);
            displayed.extend(
                parse_runs(runs)
                    .into_iter()
                    .map(|entry| (*conversation, entry)),
            );
        }
    }

    displayed
}

/// A run-encoded list of numbers, as every file in this repository writes one.
///
/// `3,5,7-25`, with a leading `-` read as a sign and a descending range counting down -
/// [`lookahead_engine::formats::runs`] is the spelling and the only implementation of it.
/// This is the set a fixture wants, where that hands back the list it read.
///
/// # Panics
///
/// If a piece is not a number or a range of them. A fixture that has stopped being readable
/// is a thing to stop for.
pub fn parse_runs(text: &str) -> HashSet<i32> {
    runs::unpack(text, "a fixture")
        .unwrap_or_else(|fault| panic!("{fault}"))
        .into_iter()
        .map(|id| i32::try_from(id).unwrap_or_else(|_| panic!("{id} is not an entry id")))
        .collect()
}

/// Every passive skill check in these conversations, decided by the save's own sheet.
///
/// ## Why a fixture computes this at all
///
/// The engine does not decide a check and never will: what the plugin sends is the OUTCOME
/// of each one, worked out from the live character sheet, and the engine's business is
/// what follows from it. An offline executor that wants to mean what an in-game run means
/// has to arrive at the same outcomes, and the only honest place to get them is the save
/// the run would load. Declaring them beside the expectation is the copy that drifts.
///
/// ## How a check is decided
///
/// The same three numbers the game uses. The entry's `DifficultyPass` is an index into the
/// difficulty table and the value there is the threshold; the skill is whichever one the
/// entry's SPEAKER is; and the comparison is
/// [`lookahead_engine::core::passive_check::outcome`], which is the plugin's own
/// arithmetic and is called here rather than repeated. An `Antipassive` entry is the line
/// that shows when you are not sharp enough, and the same call inverts for it.
///
/// ## And what the save's thoughts do to it
///
/// A thought can move a passive check's threshold or force one through regardless of the
/// numbers, and neither is on the sheet: the effects live on the thought's definition, which
/// `testing/thought-effects.json` writes down - see [`PassiveThoughts`]. Skill modifiers a
/// thought CAUSES are on the sheet like any other, and counted there.
pub fn checks_in_save(save: &str, conversations: &[i32]) -> Option<Checks> {
    let skills = skills_in_save(save);
    let thoughts = passive_thoughts_in_save(save);
    let speakers = actor_names()?;
    let index = super::conversation_index()?;

    let mut found = Checks::default();
    for conversation in read_conversations(&index, conversations) {
        for entry in &conversation.entries {
            let Some(difficulty) = entry.fields.get(PASSIVE_FIELD) else {
                continue;
            };

            let node = NodeRef {
                conversation: conversation.id,
                entry: entry.id,
            };
            let threshold = threshold_of(difficulty, node);

            // NOT A SKILL ACTOR, so the game logs an error and the plugin answers Unknown.
            // In neither set is what silence means on the wire, so leaving it out says the
            // same thing here.
            let Some(actor) = entry.fields.get(ACTOR_FIELD) else {
                continue;
            };
            let Some(name) = speakers.get(actor) else {
                continue;
            };
            let Some(skill) = skill_of_actor(name) else {
                continue;
            };

            let value = skills.get(skill).copied().unwrap_or_else(|| {
                panic!("{save}'s character sheet has no {skill}, which {node:?} tests")
            });
            let antipassive = entry.fields.contains_key(ANTIPASSIVE_FIELD);

            // FORCED THROUGH ON THE RESULT rather than folded into the threshold, as the
            // game does it: an antipassive line is the one shown when the check fails, so a
            // check forced to pass hides it.
            let fires = if thoughts.succeeding.contains(skill) {
                !antipassive
            } else {
                let moved = threshold + thoughts.threshold_shift(skill);
                passive_check::outcome(value, moved, antipassive) == Ternary::True
            };
            if fires {
                found.pass.insert(node);
            } else {
                found.fail.insert(node);
            }
        }
    }

    Some(found)
}

/// What a save's thoughts do to its passive checks, by skill.
///
/// ## Which thought counts, and which way an amount moves
///
/// MEASURED IN GAME, because the definitions say neither (de-2jlj). With lawbringer,
/// remote_viewer and age_bracket FIXED in at-trashcan, the game passed exactly nine checks it
/// had failed: a PASSIVE_TARGET_MODIFIER of -1 LOWERS the threshold by one for every skill of
/// its ability, and lawbringer's PASSIVES_SUCCEED forces Hand/Eye Coordination through. All
/// three are completion effects, applied to a FIXED thought.
///
/// A RESEARCH-PHASE EFFECT applies to a COOKING thought, and to nothing else - see
/// [`state_applying`]. No passive effect in the shipped game is a research effect, so that
/// half is held to the game's load code rather than to a measurement.
pub struct PassiveThoughts {
    /// How far each skill's thresholds move.
    shifts: HashMap<String, i32>,
    /// The skills whose checks pass whatever the numbers say.
    pub succeeding: HashSet<String>,
    /// Whether a thought forces every red check to fail.
    ///
    /// Not a passive check, but decided from the same table against the same cabinet, so it
    /// is read here rather than by a second walk over both.
    pub red_checks_fail: bool,
}

impl PassiveThoughts {
    /// How far a skill's thresholds move; zero where no thought moves them.
    pub fn threshold_shift(&self, skill: &str) -> i32 {
        self.shifts.get(skill).copied().unwrap_or(0)
    }
}

/// The state a thought has to be in for an effect of `phase` to apply.
///
/// The game's own load, `CharacterSheetPersister.DeserializeItemsAndThoughts`, applies a
/// COOKING thought's research effects and a FIXED thought's completion effects, and neither
/// applies the other list - so a research effect lifts when its thought is finished.
///
/// # Panics
///
/// If the phase is not one of the two a thought's definition has.
fn state_applying(phase: &str, thought: &str) -> &'static str {
    match phase {
        "research" => COOKING,
        "completion" => FIXED,
        other => panic!("{thought}: an effect in the '{other}' phase, which no thought has"),
    }
}

/// What a save's thoughts do to its passive checks - see [`PassiveThoughts`].
///
/// # Panics
///
/// If the thought table will not read, names an effect or phase this does not know, or the
/// save's sheet or cabinet is not the shape the game writes.
pub fn passive_thoughts_in_save(save: &str) -> PassiveThoughts {
    let states = thought_states(&world_state(save, "thoughtCabinetState"));
    let sheet = character_sheet(save);
    let ability_of: HashMap<String, String> = sheet
        .as_object()
        .expect("the sheet is an object")
        .values()
        .filter_map(|skill| {
            Some((
                skill.get("skillType")?.as_str()?.to_string(),
                skill.get("abilityType")?.as_str()?.to_string(),
            ))
        })
        .collect();

    passive_thoughts(&states, &ability_of, thought_effects())
}

/// What thoughts in `states` do to passive checks, given each skill's ability and the effects
/// table.
///
/// Apart from [`passive_thoughts_in_save`] so a table the game does not ship can be held to
/// the same rules.
///
/// # Panics
///
/// If an effect names a field, effect or phase this does not know.
pub fn passive_thoughts(
    states: &HashMap<String, String>,
    ability_of: &HashMap<String, String>,
    effects: &[serde_json::Value],
) -> PassiveThoughts {
    let mut thoughts = PassiveThoughts {
        shifts: HashMap::new(),
        succeeding: HashSet::new(),
        red_checks_fail: false,
    };
    for effect in effects {
        let text = |key: &str| {
            effect[key]
                .as_str()
                .unwrap_or_else(|| panic!("a thought effect has no '{key}': {effect}"))
        };
        let thought = text("thought");
        let wanted = state_applying(text("phase"), thought);
        if states.get(thought).map(String::as_str) != Some(wanted) {
            continue;
        }

        match text("effect") {
            "PASSIVE_TARGET_MODIFIER" => {
                let ability = text("ability");
                let amount = effect["amount"]
                    .as_i64()
                    .unwrap_or_else(|| panic!("{thought}: a threshold modifier with no amount"))
                    as i32;
                for (skill, owner) in ability_of {
                    if owner == ability {
                        *thoughts.shifts.entry(skill.clone()).or_default() += amount;
                    }
                }
            }
            "PASSIVES_SUCCEED" => {
                thoughts.succeeding.insert(text("skill").to_string());
            }
            "THC_RED_CHECK_FAILURE" => {
                thoughts.red_checks_fail = true;
            }
            other => panic!("{thought}: a passive effect '{other}' this does not know"),
        }
    }

    thoughts
}

/// The effects `tools/derive-thought-effects.py` read out of the game's thought definitions.
fn thought_effects() -> &'static [serde_json::Value] {
    static EFFECTS: OnceLock<Vec<serde_json::Value>> = OnceLock::new();
    EFFECTS.get_or_init(|| {
        let path = repo_root().join("testing").join("thought-effects.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|why| panic!("{}: {why}", path.display()));
        let document: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|why| panic!("{}: {why}", path.display()));

        document["effects"]
            .as_array()
            .unwrap_or_else(|| panic!("{} lists no effects", path.display()))
            .clone()
    })
}

/// What a save's character sheet makes of a group's passive checks.
///
/// Two sets rather than one map, because that is the shape the wire has and the reason is
/// the same: an entry in neither is one nothing is known about, which is a third answer
/// that a pair of booleans cannot carry.
#[derive(Debug, Default)]
pub struct Checks {
    /// Entries whose check fires, so the line is one the player can be shown.
    pub pass: NodeSet,
    /// Entries whose check does not, so the line is one they never will be.
    pub fail: NodeSet,
}

/// The field whose presence makes an entry a passive check.
const PASSIVE_FIELD: &str = "DifficultyPass";

/// The field marking a check that fires when it FAILS rather than when it passes.
const ANTIPASSIVE_FIELD: &str = "Antipassive";

/// The field naming the entry's speaker.
const ACTOR_FIELD: &str = "Actor";

/// What each difficulty id is worth, from the game's own table.
///
/// `ArticyBridge.ArticyDifficultyIdToDifficulty` indexed by the id, resolved through the
/// `Difficulty` enum - so id 2 is AVERAGE, which is 10, and a skill of 4 clears it once the
/// flat bonus is added. Out of order past the eighth because the enum grew a second row of
/// odd numbers between the original even ones.
const DIFFICULTY: [i32; 15] = [6, 8, 10, 12, 14, 16, 18, 20, 7, 9, 11, 13, 15, 17, 19];

/// The threshold an entry's difficulty id stands for.
///
/// # Panics
///
/// If the id is not one the table holds. The game logs an error and carries on with a
/// nonsense number; a fixture that did the same would decide checks wrongly and quietly.
fn threshold_of(difficulty: &str, node: NodeRef) -> i32 {
    let id: usize = difficulty.trim().parse().unwrap_or_else(|_| {
        panic!("{node:?} has difficulty '{difficulty}', which is not a number")
    });

    *DIFFICULTY
        .get(id)
        .unwrap_or_else(|| panic!("{node:?} has difficulty id {id}, which no table row holds"))
}

/// Which skill an actor IS, by the name the database gives it.
///
/// The game asks this of the actor's Articy id through a table keyed by it; the names are
/// the same table in the form the extractor can write, and they are the game's own
/// `Skill.actorSkillNames`. `None` for every actor that is a person rather than a skill.
///
/// THE SUB-SKILLS COLLAPSE, because the character sheet has no separate entry for them:
/// `CharacterSheet.GetSkill` answers all four Perceptions with the one Perception and
/// Convalescence with Endurance, so a check spoken by Perception (Sight) is decided by the
/// Perception the sheet holds.
fn skill_of_actor(name: &str) -> Option<&'static str> {
    const SKILLS: [(&str, &str); 29] = [
        ("Logic", "LOGIC"),
        ("Encyclopedia", "ENCYCLOPEDIA"),
        ("Rhetoric", "RHETORIC"),
        ("Drama", "DRAMA"),
        ("Conceptualization", "CONCEPTUALIZATION"),
        ("Visual Calculus", "VISUAL_CALCULUS"),
        ("Volition", "VOLITION"),
        ("Inland Empire", "INLAND_EMPIRE"),
        ("Empathy", "EMPATHY"),
        ("Authority", "AUTHORITY"),
        ("Suggestion", "SUGGESTION"),
        ("Esprit de Corps", "ESPRIT_DE_CORPS"),
        ("Physical Instrument", "PHYSICAL_INSTRUMENT"),
        ("Electrochemistry", "ELECTROCHEMISTRY"),
        ("Endurance", "ENDURANCE"),
        ("Convalescence", "ENDURANCE"),
        ("Half Light", "HALF_LIGHT"),
        ("Pain Threshold", "PAIN_THRESHOLD"),
        ("Shivers", "SHIVERS"),
        ("Hand/Eye Coordination", "HE_COORDINATION"),
        ("Perception", "PERCEPTION"),
        ("Perception (Hearing)", "PERCEPTION"),
        ("Perception (Sight)", "PERCEPTION"),
        ("Perception (Smell)", "PERCEPTION"),
        ("Perception (Taste)", "PERCEPTION"),
        ("Reaction Speed", "REACTION"),
        ("Savoir Faire", "SAVOIR_FAIRE"),
        ("Interfacing", "INTERFACING"),
        ("Composure", "COMPOSURE"),
    ];

    SKILLS
        .iter()
        .find(|(actor, _)| *actor == name)
        .map(|(_, skill)| *skill)
}

/// Every actor's name, by the id an entry's `Actor` field carries.
///
/// Read once and shared, like the index beside it: every scenario in a run asks about the
/// same 424 actors.
fn actor_names() -> Option<&'static HashMap<String, String>> {
    static NAMES: OnceLock<Option<HashMap<String, String>>> = OnceLock::new();

    NAMES
        .get_or_init(|| {
            let path = super::actors()?;
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));

            let mut names = HashMap::new();
            for line in text.lines().filter(|line| !line.trim().is_empty()) {
                let actor: IndexedActor = serde_json::from_str(line).unwrap_or_else(|e| {
                    panic!("{} has a line that will not parse: {e}", path.display())
                });
                names.insert(actor.id.to_string(), actor.name);
            }

            Some(names)
        })
        .as_ref()
}

/// One line of the actor table.
#[derive(Debug, Deserialize)]
struct IndexedActor {
    id: i32,
    name: String,
}

/// One conversation of the index, with only what deciding a check needs.
#[derive(Debug, Deserialize)]
struct IndexedConversation {
    id: i32,
    entries: Vec<IndexedEntry>,
}

/// One entry of it.
#[derive(Debug, Deserialize)]
struct IndexedEntry {
    id: i32,
    #[serde(default)]
    fields: HashMap<String, String>,
}

/// The named conversations, read out of the FULL index.
///
/// The shipped one will not do: it keeps the difficulty, since the engine has to know an
/// entry is a check at all, and drops the speaker and the inversion, since the engine never
/// decides one. Both of those are what deciding one needs.
///
/// Line by line, matching on the id the writer puts first, so a fifty megabyte file is not
/// parsed to answer about one conversation. That the id comes first is this repository's
/// own doing - see `ConversationIndexFile` - and the assertion below is what says so if it
/// ever stops being true.
///
/// # Panics
///
/// If a conversation is not in the index.
fn read_conversations(index: &Path, wanted: &[i32]) -> Vec<IndexedConversation> {
    static TEXT: OnceLock<String> = OnceLock::new();
    let text = TEXT.get_or_init(|| {
        std::fs::read_to_string(index).unwrap_or_else(|e| panic!("{}: {e}", index.display()))
    });

    let mut found = Vec::new();
    for conversation in wanted {
        let opening = format!("{{\"id\":{conversation},");
        let line = text
            .lines()
            .find(|line| line.starts_with(&opening))
            .unwrap_or_else(|| {
                panic!(
                    "conversation {conversation} is not in {}, or its lines no longer open \
                     with their id",
                    index.display(),
                )
            });

        found.push(
            serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("conversation {conversation} will not parse: {e}")),
        );
    }

    found
}

/// The value of every skill on a save's character sheet, by the name the sheet gives it.
///
/// ## Built from the parts rather than read off the total
///
/// A skill's value is its ability plus every modifier currently on it - a thought that
/// takes a point off Logic, a piece of clothing that adds one, damage - and the sheet
/// carries the total as well as the parts. This adds the parts up and then checks the
/// answer against the total, because the two disagreeing means the sheet is not what this
/// reader thinks it is, and a check decided from a misread sheet is worse than no check.
///
/// The ability is added rather than taken from the CALCULATED_ABILITY modifier, whose
/// recorded amount is zero: it is a note of WHERE the base comes from, filled in from the
/// ability when the game recalculates.
///
/// A skill is a world constant here. Nothing in a dialogue raises one mid-conversation, so
/// what the save holds is what every entry in the crawl is measured against.
///
/// # Panics
///
/// If the chain will not resolve, the sheet is missing, or the parts do not add up.
pub fn skills_in_save(save: &str) -> HashMap<String, i32> {
    /// The ability each `abilityType` names, as the sheet spells the ability's own key.
    const ABILITIES: [(&str, &str); 4] = [
        ("INT", "intellect"),
        ("PSY", "psyche"),
        ("FYS", "fysique"),
        ("MOT", "motorics"),
    ];
    /// The modifier that stands for the ability, and is counted by adding the ability.
    const CALCULATED_ABILITY: &str = "CALCULATED_ABILITY";

    let sheet = character_sheet(save);
    let causes = &sheet["SkillModifierCauseMap"];

    let mut values = HashMap::new();
    for (key, skill) in sheet.as_object().expect("the sheet is an object") {
        let Some(name) = skill.get("skillType").and_then(|name| name.as_str()) else {
            continue;
        };

        let ability_type = skill["abilityType"]
            .as_str()
            .unwrap_or_else(|| panic!("{save}'s {key} names no ability"));
        let ability_key = ABILITIES
            .iter()
            .find(|(named, _)| *named == ability_type)
            .map(|(_, key)| *key)
            .unwrap_or_else(|| panic!("{save}'s {key} names ability '{ability_type}'"));

        let mut value = whole(&sheet[ability_key]["value"], save, ability_key);
        for modifier in causes[name].as_array().into_iter().flatten() {
            if modifier["type"].as_str() == Some(CALCULATED_ABILITY) {
                continue;
            }

            value += whole(&modifier["amount"], save, name);
        }

        let total = whole(&skill["value"], save, key);
        assert_eq!(
            value, total,
            "{save}'s {key} is recorded as {total} and its ability and modifiers make {value}",
        );

        values.insert(name.to_string(), value);
    }

    values
}

/// One number off the sheet.
fn whole(value: &serde_json::Value, save: &str, what: &str) -> i32 {
    value
        .as_i64()
        .unwrap_or_else(|| panic!("{save}'s {what} holds {value}, which is not a whole number"))
        as i32
}

/// A save's character sheet, with the bases it rests on merged in.
fn character_sheet(save: &str) -> serde_json::Value {
    world_state(save, "characterSheet")
}

/// What the scene queries ask about: where the player is standing, and the weather.
///
/// ## The three that are about the world rather than the character
///
/// `IsExterior`, `IsRaining` and `IsSnowing` are the only ones, and they are the reason
/// this exists. Eighteen entries across six conversations guard on them, and EVERY ONE IS
/// HALF OF A COMPLEMENTARY PAIR - one entry guarded by `IsExterior()` and the next by
/// `(IsExterior()) == false`. So leaving one unanswered does not lose a line: unanswered
/// reads as unknown, unknown is permissive, and a crawl then reaches BOTH halves, one of
/// which the game would never draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scene {
    /// The area the save records, which is the key the outdoor table is read by.
    pub area: String,
    /// Whether the game calls that area outdoors.
    pub outside: bool,
    pub raining: bool,
    pub snowing: bool,
    /// The weather the save's own preset says it is in: `CLEAR`, `RAIN` or `SNOW`.
    ///
    /// NOT WHAT THE GUARDS READ, and carried anyway. `IsRaining()` reads the Lua variable,
    /// which is what [`Scene::raining`] answers from - but the game DERIVES that variable
    /// from the preset on every weather change and triggers the weather on load, so a save
    /// whose two halves disagree contradicts itself the moment it is read. This is what
    /// lets a fixture say they agree.
    pub weather: String,
}

/// The scenes the game calls outdoors, read off the table derived from its own asset.
///
/// TWO OF THIRTY-SEVEN, and the suffix on an area's name is not what decides it - see
/// `tools/derive-scene-properties.py` for why the table is written down rather than
/// inferred from the name it happens to agree with.
fn outdoor_scenes() -> &'static HashSet<String> {
    static OUTDOORS: OnceLock<HashSet<String>> = OnceLock::new();
    OUTDOORS.get_or_init(|| {
        let path = repo_root().join("testing").join("scenes.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|why| panic!("{}: {why}", path.display()));
        let document: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|why| panic!("{}: {why}", path.display()));

        document["outside"]
            .as_array()
            .unwrap_or_else(|| panic!("{} names no outdoor scenes", path.display()))
            .iter()
            .filter_map(|scene| scene.as_str().map(str::to_string))
            .collect()
    })
}

/// Where a save leaves the player, and what the sky is doing there.
///
/// THE WEATHER IS A LUA VARIABLE, `auto.is_raining` and `auto.is_snowing`, and the save
/// holds both. That is not obvious - nothing in the two JSON documents mentions weather -
/// and it was found in `ArcticSwimmerEasterEgg`, which watches `auto.is_snowing` for the
/// same condition the dialogue guards ask about.
///
/// # Panics
///
/// If the save records no area, or if its `Variable` table will not read.
fn scene_in_save(save: &str) -> Scene {
    let area = scene_state(save, "areaId")
        .as_str()
        .unwrap_or_else(|| panic!("{save} records no area to be in"))
        .to_string();

    let variables = variables_in_save(save);
    let says = |name: &str| matches!(variables.get(name), Some(WireValue::Bool { value: true }));

    Scene {
        outside: outdoor_scenes().contains(&area),
        area,
        raining: says(RAINING),
        snowing: says(SNOWING),
        weather: weather_of(save),
    }
}

/// What the save's weather preset is, by the table derived from the game's own assets.
///
/// # Panics
///
/// If the save records no preset, or records one the controller's list does not hold.
fn weather_of(save: &str) -> String {
    let preset = world_state(save, "weatherState")["weatherPreset"]
        .as_i64()
        .unwrap_or_else(|| panic!("{save} records no weather preset"));

    let types = weather_presets();
    usize::try_from(preset)
        .ok()
        .and_then(|at| types.get(at))
        .cloned()
        .unwrap_or_else(|| panic!("{save} is in weather preset {preset}, which the game has no"))
}

/// The weather each preset index is, read off the table derived from the game's own assets.
///
/// THE NAME IS NOT THE ANSWER: `RainClear_0` is CLEAR and `SnowClear_0` is SNOW. See
/// `tools/derive-weather-presets.py`, which resolves the controller's ordered array of
/// preset assets to the `type` each one carries.
fn weather_presets() -> &'static Vec<String> {
    static TYPES: OnceLock<Vec<String>> = OnceLock::new();
    TYPES.get_or_init(|| {
        let path = repo_root().join("testing").join("weather.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|why| panic!("{}: {why}", path.display()));
        let document: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|why| panic!("{}: {why}", path.display()));

        document["types"]
            .as_array()
            .unwrap_or_else(|| panic!("{} names no preset types", path.display()))
            .iter()
            .map(|kind| {
                kind.as_str()
                    .unwrap_or_else(|| panic!("{}: a preset type is not a name", path.display()))
                    .to_string()
            })
            .collect()
    })
}

/// One member of a save's second document, with the bases it rests on merged in.
///
/// WHERE EVERYTHING THE GAME ANSWERS FROM LIVES, and the reason an offline world was ever
/// thinner than the running one. The Lua parts carry the dialogue variables and the tables
/// that DEFINE items and thoughts; what the player is actually holding, has actually
/// thought, and has actually been asked to do is here, beside the character sheet that was
/// already being read from it.
///
/// # Panics
///
/// If the chain carries no such member. A world assembled from a member that is not there
/// would be a more permissive one than the run it stands for, silently.
fn world_state(save: &str, member_name: &str) -> serde_json::Value {
    /// The archive member the world state lives in, by the suffix the manifest names it with.
    const SECOND_BLOB: &str = ".2nd.ntwtf.json";

    document_member(save, SECOND_BLOB, member_name)
}

/// One member of a save's FIRST document, which is where the area the player is in lives.
///
/// # Panics
///
/// If the chain carries no such member, for the reason [`world_state`] gives.
fn scene_state(save: &str, member_name: &str) -> serde_json::Value {
    /// The archive member the area lives in.
    const FIRST_BLOB: &str = ".1st.ntwtf.json";

    document_member(save, FIRST_BLOB, member_name)
}

fn document_member(save: &str, suffix: &str, member_name: &str) -> serde_json::Value {
    let mut document = serde_json::Value::Null;
    for folder in chain(save) {
        let Some(member) = member(&folder, suffix) else {
            continue;
        };

        // A base writes the blob out whole and a diff writes only what it changes, the same
        // arrangement the Lua parts use - and the same reading: overlay them in order.
        let changes = member.get("_changes").unwrap_or(&member).clone();
        overlay(&mut document, &changes);
    }

    document
        .get(member_name)
        .cloned()
        .unwrap_or_else(|| panic!("{save}'s chain carries no {member_name}"))
}

/// Everything the plugin puts in a world snapshot that is not a variable or a check.
///
/// ## Why these are read rather than defaulted
///
/// THE PLUGIN ANSWERS ALL OF THEM FROM THE RUNNING GAME - `CheckItem`, `IsTaskActive`,
/// `IsTHCPresent`, the balance and the clock - and an offline world that answered none of
/// them was not a stricter world but a DIFFERENT one: an empty item set says "not held"
/// rather than "unknown", so a guard on not holding something opened a route the game
/// closes, and one on holding it closed a route the game opens. Measured on conversation
/// 29, whose group asks thirteen of these: two options carried a marker offline that the
/// game did not draw, and the Fail half of its white check landed on a different rung.
///
/// ## What each one means, and where the reading comes from
///
/// The names are the save's, and the meanings are what the game's own tables say rather
/// than what this code decides:
///
/// - PRESENT, for a thought, is any state but `UNKNOWN`. The cabinet lists every thought in
///   the game and marks the ones the player has reached - cooking, known, fixed, forgotten -
///   so presence is the absence of the one state that means "not yet".
/// - ACTIVE, for a task, is acquired and not resolved. The journal records when each was
///   taken and when each was closed, and a task closed at a time is no longer active; one
///   acquired with a null resolution is.
///
/// WHAT IS HELD IS STILL NOT ANSWERED, and the near miss is worth writing down because the
/// data looks like the answer and is not. `inventoryState.itemListState` names 206 items in
/// every save here, which is every item the database defines - it is the per-item state of
/// the catalogue, not what is in hand. Reading it as the inventory told the world the player
/// already owned the Faln speakers, which shut the route the money suite exists to measure
/// and turned its first scenario red. `CheckItem` therefore stays unanswered, which reads as
/// not held, exactly as it did before. See de-bnh6 for where to look next.
pub struct Holdings {
    /// Items in the player's possession, carried or worn.
    pub items: HashSet<String>,
    /// Items in a slot: what CheckEquipped answers about.
    pub equipped: HashSet<String>,
    /// Journal tasks taken and not yet closed.
    pub tasks: HashSet<String>,
    /// Thoughts the cabinet has reached, whatever state they are in.
    pub thoughts: HashSet<String>,
    /// What state each of those is in, which some guards ask after by name.
    pub thought_states: HashMap<String, String>,
    /// The balance, in centimes.
    pub money: i32,
    /// Minutes past midnight, and which day it is.
    pub day_minutes: i32,
    pub day_counter: i32,
    /// Where the player is standing, and what the weather is doing there.
    pub scene: Scene,
    /// Who is with them.
    pub party: Party,
}

/// Minutes in an hour, for a clock the game answers to the hour.
const MINUTES_PER_HOUR: i32 = 60;

/// The state a thought the player has never reached is in.
const NOT_YET: &str = "UNKNOWN";

/// The state a thought that is finished is in.
const FIXED: &str = "FIXED";

/// The state a thought still being thought is in.
const COOKING: &str = "COOKING";

impl Holdings {
    /// The world queries this can answer, out of the ones a group asks.
    ///
    /// ## Why some are answered and some are left alone
    ///
    /// A QUERY KEY IS THE LUA CALL ITSELF - `MoneyAmount()`, `IsTHCFixed("aces_high")` -
    /// and the plugin answers one by running it in the game. A save answers some of them
    /// exactly: the balance, the day, and what state a thought is in are all written down
    /// in it. Others are about the scene rather than the character - whether it is raining,
    /// whether this is outdoors, what is worn - and where this cannot answer one it says
    /// NOTHING, which is what the plugin sends for a query it could not run and what the
    /// engine treats as unknown. See de-bnh6 for where the rest of them live.
    pub fn answers_to(&self, asked: &[String]) -> HashMap<String, WireValue> {
        let mut answers = HashMap::new();
        for key in asked {
            let Some(answer) = self.answer(key) else {
                continue;
            };

            answers.insert(key.clone(), answer);
        }

        answers
    }

    /// One query, where this can answer it.
    fn answer(&self, key: &str) -> Option<WireValue> {
        let (call, argument) = split_call(key);

        match call {
            "MoneyAmount" => Some(WireValue::Number {
                value: f64::from(self.money),
            }),
            "DayCount" => Some(WireValue::Number {
                value: f64::from(self.day_counter),
            }),
            "IsTHCFixed" => Some(WireValue::Bool {
                value: self.thought_states.get(argument?).map(String::as_str) == Some(FIXED),
            }),
            "CheckEquipped" => Some(WireValue::Bool {
                value: self.equipped.contains(argument?),
            }),
            "IsTHCCookingOrFixed" => Some(WireValue::Bool {
                value: matches!(
                    self.thought_states.get(argument?).map(String::as_str),
                    Some(FIXED) | Some(COOKING)
                ),
            }),
            // THE THREE THAT ASK ABOUT THE SCENE. Each half of a complementary pair, so
            // leaving one unanswered opens both halves rather than losing a line - see
            // [`Scene`].
            "IsExterior" => Some(WireValue::Bool {
                value: self.scene.outside,
            }),
            "IsRaining" => Some(WireValue::Bool {
                value: self.scene.raining,
            }),
            "IsSnowing" => Some(WireValue::Bool {
                value: self.scene.snowing,
            }),
            // WHO IS WITH THE PLAYER, which the save keeps in its party state.
            "IsKimHere" => Some(WireValue::Bool {
                value: self.party.kim_here,
            }),
            "IsKimInParty" => Some(WireValue::Bool {
                value: self.party.kim_in_party,
            }),
            "IsCunoInParty" => Some(WireValue::Bool {
                value: self.party.cuno_in_party,
            }),
            _ => None,
        }
    }
}

/// A query key as the call it is: the name, and the one string argument where it has one.
fn split_call(key: &str) -> (&str, Option<&str>) {
    let Some((call, rest)) = key.split_once('(') else {
        return (key, None);
    };

    let argument = rest.trim_end_matches(')').trim_matches('"');
    (
        call,
        if argument.is_empty() {
            None
        } else {
            Some(argument)
        },
    )
}

/// What one save holds, for the world a scenario is answered against.
///
/// # Panics
///
/// If the save's chain carries no inventory, cabinet, journal, character or clock. Every
/// one of them is written by the game into every save.
pub fn holdings_in_save(save: &str) -> Holdings {
    let cabinet = world_state(save, "thoughtCabinetState");
    let journal = world_state(save, "aquiredJournalTasks");
    let character = world_state(save, "playerCharacter");
    let clock = world_state(save, "sunshineClockTimeHolder");

    let thought_states = thought_states(&cabinet);
    let carried = world_state(save, "inventoryState");
    let equipped = equipped(&carried);

    Holdings {
        // WORN COUNTS AS HELD, since an item in a slot is still the player's. What is NOT
        // in either is the catalogue: inventoryState.itemListState names every item the
        // database defines - 206 of them, in every save here - and reading THAT as the
        // inventory told the world the Faln speakers were already bought, which shut the
        // route the money suite exists to measure.
        items: held(&carried).union(&equipped).cloned().collect(),
        equipped,
        tasks: active_tasks(&journal),
        thoughts: thought_states
            .iter()
            .filter(|(_, state)| *state != NOT_YET)
            .map(|(name, _)| name.clone())
            .collect(),
        thought_states,
        money: whole(&character["Money"], save, "the balance"),
        // TO THE HOUR, because that is the resolution the game answers at: the plugin reads
        // the clock through Lua, where HourCount is as fine as it gets, and every guard in
        // the database compares hours or days. A save that knows the minute would otherwise
        // be staging a world the run it stands for cannot be in.
        day_minutes: whole(&clock["time"]["dayMinutes"], save, "the clock") / MINUTES_PER_HOUR
            * MINUTES_PER_HOUR,
        day_counter: whole(&clock["time"]["dayCounter"], save, "the day"),
        scene: scene_in_save(save),
        party: party_in_save(save),
    }
}

/// Who is with the player, as the save's `partyState` records it.
///
/// WRITTEN DOWN IN EVERY SAVE, beside the area, in the first blob. The game's `IsKimHere()`
/// and `IsKimInParty()` are `PartyManager` methods whose bodies are stripped from every
/// export, so which flags `IsKimHere` combines is read off the flags the save keeps beside
/// `isKimInParty`: Kim left waiting outside, away until morning, or asleep in his room is in
/// the party and not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Party {
    pub kim_in_party: bool,
    pub kim_here: bool,
    pub cuno_in_party: bool,
}

/// Reads [`Party`] out of a save.
///
/// # Panics
///
/// If the save's chain carries no party state, which the game writes into every save.
fn party_in_save(save: &str) -> Party {
    let party = scene_state(save, "partyState");
    let flag = |name: &str| {
        party[name]
            .as_bool()
            .unwrap_or_else(|| panic!("{save}'s party state has no {name}"))
    };

    let kim_in_party = flag("isKimInParty");
    Party {
        kim_in_party,
        kim_here: kim_in_party
            && !flag("isKimLeftOutside")
            && !flag("isKimAwayUpToMorning")
            && !flag("isKimSleepingInHisRoom"),
        cuno_in_party: flag("isCunoInParty"),
    }
}

/// The strings one field carries across a list of records.
fn named(holder: &serde_json::Value, list: &str, field: &str) -> HashSet<String> {
    holder[list]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|record| record[field].as_str().map(str::to_string))
        .collect()
}

/// What the player is carrying, by the inventory the game draws.
///
/// THE VIEW IS THE INVENTORY, and the list beside it is not. `inventoryViewState.inventory`
/// holds what is in hand, in categories - tools, clothes and the rest - where
/// `itemListState` is the per-item state of every item the database defines. Checked
/// against the game 2026-09-11: it answers CheckItem true for the flashlight and the suede
/// jacket and false for the deserter's gun and the Villiers, which is this set exactly.
fn held(inventory: &serde_json::Value) -> HashSet<String> {
    inventory["inventoryViewState"]["inventory"]
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(_, category)| category.as_array().into_iter().flatten())
        .filter_map(|entry| entry["Value"].as_str().map(str::to_string))
        .collect()
}

/// What the player has in a slot, which is what `CheckEquipped` answers about.
fn equipped(inventory: &serde_json::Value) -> HashSet<String> {
    inventory["inventoryViewState"]["equipment"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, item)| item.as_str().map(str::to_string))
        .collect()
}

/// What state the cabinet holds each thought in.
fn thought_states(cabinet: &serde_json::Value) -> HashMap<String, String> {
    cabinet["thoughtListState"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|thought| {
            Some((
                thought["name"].as_str()?.to_string(),
                thought["state"].as_str()?.to_string(),
            ))
        })
        .collect()
}

/// The tasks taken and not closed, with their subtasks.
///
/// SUBTASKS COUNT AS TASKS, because the journal keeps them under their parent and a guard
/// asks after either by the same name. A subtask's own resolution is not recorded
/// separately, so it is active while its parent is.
fn active_tasks(journal: &serde_json::Value) -> HashSet<String> {
    let resolved = |name: &String| {
        !journal["TaskResolutions"]
            .get(name)
            .is_none_or(serde_json::Value::is_null)
    };

    let mut active: HashSet<String> = journal["TaskAquisitions"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, _)| name.clone())
        .filter(|name| !resolved(name))
        .collect();

    for (parent, children) in journal["SubtaskAquisitions"]
        .as_object()
        .into_iter()
        .flatten()
    {
        if resolved(parent) {
            continue;
        }

        active.extend(
            children
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, _)| name.clone()),
        );
    }

    active
}

/// One of a save folder's top-level members, by the suffix it is named with.
fn member(folder: &Path, suffix: &str) -> Option<serde_json::Value> {
    let stem = folder.file_stem()?.to_str()?;
    read_json(&folder.join(format!("{stem}{suffix}")))
}

/// Applies `from` over `into`, key by key, all the way down.
///
/// Two objects merge; anything else replaces, which is what a diff over a list or a number
/// means. Recursive rather than a top-level merge because a diff carries only the leaves it
/// changed - one skill's value, not the skill - so a shallow merge would drop the rest of
/// whatever it touched.
fn overlay(into: &mut serde_json::Value, from: &serde_json::Value) {
    match (into.as_object_mut(), from.as_object()) {
        (Some(target), Some(source)) => {
            for (key, value) in source {
                match target.get_mut(key) {
                    Some(existing) => overlay(existing, value),
                    None => {
                        target.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        _ => *into = from.clone(),
    }
}
