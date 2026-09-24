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

/// The shared scenario definition, typed for the tests that execute it.
///
/// Its own module for the same reason `fixtures` is: two test binaries read the same rows,
/// and a second copy of the types is a second thing to keep agreeing with the file.
pub mod suites;

/// One suite scenario's world, staged from the fixtures the in-game run loads.
///
/// Its own module because the marker test and the scenario runner both stage scenarios, and
/// two stagings of "the same fixture" would drift apart.
pub mod staging;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use lookahead_engine::core::guard_value::{GuardValue, GuardValueKind};
use lookahead_engine::core::types::{DialogueNodeId, Ternary};
use lookahead_engine::world::ILookAheadWorld;

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
const CORPUS_COMMAND: [&str; 5] = ["run", "--project", "tools/DialogueExtract", "--", "corpus"];

/// What to run to rebuild the conversation index.
const INDEX_COMMAND: [&str; 5] = [
    "run",
    "--project",
    "tools/DialogueExtract",
    "--",
    "conversation-index",
];

/// What to run to rebuild the item table, which carries each item's display name.
const ITEM_NAMES_COMMAND: [&str; 5] = [
    "run",
    "--project",
    "tools/DialogueExtract",
    "--",
    "item-names",
];

/// The file that command writes.
const ITEM_NAMES_FILE: &str = "item_names.jsonl";

/// The item table, regenerated if absent; `None` only where the game data cannot be had.
///
/// WHAT IT IS FOR: answering `CheckItem` from a save. The game branches on an item's stack
/// name - the key pocket answers for one on the key ring, the bullet count for one stacked as
/// bullets, the bag and the equipment for anything else - and the pocket a save writes is
/// English display names rather than ids. See `fixtures::holdings_in_save`.
pub fn item_names() -> Option<PathBuf> {
    derived(ITEM_NAMES_FILE, &ITEM_NAMES_COMMAND, "the item table")
}

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
    println!(
        "{what} is missing; regenerating with: dotnet {}",
        args.join(" ")
    );
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
const SHIPPED_INDEX_COMMAND: [&str; 5] = [
    "run",
    "--project",
    "tools/DialogueExtract",
    "--",
    "shipped-index",
];

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
const VARIABLES_COMMAND: [&str; 5] = [
    "run",
    "--project",
    "tools/DialogueExtract",
    "--",
    "variables",
];

/// What to run to rebuild the actor table.
const ACTORS_COMMAND: [&str; 5] = ["run", "--project", "tools/DialogueExtract", "--", "actors"];

/// The database's actor table, regenerated if absent.
///
/// Who speaks a line, by the id its `Actor` field carries. What wants it is a passive
/// check: which skill one tests is decided by its speaker, and the id on its own says
/// nothing.
pub fn actors() -> Option<PathBuf> {
    derived("actors.jsonl", &ACTORS_COMMAND, "actors.jsonl")
}

/// Where the database's variable table is, regenerated if absent.
///
/// `None` where the game data cannot be had at all, which is what skips a fixture that
/// needs it rather than running it against a table declaring nothing.
pub fn variable_table_path() -> Option<PathBuf> {
    derived(
        lookahead_engine::index::VariableTable::FILE_NAME,
        &VARIABLES_COMMAND,
        lookahead_engine::index::VariableTable::FILE_NAME,
    )
}

/// The database's variable table, read once and shared.
///
/// Once, because every world built in a run wants the same 10,645 entries and reading
/// them per fixture would be the measurement measuring its own setup.
pub fn variable_table() -> Option<std::sync::Arc<lookahead_engine::index::VariableTable>> {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Option<std::sync::Arc<lookahead_engine::index::VariableTable>>> =
        OnceLock::new();

    TABLE
        .get_or_init(|| {
            lookahead_engine::index::VariableTable::read(&variable_table_path()?)
                .ok()
                .map(std::sync::Arc::new)
        })
        .clone()
}

/// The same table, for a caller that has already established the game data is there.
///
/// Panics rather than declaring nothing: a crawl against an empty table answers every
/// variable the world does not hold as false, which is not what the game does, so a suite
/// that ran anyway would be checking the wrong engine quietly. A caller that can legitimately
/// carry on without the data checks [`variable_table_path`] and skips.
pub fn declared() -> std::sync::Arc<lookahead_engine::index::VariableTable> {
    variable_table().expect("the database's variable table reads")
}

/// Where that table is, for a caller that opens an engine rather than building a world.
pub fn declared_path() -> PathBuf {
    variable_table_path().expect("the database's variable table is there")
}

/// A table declaring nothing, for a world built to answer one mechanism's questions.
///
/// SAID OUT LOUD rather than defaulted to, which is the whole point of there being no
/// default table: a fixture that names the four variables its subject reads leaves every
/// other one to the table, and the database's initials would decide guards the fixture
/// never meant to arrange. Declaring nothing keeps those guards where the fixture put them.
pub fn nothing_declared() -> std::sync::Arc<lookahead_engine::index::VariableTable> {
    std::sync::Arc::new(lookahead_engine::index::VariableTable::empty())
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
/// `GameWorld` answers UNKNOWN for anything it has not been told, which is the safe
/// answer for a search and the useless one for a measurement: every guard mentioning an
/// unset variable becomes undecidable, and most of the database's variables are unset for
/// most of a playthrough. A save answers. An unset Lua variable is nil and nil is falsy,
/// so a variable nobody has written reads FALSE, and the facts a save settles - who is in
/// the party, what is worn, what is in the thought cabinet - are simply known.
///
/// None of it can be changed by a search, which is what makes one answer good for the
/// whole walk.
pub struct SaveWorld {
    /// What every variable this world was not told about reads.
    ///
    /// The database's table names all 10,645, of which 142 are numbers - and a counter has to
    /// answer as a NUMBER rather than as false, or every ordering comparison over it is
    /// undecidable. See de-sze.5.4.
    declared: std::sync::Arc<dyn lookahead_engine::world::IVariableTable>,
    /// What the character is wearing, by `CheckEquipped` name.
    equipped: HashSet<String>,
    /// Thoughts in the cabinet, by `IsTHCPresent` name.
    ///
    /// The widest of the three sets and the only one a search can add to. The game keeps
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

impl SaveWorld {
    /// Midday on the first day, nothing worn, nothing internalised, no money.
    ///
    /// NO `Default`, because there is no table to default to: what an unanswered variable
    /// reads is the caller's to supply. See [`lookahead_engine::world::IVariableTable`].
    pub fn declaring(
        declared: std::sync::Arc<dyn lookahead_engine::world::IVariableTable>,
    ) -> Self {
        Self {
            declared,
            equipped: HashSet::new(),
            gained: HashSet::new(),
            cooking: HashSet::new(),
            fixed: HashSet::new(),
            money: 0,
            day_minutes: 12 * 60,
            day_counter: 1,
        }
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

    /// LOCKED, as the plugin sends it and as `save_world` builds it.
    ///
    /// A measurement is worth having because it measures what ships, and what ships is a
    /// crawl whose clock does not move: the plugin locks it whatever the game reads, so a
    /// world here that left it unlocked would carry a clock register no player's search
    /// carries and move time no player's search moves. See de-gh1o for the lock itself.
    fn is_clock_locked(&self) -> bool {
        true
    }

    /// Every roll is left free to succeed. This world does not read what the save's thoughts
    /// do, and letting a red check pass is the permissive answer.
    fn red_check_may_pass(&self, _node: lookahead_engine::core::types::DialogueNodeId) -> bool {
        true
    }

    fn get_variable(&self, variable: lookahead_engine::core::state::VariableRef<'_>) -> GuardValue {
        // THIS WORLD IS TOLD NO VARIABLES AT ALL, so every one of them is the table's to
        // answer. The declared type is the whole of de-sze.5.4: a counter answered as a
        // boolean makes every ordering comparison over it undecidable, and there is no way
        // to tell a counter from a flag by looking at its name.
        self.declared.unset(variable.name())
    }

    fn initially_has_item(&self, _name: &str) -> bool {
        false
    }

    /// What the save says is in the thought cabinet.
    ///
    /// Answered here rather than in [`Self::query`] because `IsTHCPresent` is now
    /// slot-backed: `BoundContext::query` and the guard compiler both ask this for a
    /// thought the group does not gain, and ask the search's own state for one it does.
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
            // Profile state no save records; see `fixtures::HARDCORE_PLAYTHROUGH_COMPLETED`.
            lookahead_engine::core::game_mode::WAS_GAME_BEATEN_IN_HARDCORE_MODE => {
                GuardValue::from_boolean(fixtures::HARDCORE_PLAYTHROUGH_COMPLETED)
            }
            "IsCunoInParty" => GuardValue::from_boolean(false),
            "CheckEquipped" => membership(&self.equipped),
            "IsTHCCooking" => membership(&self.cooking),
            "IsTHCFixed" => membership(&self.fixed),
            // Cooking or fixed, which is a NARROWER question than IsTHCPresent - that one
            // asks whether the thought is in the cabinet at all, is answered from
            // `initially_has_thought` because a search can change it, and used to be
            // answered here as cooking-or-fixed. That was this function's semantics given
            // to that one's name: `THCLuaFunctions.IsTHCCookingOrFixed` is the cooking
            // fallthrough to fixed, while `IsTHCPresent` is `gainedThoughts.Contains`.
            // Read that way, a thought the player had gained but not internalised
            // answered false, which is the opposite of what the game says.
            "IsTHCCookingOrFixed" => Self::subject(arguments)
                .map(|s| {
                    GuardValue::from_boolean(self.cooking.contains(s) || self.fixed.contains(s))
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
    SaveWorld::declaring(declared())
        // Worn from the first morning, and the guards ask about it more than anything
        // else worn - six of the eight CheckEquipped calls in conversation 631's group.
        .wearing("neck_tie")
}

/// The entry the MOST OTHER ENTRIES can reach, ties broken by id.
///
/// WHAT A MEASUREMENT ASKS ABOUT WHEN IT WANTS THE BIGGEST SEARCH THERE IS, and the
/// definition matters more than it looks. A backward pass visits exactly the entries that
/// can reach its target - `Backward::can_reach` bounds the fixed point by them - so what
/// makes a pass big is how many ancestors the target has, and nothing else.
///
/// DEPTH IS THE WRONG PROXY FOR THAT, which is worth stating because it is the obvious one
/// and it is backwards. The entry furthest from the start sits at the end of a thin tail
/// and typically has FEWER ancestors than a hub half way in; measured on conversation 14, a
/// pass to the deepest entry settled holding 32 diagram nodes, which is a measurement of
/// nothing. Counting ancestors asks the question directly.
///
/// Guards ignored, so it is a structural count rather than a reachable one - which is what
/// a workload wants: an entry a guard happens to shut is still one the pass has to walk the
/// graph to refuse. Ties are broken by id so that a row is the same search every time: a
/// measurement that picked a different target per run would report a different number per
/// run, and a real change would look like noise.
pub fn heaviest_target(
    graph: &lookahead_engine::graph::LookAheadGraph,
    start: DialogueNodeId,
) -> DialogueNodeId {
    use std::collections::{HashMap, HashSet, VecDeque};

    // Every entry's incoming links, which is the direction ancestors are counted along.
    let mut parents: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
    for node in graph.nodes() {
        for &child in &node.links {
            if graph.get(child).is_some() {
                parents.entry(child).or_default().push(node.id);
            }
        }
    }

    let ancestors = |target: DialogueNodeId| -> usize {
        let mut seen: HashSet<DialogueNodeId> = HashSet::from([target]);
        let mut queue: VecDeque<DialogueNodeId> = VecDeque::from([target]);
        while let Some(id) = queue.pop_front() {
            for &parent in parents.get(&id).into_iter().flatten() {
                if seen.insert(parent) {
                    queue.push_back(parent);
                }
            }
        }
        seen.len()
    };

    graph
        .nodes()
        .map(|node| node.id)
        .max_by_key(|id| (ancestors(*id), id.conversation_id, id.entry_id))
        .unwrap_or(start)
}

/// The variable asking the committed-save tests to check every save, not only changed ones.
///
/// ## WHY THIS ONE IS STILL A VARIABLE - de-3dx9
///
/// Every other option in this project is a CLI argument, because an argument documents itself
/// and `--help` is never stale. This one cannot be:
///
/// - CARGO OWNS THE COMMAND LINE. A test binary is handed libtest's arguments - filters,
///   `--nocapture`, `--test-threads` - and taking one of its own means fighting libtest for it
///   or building a custom harness, which is a great deal of machinery for one flag.
/// - IT WIDENS RATHER THAN SELECTS, so `#[ignore]` and cargo's own `--ignored` do not fit
///   either: it does not choose which tests run, it changes what THREE of them check, and an
///   ignored twin of each would be three more test functions saying the same thing.
///
/// WHAT IT IS FOR, given the automatic triggers below already cover the cases that matter: it
/// forces the exhaustive pass when somebody wants it anyway - before a release, or when they
/// distrust the git detection. That is a real want and a rare one, which is the right shape for
/// a variable rather than an argument.
///
/// SPELLED OUT AT THE CALL SITE rather than held in a const, so that `tools/survey-env.py` and
/// `tests/environment_table.rs` both find it by the rule they are built on. Held in a const, the
/// only thing that put it in the table was the prose beside it - and a variable whose row
/// survives because of a sentence is one that leaves the table silently when the sentence is
/// edited.

/// Where the save reader and writer live: a change here can break any save, changed or not.
const SAVE_CODE: [&str; 2] = ["crates/gct-save-files", "crates/gct-formats"];

/// The committed saves worth checking this run, out of `saves`.
///
/// ## Why not every one
///
/// The committed-save tests re-read and re-write every save under `testing/`, which is most
/// of a full `cargo test` - about two minutes of it. A save that has not changed since the last
/// commit was checked when it was committed, so checking it again proves nothing new.
///
/// ## What is checked
///
/// Every save with a file changed since `HEAD` - tracked or untracked - and every save BUILT ON
/// one, since a save written as a change reads its base. ALL of them where:
///
/// - `DEGCT_ALL_COMMITTED_SAVES` is set;
/// - the save reader or writer has changed, which is what these tests exist to hold to the
///   committed bytes;
/// - git cannot be asked, because skipping on no evidence would hide a broken save.
///
/// Says which it did, so a run that checked nothing is visibly one that checked nothing.
pub fn committed_saves_to_check(saves: Vec<PathBuf>) -> Vec<PathBuf> {
    let every = |why: &str| {
        println!("checking all {} committed saves: {why}", saves.len());
        saves.clone()
    };
    if lookahead_engine::core::env::is_set("ALL_COMMITTED_SAVES") {
        return every(&format!(
            "{} is set",
            lookahead_engine::core::env::qualified("ALL_COMMITTED_SAVES")
        ));
    }

    let root = repo_root();
    let asked = Command::new("git")
        .current_dir(&root)
        .args(["status", "--porcelain", "--untracked-files=all", "--"])
        .arg("testing")
        .args(SAVE_CODE)
        .output();
    let changed: Vec<PathBuf> = match asked {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.get(3..))
            // A rename reads "old -> new"; both ends have changed.
            .flat_map(|paths| paths.split(" -> ").map(str::to_string).collect::<Vec<_>>())
            .map(|path| root.join(path.trim_matches('"')))
            .collect(),
        _ => return every("git could not say what changed"),
    };

    if changed.iter().any(|path| {
        SAVE_CODE
            .iter()
            .any(|code| path.starts_with(root.join(code)))
    }) {
        return every("the save reader or writer has changed");
    }

    let canonical =
        |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let touched: Vec<PathBuf> = saves
        .iter()
        .filter(|save| changed.iter().any(|path| path.starts_with(save.as_path())))
        .map(|save| canonical(save))
        .collect();

    let selected: Vec<PathBuf> = saves
        .iter()
        .filter(|save| {
            match lookahead_engine::formats::expanded_save::chain(
                &lookahead_engine::formats::expanded_save::OnDisk,
                save,
            ) {
                Ok(links) => links
                    .iter()
                    .any(|link| touched.contains(&canonical(&link.directory))),
                // A save whose chain will not even read is checked, so the test says why.
                Err(_) => true,
            }
        })
        .cloned()
        .collect();

    println!(
        "checking {} of {} committed saves - those changed since HEAD and those built on them; \
         set {} to check all",
        selected.len(),
        saves.len(),
        lookahead_engine::core::env::qualified("ALL_COMMITTED_SAVES"),
    );
    selected
}

/// What the guard compiler a request builds does with some entries' guards.
pub struct CompiledGuards {
    /// How many reputation questions it answered from the world, because no search from the
    /// request's starts could change their winner.
    pub reputation_from_world: usize,
    /// How many guards it could not decide.
    pub fallbacks: usize,
}

/// Compiles `entries`' guards with the compiler `bridge::answer` builds for `request`: the
/// group fitted to the world, the layout narrowed to the request, and the menu trimmed and its
/// reputation ranges settled from the request's starts.
pub fn compiled_guards(
    index: &lookahead_engine::index::Index,
    request: &lookahead_engine::bridge::LookAheadRequest,
    entries: &[DialogueNodeId],
) -> CompiledGuards {
    use lookahead_engine::bridge::{
        COUNTER_CAP, GameWorld, entered_at_of, questions_of, starts_of,
    };
    use lookahead_engine::symbolic::data_layout::DataLayout;
    use lookahead_engine::symbolic::guard_formula::GuardCompiler;
    use lookahead_engine::symbolic::vars::DataVars;

    let (mut graph, group) =
        lookahead_engine::index::build_group_graph(index, request.conversation)
            .expect("the request's group builds");
    let mut world = GameWorld::declaring_nothing(request.world.clone());
    world
        .resolve(&questions_of(&graph, group))
        .expect("the world answers the group's questions");
    graph.fit(&lookahead_engine::graph::Fitting::read(&graph, &world));

    let symbols = graph.symbols().clone();
    let layout = DataLayout::for_group_entered_at(
        &graph,
        &world,
        COUNTER_CAP,
        Some(&entered_at_of(request)),
    );
    let vars = DataVars::try_new(&layout, &symbols, request.diagram_budget())
        .expect("the manager fits the budget");
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(&world)
        .with_constant_clock(DataLayout::group_passes_time(&graph));
    // THE MENU AS `answer_starts` PREPARES IT, which compiles guards of its own and settles the
    // reputation ranges; only what the entries below add is counted.
    let (trimmed, _) =
        lookahead_engine::bridge::walkable_menu(&graph, &mut compiler, &starts_of(request));
    let (from_world, fallbacks) = (compiler.reputation_from_world(), compiler.fallbacks());

    for &entry in entries {
        let node = trimmed
            .graph
            .get(entry)
            .unwrap_or_else(|| panic!("{entry:?} is not in the group"));
        compiler.compile_for(entry, &node.guard);
    }
    CompiledGuards {
        reputation_from_world: compiler.reputation_from_world() - from_world,
        fallbacks: compiler.fallbacks() - fallbacks,
    }
}
