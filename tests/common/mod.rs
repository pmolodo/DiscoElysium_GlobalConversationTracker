// SPDX-License-Identifier: MIT
#![allow(dead_code)] // Each test binary uses a different part of this.

//! Finding the extracted game data these tests run on, and REGENERATING it when it is
//! missing.
//!
//! ## Why not just skip
//!
//! The corpus is extracted game content and is not committed, so a test that needs it has
//! to cope with its absence. The obvious answer - pass silently - is the wrong one: a
//! corpus that quietly stops being generated turns every test that depends on it into a
//! test that always passes, which is worse than having no test, because it still reads as
//! green. That is how a whole class of coverage disappears without anyone deciding to
//! drop it.
//!
//! So the order here is: use it, else build it, else say loudly why it cannot be built.
//!
//! ## The one case that is still allowed to skip
//!
//! Building the corpus needs the exported dialogue database, which is many gigabytes of
//! AssetRipper output from a real game install. A checkout on a machine without the game
//! cannot produce it by any means, and failing there would mean the suite could only ever
//! be run by someone with the game.
//!
//! That case skips - and says so in terms nobody will mistake for success. Every other
//! failure, including the extractor running and not producing the file, is a hard error.

/// Reading the committed scenario fixtures - the staged global state, and what a scenario
/// save has already displayed.
///
/// Its own module because more than one test binary assembles the same world now, and the
/// two copies of "what has this save read" were the beginning of exactly the drift the
/// shared scenario definition exists to prevent.
pub mod fixtures;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use lookahead_engine::core::guard_value::{GuardValue, GuardValueKind};
use lookahead_engine::core::types::{DialogueNodeId, Ternary};
use lookahead_engine::world::world::ILookAheadWorld;

/// Serialises regeneration across the tests in one binary.
///
/// Cargo runs the tests within a binary IN PARALLEL, so without this every test that
/// finds the corpus missing launches its own `dotnet run` at the same moment, and they
/// collide over the build output - `Cannot open DialogueAsset.dll for writing, being used
/// by another process`. Observed, not anticipated: four corpus tests raced on the first
/// run after the file was deleted.
///
/// Holding the lock is not enough on its own; the file is re-checked after acquiring it,
/// because by then another test has usually just built it.
static REGENERATION: Mutex<()> = Mutex::new(());

/// Where the extractor writes, relative to the repo root.
const DERIVED: &str = ".game_reference_copies/derived";

/// The exported database the extractor reads, relative to the repo root.
const SOURCE_ASSET: &str = ".game_reference_copies/AssetRipperExport/ExportedProject/Assets/\
Dialogue Databases/Disco Elysium.asset";

/// What to run to rebuild the corpus files.
const CORPUS_COMMAND: [&str; 5] =
    ["run", "--project", "tools/DialogueExtract", "--", "corpus"];

/// What to run to rebuild the conversation index.
const INDEX_COMMAND: [&str; 5] =
    ["run", "--project", "tools/DialogueExtract", "--", "conversation-index"];

/// The repo root, found by walking up from this crate.
pub fn repo_root() -> PathBuf {
    let mut dir: Option<&Path> = Some(Path::new(env!("CARGO_MANIFEST_DIR")));
    while let Some(d) = dir {
        if d.join(".game_reference_copies").exists() || d.join(".git").exists() {
            return d.to_path_buf();
        }
        dir = d.parent();
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Whether the exported database the extractor reads is present.
///
/// The one thing no amount of running things can conjure up.
fn source_asset_present(root: &Path) -> bool {
    root.join(SOURCE_ASSET).exists()
}

/// Runs the extractor, and fails loudly if it does not succeed.
fn extract(root: &Path, args: &[&str], what: &str) {
    println!("{what} is missing; regenerating with: dotnet {}", args.join(" "));
    let output = Command::new("dotnet")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| panic!("could not run dotnet to regenerate {what}: {e}"));

    if !output.status.success() {
        panic!(
            "regenerating {what} failed ({}).\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

/// A derived file, regenerated if absent; `None` only when the game data cannot be had.
///
/// `args` is the extractor invocation that would produce it.
fn derived(file_name: &str, args: &[&str], what: &str) -> Option<PathBuf> {
    let root = repo_root();
    let path = root.join(DERIVED).join(file_name);
    if path.exists() {
        return Some(path);
    }

    // One regeneration at a time. A poisoned lock is not a reason to give up - it only
    // means some other test panicked while holding it, which says nothing about whether
    // the file can be built.
    let _guard = REGENERATION.lock().unwrap_or_else(|e| e.into_inner());

    // Another test may have built it while this one waited.
    if path.exists() {
        return Some(path);
    }

    if !source_asset_present(&root) {
        // Deliberately shouty. This is the only path that lets a test pass without
        // having tested anything, and it should never be mistaken for a clean run.
        println!(
            "\n!! SKIPPING: {what} is missing and cannot be regenerated.\n\
             !! The exported dialogue database is not present at:\n\
             !!   {}\n\
             !! Nothing was tested. This needs a game install and an AssetRipper export.\n",
            root.join(SOURCE_ASSET).display(),
        );
        return None;
    }

    extract(&root, args, what);

    if !path.exists() {
        panic!(
            "regenerating {what} reported success but did not produce {}",
            path.display()
        );
    }

    Some(path)
}

/// One of the two corpus files, regenerated if absent.
pub fn corpus_file(file_name: &str) -> Option<PathBuf> {
    derived(file_name, &CORPUS_COMMAND, file_name)
}

/// The conversation index, regenerated if absent.
pub fn conversation_index() -> Option<PathBuf> {
    derived(
        "conversation_index.jsonl",
        &INDEX_COMMAND,
        "conversation_index.jsonl",
    )
}

/// What to run to rebuild the trimmed index the mod ships.
const SHIPPED_INDEX_COMMAND: [&str; 5] =
    ["run", "--project", "tools/DialogueExtract", "--", "shipped-index"];

/// The index as the mod ships it, regenerated if absent.
///
/// Depends on the full index, which `shipped-index` reads rather than re-scanning the
/// asset - so this asks for that one first, and a checkout with neither builds both in
/// order.
pub fn shipped_index() -> Option<PathBuf> {
    conversation_index()?;
    derived(
        "conversation_index.trimmed.jsonl",
        &SHIPPED_INDEX_COMMAND,
        "conversation_index.trimmed.jsonl",
    )
}

/// What to run to rebuild the variable table.
const VARIABLES_COMMAND: [&str; 5] =
    ["run", "--project", "tools/DialogueExtract", "--", "variables"];

/// The database's variable table, read once and shared.
///
/// Once, because every world built in a run wants the same 10,645 entries and reading
/// them per fixture would be the measurement measuring its own setup. `None` where the
/// game data cannot be had, in which case a world falls back to the old guess - which is
/// what it had always done, so nothing gets worse where the table is missing.
fn variable_table() -> Option<std::sync::Arc<lookahead_engine::index::VariableTable>> {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Option<std::sync::Arc<lookahead_engine::index::VariableTable>>> =
        OnceLock::new();

    TABLE
        .get_or_init(|| {
            let path = derived(
                lookahead_engine::index::VariableTable::FILE_NAME,
                &VARIABLES_COMMAND,
                lookahead_engine::index::VariableTable::FILE_NAME,
            )?;
            lookahead_engine::index::VariableTable::read(&path).ok().map(std::sync::Arc::new)
        })
        .clone()
}

/// A world shaped like a real save, for measuring the guard corpus against.
///
/// Shared rather than written twice. Two measurements are only comparable if they run
/// against the same world, and this was duplicated verbatim in `guard_coverage` and
/// `modelling_gaps` with a comment in each saying so - which is a convention, not a
/// guarantee.
///
/// ## What makes it save-shaped rather than test-shaped
///
/// `TestWorld` answers UNKNOWN for anything it has not been told, which is the safe
/// answer for a crawl and the useless one for a measurement: every guard mentioning an
/// unset variable becomes undecidable, and most of the database's variables are unset for
/// most of a playthrough. A save answers. An unset Lua variable is nil and nil is falsy,
/// so a variable nobody has written reads FALSE, and the facts a save settles - who is in
/// the party, what is worn, what is in the thought cabinet - are simply known.
///
/// None of it can be changed by a crawl, which is what makes one answer good for the
/// whole walk.
pub struct SaveWorld {
    /// Counter variables, which must answer as NUMBERS rather than as false.
    ///
    /// The one place the blanket "unset reads false" rule gives a wrong-shaped answer.
    /// A guard like `Variable["jam.jammystery_lorrymans_questioned"] >= 3` compares a
    /// counter, and in the game these exist as numbers initialised to zero; answering
    /// boolean false makes the comparison undecidable, because `try_as_number` gives
    /// nothing for a boolean and `GuardExpression::evaluate` gives up in exactly the same
    /// way. The engine and the compiler agree - they are both just being told the wrong
    /// thing.
    ///
    /// Answering NUMBER ZERO for everything instead is not the fix, and would be a far
    /// worse bug. `GuardValue::equals` is kind-sensitive, so a number never equals a
    /// boolean, and 5,994 of the 13,059 distinct guards in the database end in
    /// `== false`. Every one of them would start answering false.
    ///
    /// The real answer is the declared type, which the dialogue database has and the
    /// extracted index does not yet carry - see de-sze.5.4. Until then a measurement
    /// names the counters it needs, which is honest as long as it is understood as a
    /// fixture rather than as a model.
    numeric: HashSet<String>,
    /// What the database declares its variables to be, where it has been extracted.
    ///
    /// Supersedes [`SaveWorld::numeric`], which was the same idea done by hand: a
    /// measurement had to name each counter it needed and be wrong about the rest. The
    /// table names all 10,645, of which 142 are numbers.
    declared: Option<std::sync::Arc<lookahead_engine::index::VariableTable>>,
    /// What the character is wearing, by `CheckEquipped` name.
    equipped: HashSet<String>,
    /// Thoughts in the cabinet, by `IsTHCPresent` name.
    ///
    /// The widest of the three sets and the only one a crawl can add to. The game keeps
    /// `gainedThoughts` apart from the cooking and fixed effects, and internalising a
    /// thought never leaves that set - so anything cooking or fixed is present too, which
    /// [`SaveWorld::cooking`] and [`SaveWorld::internalised`] maintain rather than leave
    /// to the caller to remember.
    gained: HashSet<String>,
    /// Thoughts being internalised, by `IsTHCCooking` name.
    cooking: HashSet<String>,
    /// Thoughts already internalised, by `IsTHCFixed` name.
    fixed: HashSet<String>,
    money: i32,
    day_minutes: i32,
    day_counter: i32,
}

impl Default for SaveWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl SaveWorld {
    /// Midday on the first day, nothing worn, nothing internalised, no money.
    pub fn new() -> Self {
        Self {
            numeric: HashSet::new(),
            declared: variable_table(),
            equipped: HashSet::new(),
            gained: HashSet::new(),
            cooking: HashSet::new(),
            fixed: HashSet::new(),
            money: 0,
            day_minutes: 12 * 60,
            day_counter: 1,
        }
    }

    /// Names a variable the save holds as a NUMBER, so an ordering comparison can read it.
    pub fn with_counter(mut self, name: &str) -> Self {
        self.numeric.insert(name.to_string());
        self
    }

    pub fn wearing(mut self, name: &str) -> Self {
        self.equipped.insert(name.to_string());
        self
    }

    /// A thought in the cabinet, not internalised.
    pub fn gained(mut self, name: &str) -> Self {
        self.gained.insert(name.to_string());
        self
    }

    pub fn cooking(mut self, name: &str) -> Self {
        self.cooking.insert(name.to_string());
        self.gained(name)
    }

    pub fn internalised(mut self, name: &str) -> Self {
        self.fixed.insert(name.to_string());
        self.gained(name)
    }

    pub fn with_money(mut self, centimes: i32) -> Self {
        self.money = centimes;
        self
    }

    pub fn at(mut self, day: i32, hour: i32) -> Self {
        self.day_counter = day;
        self.day_minutes = hour * 60;
        self
    }

    /// The single text argument a query names its subject with.
    fn subject(arguments: &[GuardValue]) -> Option<&str> {
        match arguments {
            [value] if value.kind() == GuardValueKind::Text => Some(value.text()),
            _ => None,
        }
    }
}

impl ILookAheadWorld for SaveWorld {
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
        false
    }

    fn get_variable(&self, name: &str) -> GuardValue {
        // The declared type first, where the database has one. That is the whole of
        // de-sze.5.4: a counter answered as a boolean makes every ordering comparison over
        // it undecidable, and there is no way to tell a counter from a flag by looking at
        // its name.
        if let Some(declared) = self.declared.as_ref().and_then(|table| table.initial(name)) {
            return declared.clone();
        }

        if self.numeric.contains(name) {
            GuardValue::from_number(0.0)
        } else {
            // Unset reads false, which is what the game does - an unset Lua variable is
            // nil and nil is falsy. Reached now only for a name the database does not
            // declare at all.
            GuardValue::from_boolean(false)
        }
    }

    fn initially_has_item(&self, _name: &str) -> bool {
        false
    }

    fn initially_task_active(&self, _name: &str) -> bool {
        false
    }

    /// What the save says is in the thought cabinet.
    ///
    /// Answered here rather than in [`Self::query`] because `IsTHCPresent` is now
    /// slot-backed: `BoundContext::query` and the guard compiler both ask this for a
    /// thought the group does not gain, and ask the crawl's own state for one it does.
    fn initially_has_thought(&self, name: &str) -> bool {
        self.gained.contains(name)
    }

    /// The facts a save settles. Everything else stays unknown, and says so by falling
    /// back rather than by guessing.
    fn query(&self, name: &str, arguments: &[GuardValue]) -> GuardValue {
        let membership = |set: &HashSet<String>| {
            Self::subject(arguments)
                .map(|s| GuardValue::from_boolean(set.contains(s)))
                .unwrap_or_else(GuardValue::unknown)
        };

        match name {
            "IsKimHere" | "IsKimInParty" => GuardValue::from_boolean(true),
            "IsCunoInParty" => GuardValue::from_boolean(false),
            "CheckEquipped" => membership(&self.equipped),
            "IsTHCCooking" => membership(&self.cooking),
            "IsTHCFixed" => membership(&self.fixed),
            // Cooking or fixed, which is a NARROWER question than IsTHCPresent - that one
            // asks whether the thought is in the cabinet at all, is answered from
            // `initially_has_thought` because a crawl can change it, and used to be
            // answered here as cooking-or-fixed. That was this function's semantics given
            // to that one's name: `THCLuaFunctions.IsTHCCookingOrFixed` is the cooking
            // fallthrough to fixed, while `IsTHCPresent` is `gainedThoughts.Contains`.
            // Read that way, a thought the player had gained but not internalised
            // answered false, which is the opposite of what the game says.
            "IsTHCCookingOrFixed" => Self::subject(arguments)
                .map(|s| {
                    GuardValue::from_boolean(
                        self.cooking.contains(s) || self.fixed.contains(s),
                    )
                })
                .unwrap_or_else(GuardValue::unknown),
            _ => GuardValue::unknown(),
        }
    }

    fn check_passes(&self, _node: DialogueNodeId) -> Ternary {
        Ternary::Unknown
    }

    fn is_seen(&self, _node: DialogueNodeId) -> bool {
        false
    }
}

/// The one save every corpus measurement is taken against.
///
/// A function rather than a constant so the measurements cannot drift apart by
/// configuring their own; the numbers they print are only comparable against one world.
///
/// Every fact here is a CHOICE, and a different save would give different figures. What
/// it is not is a guess dressed as a model: the point is that a real save answers these
/// questions, and a world that refuses them makes guards look undecidable when the only
/// undecided thing is the fixture.
pub fn measurement_save() -> SaveWorld {
    SaveWorld::new()
        // Worn from the first morning, and the guards ask about it more than anything
        // else worn - six of the eight CheckEquipped calls in conversation 631's group.
        .wearing("neck_tie")
        // Counters the guards compare with an ordering operator. Named one at a time
        // because nothing yet carries the declared type that would make this automatic -
        // see de-sze.5.4, which is the real fix.
        .with_counter("jam.jammystery_lorrymans_questioned")
        .with_counter("pier.joyce_lorry_reporting_counter")
}
