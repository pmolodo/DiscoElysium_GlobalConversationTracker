// SPDX-License-Identifier: MIT
//! What crosses between the plugin and this engine, and what it means.
//!
//! [`crate::host`] is the shell - frames, processes, the pipe. This is the part worth
//! reading: the question the plugin asks, the snapshot of the world it asks it against,
//! and the answer that comes back. All of it ordinary Rust, so all of it testable without
//! starting anything.
//!
//! ## Two calls, and why not one
//!
//! THE ENGINE ASKS THE QUESTIONS. [`questions_for`] walks a group's parsed guards and
//! returns every question a search over it can ask - by the exact key the answer must come
//! back under. The plugin then answers those keys and hands them over in
//! [`LookAheadRequest`].
//!
//! The alternative was for the plugin to build the keys itself, and it is worse in a way
//! that would not show up until it mattered: the two sides would have to render
//! `CheckEquipped("neck_tie")` identically, forever, including how a number is formatted
//! and how a string is escaped. One disagreement and the answer silently goes missing,
//! the query reads Unknown, the guard turns permissive, and the marker is wrong in a way
//! nothing reports. Having the engine name its own keys removes the possibility.
//!
//! It costs one extra call per conversation group, and the questions do not change while
//! the game is running, so the plugin can ask once and keep them.
//!
//! ## The world crosses as a snapshot
//!
//! Not as callbacks. The C# already treats it that way - `GameLookAheadWorld` is built
//! per response menu and caches each query for the life of the search - so nothing is lost,
//! and what is gained is that no Rust frame ever calls back into managed code.
//!
//! Anything the plugin does not answer reads UNKNOWN, which is the permissive direction:
//! an unanswered question widens the reachable set rather than narrowing it, costing a
//! wasted click instead of hiding content the player has never seen. That is what the
//! managed engine does with an unanswerable query, and it must stay true here.
//!
//! The one exception is a DIALOGUE VARIABLE the database declares, because such a variable
//! is not unanswerable - a variable nobody has written is at the value the database gives
//! it. Where the variable table has been deployed, that value is the fallback; see
//! [`SnapshotWorld`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::core::guard::GuardExpression;
use crate::core::guard_value::{GuardValue, GuardValueKind};
use crate::core::types::{DialogueCheckKind, DialogueNodeId, Novelty, Ternary};
use crate::core::types::StartBranch;
use crate::symbolic::budget::DiagramBudget;
use crate::symbolic::data_layout::DataLayout;
use crate::symbolic::guard_formula::GuardCompiler;
use crate::symbolic::isolated;
use crate::symbolic::known::GroupShape;
use crate::symbolic::novelty_search;
use crate::symbolic::portfolio;
use crate::symbolic::reachability::seed_of;
use crate::symbolic::vars::DataVars;
use oxidd::bdd::BDDFunction;
use crate::graph::graph::LookAheadGraph;
use crate::index::{build_group_graph, Index, VariableTable};
use crate::world::world::ILookAheadWorld;

/// One entry, as it crosses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeRef {
    pub conversation: i32,
    pub entry: i32,
}

impl From<DialogueNodeId> for NodeRef {
    fn from(id: DialogueNodeId) -> Self {
        Self { conversation: id.conversation_id, entry: id.entry_id }
    }
}

impl From<NodeRef> for DialogueNodeId {
    fn from(node: NodeRef) -> Self {
        DialogueNodeId::new(node.conversation, node.entry)
    }
}

/// The run separator inside a [`NodeSet`]'s entry list.
///
/// ## The same one every file in this repository uses
///
/// `3,5,7-25`, which is what `SparseOrder` writes on the C# side - the sparse saves, their
/// diffs, and the global state file's entry sets since format 4. The wire is the last
/// thing here that spelled a run its own way, and it had one implementation of its own on
/// each side of the bridge: three encoders for one idea.
///
/// IT USED TO BE `..`, on the stated grounds that a hyphen becomes ambiguous the first
/// time an id is negative. That reasoning does not survive the other implementation, which
/// has always looked for the separator PAST THE FIRST CHARACTER so a leading `-` reads as
/// a sign - one condition, and the ambiguity is gone. What was left was a second spelling
/// with nothing behind it.
const RUN_SEPARATOR: &str = "-";

/// A set of entries, in the shape it crosses the bridge in.
///
/// ## Why this is not just a list
///
/// Three fields of a request name entries - what the player has been shown, what is unseen
/// this game, what is unseen in any game - and a group is 4,514 entries for conversation
/// 631. Measured (`tests/request_size.rs`), one such set costs:
///
/// ```text
/// one entry set                            members   objects      ids     runs    bits
/// everything seen (a completionist save)      4514    149040    18027      146     754
/// one entry in ten, clustered                  451     14774     1703       16     754
/// one entry in ten, scattered                  452     14925     1850     1850     754
/// ```
///
/// `objects` is `[{"conversation":631,"entry":12},...]`, which is what the first crossing
/// wrote and what a whole request at 341,357 bytes was mostly made of. `ids` groups by
/// conversation, which is what the mod's own state file has done on disk since format
/// version 3. `runs` collapses consecutive ids, and `bits` is a base64 bitmap in the order
/// [`questions_for`] returned the entries.
///
/// `runs` is chosen. It is the smallest on the data that actually occurs, because a save's
/// history is CLUSTERED - a player walks through a conversation rather than through every
/// seventh entry of one - and the staged worst-case global state covers the whole group in
/// one run. Its bad case is a perfectly alternating set, where it costs what `ids` costs.
///
/// `bits` is smaller in that bad case and is still not chosen. At 146 bytes for a whole
/// group there is nothing left to buy, and what it would cost is self-description: a
/// bitmap only means anything against a matching entries list, so a plugin holding a
/// cached one against a rebuilt index would send bits that decode cleanly and mean
/// something else. These carry their own ids.
///
/// That trade goes the other way for the ANSWERS, where the constant part is most of the
/// request - see [`WorldSnapshot::variable_values`].
///
/// ## What it looks like
///
/// ```json
/// {"631": "0-40,42,50-99", "636": "3"}
/// ```
///
/// A list of `[{"conversation":631,"entry":12}]` objects is still ACCEPTED, so a
/// hand-written fixture or a test can say what it means the long way. Nothing writes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeSet {
    nodes: HashSet<NodeRef>,
}

impl NodeSet {
    pub fn contains(&self, node: &NodeRef) -> bool {
        self.nodes.contains(node)
    }

    pub fn insert(&mut self, node: NodeRef) -> bool {
        self.nodes.insert(node)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &NodeRef> {
        self.nodes.iter()
    }

    /// The runs, by conversation, as they are written.
    fn runs(&self) -> BTreeMap<String, String> {
        let mut by_conversation: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
        for node in &self.nodes {
            by_conversation.entry(node.conversation).or_default().push(node.entry);
        }

        by_conversation
            .into_iter()
            .map(|(conversation, mut entries)| {
                entries.sort_unstable();
                (conversation.to_string(), write_runs(&entries))
            })
            .collect()
    }
}

impl FromIterator<NodeRef> for NodeSet {
    fn from_iter<T: IntoIterator<Item = NodeRef>>(nodes: T) -> Self {
        Self { nodes: nodes.into_iter().collect() }
    }
}

impl Serialize for NodeSet {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.runs().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for NodeSet {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// Either shape, told apart by which one parses.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Runs(BTreeMap<String, String>),
            Nodes(Vec<NodeRef>),
        }

        match Wire::deserialize(deserializer)? {
            Wire::Nodes(nodes) => Ok(nodes.into_iter().collect()),
            Wire::Runs(runs) => {
                let mut set = NodeSet::default();
                for (conversation, entries) in runs {
                    let conversation: i32 = conversation.parse().map_err(|_| {
                        serde::de::Error::custom(format!(
                            "'{conversation}' is not a conversation id",
                        ))
                    })?;
                    for entry in read_runs(&entries).map_err(serde::de::Error::custom)? {
                        set.insert(NodeRef { conversation, entry });
                    }
                }
                Ok(set)
            }
        }
    }
}

/// Sorted ids as `0-40,42,50-99`.
fn write_runs(entries: &[i32]) -> String {
    let mut runs: Vec<String> = Vec::new();
    let mut index = 0;
    while index < entries.len() {
        let first = entries[index];
        let mut last = first;
        // Duplicates cannot occur - these come out of a set - so a run is strictly
        // ascending and one step at a time.
        while index + 1 < entries.len() && entries[index + 1] == last + 1 {
            index += 1;
            last = entries[index];
        }

        runs.push(if first == last {
            first.to_string()
        } else {
            format!("{first}{RUN_SEPARATOR}{last}")
        });
        index += 1;
    }

    runs.join(",")
}

/// Reads back what [`write_runs`] wrote.
///
/// Refuses anything it does not understand rather than skipping it. A run list that is
/// silently half-read is a world that quietly answers "not seen" for entries the player
/// has read, and the marker is then wrong with nothing to say so.
fn read_runs(text: &str) -> Result<Vec<i32>, String> {
    let mut entries = Vec::new();
    for run in text.split(',') {
        let run = run.trim();
        if run.is_empty() {
            continue;
        }

        // PAST THE FIRST CHARACTER, so a leading '-' reads as a sign rather than as a
        // separator. That one condition is the whole of what the old `..` was chosen to
        // avoid, and it is why the wire could join the files on a spelling - see
        // RUN_SEPARATOR.
        let (first, last) = match run.char_indices().skip(1).find(|(_, c)| *c == '-') {
            Some((at, _)) => (&run[..at], &run[at + 1..]),
            None => (run, run),
        };

        let first: i32 = first
            .trim()
            .parse()
            .map_err(|_| format!("'{run}' is not an entry id or a range of them"))?;
        let last: i32 = last
            .trim()
            .parse()
            .map_err(|_| format!("'{run}' is not an entry id or a range of them"))?;
        if last < first {
            return Err(format!("'{run}' runs backwards"));
        }

        entries.extend(first..=last);
    }

    Ok(entries)
}

/// A value, in a shape a C# caller can write without knowing this crate.
///
/// Tagged with a `kind` string rather than serde's own enum encodings, because those are
/// pleasant in Rust and awkward everywhere else - an externally tagged enum comes out as
/// an object for one variant and a bare string for another, which is a needless trap for
/// whoever writes the other side.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum WireValue {
    #[serde(rename = "bool")]
    Bool { value: bool },
    #[serde(rename = "number")]
    Number { value: f64 },
    #[serde(rename = "text")]
    Text { value: String },
    /// Not knowable. The permissive answer, and the default for anything unanswered.
    #[serde(rename = "unknown")]
    Unknown,
}

impl From<&WireValue> for GuardValue {
    fn from(value: &WireValue) -> Self {
        match value {
            WireValue::Bool { value } => GuardValue::from_boolean(*value),
            WireValue::Number { value } => GuardValue::from_number(*value),
            WireValue::Text { value } => GuardValue::from_text(value.clone()),
            WireValue::Unknown => GuardValue::unknown(),
        }
    }
}

/// The player's situation, as the plugin sees it, for one look-ahead.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorldSnapshot {
    pub money: i32,
    pub day_minutes: i32,
    pub day_counter: i32,
    pub clock_locked: bool,
    /// Dialogue variables, by name.
    ///
    /// For a caller that wants to say what it means: a fixture, a test, a tool. The plugin
    /// uses [`WorldSnapshot::variable_values`] instead, and where both name the same
    /// variable this one wins, because naming it is the more specific statement.
    #[serde(default)]
    pub variables: HashMap<String, WireValue>,
    /// The same answers, in the order [`Questions::variables`] listed the names.
    ///
    /// ## Why a caller would send these instead
    ///
    /// The names are CONSTANT for a group, and the plugin already holds them: it asked for
    /// the questions once and cached them, because they cannot change while the game is
    /// running. Sending them back with every response menu is sending back what the engine
    /// itself said - 306 of them for conversation 631's group, and measured
    /// (`tests/request_size.rs`) they were 14 KB of a 22.5 KB request, repeated per menu.
    ///
    /// This is the shape that was NOT chosen for the entry sets, and the difference is
    /// what it buys. A run list already costs 146 bytes for a whole group, so nothing is
    /// left to win there and self-description is free; here it is most of the request. The
    /// coupling is also checkable rather than silent: a positional list of the wrong LENGTH
    /// is refused outright, where a bitmap of the right length simply means something else.
    #[serde(default)]
    pub variable_values: Vec<WireValue>,
    /// World queries, by the key [`questions_for`] gave them.
    #[serde(default)]
    pub queries: HashMap<String, WireValue>,
    /// The same answers, in the order [`Questions::queries`] listed the keys.
    #[serde(default)]
    pub query_values: Vec<WireValue>,
    /// Items held when the search starts.
    #[serde(default)]
    pub items: HashSet<String>,
    /// Journal tasks active when the search starts.
    #[serde(default)]
    pub tasks: HashSet<String>,
    /// Thoughts in the cabinet when the search starts.
    #[serde(default)]
    pub thoughts: HashSet<String>,
    /// Entries whose skill check the plugin says PASSES.
    ///
    /// Two sets rather than one list of outcomes, because the third outcome is "not
    /// known", and that is what an entry in neither set already means. A `Ternary` on the
    /// wire would have carried an Unknown that says exactly what silence says.
    #[serde(default)]
    pub checks_pass: NodeSet,
    /// Entries whose skill check the plugin says FAILS.
    #[serde(default)]
    pub checks_fail: NodeSet,
    /// Entries the player has already been shown.
    #[serde(default)]
    pub seen: NodeSet,
}

impl WorldSnapshot {
    /// Moves the positional answers onto the names the engine asked under.
    ///
    /// Must run before the snapshot is asked anything. A positional list that is present
    /// and the wrong length is REFUSED rather than zipped as far as it goes: a caller
    /// answering a stale questions list would otherwise have every answer after the first
    /// difference land on the wrong variable, and the marker would be wrong with nothing
    /// to report it.
    pub fn resolve(&mut self, questions: &Questions) -> Result<(), String> {
        place("variable", &questions.variables, &self.variable_values, &mut self.variables)?;
        place("query", &questions.queries, &self.query_values, &mut self.queries)?;
        self.variable_values.clear();
        self.query_values.clear();
        Ok(())
    }
}

/// Names `values` by `asked`, without disturbing anything `named` already says.
fn place(
    what: &str,
    asked: &[String],
    values: &[WireValue],
    named: &mut HashMap<String, WireValue>,
) -> Result<(), String> {
    if values.is_empty() {
        return Ok(());
    }

    if values.len() != asked.len() {
        return Err(format!(
            "{} {what} answers came back for {} questions; the caller is answering a \
             different list of them than this group asks",
            values.len(),
            asked.len(),
        ));
    }

    for (name, value) in asked.iter().zip(values) {
        named.entry(name.clone()).or_insert_with(|| value.clone());
    }

    Ok(())
}

/// A [`WorldSnapshot`], answering as a world.
///
/// ## The variable table behind it
///
/// A snapshot answers what the plugin could read. What it could not read reads Unknown,
/// which is permissive and correct for a genuinely unanswerable question - but a dialogue
/// variable nobody has written is not unanswerable, it is at the value the database
/// declares. Answering Unknown for it makes every guard over it undecidable; answering
/// boolean false for it makes every ordering comparison over a COUNTER undecidable, which
/// is the bug de-sze.5.4 exists about.
///
/// So where the table has been loaded - `variables.jsonl`, deployed beside the index - a
/// variable the snapshot could not answer falls back to its declared initial value, which
/// carries the right KIND as well as the right value. Without the table nothing changes
/// and it stays Unknown.
pub struct SnapshotWorld {
    snapshot: WorldSnapshot,
    /// What the database declares its variables to be, where it has been deployed.
    declared: Option<Arc<VariableTable>>,
}

impl SnapshotWorld {
    /// A world that knows only what the snapshot says.
    pub fn new(snapshot: WorldSnapshot) -> Self {
        Self { snapshot, declared: None }
    }

    /// The same, falling back to the database's declared variables.
    pub fn declaring(snapshot: WorldSnapshot, declared: Option<Arc<VariableTable>>) -> Self {
        Self { snapshot, declared }
    }
}

impl ILookAheadWorld for SnapshotWorld {
    fn money(&self) -> i32 {
        self.snapshot.money
    }

    fn day_minutes(&self) -> i32 {
        self.snapshot.day_minutes
    }

    fn day_counter(&self) -> i32 {
        self.snapshot.day_counter
    }

    fn is_clock_locked(&self) -> bool {
        self.snapshot.clock_locked
    }

    fn get_variable(&self, name: &str) -> GuardValue {
        let answered = self.snapshot.variables.get(name).map(GuardValue::from);
        if let Some(value) = answered {
            if value.kind() != GuardValueKind::Unknown {
                return value;
            }
        }

        // The plugin could not read it. What the database says it starts as is a better
        // answer than "no idea", and is the only one that gets a counter's KIND right.
        self.declared
            .as_ref()
            .and_then(|table| table.initial(name))
            .cloned()
            .unwrap_or_else(GuardValue::unknown)
    }

    fn initially_has_item(&self, name: &str) -> bool {
        self.snapshot.items.contains(name)
    }

    fn initially_task_active(&self, name: &str) -> bool {
        self.snapshot.tasks.contains(name)
    }

    fn initially_has_thought(&self, name: &str) -> bool {
        self.snapshot.thoughts.contains(name)
    }

    fn query(&self, name: &str, arguments: &[GuardValue]) -> GuardValue {
        self.snapshot
            .queries
            .get(&query_key(name, arguments))
            .map(GuardValue::from)
            .unwrap_or_else(GuardValue::unknown)
    }

    fn check_passes(&self, node: DialogueNodeId) -> Ternary {
        let node = NodeRef::from(node);
        if self.snapshot.checks_pass.contains(&node) {
            Ternary::True
        } else if self.snapshot.checks_fail.contains(&node) {
            Ternary::False
        } else {
            Ternary::Unknown
        }
    }

    fn is_seen(&self, node: DialogueNodeId) -> bool {
        self.snapshot.seen.contains(&NodeRef::from(node))
    }
}

/// The key a world query's answer is carried under.
///
/// Written once and used from both ends - [`questions_for`] hands these out and
/// [`SnapshotWorld::query`] looks them up - so the two cannot disagree about what a
/// question is called. That is the whole reason the engine names its own keys.
pub fn query_key(name: &str, arguments: &[GuardValue]) -> String {
    let rendered: Vec<String> = arguments.iter().map(|value| value.to_string()).collect();
    format!("{name}({})", rendered.join(", "))
}

/// Everything a search over one group can ask the world.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Questions {
    /// The conversations the group covers, so the plugin knows what it committed to.
    pub conversations: Vec<i32>,
    /// Dialogue variables read by some guard.
    pub variables: Vec<String>,
    /// World queries, by the key their answers must come back under.
    pub queries: Vec<String>,
    /// Items some guard asks about.
    pub items: Vec<String>,
    /// Journal tasks some guard asks about.
    pub tasks: Vec<String>,
    /// Thoughts some guard asks about.
    pub thoughts: Vec<String>,
    /// Entries carrying a skill check, whose outcome the world decides.
    pub checks: Vec<NodeRef>,
    /// Every entry, because any of them may have been seen.
    pub entries: Vec<NodeRef>,
}

/// What the plugin asks.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LookAheadRequest {
    /// Any conversation in the group; the engine loads the whole group from it.
    pub conversation: i32,
    /// The option entries to score. One answer comes back per start.
    pub starts: Vec<NodeRef>,
    /// Entries the player has never seen in any game.
    #[serde(default)]
    pub unseen_any_game: NodeSet,
    /// Entries unseen this game but seen in a previous one.
    #[serde(default)]
    pub unseen_this_game: NodeSet,
    /// The most search states one option may hold, or zero for no such limit.
    ///
    /// A TEST-ONLY KNOB, and the only budget here that is not a player's. NO CONFIGURATION
    /// SETTING WRITES IT: `LookAheadStateBudget` was removed in de-7z0f because a count of
    /// search states is not a quantity anybody outside this repository can reason about -
    /// the same 200,000 of them cost 136 MB in one conversation and 455 MB in another. What
    /// a player sets is memory and time.
    ///
    /// It survives on the wire because it is the only limit that can stop a search BEFORE
    /// ITS FIRST EXPANSION, and the in-game suites need one that does. A suite that starves
    /// a search is checking that giving up is distinguishable from finding nothing (de-pvq),
    /// which needs the search to reliably not finish.
    ///
    /// THE MEMORY BUDGET CANNOT DO THAT JOB, and measuring says why. It is checked when a
    /// node is dequeued, against what the frontier holds - which after seeding is one
    /// state. The ceiling fan's group, which is what the branch-shape scenarios stand in,
    /// carries TWELVE SLOTS: about 96 bytes a state, so a megabyte holds some eleven
    /// thousand of them and the whole search is over long before the first check fires. A
    /// megabyte is the smallest a player can express, and it is four orders of magnitude
    /// too coarse. A state budget of one, by contrast, is compared against a frontier that
    /// already holds the seed, so it stops the search having looked at nothing.
    ///
    /// Reaches the engine only through the harness's `prepare-look-ahead-suite` probe
    /// command, never through the config file.
    #[serde(default)]
    pub state_budget: usize,

    /// The longest one option may run for in milliseconds; zero for no limit.
    ///
    /// Zero means NO LIMIT rather than the default, matching the plugin's setting, where
    /// zero is documented as no time limit.
    #[serde(default)]
    pub time_budget_ms: u64,

    /// The most memory one option's search may hold, in MEGABYTES; zero for the default.
    ///
    /// MEGABYTES ON THE WIRE AND BYTES IN THE ENGINE, deliberately. This number is set by a
    /// player in a configuration file, and "256" is a figure a person can hold in their
    /// head where 268435456 is not. The conversion is one multiplication at the only place
    /// the two units meet.
    ///
    /// THE ONE THAT GOVERNS, and since de-7z0f the only budget on the wire that counts
    /// what a search HOLDS. There used to be a state budget beside it and it is gone: a
    /// state carries one slot per tracked variable in its group, so a budget counted in
    /// states bought between 136 and 455 megabytes depending on which conversation the
    /// player was standing in - which is not a quantity anybody outside this repository
    /// can set meaningfully. See de-e23q.
    #[serde(default)]
    pub memory_budget_mb: usize,

    pub world: WorldSnapshot,
}

impl LookAheadRequest {
    /// The engine options this request asks for.
    /// What the diagram manager may allocate, from the player's memory budget.
    ///
    /// Asked for FALLIBLY at the other end - the manager preallocates its node store and
    /// that allocation aborts rather than failing, so a machine that cannot supply it must
    /// be found out about before it is spent. See de-0a3a and `DataVars::try_new`.
    /// Public so [`crate::service`] can ask what a request would size a manager to WITHOUT
    /// building one - which is half of deciding whether a live workspace still serves it.
    pub fn diagram_budget(&self) -> DiagramBudget {
        let bytes = if self.memory_budget_mb == 0 {
            DiagramBudget::DEFAULT_MEMORY_BUDGET
        } else {
            self.memory_budget_mb * 1024 * 1024
        };
        DiagramBudget::new(bytes)
    }

    /// How long each half of the search may take.
    ///
    /// THE PLAYER SETS ONE NUMBER and it means the whole answer, so the backward driver
    /// gets it and the per-candidate cap is kept under it - a candidate allowed longer than
    /// the whole search would make the outer limit decorative. The forward slice keeps its
    /// own 50ms: it is sized to be worth the sets it leaves behind, not to finish, and it
    /// is spent before the clock the player set starts mattering.
    fn search_budget(&self) -> portfolio::Budget {
        let default = portfolio::Budget::default();

        // THE STATE BUDGET IS THE KNOB THAT STARVES A SEARCH, and that is all it ever was:
        // a test-only setting, not on the wire for players, whose whole job is to make a
        // search give up on demand so the mod's uncertain marker can be checked. See
        // `Self::state_budget` and `tests/branch_shapes.rs`.
        //
        // It is spent on the rations this engine has: at most that many candidates, and no
        // time to finish one, which is what a search that cannot establish anything looks
        // like from here. The number means "how little", and nothing else.
        if self.state_budget > 0 {
            return portfolio::Budget {
                forwards: std::time::Duration::ZERO,
                backwards: default.backwards,
                each: std::time::Duration::ZERO,
                targets: self.state_budget,
                // Nothing to prune with: the forward slice gets no time at all here, so
                // there is no settled run and `Known` would refuse to narrow anyway.
                pruning: default.pruning,
            };
        }

        if self.time_budget_ms == 0 {
            return default;
        }

        let whole = std::time::Duration::from_millis(self.time_budget_ms);
        portfolio::Budget {
            forwards: default.forwards.min(whole),
            backwards: whole,
            each: default.each.min(whole),
            targets: default.targets,
            pruning: default.pruning,
        }
    }

}

/// What one option scored.
///
/// ## One answer per THING THAT CAN BE CHOSEN, not per menu entry
///
/// A white or red check is two options wearing one line of text, and it comes back as TWO
/// of these - one for each outcome, told apart by [`Self::branch`]. So a menu of n options
/// with k checks is answered by n + k of these, from a request that still names n starts:
/// which entries are rolled checks is what the caller is asking, so it cannot be expected
/// to say so up front.
///
/// WHY NOT ONE ANSWER WITH THE PAIR INSIDE IT, which is what this was. Because every rule
/// about a start then needed a second, branch-shaped version of itself: "refuse a search
/// that cannot improve on where this lands" had to be restated inside the branch code, and
/// the top-rung guard that followed it was a separate fix rather than a consequence of the
/// first. Two outcomes that ARE two starts get the rules once.
///
/// It also fixes a smaller thing that was simply wrong: the combined answer reported ZERO
/// states explored and zero entries reached for a rolled check, because the pair it was
/// derived from carried no cost at all - so every search a check ran was invisible to the
/// diagnostics. Each outcome now carries its own figures like any other start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookAheadAnswer {
    pub start: NodeRef,

    /// Which outcome of a rolled check this is, `"pass"` or `"fail"`.
    ///
    /// ABSENT ON AN ORDINARY OPTION, and that is how the mod decides whether to draw the
    /// Pass / Fail line: a check has two outcomes worth telling apart, an ordinary option
    /// has one. It is on the ANSWER rather than in [`NodeRef`] deliberately - that type is
    /// the wire's identity, a dictionary key and a run-encoded set member, and none of
    /// that should learn about branches to carry a field only starts use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,

    /// The best novelty already known about this start, before any search.
    ///
    /// THE SAME QUANTITY IN BOTH CASES, which is the point of flattening: for an ordinary
    /// option it is the option's own novelty, and for an outcome it is the best novelty
    /// among the entries that outcome leads to DIRECTLY. Either way it is the baseline a
    /// search has to beat to be worth running, and the thing the mod colours the word by.
    #[serde(default)]
    pub destination: i32,
    /// 0 seen, 1 unseen this game, 2 unseen in any game.
    pub best: i32,
    /// The entry that proved it, where something did.
    pub witness: Option<NodeRef>,
    /// Whether the search settled. False means `best` is a lower bound.
    pub complete: bool,
    pub elapsed_ms: u64,
    /// How many states were enumerated.
    ///
    /// Carried because the plugin's diagnostics are about what an ANSWER COSTS, and a time
    /// alone cannot say whether a menu was slow because the search was large or because
    /// the machine was busy. This is the number that is the same on both.
    #[serde(default)]
    pub states_explored: usize,
    /// How many entries it reached.
    #[serde(default)]
    pub nodes_reached: usize,
    /// What stopped it: "none", "states" or "time".
    ///
    /// More than [`Self::complete`] says, and the difference is what a player tuning the
    /// budgets needs: a search that ran out of STATES wants a bigger state budget, and one
    /// that ran out of TIME on the same states wants a slower machine or a longer clock.
    /// A single "it gave up" cannot tell them which dial to turn.
    #[serde(default)]
    pub stopped_by: String,
}

/// What an outcome is called on the wire.
///
/// Strings rather than a number, because this crosses into a file a person reads in the
/// diagnostics and into a hand-written fixture; `"pass"` says what `1` does not.
pub const PASS: &str = "pass";

/// The other one.
pub const FAIL: &str = "fail";

/// What one outcome is called on the wire.
///
/// `Either` never reaches here: it is what an ordinary option asks for, and an ordinary
/// option is the `None` case, which carries no branch name at all. Naming it would put a
/// third value on the wire for a thing the mod does not draw.
fn branch_name(branch: StartBranch) -> &'static str {
    match branch {
        StartBranch::Pass => PASS,
        StartBranch::Fail => FAIL,
        StartBranch::Either => {
            unreachable!("an ordinary option is asked for with no branch, not with Either")
        }
    }
}

/// What comes back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookAheadResponse {
    pub answers: Vec<LookAheadAnswer>,
    /// Set when the whole request failed; `answers` is then empty.
    pub error: Option<String>,
}

impl LookAheadResponse {
    /// A request that was refused, and the reason it was refused for.
    ///
    /// Reachable from `service` as well as from here since de-r4e0: the workspace path
    /// can refuse a request too, and a refusal that could not be spelled the same way
    /// there came back as an empty answer list instead - which reads as "nothing was
    /// established" rather than "this was not asked".
    pub(crate) fn failed(reason: String) -> Self {
        Self { answers: Vec::new(), error: Some(reason) }
    }

    /// The answer for one start, or for one outcome of it.
    ///
    /// THE ACCESS PATTERN THE FLATTENING CREATES, written once rather than in each caller.
    /// A start is now named by an entry AND which of its outcomes, so looking one up takes
    /// both - and getting it wrong is quiet: `find(check, None)` on a rolled check matches
    /// nothing, which is the honest answer rather than the pass half by accident.
    pub fn find(&self, start: NodeRef, branch: Option<&str>) -> Option<&LookAheadAnswer> {
        self.answers
            .iter()
            .find(|answer| answer.start == start && answer.branch.as_deref() == branch)
    }

    /// Both outcomes of a rolled check, in the order the mod draws them.
    ///
    /// None where the start is not a rolled check, which is what the mod reads to decide
    /// whether an option earns a Pass / Fail line at all.
    pub fn outcomes(&self, start: NodeRef) -> Option<(&LookAheadAnswer, &LookAheadAnswer)> {
        Some((self.find(start, Some(PASS))?, self.find(start, Some(FAIL))?))
    }
}

/// Every question a search over `conversation`'s group can ask.
pub fn questions_for(index: &Index, conversation: i32) -> Result<Questions, String> {
    let (graph, group) = build_group_graph(index, conversation)?;
    Ok(questions_of(&graph, group))
}

/// The same, for a group already built.
///
/// Split out because [`answer`] needs both the graph and the questions, and building the
/// group twice per response menu to get them would be paying for the expensive half twice.
/// Public so [`crate::workspace`] can work them out ONCE for a group it will serve many
/// requests over. They depend on the graph and on nothing a request carries.
pub fn questions_of(graph: &LookAheadGraph, group: Vec<i32>) -> Questions {
    let mut found = Questions { conversations: group, ..Default::default() };
    let mut variables = HashSet::new();
    let mut queries = HashSet::new();
    let mut items = HashSet::new();
    let mut tasks = HashSet::new();
    let mut thoughts = HashSet::new();

    for node in graph.nodes() {
        collect(
            &node.guard,
            &mut variables,
            &mut queries,
            &mut items,
            &mut tasks,
            &mut thoughts,
        );

        found.entries.push(NodeRef::from(node.id));
        if node.kind != DialogueCheckKind::None {
            found.checks.push(NodeRef::from(node.id));
        }
    }

    // SORTED, and that is load-bearing rather than tidy. The plugin caches this list
    // against a conversation and answers it POSITIONALLY - see
    // `WorldSnapshot::variable_values` - so the order is the agreement between the two
    // sides, and a list that reordered itself between two calls would silently move every
    // answer onto the wrong question.
    found.variables = sorted(variables);
    found.queries = sorted(queries);
    found.items = sorted(items);
    found.tasks = sorted(tasks);
    found.thoughts = sorted(thoughts);
    found.entries.sort_by_key(|node| (node.conversation, node.entry));
    found.checks.sort_by_key(|node| (node.conversation, node.entry));

    found
}

fn sorted(names: HashSet<String>) -> Vec<String> {
    let mut all: Vec<String> = names.into_iter().collect();
    all.sort();
    all
}

/// Walks one guard, collecting what it asks the world.
///
/// The three subject-taking queries are pulled out by subject rather than left as opaque
/// calls, because the engine answers those from search state when the group moves them -
/// `BoundContext::query` intercepts exactly these - and the plugin needs to supply the
/// STARTING value for each, not an answer to the call.
fn collect(
    guard: &GuardExpression,
    variables: &mut HashSet<String>,
    queries: &mut HashSet<String>,
    items: &mut HashSet<String>,
    tasks: &mut HashSet<String>,
    thoughts: &mut HashSet<String>,
) {
    match guard {
        GuardExpression::Variable(name) => {
            variables.insert(name.clone());
        }
        GuardExpression::Not(inner) => {
            collect(inner, variables, queries, items, tasks, thoughts);
        }
        GuardExpression::And(a, b)
        | GuardExpression::Or(a, b)
        | GuardExpression::Comparison(_, a, b) => {
            collect(a, variables, queries, items, tasks, thoughts);
            collect(b, variables, queries, items, tasks, thoughts);
        }
        GuardExpression::Call(name, args) => {
            let subject = match &args[..] {
                [GuardExpression::Literal(value)]
                    if value.kind() == GuardValueKind::Text =>
                {
                    Some(value.text().to_string())
                }
                _ => None,
            };

            match (name.as_str(), subject) {
                ("CheckItem", Some(subject)) => {
                    items.insert(subject);
                }
                ("IsTaskActive", Some(subject)) => {
                    tasks.insert(subject);
                }
                ("IsTHCPresent", Some(subject)) => {
                    thoughts.insert(subject);
                }
                // `FlagSet(name)` is `Variable[name]` written another way, and the engine
                // answers it from the same place.
                ("FlagSet", Some(subject)) => {
                    variables.insert(subject);
                }
                _ => {
                    // Only literal arguments can be answered ahead of time. A computed
                    // argument would have to be evaluated per state, which is exactly what
                    // a snapshot cannot do - so it is left out, reads Unknown, and the
                    // guard turns permissive.
                    let values: Option<Vec<GuardValue>> = args
                        .iter()
                        .map(|arg| match arg {
                            GuardExpression::Literal(value) => Some(value.clone()),
                            _ => None,
                        })
                        .collect();
                    if let Some(values) = values {
                        queries.insert(query_key(name, &values));
                    }
                }
            }

            for arg in args {
                collect(arg, variables, queries, items, tasks, thoughts);
            }
        }
        GuardExpression::Literal(_) => {}
    }
}

/// Answers one request, from the symbolic portfolio.
///
/// THE CROSSING IS TRUSTED, which is why the answer can be built here rather than checked
/// against something simpler: `tests/bridge_contract.rs` puts one world through JSON and
/// asks the same question without it, over real groups, and requires the same answer; the
/// in-game suites check snapshot agreement per suite and report zero differences across
/// variables, items, tasks, checks and entries; and the wire's shape is pinned by
/// `tests/request_size.rs`.
///
/// TWO THINGS ABOUT THIS ENGINE ARE STILL OPEN, and a reader chasing a failure should know
/// them: an unexplained stack overflow on some conversations (de-fpax, de-8hh2.13), and a
/// manager that commits its whole memory budget up front through an allocation that aborts
/// rather than fails - which is why [`DataVars::try_new`] is asked fallibly here (de-0a3a).
/// Neither is about marshalling.
///
/// The conversations a request's starts live in, sorted and without repeats.
///
/// WHAT THE LAYOUT IS NARROWED TO - see [`DataLayout::for_group_entered_at`] and de-3x76.8
/// - and therefore part of what a kept workspace is valid for.
///
/// TAKEN FROM THE STARTS RATHER THAN FROM `request.conversation`, which is almost always
/// the same single conversation and is not guaranteed to be. The plugin groups its starts
/// by conversation before sending, so in play this is one id; but nothing on the wire
/// enforces it, and the tests and measurements do send starts from anywhere in the group.
/// Narrowing to the named conversation alone would then drop a slot that a start somewhere
/// else genuinely reads, which is a wrong answer rather than a slower one.
///
/// A request with no starts falls back to the conversation it names, so the layout is
/// narrowed to something rather than to nothing.
pub fn entered_at_of(request: &LookAheadRequest) -> Vec<i32> {
    let mut conversations: Vec<i32> =
        request.starts.iter().map(|start| start.conversation).collect();
    if conversations.is_empty() {
        conversations.push(request.conversation);
    }

    conversations.sort_unstable();
    conversations.dedup();
    conversations
}

/// `declared` is the database's variable table where it has been deployed beside the
/// index; see [`SnapshotWorld`] for what it is for and what its absence costs.
pub fn answer(
    index: &Index,
    declared: Option<Arc<VariableTable>>,
    request: &LookAheadRequest,
) -> LookAheadResponse {
    let (graph, group) = match build_group_graph(index, request.conversation) {
        Ok(built) => built,
        Err(reason) => return LookAheadResponse::failed(reason),
    };

    // The questions this group asks, so positional answers can be put back onto their
    // names. Derived from the graph just built rather than by building it again.
    let questions = questions_of(&graph, group);
    let mut snapshot = request.world.clone();
    if let Err(reason) = snapshot.resolve(&questions) {
        return LookAheadResponse::failed(reason);
    }

    let world = SnapshotWorld::declaring(snapshot, declared);
    let novelty = |id: DialogueNodeId| {
        let node = NodeRef::from(id);
        if request.unseen_any_game.contains(&node) {
            Novelty::UnseenAnyGame
        } else if request.unseen_this_game.contains(&node) {
            Novelty::UnseenThisGame
        } else {
            Novelty::SeenThisGame
        }
    };

    // ON A THREAD OF ITS OWN, and everything the diagram manager owns is built inside it
    // and dropped inside it - see `symbolic::isolated`. Releasing a large diagram walks it
    // recursively, so a thread that is going to hold one wants room; the stack is belt and
    // braces rather than the fix for de-8hh2.13.
    //
    // ONE THREAD PER REQUEST, not per start. The layout, the variables, the compiled guards
    // and the seed are facts about the GROUP, and a menu asks about a dozen options in it.
    //
    // AND THAT IS SAFE, MEASURED, which it was not known to be. What accumulates is a
    // SECOND MANAGER built on a thread that has already built one - not a second search -
    // so a request that builds exactly one manager inside its thread and runs every start
    // against it never reaches the fault, however many starts there are. See the table in
    // `symbolic::isolated`; `measurements/menu_residue.rs` puts this call itself to it,
    // forty-five runs at budgets to six gigabytes, and once at three hundred and
    // eighty-four starts. A thread per start would have cost about twelve milliseconds an
    // option at the player's default budget, rebuilding the diagram side each time, to buy
    // nothing.
    // CAUGHT RATHER THAN RE-RAISED, since de-x8ms.10. A panic in here is not a stack trace
    // somebody reads: the engine is a child process the mod talks to over a pipe, so it is
    // the engine VANISHING mid-menu, and the mod reporting that it "stopped answering while
    // reading a frame length". Every option in flight is lost and the feature is off until
    // the mod starts a replacement. Losing the answers and keeping the process costs one
    // unmarked menu instead.
    //
    // IT DOES NOT COVER THE FAULT THE THREAD IS FOR. A stack overflow is not a panic and
    // cannot be caught - see `symbolic::isolated`.
    let answers = isolated::on_its_own_thread_caught(|| {
        answer_within(&graph, &world, request, &novelty)
    });

    match answers {
        Ok(Some(answers)) => LookAheadResponse { answers, error: None },
        // THE MACHINE, not the budget: the manager preallocates its node store and that
        // allocation aborts rather than failing, so it is asked for fallibly first - see
        // de-0a3a. Every option is answered "nothing established" rather than the request
        // failing, because a menu with no markers is what a mod without an engine draws
        // and the player has seen it before.
        Ok(None) => LookAheadResponse {
            answers: request
                .starts
                .iter()
                .map(|start| unanswered(*start, "no-ram"))
                .collect(),
            error: None,
        },
        // THE SAME SHAPE AS "no-ram", DELIBERATELY. Both mean the same thing to the caller -
        // no start was established, draw the menu unmarked - and the mod already knows how
        // to do that. A different shape here would be a second thing for it to learn in
        // order to behave identically.
        //
        // `error` STAYS None for the same reason it does above: this is an answer about the
        // question, not a failure of the request. Which start it was is in the answers and
        // why is in `stopped_by`.
        //
        // SAID ON STDERR, because that is the only channel this process has that is not the
        // wire, and a panic that produced no message anywhere would leave the mod reporting
        // an engine that answered nothing for no visible reason. The default hook has
        // already printed its own line by the time this runs; this one names what it cost.
        Err(panicked) => {
            eprintln!(
                "look-ahead: a search panicked and was contained - {}. \
                 {} start(s) are answered as nothing established; the engine is still up.",
                isolated::panic_message(panicked.as_ref()),
                request.starts.len(),
            );
            LookAheadResponse {
                answers: request
                    .starts
                    .iter()
                    .map(|start| unanswered(*start, "crashed"))
                    .collect(),
                error: None,
            }
        }
    }
}

/// How high a counter is modelled before it saturates.
///
/// SIXTEEN, AND EVERY MEASUREMENT IN THE REPOSITORY WAS MADE AT IT -
/// `measurements/performance_matrix.rs` and the symbolic tests all use this number. Changing it
/// changes which states a search can tell apart, so a run measured under one cap says
/// nothing about a search under another.
pub const COUNTER_CAP: i32 = 16;

/// The answers for one request, from inside the thread that owns the diagram.
///
/// `None` where the machine could not supply the diagram's memory, which is a fact about
/// the machine rather than about any option, so it is reported once for all of them.
fn answer_within<F>(
    graph: &LookAheadGraph,
    world: &dyn ILookAheadWorld,
    request: &LookAheadRequest,
    novelty: &F,
) -> Option<Vec<LookAheadAnswer>>
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    let symbols = graph.symbols().clone();
    // NARROWED TO WHAT THE REQUEST'S CONVERSATION CAN REACH - de-3x76.8. The group is much
    // bigger than the conversation the player is standing in, and a slot read only by
    // guards beyond what this one reaches is carried for nothing.
    let layout = DataLayout::for_group_entered_at(
        graph,
        world,
        COUNTER_CAP,
        Some(&entered_at_of(request)),
    );
    let vars = DataVars::try_new(&layout, &symbols, request.diagram_budget())?;
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, world, &vars);

    // ONCE FOR THE MENU, like the manager and the compiler above. The parent map and the
    // SCC decomposition are facts about the LINKS - no start, no world, no budget - and
    // every option below wants the same ones. Each used to build its own: twenty-four
    // Tarjan passes over conversation 631's 4,514 entries for one answer, which
    // `measurements/per_start_setup.rs` priced at 246 ms a menu. See `GroupShape`.
    let shape = GroupShape::of(graph);

    Some(answer_starts(graph, world, request, novelty, &mut compiler, &seed, &shape))
}

/// The answers for one request, against a manager and a compiler somebody else built.
///
/// ## Why this is separated from [`answer_within`]
///
/// Because [`crate::workspace`] builds those two ONCE and keeps them, where `answer_within`
/// builds them per request and drops them. Everything below is what a request costs after
/// the diagram side exists, and it is the same code either way - which is the point: a
/// workspace must not be a second implementation of what a menu means.
///
/// The caller owns the diagram side, so it also owns the de-fpax invariant: this must run
/// on the thread that built `compiler`'s manager.
#[allow(clippy::too_many_arguments)]
pub fn answer_starts<'a, F>(
    graph: &LookAheadGraph,
    world: &dyn ILookAheadWorld,
    request: &LookAheadRequest,
    novelty: &F,
    compiler: &mut GuardCompiler<'a>,
    seed: &BDDFunction,
    shape: &GroupShape,
) -> Vec<LookAheadAnswer>
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    let budget = request.search_budget();
    let mut answers = Vec::with_capacity(request.starts.len());

    for start in &request.starts {
        let id = DialogueNodeId::from(*start);
        if graph.get(id).is_none() {
            // Not an error for the request as a whole: a menu can offer an option the
            // loaded group does not carry, and the honest answer about it is "nothing
            // known" rather than a failed call for every other option too.
            answers.push(unanswered(*start, "none"));
            continue;
        }

        // A ROLLED CHECK IS TWO STARTS, because it is two options wearing one line of text
        // and the mod draws them apart - see de-fes. Two searches rather than one, and only
        // here: a menu of ordinary options costs what it did.
        let rolled = matches!(
            graph.get(id).map(|node| node.kind),
            Some(DialogueCheckKind::Red) | Some(DialogueCheckKind::White)
        );

        let branches: &[StartBranch] = if rolled {
            &[StartBranch::Pass, StartBranch::Fail]
        } else {
            &[StartBranch::Either]
        };

        for branch in branches {
            answers.push(scored(
                graph, id, *start, world, novelty, *branch, seed, compiler, &budget, shape,
            ));
        }
    }

    answers
}

/// An option with nothing established about it, and why.
fn unanswered(start: NodeRef, stopped_by: &str) -> LookAheadAnswer {
    LookAheadAnswer {
        start,
        branch: None,
        destination: Novelty::SeenThisGame as i32,
        best: Novelty::SeenThisGame as i32,
        witness: None,
        complete: false,
        elapsed_ms: 0,
        states_explored: 0,
        nodes_reached: 0,
        stopped_by: stopped_by.to_string(),
    }
}

/// What one start scored: an ordinary option, or one outcome of a rolled check.
///
/// ONE ROUTINE FOR BOTH, which is the whole of de-8hh2.6. The two differ in exactly one
/// place now - where the search is measured from - and everything else about them is the
/// same question. They used to be two routines that had to be kept saying the same thing,
/// and the drift showed: the rule that refuses a search which cannot improve on its
/// baseline had to be restated for branches, and the top-rung guard that follows from it
/// was a separate fix rather than a consequence.
#[allow(clippy::too_many_arguments)]
fn scored<'a, F>(
    graph: &LookAheadGraph,
    id: DialogueNodeId,
    start: NodeRef,
    world: &dyn ILookAheadWorld,
    novelty: F,
    branch: StartBranch,
    seed: &BDDFunction,
    compiler: &mut GuardCompiler<'a>,
    budget: &portfolio::Budget,
    shape: &GroupShape,
) -> LookAheadAnswer
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    let began = std::time::Instant::now();

    // WHERE THE SEARCH IS MEASURED FROM, and both cases name actual entries: the option
    // itself, or the entries the outcome opens. The baseline is their best class, and they
    // are also where the refusal walks from - one fact, used twice, so a walk can never be
    // measuring from somewhere the baseline did not come from.
    let where_from = novelty_search::Where::of(
        graph, id, branch, seed, compiler, world, COUNTER_CAP as u32,
    );
    let from: Vec<DialogueNodeId> = match branch {
        StartBranch::Either => vec![id],
        _ => where_from.destinations(graph, compiler, world, COUNTER_CAP as u32),
    };
    let destination = from
        .iter()
        .map(|id| novelty(*id))
        .max()
        .unwrap_or(Novelty::SeenThisGame);

    let answered = |best: Novelty,
                    complete,
                    witness: Option<DialogueNodeId>,
                    asked,
                    stopped: &str| LookAheadAnswer {
        start,
        branch: match branch {
            StartBranch::Either => None,
            branch => Some(branch_name(branch).to_string()),
        },
        destination: destination as i32,
        best: best as i32,
        witness: witness.map(NodeRef::from),
        complete,
        elapsed_ms: began.elapsed().as_millis() as u64,
        // A SET-BASED SEARCH DOES NOT ENUMERATE STATES, so a count of them is meaningless
        // here and reported as the zero it is. What this search counts instead is candidates
        // asked about, which is `nodes_reached`'s nearest true relative: the entries it had
        // to consider before it could answer.
        states_explored: 0,
        nodes_reached: asked,
        stopped_by: stopped.to_string(),
    };

    // NOTHING BETTER IS REACHABLE, so there is no search to run.
    //
    // A COMPLETE ANSWER, not a gave-up one: this establishes that nothing outranks the
    // baseline, which is exactly what a finished search finding nothing would. And `best`
    // is the baseline rather than the floor, because that is what was established - the
    // start reaches where it reaches, and nothing beyond it does better.
    //
    // WHERE IT DOES NOT REFUSE, it has named the class the search should hunt, and that
    // walk is not done again further in. See `class_worth_hunting`.
    let Some(hunting) = class_worth_hunting(graph, &from, destination, &novelty) else {
        return answered(destination, true, None, 0, "none");
    };

    let found = portfolio::best_novelty(
        graph, id, branch, seed, compiler, world, COUNTER_CAP as u32, &novelty, hunting,
        budget, shape,
    );

    answered(
        found.best,
        found.by != portfolio::Answered::Partly,
        found.witness,
        found.targets_asked,
        stopped_name(found.stopped_by),
    )
}

/// The name a stopped search crosses the wire under.
///
/// THE WORDS ARE THE WIRE'S, and they do not change, because the C# side and the harness read them
/// and what they MEAN has not changed: a ration ran out, and which one decides whether a
/// player can do anything about it. What has changed is which rations exist - a set-based
/// search has candidates and a clock where a state-at-a-time one would have states and bytes.
fn stopped_name(stopped: novelty_search::StoppedBy) -> &'static str {
    match stopped {
        novelty_search::StoppedBy::Nothing => "none",
        // The candidate budget: the search ran out of things it was allowed to ask about,
        // which is this engine's "states" - a ceiling on how much work one answer may cost.
        novelty_search::StoppedBy::Targets => "states",
        novelty_search::StoppedBy::Time => "time",
        // A pass that could not finish, which is either its own clock or the diagram
        // running out of nodes. `out_of_nodes` tells them apart, and the wire has one word
        // for the pair until something reads them apart.
        novelty_search::StoppedBy::Incomplete => "memory",
    }
}


/// The class a search from these starts should hunt, or `None` when there is nothing to find.
///
/// FROM WHAT THE OUTCOME OPENS, for a branch, and from the option itself for an option.
/// That is what makes "a start is a result like any other" true here: the starts ARE the
/// baseline - the outcome's own destinations, whose best class is what `destination` is -
/// so scoring them can never manufacture an improvement, while an entry beyond them can.
///
/// WALKING FROM THE CHECK INSTEAD would count the check's own class and the other
/// outcome's half of the graph. The second is a safe over-approximation - it costs a search
/// that finds nothing. The first is not: the check is the option the player is standing on,
/// so counting it says passing leads somewhere it does not.
///
/// THE ONE PLACE THE QUESTION IS ASKED, for an ordinary option and for each outcome of a
/// rolled check alike. A search exists to find something that OUTRANKS a baseline: the
/// option's own novelty for an ordinary option, and where the outcome LANDS for a branch.
/// Both cases refuse for the same two reasons, in the same order, so neither can drift
/// from the other and a rule added here reaches all three paths at once.
///
/// ONE ANSWER RATHER THAN A YES OR NO. A yes would have to be followed by the same walk
/// again inside the search, to decide which class to hunt - the same question twice per
/// start, and once per outcome of every rolled check. The walk names the class, so the
/// refusal and the target are one fact: `None` is the refusal, and anything else is what
/// the forward slice is sent after.
///
/// NOTHING OUTRANKS THE TOP RUNG. Text no save has read is as novel as anything gets, so
/// there is nothing for a search to find and the answer is settled without walking at all -
/// forward or backward, since this is decided before any strategy is chosen.
///
/// AND NOTHING IS REACHABLE THAT WOULD BEAT IT. That walk is a few thousand pointer-follows
/// against a search that is thousands of diagram operations, and it stops early whenever it
/// meets the top rung - so the case it costs anything in is the case where it saves a whole
/// search. See [`LookAheadGraph::best_linked_class`].
///
/// A BRANCH'S DESTINATIONS ARE COMPUTED ANYWAY, for the baseline, so walking from them
/// costs nothing extra and is strictly tighter than walking from the check: the other
/// outcome's half of the graph is no longer counted for this one.
///
/// PUBLIC SO THE MEASUREMENT CAN ASK THE SAME QUESTION, since de-qh27.
///
/// The `fwdbwd` column of the performance matrix is the one that claims to be what the game
/// runs, and it was calling `portfolio::best_novelty` unconditionally where the game refuses
/// to search at all. A second copy of this predicate is exactly how that drifted, so it is
/// exported rather than reimplemented.
pub fn class_worth_hunting<F>(
    graph: &LookAheadGraph,
    starts: &[DialogueNodeId],
    baseline: Novelty,
    novelty: F,
) -> Option<Novelty>
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    // The cheap half first: nothing outranks the top rung, so a baseline there is settled
    // without walking anything at all.
    if baseline >= Novelty::UnseenAnyGame {
        return None;
    }

    starts
        .iter()
        .filter_map(|start| graph.best_linked_class(*start, &novelty))
        .max()
        .filter(|best| *best > baseline)
}

#[cfg(test)]
mod branch_wire_tests {
    use super::*;
    use crate::test_graph::{node, Entry, GraphBuilder};
    use crate::world::test_world::TestWorld;

    /// One start scored, with the diagram apparatus `answer_within` would have built.
    ///
    /// The same shape as the real path, small enough to read: the layout, the variables and
    /// the compiled guards are the group's, and the search is asked one question about one
    /// outcome.
    fn score_one<F>(
        graph: &LookAheadGraph,
        world: &TestWorld,
        start: DialogueNodeId,
        branch: StartBranch,
        novelty: F,
    ) -> LookAheadAnswer
    where
        F: Fn(DialogueNodeId) -> Novelty,
    {
        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(graph, COUNTER_CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(world);
        let seed = seed_of(graph, world, &vars);

        scored(
            graph,
            start,
            NodeRef::from(start),
            world,
            novelty,
            branch,
            &seed,
            &mut compiler,
            &portfolio::Budget::default(),
            &GroupShape::of(graph),
        )
    }

    /// A check whose outcomes land on different rungs, both below the top one.
    ///
    /// 0 is the check. Passing opens 1, which this save has read, and 2 lies past it;
    /// failing opens 3. Nothing anywhere is unseen in any game, which is what makes the
    /// OPTION not worth searching while one of its OUTCOMES still is.
    fn check_landing_on_something_read() -> LookAheadGraph {
        GraphBuilder::new()
            .add(Entry::new(0).kind(DialogueCheckKind::White).flag("roll").links(&[1, 3]))
            .add(Entry::new(1).guard(r#"Variable["roll"] == true"#).links(&[2]))
            .add(Entry::new(2))
            .add(Entry::new(3).guard(r#"Variable["roll"] == false"#))
            .build()
    }

    /// The option's refusal does not settle its outcomes' questions.
    ///
    /// THE BUG THE ONE-PLACE REFUSAL FIXED. "Nothing outranks the OPTION" and "nothing
    /// outranks where this OUTCOME lands" are different questions whenever an outcome
    /// lands lower than the option does, and the branches used to be handed the option's
    /// answer. Here the option is unseen-this-game and nothing beats that, so no search
    /// runs for it - but passing lands on text this save has READ, and the unread entry
    /// past it outranks that. The pass half has an asterisk to draw; it used to draw none.
    #[test]
    fn a_refused_option_still_lets_each_outcome_ask_for_itself() {
        let graph = check_landing_on_something_read();
        let world = TestWorld::new();

        // 1 is read; everything else is unseen this game. Nothing is unseen anywhere.
        let novelty = |id: DialogueNodeId| {
            if id == node(1) { Novelty::SeenThisGame } else { Novelty::UnseenThisGame }
        };

        assert!(
            class_worth_hunting(&graph, &[node(0)], novelty(node(0)), novelty).is_none(),
            "the option should be refused: nothing outranks unseen-this-game here",
        );

        let pass = score_one(&graph, &world, node(0), StartBranch::Pass, novelty);
        assert_eq!(pass.branch.as_deref(), Some(PASS));
        assert_eq!(pass.destination, Novelty::SeenThisGame as i32, "passing opens 1");
        assert_eq!(
            pass.best, Novelty::UnseenThisGame as i32,
            "the unread entry past 1 outranks where passing lands, and should be reported",
        );

        let fail = score_one(&graph, &world, node(0), StartBranch::Fail, novelty);
        assert_eq!(fail.branch.as_deref(), Some(FAIL));
        assert_eq!(fail.destination, Novelty::UnseenThisGame as i32, "failing opens 3");
        assert_eq!(fail.best, fail.destination, "and nothing past 3 beats it");

        // THE TWO ARE ONE START EACH, and they say so: same entry, different outcome.
        assert_eq!(pass.start, fail.start);
        assert_ne!(pass.branch, fail.branch);
    }

    /// An outcome on the top rung is refused in the same place, and costs nothing.
    #[test]
    fn an_outcome_on_the_top_rung_is_refused_without_a_search() {
        let graph = check_landing_on_something_read();

        // Everything unseen anywhere, which is what a fresh profile looks like.
        let novelty = |_: DialogueNodeId| Novelty::UnseenAnyGame;

        assert!(
            class_worth_hunting(&graph, &[node(0)], Novelty::UnseenAnyGame, novelty).is_none(),
            "nothing outranks the top rung, so there is nothing to search for",
        );
    }

    /// THE REFUSAL KNOWS WHICH CLASS IT SAW, and that is what the search after it needs.
    ///
    /// Same graph, same baseline, two novelty functions that differ only in the class the
    /// reachable entry carries. A walk that stopped at the first entry beating the baseline
    /// would answer both the same way and name neither class - see `symbolic::portfolio`
    /// for what the class is for.
    #[test]
    fn the_refusal_is_decided_by_the_best_class_reachable() {
        let graph = check_landing_on_something_read();

        // Unseen-here everywhere: nothing outranks an unseen-here baseline.
        let here_only = |_: DialogueNodeId| Novelty::UnseenThisGame;
        assert_eq!(
            class_worth_hunting(&graph, &[node(0)], Novelty::UnseenThisGame, here_only),
            None,
            "reachable, but not better than the baseline, so there is nothing to hunt",
        );

        // One entry past the check is unseen ANYWHERE, and that outranks the same baseline.
        let one_top_rung = |id: DialogueNodeId| {
            if id == node(2) { Novelty::UnseenAnyGame } else { Novelty::UnseenThisGame }
        };
        assert_eq!(
            class_worth_hunting(&graph, &[node(0)], Novelty::UnseenThisGame, one_top_rung),
            Some(Novelty::UnseenAnyGame),
            "and the class it names is what the forward slice is sent after",
        );
    }

    /// A BRANCH IS MEASURED FROM WHAT IT OPENS, not from the check.
    ///
    /// THE CASE THAT SETTLED THE RULE. 0 is a white check no save has displayed; passing
    /// opens 1, which this save has READ. Walking from the check would count the check's
    /// own top rung and report that passing leads somewhere unread - it does not, it leads
    /// to 1, and the check is the option the player is standing on. Walking from 1, which
    /// is where the baseline came from, cannot make that mistake.
    ///
    /// The `fan-reaches-gives-up` shape is this, one rung down, and it is what caught it.
    #[test]
    fn a_branch_is_measured_from_its_destinations_and_not_from_the_check() {
        let graph = check_landing_on_something_read();

        // The check is unseen anywhere; everything it opens has been read here.
        let novelty = |id: DialogueNodeId| {
            if id == node(0) { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
        };

        // Passing opens 1. From there, nothing outranks the floor - 2 is read as well.
        assert_eq!(
            class_worth_hunting(&graph, &[node(1)], Novelty::SeenThisGame, novelty),
            None,
            "the check's own class is not something passing leads to",
        );

        // And walking from the check would have said otherwise, which is the bug.
        assert_eq!(
            class_worth_hunting(&graph, &[node(0)], Novelty::SeenThisGame, novelty),
            Some(Novelty::UnseenAnyGame),
        );
    }

    /// What lies BEYOND a destination is still found, which is the half that must not break.
    #[test]
    fn something_past_what_an_outcome_opens_is_still_hunted() {
        let graph = check_landing_on_something_read();

        // Passing opens 1, which is read; 2 lies past it and no save has read that.
        let novelty = |id: DialogueNodeId| {
            if id == node(2) { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
        };

        assert_eq!(
            class_worth_hunting(&graph, &[node(1)], Novelty::SeenThisGame, novelty),
            Some(Novelty::UnseenAnyGame),
        );
    }

    /// Nothing unseen at all is refused without a search, and says so as `None`.
    #[test]
    fn a_group_with_nothing_unseen_is_refused() {
        let graph = check_landing_on_something_read();
        let read = |_: DialogueNodeId| Novelty::SeenThisGame;

        assert_eq!(graph.best_linked_class(node(0), read), None);
        assert_eq!(class_worth_hunting(&graph, &[node(0)], Novelty::SeenThisGame, read), None);
    }

    /// An outcome's answer round-trips as JSON, naming which outcome it is.
    #[test]
    fn an_outcome_survives_the_wire() {
        let answer = LookAheadAnswer {
            start: NodeRef { conversation: 451, entry: 12 },
            branch: Some(PASS.to_string()),
            destination: 0,
            best: 2,
            witness: None,
            complete: true,
            elapsed_ms: 4,
            states_explored: 90,
            nodes_reached: 30,
            stopped_by: "none".to_string(),
        };

        let text = serde_json::to_string(&answer).expect("an answer serialises");
        let back: LookAheadAnswer = serde_json::from_str(&text).expect("and parses back");
        assert_eq!(back, answer);
    }

    /// An ordinary option names no outcome, and costs nothing to say so.
    ///
    /// The absence is load bearing: it is what the mod reads to decide whether an option
    /// gets a Pass / Fail line at all.
    #[test]
    fn an_ordinary_option_names_no_outcome() {
        let answer = LookAheadAnswer {
            start: NodeRef { conversation: 451, entry: 12 },
            branch: None,
            destination: 0,
            best: 0,
            witness: None,
            complete: true,
            elapsed_ms: 0,
            states_explored: 1,
            nodes_reached: 1,
            stopped_by: "none".to_string(),
        };

        let text = serde_json::to_string(&answer).expect("an answer serialises");
        assert!(!text.contains("branch"), "an absent outcome still crossed: {text}");

        let back: LookAheadAnswer = serde_json::from_str(&text).expect("and parses back");
        assert_eq!(back.branch, None);
    }

    /// A reader that has never heard of outcomes still parses an answer.
    #[test]
    fn an_answer_without_the_field_still_parses() {
        let text = r#"{"start":{"conversation":1,"entry":2},"best":1,"complete":true,
            "elapsed_ms":0,"states_explored":0,"nodes_reached":0,"stopped_by":"none"}"#;

        let answer: LookAheadAnswer = serde_json::from_str(text).expect("it parses");
        assert_eq!(answer.branch, None);
        assert_eq!(answer.destination, 0, "an absent destination reads as the bottom rung");
    }

    /// A rolled check comes back as TWO answers, and an ordinary option as one.
    ///
    /// The shape of the whole change, asked of `answer` rather than of its parts: a menu
    /// of n options with k checks is answered by n + k of these, from a request that names
    /// n starts.
    #[test]
    fn a_check_is_two_answers_and_an_option_is_one() {
        let graph = check_landing_on_something_read();
        let index = crate::index::Index::new();
        let _ = index;

        let world = TestWorld::new();
        let novelty = |_: DialogueNodeId| Novelty::UnseenThisGame;

        let outcomes: Vec<LookAheadAnswer> = [StartBranch::Pass, StartBranch::Fail]
            .into_iter()
            .map(|branch| score_one(&graph, &world, node(0), branch, novelty))
            .collect();

        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].branch.as_deref(), Some(PASS));
        assert_eq!(outcomes[1].branch.as_deref(), Some(FAIL));

        // 2 rolls nothing, so it is one start and names no outcome.
        let plain = score_one(&graph, &world, node(2), StartBranch::Either, novelty);
        assert_eq!(plain.branch, None);
    }
}

/// The graph a group builds, for a caller that wants to look before asking.
pub fn group_of(index: &Index, conversation: i32) -> Result<LookAheadGraph, String> {
    build_group_graph(index, conversation).map(|(graph, _)| graph)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::core::state::StateSymbols;
    use crate::graph::node::LookAheadNode;
    use crate::parser::guard_parser::parse_guard;

    /// A guard, walked for what it asks.
    fn asked(text: &str) -> Questions {
        let guard = parse_guard(text).expect("the fixture parses");
        let mut variables = HashSet::new();
        let mut queries = HashSet::new();
        let mut items = HashSet::new();
        let mut tasks = HashSet::new();
        let mut thoughts = HashSet::new();
        collect(
            &guard,
            &mut variables,
            &mut queries,
            &mut items,
            &mut tasks,
            &mut thoughts,
        );

        Questions {
            variables: sorted(variables),
            queries: sorted(queries),
            items: sorted(items),
            tasks: sorted(tasks),
            thoughts: sorted(thoughts),
            ..Default::default()
        }
    }

    #[test]
    fn a_variable_is_asked_for_by_name() {
        let found = asked(r#"Variable["jam.asked"]"#);
        assert_eq!(found.variables, vec!["jam.asked".to_string()]);
    }

    /// The three the engine answers from search state are pulled out by SUBJECT, because
    /// what the plugin must supply for them is a starting value rather than an answer.
    #[test]
    fn the_slot_backed_queries_are_asked_for_by_subject() {
        let found = asked(
            r#"CheckItem("badge") and IsTaskActive("TASK.x") and IsTHCPresent("jamais_vu")"#,
        );

        assert_eq!(found.items, vec!["badge".to_string()]);
        assert_eq!(found.tasks, vec!["TASK.x".to_string()]);
        assert_eq!(found.thoughts, vec!["jamais_vu".to_string()]);
        assert!(found.queries.is_empty(), "these must not also be asked as calls");
    }

    /// A flag is a dialogue variable written another way, and is asked for as one.
    #[test]
    fn a_flag_is_asked_for_as_a_variable() {
        let found = asked(r#"FlagSet("church.done")"#);
        assert_eq!(found.variables, vec!["church.done".to_string()]);
        assert!(found.queries.is_empty());
    }

    #[test]
    fn an_ordinary_query_is_asked_for_by_its_key() {
        let found = asked(r#"IsKimHere() and CheckEquipped("neck_tie")"#);
        assert_eq!(
            found.queries,
            vec!["CheckEquipped(\"neck_tie\")".to_string(), "IsKimHere()".to_string()],
        );
    }

    /// The key the engine hands out is the key it later looks up. Written as a test
    /// because the two sides agreeing is the whole point of naming them here.
    #[test]
    fn the_key_asked_for_is_the_key_answered() {
        let found = asked(r#"CheckEquipped("neck_tie")"#);
        let key = &found.queries[0];

        let mut world = WorldSnapshot::default();
        world.queries.insert(key.clone(), WireValue::Bool { value: true });
        let world = SnapshotWorld::new(world);

        let answer = world.query(
            "CheckEquipped",
            &[GuardValue::from_text("neck_tie".to_string())],
        );
        assert!(answer.boolean(), "the answer did not come back under the key given");
    }

    /// A variable the plugin could not read falls back to what the database declares.
    ///
    /// The KIND is what this is about. A counter answered Unknown makes `>= 3`
    /// undecidable, and answered boolean false makes it undecidable in exactly the same
    /// way - `try_as_number` gives nothing for a boolean. Only a number answers it.
    #[test]
    fn a_variable_the_game_could_not_read_falls_back_to_what_the_database_declares() {
        let mut table = VariableTable::default();
        table.add(&crate::index::VariableRecord {
            name: "jam.lorrymans_questioned".to_string(),
            declared: "Number".to_string(),
            initial: "0".to_string(),
        });
        table.add(&crate::index::VariableRecord {
            name: "church.done".to_string(),
            declared: "Boolean".to_string(),
            initial: "False".to_string(),
        });

        let mut snapshot = WorldSnapshot::default();
        // What the plugin sends for a variable Lua would not answer.
        snapshot.variables.insert("church.done".to_string(), WireValue::Unknown);
        // And one it did answer, which must not be overridden by the table's initial.
        snapshot
            .variables
            .insert("jam.lorrymans_questioned".to_string(), WireValue::Number { value: 4.0 });

        let world = SnapshotWorld::declaring(snapshot, Some(Arc::new(table)));

        assert_eq!(world.get_variable("jam.lorrymans_questioned").try_as_number(), Some(4.0));
        assert_eq!(world.get_variable("church.done").kind(), GuardValueKind::Boolean);
        // Never named at all, and the table does not declare it either.
        assert_eq!(world.get_variable("nothing.declares.this").kind(), GuardValueKind::Unknown);
    }

    /// A declared counter nobody wrote answers as a NUMBER, so an ordering guard decides.
    #[test]
    fn a_declared_counter_answers_as_a_number_rather_than_undecidably() {
        let mut table = VariableTable::default();
        table.add(&crate::index::VariableRecord {
            name: "pier.reporting_counter".to_string(),
            declared: "Number".to_string(),
            initial: "0".to_string(),
        });

        // Nothing about it in the snapshot at all, which is what a group whose variable
        // the plugin never saw looks like.
        let world = SnapshotWorld::declaring(WorldSnapshot::default(), Some(Arc::new(table)));
        assert_eq!(world.get_variable("pier.reporting_counter").try_as_number(), Some(0.0));

        // And without the table it is Unknown, which is what it was before.
        let bare = SnapshotWorld::new(WorldSnapshot::default());
        assert_eq!(bare.get_variable("pier.reporting_counter").kind(), GuardValueKind::Unknown);
    }

    /// A positional answer lands on the name the engine asked under.
    #[test]
    fn positional_answers_are_put_back_onto_their_names() {
        let questions = Questions {
            variables: vec!["a.first".to_string(), "b.second".to_string()],
            queries: vec!["IsKimHere()".to_string()],
            ..Default::default()
        };

        let mut snapshot = WorldSnapshot {
            variable_values: vec![
                WireValue::Number { value: 4.0 },
                WireValue::Bool { value: true },
            ],
            query_values: vec![WireValue::Bool { value: true }],
            ..Default::default()
        };
        snapshot.resolve(&questions).expect("the lists are the same length");

        let world = SnapshotWorld::new(snapshot);
        assert_eq!(world.get_variable("a.first").try_as_number(), Some(4.0));
        assert!(world.get_variable("b.second").boolean());
        assert!(world.query("IsKimHere", &[]).boolean());
    }

    /// A named answer beats a positional one, because naming it says more.
    #[test]
    fn a_named_answer_wins_over_the_positional_one() {
        let questions = Questions {
            variables: vec!["a.first".to_string()],
            ..Default::default()
        };

        let mut snapshot = WorldSnapshot {
            variable_values: vec![WireValue::Number { value: 4.0 }],
            ..Default::default()
        };
        snapshot.variables.insert("a.first".to_string(), WireValue::Number { value: 9.0 });
        snapshot.resolve(&questions).expect("the lists are the same length");

        assert_eq!(
            SnapshotWorld::new(snapshot).get_variable("a.first").try_as_number(),
            Some(9.0),
        );
    }

    /// Answers to a DIFFERENT list of questions are refused, not zipped as far as they go.
    ///
    /// The failure this exists to prevent: a caller answering a stale questions list has
    /// every answer after the first difference land on the wrong variable, and the marker
    /// is then wrong with nothing whatever to report it.
    #[test]
    fn a_positional_list_of_the_wrong_length_is_refused() {
        let questions = Questions {
            variables: vec!["a.first".to_string(), "b.second".to_string()],
            ..Default::default()
        };

        let mut snapshot = WorldSnapshot {
            variable_values: vec![WireValue::Bool { value: true }],
            ..Default::default()
        };

        let refused = snapshot.resolve(&questions).expect_err("it must be refused");
        assert!(refused.contains("1 variable answers came back for 2"), "{refused}");
    }

    /// Anything unanswered reads Unknown, which is the permissive direction.
    #[test]
    fn an_unanswered_question_is_unknown_rather_than_false() {
        let world = SnapshotWorld::new(WorldSnapshot::default());

        assert_eq!(world.get_variable("never.mentioned").kind(), GuardValueKind::Unknown);
        assert_eq!(world.query("IsKimHere", &[]).kind(), GuardValueKind::Unknown);
        assert_eq!(world.check_passes(DialogueNodeId::new(1, 2)), Ternary::Unknown);
    }

    /// All three check outcomes come across, and the third one is silence.
    #[test]
    fn a_check_answer_carries_all_three_outcomes() {
        let world = SnapshotWorld::new(WorldSnapshot {
            checks_pass: NodeSet::from_iter([NodeRef { conversation: 1, entry: 1 }]),
            checks_fail: NodeSet::from_iter([NodeRef { conversation: 1, entry: 2 }]),
            ..Default::default()
        });

        assert_eq!(world.check_passes(DialogueNodeId::new(1, 1)), Ternary::True);
        assert_eq!(world.check_passes(DialogueNodeId::new(1, 2)), Ternary::False);
        assert_eq!(world.check_passes(DialogueNodeId::new(1, 3)), Ternary::Unknown);
    }

    #[test]
    fn seen_entries_are_carried_across() {
        let world = SnapshotWorld::new(WorldSnapshot {
            seen: NodeSet::from_iter([NodeRef { conversation: 7, entry: 3 }]),
            ..Default::default()
        });

        assert!(world.is_seen(DialogueNodeId::new(7, 3)));
        assert!(!world.is_seen(DialogueNodeId::new(7, 4)));
    }

    /// The runs a set is written as, pinned - the plugin writes these by hand.
    #[test]
    fn an_entry_set_is_written_as_runs_by_conversation() {
        let set = NodeSet::from_iter(
            [0, 1, 2, 3, 5, 9, 10]
                .map(|entry| NodeRef { conversation: 631, entry })
                .into_iter()
                .chain([NodeRef { conversation: 636, entry: 7 }]),
        );

        assert_eq!(
            serde_json::to_string(&set).expect("it serialises"),
            r#"{"631":"0-3,5,9-10","636":"7"}"#,
        );
    }

    /// A negative id survives the hyphen, which is the whole reason it can BE a hyphen.
    ///
    /// The separator was `..` on the stated grounds that a hyphen becomes ambiguous the
    /// first time an id is negative. It does not: the separator is looked for past the
    /// first character, so a leading `-` is a sign. That argument is what let the wire
    /// join every file in this repository on one spelling, so it is asserted here rather
    /// than left as a claim in a comment - including the case that would actually be
    /// ambiguous, a negative id at BOTH ends of a run.
    #[test]
    fn a_negative_id_survives_the_hyphen_at_either_end_of_a_run() {
        let set = NodeSet::from_iter(
            [-5, -4, -3, -1, 2, 3].map(|entry| NodeRef { conversation: 9, entry }),
        );

        let written = serde_json::to_string(&set).expect("it serialises");
        assert_eq!(written, r#"{"9":"-5--3,-1,2-3"}"#);

        let back: NodeSet = serde_json::from_str(&written).expect("it reads");
        assert_eq!(back, set);
    }

    #[test]
    fn an_entry_set_comes_back_from_its_runs() {
        let set: NodeSet =
            serde_json::from_str(r#"{"631":"0-3,5","636":"7"}"#).expect("it reads");

        assert_eq!(set.len(), 6);
        assert!(set.contains(&NodeRef { conversation: 631, entry: 3 }));
        assert!(!set.contains(&NodeRef { conversation: 631, entry: 4 }));
        assert!(set.contains(&NodeRef { conversation: 636, entry: 7 }));
    }

    /// An empty set is an empty object, not an absent field with a different meaning.
    #[test]
    fn an_empty_entry_set_survives_the_round_trip() {
        let text = serde_json::to_string(&NodeSet::default()).expect("it serialises");
        assert_eq!(text, "{}");

        let back: NodeSet = serde_json::from_str(&text).expect("it reads");
        assert!(back.is_empty());
    }

    /// The long shape still reads, so a fixture can spell out what it means.
    #[test]
    fn an_entry_set_still_accepts_a_list_of_entries() {
        let set: NodeSet = serde_json::from_str(
            r#"[{"conversation":631,"entry":4},{"conversation":631,"entry":5}]"#,
        )
        .expect("it reads");

        assert_eq!(set.len(), 2);
        assert!(set.contains(&NodeRef { conversation: 631, entry: 5 }));
    }

    /// A run list that is not understood is refused, not half-read.
    ///
    /// Skipping what it cannot parse would leave a world quietly answering "not seen" for
    /// entries the player has read, and the marker would then be wrong with nothing to say
    /// so.
    #[test]
    fn a_run_list_that_makes_no_sense_is_refused() {
        for bad in [r#"{"631":"0-x"}"#, r#"{"631":"9-3"}"#, r#"{"nope":"1"}"#] {
            assert!(
                serde_json::from_str::<NodeSet>(bad).is_err(),
                "'{bad}' was accepted",
            );
        }
    }

    /// The request and the response survive a round trip through JSON, which is the only
    /// form either of them ever travels in.
    #[test]
    fn a_request_survives_json() {
        let mut world = WorldSnapshot { money: 250, day_minutes: 720, ..Default::default() };
        world.variables.insert("x".to_string(), WireValue::Number { value: 3.0 });
        world.queries.insert("IsKimHere()".to_string(), WireValue::Bool { value: true });

        let request = LookAheadRequest {
            conversation: 631,
            starts: vec![NodeRef { conversation: 631, entry: 4 }],
            unseen_any_game: NodeSet::from_iter([NodeRef { conversation: 631, entry: 9 }]),
            unseen_this_game: NodeSet::default(),
            state_budget: 0,
            time_budget_ms: 0,
            memory_budget_mb: 0,
            world,
        };

        let text = serde_json::to_string(&request).expect("it serialises");
        let back: LookAheadRequest = serde_json::from_str(&text).expect("it comes back");

        assert_eq!(back.conversation, 631);
        assert_eq!(back.world.money, 250);
        assert!(back.unseen_any_game.contains(&NodeRef { conversation: 631, entry: 9 }));
        assert!(matches!(
            back.world.queries.get("IsKimHere()"),
            Some(WireValue::Bool { value: true }),
        ));
    }

    /// A memory budget in megabytes reaches the engine as bytes.
    ///
    /// Load-bearing, because the marker comes from here rather than from the managed
    /// engine: a budget the plugin configured and this engine ignored would be a dial
    /// connected to nothing, and the in-game suite that starves a search would stop
    /// testing anything at all.
    ///
    /// The one place the two units meet, and a factor of a million is the kind of mistake
    /// that turns a 256 MB allowance into a 256 byte one - which would stop every search
    /// instantly and look like the engine being broken rather than a unit being wrong.
    #[test]
    fn a_memory_budget_crosses_as_megabytes_and_arrives_as_bytes() {
        let request = LookAheadRequest {
            conversation: 1,
            starts: Vec::new(),
            unseen_any_game: NodeSet::default(),
            unseen_this_game: NodeSet::default(),
            state_budget: 0,
            time_budget_ms: 0,
            memory_budget_mb: 64,
            world: WorldSnapshot::default(),
        };

        assert_eq!(request.diagram_budget().memory(), 64 * 1024 * 1024);
    }

    /// Zero means the engine's own default rather than no budget at all.
    ///
    /// The opposite convention to the TIME budget, where zero means "the search's own
    /// pacing", and the difference is deliberate: a search with no clock still stops, and
    /// one with no memory limit is the thing this budget exists to prevent. An absent
    /// setting must not turn the protection off.
    #[test]
    fn an_unset_memory_budget_is_the_default_rather_than_none() {
        let request = LookAheadRequest {
            conversation: 1,
            starts: Vec::new(),
            unseen_any_game: NodeSet::default(),
            unseen_this_game: NodeSet::default(),
            state_budget: 0,
            time_budget_ms: 0,
            memory_budget_mb: 0,
            world: WorldSnapshot::default(),
        };

        assert_eq!(
            request.diagram_budget().memory(),
            DiagramBudget::DEFAULT_MEMORY_BUDGET,
        );
        assert!(request.diagram_budget().memory() > 0, "the default turned the budget off");
    }

    /// A time budget on the wire is the one the backward driver runs under.
    ///
    /// AND THE PER-CANDIDATE CAP IS KEPT UNDER IT. One candidate allowed longer than the
    /// whole search would make the player's setting decorative - the first candidate would
    /// spend it and the outer limit would never be consulted.
    #[test]
    fn a_time_budget_that_crosses_is_the_budget_the_search_runs_under() {
        let request = LookAheadRequest {
            conversation: 1,
            starts: Vec::new(),
            unseen_any_game: NodeSet::default(),
            unseen_this_game: NodeSet::default(),
            state_budget: 0,
            time_budget_ms: 250,
            memory_budget_mb: 0,
            world: WorldSnapshot::default(),
        };

        let budget = request.search_budget();
        assert_eq!(budget.backwards, std::time::Duration::from_millis(250));
        assert!(budget.each <= budget.backwards, "a candidate may not outlast the search");
        assert!(budget.forwards <= budget.backwards, "nor may the slice before it");
    }

    /// Zero means "this engine's default" for memory and "the search's own pacing" for time.
    ///
    /// The two zeros mean different things because the plugin's two settings do: the
    /// memory budget has a default it always applies, and the time budget documents zero
    /// as no limit of the player's. Reading either the other way would silently change what
    /// a player configured.
    #[test]
    fn a_budget_of_zero_means_what_the_plugins_setting_means() {
        let request = LookAheadRequest {
            conversation: 1,
            starts: Vec::new(),
            unseen_any_game: NodeSet::default(),
            unseen_this_game: NodeSet::default(),
            state_budget: 0,
            time_budget_ms: 0,
            memory_budget_mb: 0,
            world: WorldSnapshot::default(),
        };

        assert_eq!(
            request.diagram_budget().memory(),
            DiagramBudget::DEFAULT_MEMORY_BUDGET,
        );

        // No number of the player's, so every part of the search keeps its own pacing.
        assert_eq!(request.search_budget().backwards, portfolio::Budget::default().backwards);
        assert_eq!(request.search_budget().each, portfolio::Budget::default().each);
    }

    /// A value's wire form is what the other side has to write, so it is pinned here.
    #[test]
    fn a_value_looks_the_way_the_other_side_expects() {
        let rendered = serde_json::to_string(&WireValue::Bool { value: true }).unwrap();
        assert_eq!(rendered, r#"{"kind":"bool","value":true}"#);

        let rendered = serde_json::to_string(&WireValue::Unknown).unwrap();
        assert_eq!(rendered, r#"{"kind":"unknown"}"#);
    }

    /// An option the loaded group does not carry is answered, not fatal.
    #[test]
    fn an_unknown_start_does_not_fail_the_whole_request() {
        let mut symbols = StateSymbols::new();
        let _ = symbols.variable("unused");
        let node = LookAheadNode::new(
            DialogueNodeId::new(1, 0),
            false,
            DialogueCheckKind::None,
            GuardExpression::always_true(),
            Vec::new(),
            Vec::new(),
            0,
            false,
            false,
            -1,
            -1,
            false,
            -1,
        );
        let graph = LookAheadGraph::new(vec![node], symbols).unwrap();

        // Straight at the private path rather than through an index, because what is being
        // checked is the answer for a start the graph does not hold.
        assert!(graph.get(DialogueNodeId::new(1, 99)).is_none());
    }
}
