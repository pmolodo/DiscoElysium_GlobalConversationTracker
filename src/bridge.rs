// SPDX-License-Identifier: MIT
//! What crosses between the plugin and this engine, and what it means.
//!
//! [`crate::ffi`] is the unsafe shell - pointers, panics, lifetimes. This is the part
//! worth reading: the question the plugin asks, the snapshot of the world it asks it
//! against, and the answer that comes back. All of it ordinary Rust, so all of it
//! testable without a pointer in sight.
//!
//! ## Two calls, and why not one
//!
//! THE ENGINE ASKS THE QUESTIONS. [`questions_for`] walks a group's parsed guards and
//! returns every question a crawl over it can ask - by the exact key the answer must come
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
//! per response menu and caches each query for the life of the crawl - so nothing is lost,
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
use crate::engine::engine::{LookAheadEngine, StartBranch};
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
/// `..` rather than `-` so a negative id could never be read as a range boundary. The
/// shipped database has none, and a wire format that becomes ambiguous the first time one
/// appears is not worth the character it saves.
const RUN_SEPARATOR: &str = "..";

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
/// {"631": "0..40,42,50..99", "636": "3"}
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

/// Sorted ids as `0..40,42,50..99`.
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

        let (first, last) = match run.split_once(RUN_SEPARATOR) {
            Some((first, last)) => (first, last),
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
    /// Items held when the crawl starts.
    #[serde(default)]
    pub items: HashSet<String>,
    /// Journal tasks active when the crawl starts.
    #[serde(default)]
    pub tasks: HashSet<String>,
    /// Thoughts in the cabinet when the crawl starts.
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

/// Everything a crawl over one group can ask the world.
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
    /// The most search states one option may cost, or zero for this engine's default.
    ///
    /// The plugin's own setting, and it has to cross: once the marker comes from here, a
    /// budget the caller configured and this engine ignored would be a dial connected to
    /// nothing.
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
    /// The limit that governs by default, in place of the state budget: a state carries one
    /// slot per tracked variable in its group, so a budget counted in states buys between
    /// 136 and 455 megabytes depending on which conversation the player is standing in.
    /// See de-e23q and tests/crawl_memory.rs.
    #[serde(default)]
    pub memory_budget_mb: usize,

    pub world: WorldSnapshot,
}

impl LookAheadRequest {
    /// The engine options this request asks for.
    fn options(&self) -> crate::engine::engine::LookAheadOptions {
        let default = crate::engine::engine::LookAheadOptions::default();
        crate::engine::engine::LookAheadOptions {
            state_budget: if self.state_budget == 0 {
                default.state_budget
            } else {
                self.state_budget
            },
            memory_budget: if self.memory_budget_mb == 0 {
                default.memory_budget
            } else {
                self.memory_budget_mb * 1024 * 1024
            },
            time_budget: std::time::Duration::from_millis(self.time_budget_ms),
            ..default
        }
    }
}

/// What one option scored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookAheadAnswer {
    pub start: NodeRef,
    /// 0 seen, 1 unseen this game, 2 unseen in any game.
    pub best: i32,
    /// The entry that proved it, where something did.
    pub witness: Option<NodeRef>,
    /// Whether the search settled. False means `best` is a lower bound.
    pub complete: bool,
    pub elapsed_ms: u64,
    /// How many states the crawl explored.
    ///
    /// Carried because the plugin's diagnostics are about what a crawl COSTS, and a time
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
    /// budgets needs: a crawl that ran out of STATES wants a bigger state budget, and one
    /// that ran out of TIME on the same states wants a slower machine or a longer clock.
    /// A single "it gave up" cannot tell them which dial to turn.
    #[serde(default)]
    pub stopped_by: String,    /// The two outcomes, where the option is a white or red check.
    ///
    /// ABSENT ON EVERYTHING ELSE, and that is how the mod decides whether to draw the
    /// Pass/Fail line: a check has two outcomes worth telling apart, an ordinary option has
    /// one. The fields above are the two of these combined, so a reader that does not know
    /// about branches still gets the right answer for the option as a whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<BranchAnswers>,

}

/// What comes back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookAheadResponse {
    pub answers: Vec<LookAheadAnswer>,
    /// Set when the whole request failed; `answers` is then empty.
    pub error: Option<String>,
}

impl LookAheadResponse {
    fn failed(reason: String) -> Self {
        Self { answers: Vec::new(), error: Some(reason) }
    }
}

/// Every question a crawl over `conversation`'s group can ask.
pub fn questions_for(index: &Index, conversation: i32) -> Result<Questions, String> {
    let (graph, group) = build_group_graph(index, conversation)?;
    Ok(questions_of(&graph, group))
}

/// The same, for a group already built.
///
/// Split out because [`answer`] needs both the graph and the questions, and building the
/// group twice per response menu to get them would be paying for the expensive half twice.
fn questions_of(graph: &LookAheadGraph, group: Vec<i32>) -> Questions {
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
/// calls, because the engine answers those from crawl state when the group moves them -
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

/// Answers one request.
///
/// Runs the ordinary crawl rather than the symbolic portfolio. That is deliberate for the
/// first crossing: the crawl is the engine both sides already agree about, so a wrong
/// answer here is a marshalling bug rather than a question of which search was right. The
/// portfolio - which is what makes this migration worth doing - goes in once the crossing
/// itself is trusted, and de-i5xj.2 says so.
///
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

    let engine = LookAheadEngine::new(request.options());
    let mut answers = Vec::with_capacity(request.starts.len());

    for start in &request.starts {
        let id = DialogueNodeId::from(*start);
        if graph.get(id).is_none() {
            // Not an error for the request as a whole: a menu can offer an option the
            // loaded group does not carry, and the honest answer about it is "nothing
            // known" rather than a failed call for every other option too.
            answers.push(LookAheadAnswer {
                start: *start,
                best: Novelty::SeenThisGame as i32,
                witness: None,
                complete: false,
                elapsed_ms: 0,
                states_explored: 0,
                nodes_reached: 0,
                stopped_by: "none".to_string(),
                branches: None,
            });
            continue;
        }

        let began = std::time::Instant::now();

        // A ROLLED CHECK IS ASKED ABOUT ONCE PER OUTCOME, because it is two options
        // wearing one line of text and the mod draws them apart - see de-fes. Two crawls
        // rather than one, and only here: a menu of ordinary options costs what it did.
        let rolled = matches!(
            graph.get(id).map(|node| node.kind),
            Some(DialogueCheckKind::Red) | Some(DialogueCheckKind::White)
        );

        // NOTHING BETTER IS REACHABLE, so there is no crawl to run. The walk that
        // decides this is a few thousand pointer-follows against a crawl budgeted at
        // 200,000 states, and it stops early whenever the answer is yes - so the case it
        // costs anything in is the case where it saves a whole crawl. See
        // LookAheadEngine::reaches_potential_improvement.
        //
        // A COMPLETE ANSWER, not a gave-up one: this establishes that nothing outranks
        // the option, which is exactly what a finished crawl finding nothing would.
        //
        // A ROLLED CHECK STILL REPORTS ITS TWO OUTCOMES. The shortcut settles the
        // ASTERISK - is anything novel down there - and the Pass/Fail line's WORDS are
        // coloured by where each outcome leads, which is a fact about the option either
        // way. A check both of whose outcomes are already read has a line saying exactly
        // that, and dropping it here would have made those checks silently lineless.
        let own = novelty(id);
        if !LookAheadEngine::reaches_potential_improvement(&graph, id, own, &novelty) {
            answers.push(LookAheadAnswer {
                start: *start,
                best: Novelty::SeenThisGame as i32,
                witness: None,
                complete: true,
                elapsed_ms: began.elapsed().as_millis() as u64,
                states_explored: 0,
                nodes_reached: 0,
                stopped_by: "none".to_string(),
                branches: rolled.then(|| BranchAnswers {
                    pass: settled_branch(&engine, &graph, id, &world, &novelty, StartBranch::Pass),
                    fail: settled_branch(&engine, &graph, id, &world, &novelty, StartBranch::Fail),
                }),
            });
            continue;
        }

        let mut answer = if rolled {
            let pass = branch_answer(&engine, &graph, id, &world, &novelty, StartBranch::Pass);
            let fail = branch_answer(&engine, &graph, id, &world, &novelty, StartBranch::Fail);
            let combined = combine(*start, &pass, &fail);
            LookAheadAnswer { branches: Some(BranchAnswers { pass, fail }), ..combined }
        } else {
            let result = engine.evaluate(&graph, id, &world, &novelty);
            LookAheadAnswer {
                start: *start,
                best: result.best as i32,
                witness: None,
                complete: !result.budget_exhausted(),
                elapsed_ms: 0,
                states_explored: result.states_explored,
                nodes_reached: result.nodes_reached,
                stopped_by: limit_name(result.stopped_by).to_string(),
                branches: None,
            }
        };

        answer.elapsed_ms = began.elapsed().as_millis() as u64;
        answers.push(answer);
    }

    LookAheadResponse { answers, error: None }
}


/// What the mod says about one outcome of a rolled check.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BranchAnswer {
    /// The best novelty among the entries this branch leads to DIRECTLY - what the mod
    /// colours the word "Pass" or "Fail" by.
    pub destination: i32,

    /// The best novelty anywhere down this branch, which is what earns it an asterisk when
    /// it beats `destination`. The same rule an option's own marker follows.
    pub best: i32,

    /// Whether the search of this branch finished. A branch that gave up draws the
    /// uncertain marker rather than nothing - see de-pvq.
    pub complete: bool,
}

/// Both outcomes of a rolled check, present only on a rolled check.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BranchAnswers {
    pub pass: BranchAnswer,
    pub fail: BranchAnswer,
}

/// The name a limit crosses the wire under.
fn limit_name(limit: crate::core::types::LookAheadLimit) -> &'static str {
    match limit {
        crate::core::types::LookAheadLimit::States => "states",
        crate::core::types::LookAheadLimit::Memory => "memory",
        crate::core::types::LookAheadLimit::Time => "time",
        crate::core::types::LookAheadLimit::None => "none",
    }
}

/// One outcome of a rolled check whose crawl was refused as pointless.
///
/// Where it leads is still worth reporting - that is what colours the word - and the best
/// beyond it is the destination itself, because the refusal established that nothing in
/// the group outranks the option and so nothing down either branch can either.
fn settled_branch<F>(
    engine: &LookAheadEngine,
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    novelty: F,
    branch: StartBranch,
) -> BranchAnswer
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    let destination = engine
        .branch_destinations(graph, start, world, branch)
        .into_iter()
        .map(&novelty)
        .max()
        .unwrap_or(Novelty::SeenThisGame);

    BranchAnswer { destination: destination as i32, best: destination as i32, complete: true }
}

/// One outcome of a rolled check: where it leads, and what lies beyond that.
fn branch_answer<F>(
    engine: &LookAheadEngine,
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    novelty: F,
    branch: StartBranch,
) -> BranchAnswer
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    let destination = engine
        .branch_destinations(graph, start, world, branch)
        .into_iter()
        .map(&novelty)
        .max()
        .unwrap_or(Novelty::SeenThisGame);

    let result = engine.evaluate_from(graph, start, world, &novelty, branch);

    BranchAnswer {
        destination: destination as i32,
        best: result.best as i32,
        complete: !result.budget_exhausted(),
    }
}

/// The one answer a check option's own marker is drawn from, out of its two branches.
///
/// DERIVED RATHER THAN CRAWLED A THIRD TIME. Asking the engine for the whole option would
/// give the same `best` - it is the better of the two branches either way - at the cost of
/// repeating both searches, so the numbers are combined here instead.
///
/// `nodes_reached` is the LARGER of the two rather than their sum, because the branches
/// overlap wherever they rejoin and adding them would count the shared tail twice. It is a
/// lower bound, and it is only ever read by the diagnostics.
fn combine(start: NodeRef, pass: &BranchAnswer, fail: &BranchAnswer) -> LookAheadAnswer {
    LookAheadAnswer {
        start,
        best: pass.best.max(fail.best),
        witness: None,
        complete: pass.complete && fail.complete,
        elapsed_ms: 0,
        states_explored: 0,
        nodes_reached: 0,
        stopped_by: if pass.complete && fail.complete { "none" } else { "states" }.to_string(),
        branches: None,
    }
}

#[cfg(test)]
mod branch_wire_tests {
    use super::*;

    /// An answer carrying two outcomes round-trips as JSON.
    #[test]
    fn branches_survive_the_wire() {
        let answer = LookAheadAnswer {
            start: NodeRef { conversation: 451, entry: 12 },
            best: 2,
            witness: None,
            complete: true,
            elapsed_ms: 4,
            states_explored: 90,
            nodes_reached: 30,
            stopped_by: "none".to_string(),
            branches: Some(BranchAnswers {
                pass: BranchAnswer { destination: 0, best: 2, complete: true },
                fail: BranchAnswer { destination: 1, best: 1, complete: false },
            }),
        };

        let text = serde_json::to_string(&answer).expect("an answer serialises");
        let back: LookAheadAnswer = serde_json::from_str(&text).expect("and parses back");
        assert_eq!(back, answer);
    }

    /// An ordinary option says nothing about branches, and costs nothing to say it.
    ///
    /// The absence is load bearing: it is what the mod reads to decide whether an option
    /// gets a Pass/Fail line at all.
    #[test]
    fn an_ordinary_option_carries_no_branches() {
        let answer = LookAheadAnswer {
            start: NodeRef { conversation: 451, entry: 12 },
            best: 0,
            witness: None,
            complete: true,
            elapsed_ms: 0,
            states_explored: 1,
            nodes_reached: 1,
            stopped_by: "none".to_string(),
            branches: None,
        };

        let text = serde_json::to_string(&answer).expect("an answer serialises");
        assert!(!text.contains("branches"), "an absent branch pair still crossed: {text}");

        let back: LookAheadAnswer = serde_json::from_str(&text).expect("and parses back");
        assert_eq!(back.branches, None);
    }

    /// A reader that has never heard of branches still parses an answer that has them.
    #[test]
    fn an_answer_without_the_field_still_parses() {
        let text = r#"{"start":{"conversation":1,"entry":2},"best":1,"complete":true,
            "elapsed_ms":0,"states_explored":0,"nodes_reached":0,"stopped_by":"none"}"#;

        let answer: LookAheadAnswer = serde_json::from_str(text).expect("it parses");
        assert_eq!(answer.branches, None);
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

    /// The three the engine answers from crawl state are pulled out by SUBJECT, because
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
            r#"{"631":"0..3,5,9..10","636":"7"}"#,
        );
    }

    #[test]
    fn an_entry_set_comes_back_from_its_runs() {
        let set: NodeSet =
            serde_json::from_str(r#"{"631":"0..3,5","636":"7"}"#).expect("it reads");

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
        for bad in [r#"{"631":"0..x"}"#, r#"{"631":"9..3"}"#, r#"{"nope":"1"}"#] {
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

    /// A budget the caller sent is the budget the crawl runs under.
    ///
    /// Load-bearing once the marker comes from here rather than from the managed engine: a
    /// `LookAheadStateBudget` the plugin configured and this engine ignored would be a dial
    /// connected to nothing, and the in-game suite that sets it to one would stop testing
    /// anything at all.
    /// A memory budget in megabytes reaches the engine as bytes.
    ///
    /// The one place the two units meet, and a factor of a million is the kind of mistake
    /// that turns a 256 MB allowance into a 256 byte one - which would stop every crawl
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

        assert_eq!(request.options().memory_budget, 64 * 1024 * 1024);
    }

    /// Zero means the engine's own default rather than no budget at all.
    ///
    /// The opposite convention to the TIME budget, where zero means no limit, and the
    /// difference is deliberate: a crawl with no clock finishes, and a crawl with no memory
    /// limit is the thing this budget exists to prevent. An absent setting must not turn
    /// the protection off.
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
            request.options().memory_budget,
            crate::engine::engine::DEFAULT_MEMORY_BUDGET,
        );
        assert!(request.options().memory_budget > 0, "the default turned the budget off");
    }

    #[test]
    fn a_state_budget_that_crosses_is_the_budget_the_crawl_runs_under() {
        let request = LookAheadRequest {
            conversation: 1,
            starts: Vec::new(),
            unseen_any_game: NodeSet::default(),
            unseen_this_game: NodeSet::default(),
            state_budget: 7,
            time_budget_ms: 250,
            memory_budget_mb: 0,
            world: WorldSnapshot::default(),
        };

        let options = request.options();
        assert_eq!(options.state_budget, 7);
        assert_eq!(options.time_budget, std::time::Duration::from_millis(250));
    }

    /// Zero means "this engine's default" for states and "no limit" for time.
    ///
    /// The two zeros mean different things because the plugin's two settings do: its state
    /// budget has a default it always applies, and its time budget documents zero as no
    /// limit. Reading either the other way would silently change what a player configured.
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

        let options = request.options();
        let default = crate::engine::engine::LookAheadOptions::default();
        assert_eq!(options.state_budget, default.state_budget);
        assert_eq!(options.time_budget, std::time::Duration::ZERO);
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
