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
//! every call identically, forever, including how a number is formatted
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

use crate::core::equipment;
use crate::core::guard::{Guard, GuardExpression};
use crate::core::guard_value::{GuardValue, GuardValueKind};
use crate::core::state::{ITEM_PREFIX, TASK_PREFIX, THOUGHT_PREFIX, VariableRef};
use crate::core::types::StartBranch;
use crate::core::types::{DialogueCheckKind, DialogueNodeId, Novelty, Ternary};
use crate::formats::runs;
use crate::graph::LookAheadGraph;
use crate::index::{Index, VariableTable, build_group_graph};
use crate::symbolic::answer;
use crate::symbolic::budget::DiagramBudget;
use crate::symbolic::data_layout::DataLayout;
use crate::symbolic::guard_formula::GuardCompiler;
use crate::symbolic::isolated;
use crate::symbolic::known::GroupShape;
use crate::symbolic::novelty_search;
use crate::symbolic::reachability::seed_of;
use crate::symbolic::vars::DataVars;
use crate::world::ILookAheadWorld;
use oxidd::bdd::BDDFunction;

/// One entry, as it crosses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeRef {
    pub conversation: i32,
    pub entry: i32,
}

impl From<DialogueNodeId> for NodeRef {
    fn from(id: DialogueNodeId) -> Self {
        Self {
            conversation: id.conversation_id,
            entry: id.entry_id,
        }
    }
}

impl From<NodeRef> for DialogueNodeId {
    fn from(node: NodeRef) -> Self {
        DialogueNodeId::new(node.conversation, node.entry)
    }
}

/// The run separator inside a [`NodeSet`]'s entry list, for the one message that quotes it.
///
/// The spelling itself is [`crate::formats::runs`], which is what every file in this
/// repository uses - the sparse saves, their diffs, and the mod's own state file. The wire
/// reads and writes through it rather than saying the same thing a second way.
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
            by_conversation
                .entry(node.conversation)
                .or_default()
                .push(node.entry);
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
        Self {
            nodes: nodes.into_iter().collect(),
        }
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
                        set.insert(NodeRef {
                            conversation,
                            entry,
                        });
                    }
                }
                Ok(set)
            }
        }
    }
}

/// Sorted ids as `0-40,42,50-99`.
fn write_runs(entries: &[i32]) -> String {
    let widened: Vec<i64> = entries.iter().map(|entry| i64::from(*entry)).collect();
    runs::pack(&widened)
}

/// Reads back what [`write_runs`] wrote.
///
/// Refuses anything it does not understand rather than skipping it. A run list that is
/// silently half-read is a world that quietly answers "not seen" for entries the player
/// has read, and the marker is then wrong with nothing to say so.
///
/// TWO REFUSALS OF ITS OWN, on top of the spelling itself. An entry set's ids come out of a
/// sorted collection and are entry ids, so a run that COUNTS DOWN or a bound too large to
/// BE an entry id is a document that is not what it claims to be - where in a save's
/// dialogue variables a descending run is the ordinary case.
fn read_runs(text: &str) -> Result<Vec<i32>, String> {
    let mut entries = Vec::new();
    for (first, last) in runs::bounds(text, "an entry set").map_err(|fault| fault.to_string())? {
        if last < first {
            return Err(format!("'{first}{RUN_SEPARATOR}{last}' runs backwards"));
        }

        let widen =
            |bound: i64| i32::try_from(bound).map_err(|_| format!("'{bound}' is not an entry id"));
        entries.extend(widen(first)?..=widen(last)?);
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
    /// Whether a thought forces every red check to fail - see
    /// [`ILookAheadWorld::red_check_may_pass`].
    #[serde(default)]
    pub red_checks_fail: bool,
    /// What was read for each request, by the request that asked for it.
    ///
    /// Named rather than positional, the way [`Self::variables`] is: a fixture or a test can
    /// say what it means, and what it says wins over a positional answer.
    #[serde(default)]
    pub data: HashMap<DataRequest, DataAnswer>,
    /// The same answers, in the order [`Questions::data`] listed the requests.
    ///
    /// POSITIONAL, like [`Self::variable_values`] and for the same reason: the requests are
    /// constant for a group and the plugin already holds them, so sending them back would be
    /// sending back what the engine itself said. A list of the wrong LENGTH is refused
    /// outright by [`Self::resolve`].
    #[serde(default)]
    pub data_values: Vec<DataAnswer>,
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
        place(
            "variable",
            &questions.variables,
            &self.variable_values,
            &mut self.variables,
        )?;
        place(
            "query",
            &questions.queries,
            &self.query_values,
            &mut self.queries,
        )?;
        place_data(&questions.data, &self.data_values, &mut self.data)?;
        self.variable_values.clear();
        self.query_values.clear();
        self.data_values.clear();
        Ok(())
    }
}

/// Names `answers` by the requests that asked for them, without disturbing anything `named`
/// already says.
///
/// The same three rules as [`place`], for the same reasons: an empty list is a caller that
/// did not use the channel, a wrong-length one is refused rather than zipped as far as it
/// goes, and an answer already named wins over a positional one.
fn place_data(
    asked: &[DataRequest],
    answers: &[DataAnswer],
    named: &mut HashMap<DataRequest, DataAnswer>,
) -> Result<(), String> {
    if answers.is_empty() {
        return Ok(());
    }

    if answers.len() != asked.len() {
        return Err(format!(
            "{} data answers came back for {} requests; the caller is answering a \
             different list of them than this group asks",
            answers.len(),
            asked.len(),
        ));
    }

    for (request, answer) in asked.iter().zip(answers) {
        named
            .entry(request.clone())
            .or_insert_with(|| answer.clone());
    }

    Ok(())
}

/// Answers every failed white check's failure slot as true, among the named variables.
///
/// THE ONE PLACE A LOCK BECOMES A VARIABLE, for the wire and for the offline fixtures alike.
/// The game keeps a failed white check in a table of its own rather than in Lua, and refuses
/// it while it stays there; the engine closes a check whose failure slot is set. So the lock
/// is answered as that slot, named - and a named variable wins over a positional answer, so
/// nothing the plugin reads from Lua can reopen it. See [`crate::index::FAILED_FLAG_SUFFIX`].
pub fn lock_failed_white_checks(
    variables: &mut HashMap<String, WireValue>,
    flags: impl IntoIterator<Item = String>,
) {
    for flag in flags {
        variables.insert(
            format!("{flag}{}", crate::index::FAILED_FLAG_SUFFIX),
            WireValue::Bool { value: true },
        );
    }
}

/// Why an option on the menu is closed when the search starts, though the player could open
/// it.
#[derive(Clone, Copy)]
enum Lock<'w> {
    /// A check the save has failed: its failure slot, answered true.
    FailedCheck(&'w str),
    /// A price the purse cannot cover.
    Unaffordable(i32),
    /// A red check whose roll a thought forces to fail, which locks its Pass half only.
    RedPassForbidden(DialogueNodeId),
}

/// A world that lifts every [`Lock`] on one option, and answers everything else as `inner`
/// does.
///
/// FOR SEEDING A LOCKED OPTION'S HALVES AS IF IT WERE OPEN, and for nothing else - see
/// [`answer_starts`]. Lifting the locks for the option's own start lets its words say what
/// opening it would reach - a failed check's slot answered unset, a purse that covers the
/// price - while every route through it from anywhere else stays shut. All of them, because
/// an option locked twice and lifted once is still closed.
struct Unlocked<'w> {
    inner: &'w dyn ILookAheadWorld,
    locks: &'w [Lock<'w>],
}

impl ILookAheadWorld for Unlocked<'_> {
    fn money(&self) -> i32 {
        self.locks
            .iter()
            .filter_map(|lock| match lock {
                Lock::Unaffordable(price) => Some(*price),
                Lock::FailedCheck(_) | Lock::RedPassForbidden(_) => None,
            })
            .fold(self.inner.money(), i32::max)
    }

    fn day_minutes(&self) -> i32 {
        self.inner.day_minutes()
    }

    fn day_counter(&self) -> i32 {
        self.inner.day_counter()
    }

    fn is_clock_locked(&self) -> bool {
        self.inner.is_clock_locked()
    }

    fn get_variable(&self, variable: VariableRef<'_>) -> GuardValue {
        if self
            .locks
            .iter()
            .any(|lock| matches!(lock, Lock::FailedCheck(failed) if *failed == variable.name()))
        {
            GuardValue::from_boolean(false)
        } else {
            self.inner.get_variable(variable)
        }
    }

    fn initially_has_item(&self, name: &str) -> bool {
        self.inner.initially_has_item(name)
    }

    fn initially_task_active(&self, name: &str) -> bool {
        self.inner.initially_task_active(name)
    }

    fn initially_has_thought(&self, name: &str) -> bool {
        self.inner.initially_has_thought(name)
    }

    fn query(&self, name: &str, arguments: &[GuardValue]) -> GuardValue {
        self.inner.query(name, arguments)
    }

    fn check_passes(&self, node: DialogueNodeId) -> Ternary {
        self.inner.check_passes(node)
    }

    fn is_seen(&self, node: DialogueNodeId) -> bool {
        self.inner.is_seen(node)
    }

    fn red_check_may_pass(&self, node: DialogueNodeId) -> bool {
        self.locks
            .iter()
            .any(|lock| matches!(lock, Lock::RedPassForbidden(option) if *option == node))
            || self.inner.red_check_may_pass(node)
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
        Self {
            snapshot,
            declared: None,
        }
    }

    /// The same, falling back to the database's declared variables.
    pub fn declaring(snapshot: WorldSnapshot, declared: Option<Arc<VariableTable>>) -> Self {
        Self { snapshot, declared }
    }

    /// Whether `subject` is in the set `kind` answered with.
    ///
    /// `None` where nothing answered that request at all, which stays Unknown rather than
    /// reading as "not in the set": a set nobody sent is not an empty set.
    fn in_set(&self, kind: DataKind, subject: &str) -> Option<bool> {
        let answer = self.snapshot.data.get(&DataRequest::set(kind))?;
        // NOT READ IS NOT EMPTY. A plugin that could not reach the cabinet sends an answer
        // saying so, and treating that as an empty set would answer "not in it" for
        // everything - which closes routes rather than opening them.
        if !answer.read {
            return None;
        }

        Some(answer.names.iter().any(|name| name == subject))
    }

    /// The item one equipment slot holds - empty for an empty slot - or `None` where the
    /// slot was not read.
    fn in_slot(&self, slot: &str) -> Option<&str> {
        let answer = self
            .snapshot
            .data
            .get(&DataRequest::about(DataKind::EquippedInSlot, slot))?;
        if !answer.read {
            return None;
        }

        match &answer.value {
            WireValue::Text { value } => Some(value),
            _ => None,
        }
    }
}

/// The sets a cabinet question is answered from, or `None` for anything else.
///
/// `IsTHCCookingOrFixed` is two sets because that is what it is - cooking with a fallthrough
/// to fixed - and keeping it as a pair here is what stops it being confused with
/// `IsTHCPresent`, which is the BROADER question and has burnt this code once already.
fn thought_state_kinds(name: &str) -> Option<&'static [DataKind]> {
    match name {
        "IsTHCCooking" => Some(&[DataKind::ThoughtsCooking]),
        "IsTHCFixed" => Some(&[DataKind::ThoughtsFixed]),
        "IsTHCCookingOrFixed" => Some(&[DataKind::ThoughtsCooking, DataKind::ThoughtsFixed]),
        _ => None,
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

    fn get_variable(&self, variable: VariableRef<'_>) -> GuardValue {
        let name = variable.name();
        let answered = self.snapshot.variables.get(name).map(GuardValue::from);
        if let Some(value) = answered
            && value.kind() != GuardValueKind::Unknown
        {
            return value;
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
        // THE CABINET'S NARROW QUESTIONS, answered from the sets the plugin enumerated.
        // Nothing asks it to evaluate these any more - see `collect` - so there is no query
        // key to fall back to, and a set nobody sent leaves the question Unknown.
        if let Some(kinds) = thought_state_kinds(name) {
            let Some(subject) = arguments
                .first()
                .filter(|value| value.kind() == GuardValueKind::Text)
                .map(|value| value.text())
            else {
                return GuardValue::unknown();
            };

            let mut answered = false;
            for kind in kinds.iter().copied() {
                match self.in_set(kind, subject) {
                    // IN ANY OF THEM IS ENOUGH, which is what cooking-or-fixed means and is
                    // the only case for the other two.
                    Some(true) => return GuardValue::from_boolean(true),
                    Some(false) => answered = true,
                    None => {}
                }
            }

            return if answered {
                GuardValue::from_boolean(false)
            } else {
                GuardValue::unknown()
            };
        }

        // WHAT IS WORN, answered from the slots the plugin read. Like the cabinet, there is
        // no query key behind it to fall back to.
        if name == equipment::CHECK_EQUIPPED {
            let item = match arguments {
                [value] if value.kind() == GuardValueKind::Text => value.text(),
                _ => return GuardValue::unknown(),
            };

            return equipment::is_equipped(item, |slot| self.in_slot(slot))
                .map_or_else(GuardValue::unknown, GuardValue::from_boolean);
        }

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

    fn red_check_may_pass(&self, _node: DialogueNodeId) -> bool {
        !self.snapshot.red_checks_fail
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

/// One thing the engine wants READ rather than evaluated.
///
/// The other way it asks - [`Questions::queries`] - hands over a rendered Lua call for the
/// plugin to run. That is exact, because the game answers it, and it is also why a guard
/// calling `FinishTask` closed a journal task every time a menu opened. A kind names data
/// instead, so servicing one is a read and cannot be anything else.
///
/// `subject` names the one thing a per-subject kind asks about, and is empty for a kind that
/// answers with a whole set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataKind {
    /// The thoughts the cabinet is working on - `CharacterThoughts.cookingEffects`.
    ThoughtsCooking,
    /// The thoughts already internalised - `CharacterThoughts.fixedEffects`.
    ThoughtsFixed,
    /// The item in one equipment slot, named in the subject by its `EquipmentSlotType` name -
    /// `InventoryViewData.GetEquipped`. Answered as text: the item, or empty for an empty
    /// slot. See [`crate::core::equipment`].
    EquippedInSlot,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DataRequest {
    pub kind: DataKind,
    /// What it is about, for a kind that names one thing.
    ///
    /// A set-valued kind leaves this empty. [`DataKind::EquippedInSlot`] names the slot.
    ///
    /// A STRING, though the possible values are as closed a set as [`DataKind`] is: item and
    /// thought names are DERIVED GAME DATA, versioned with the content rather than with this
    /// crate, and they already cross as strings in [`Questions::items`], [`Questions::tasks`]
    /// and [`Questions::thoughts`]. A kind is this protocol's own vocabulary, which is what
    /// makes it an enum and this not.
    #[serde(default)]
    pub subject: String,
}

impl DataRequest {
    /// A request for a whole set, which names no subject.
    ///
    /// A SET RATHER THAN A QUESTION PER SUBJECT where the game holds one: the cabinet's
    /// cooking and fixed thoughts are two collections the plugin walks once, which is both
    /// cheaper than a request per thought and the shape a save already stores them in.
    pub fn set(kind: DataKind) -> Self {
        Self {
            kind,
            subject: String::new(),
        }
    }

    /// A request about one named subject.
    pub fn about(kind: DataKind, subject: &str) -> Self {
        Self {
            kind,
            subject: subject.to_string(),
        }
    }
}

/// What an unanswered data request carries.
fn unknown_value() -> WireValue {
    WireValue::Unknown
}

/// What the plugin found for one [`DataRequest`].
///
/// Two shapes in one, because a kind answers in one or the other and which it uses is part
/// of what the kind means. Neither present is "not knowable", the same permissive default an
/// unanswered query carries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataAnswer {
    /// For a kind that answers about one subject.
    ///
    /// [`WireValue::Unknown`] where the plugin could not say, which is the permissive answer
    /// and is spelled the one way the rest of the wire spells it - there is deliberately no
    /// second way to mean "no answer" here.
    #[serde(default = "unknown_value")]
    pub value: WireValue,
    /// For a kind that answers with a set of names.
    #[serde(default)]
    pub names: Vec<String>,
    /// Whether the request was serviced at all.
    ///
    /// EXPLICIT, because an empty [`Self::names`] means two different things without it: a
    /// set that was read and is empty, and a set nobody could read. The first says the
    /// subject is not in it; the second is not knowable. Reading the second as the first
    /// CLOSES a route the game opens, and this engine is only allowed to be wrong the other
    /// way.
    #[serde(default)]
    pub read: bool,
}

impl Default for DataAnswer {
    fn default() -> Self {
        Self {
            value: WireValue::Unknown,
            names: Vec::new(),
            read: false,
        }
    }
}

impl DataAnswer {
    /// An answer that is a whole set of names, read successfully.
    pub fn of_names(names: impl IntoIterator<Item = String>) -> Self {
        Self {
            value: WireValue::Unknown,
            names: names.into_iter().collect(),
            read: true,
        }
    }

    /// An answer about one subject, read successfully.
    pub fn of_value(value: WireValue) -> Self {
        Self {
            value,
            names: Vec::new(),
            read: true,
        }
    }
}

/// Everything a search over one group can ask the world.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Questions {
    /// The conversations the group covers, so the plugin knows what it committed to.
    pub conversations: Vec<i32>,
    /// Dialogue variables some guard reads or the search is seeded from.
    pub variables: Vec<String>,
    /// World queries, by the key their answers must come back under.
    pub queries: Vec<String>,
    /// Items some guard asks about or the search is seeded from.
    pub items: Vec<String>,
    /// Journal tasks some guard asks about or the search is seeded from.
    pub tasks: Vec<String>,
    /// Thoughts some guard asks about or the search is seeded from.
    pub thoughts: Vec<String>,
    /// Entries carrying a skill check, whose outcome the world decides.
    pub checks: Vec<NodeRef>,
    /// Every entry, because any of them may have been seen.
    pub entries: Vec<NodeRef>,
    /// World state the engine wants read rather than evaluated.
    #[serde(default)]
    pub data: Vec<DataRequest>,
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

    /// The longest the WHOLE MENU may run for in milliseconds; zero for no limit.
    ///
    /// ## Why one option's budget is not enough
    ///
    /// [`Self::time_budget_ms`] bounds one option and a request is a whole menu, so what a
    /// player waits for is the SUM - and until de-dt75.3 nothing bounded the sum. The
    /// per-option setting's own help text said as much: "a menu draws one of these per
    /// option, so a menu's worst case is this times the number of options". A rolled check
    /// is two searches rather than one, so the worst case is twice the option count times
    /// the dial: about twenty seconds for a wide menu at the shipped thousand milliseconds.
    ///
    /// THE THING THAT WAS WAITING ON IT IS A RESPONSE MENU BEING DRAWN, and the host's read
    /// deadline - thirty seconds, and not a budget: crossing it KILLS the engine - was the
    /// only thing further out. The margin between a slow menu and a dead engine was about
    /// one option.
    ///
    /// ## What it does to an option it cannot afford
    ///
    /// Nothing established, reported as an ordinary gave-up answer - so the mod draws the
    /// UNCERTAIN marker, the same one an option whose own budget ran out gets, and for the
    /// same reason: a search ran against it and did not finish, which is not the same claim
    /// as "there is nothing down there". See de-pvq for why that distinction is drawn at all
    /// and [`answer_starts`] for the arithmetic and for the ordering consequence, which is
    /// real and is stated there rather than hidden.
    #[serde(default)]
    pub menu_time_budget_ms: u64,

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

    /// What this conversation has shown the player since it started, oldest first: every line
    /// displayed and every option chosen. It must begin at the conversation's start, since the
    /// hubs are followed from there.
    ///
    /// WHERE THE PLAYER HAS BEEN, which a menu's options cannot say. An option whose only
    /// route to unread content runs back through what the player has passed since the hubs
    /// they are inside is looping back rather than leading onward - see
    /// [`crate::symbolic::hub`] for how the hubs are followed, and
    /// [`crate::symbolic::menu::mark_onward`] for what is cut. Empty says nothing about where
    /// the player is, and the onward question then cuts the menu's siblings alone.
    #[serde(default)]
    pub encountered: Vec<NodeRef>,

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

    /// How long the search may take, and how that is divided.
    ///
    /// THE PLAYER SETS ONE NUMBER and it means the whole answer, so the backward driver
    /// gets it and so does a candidate - a candidate allowed longer than the whole search
    /// would make the outer limit decorative, and one allowed less only makes the search
    /// give up sooner, which is what [`answer::Budget::each`] is about.
    ///
    /// PUBLIC SO THE WALL CAN BE ASSERTED, since de-cluo. What the dial produces is a claim
    /// made to the player - "the longest one option's look-ahead may run for" - and
    /// tests/time_budget_binds.rs pins its SHAPE rather than timing a real search, because a
    /// timing test on a busy machine fails for reasons that are nobody's fault.
    pub fn search_budget(&self) -> answer::Budget {
        let default = answer::Budget::default();

        // THE STATE BUDGET IS THE KNOB THAT STARVES A SEARCH, and that is all it ever was:
        // a test-only setting, not on the wire for players, whose whole job is to make a
        // search give up on demand so the mod's uncertain marker can be checked. See
        // `Self::state_budget` and `tests/branch_shapes.rs`.
        //
        // It is spent on the one ration that can produce that: no time to finish a
        // candidate, which is what a search that cannot establish anything looks like from
        // here. Any value above zero means the same thing, and means nothing else.
        if self.state_budget > 0 {
            return answer::Budget {
                // THE ATTEMPT KEEPS A CLOCK. A wall of zero would stop the loop before its
                // first candidate, so the search would give up without ever running a pass -
                // a different failure from the one this setting exists to provoke.
                overall: default.backwards,
                backwards: default.backwards,
                each: std::time::Duration::ZERO,
            };
        }

        if self.time_budget_ms == 0 {
            return default;
        }

        let whole = std::time::Duration::from_millis(self.time_budget_ms);
        answer::Budget {
            // THE PLAYER'S NUMBER IS THE WALL, which is what they were told it was. de-cluo:
            // it used to be the backward ration alone, with a candidate allowed to overrun it
            // by a whole `each` - so a dial set to 1000 could return at about 1300. The
            // rations below stay estimates of what each part should need, and are narrowed to
            // what is left.
            overall: whole,
            backwards: whole,
            // THE DIAL, NOT A SLIVER OF IT. A candidate is held to the wall and to nothing
            // narrower, so a player who raises the number gives it to the pass that needs
            // it. The driver still narrows this to the time left when the pass begins.
            each: whole,
        }
    }

    /// The longest the whole menu may take, or `Duration::MAX` where no limit was asked for.
    ///
    /// `Duration::MAX` RATHER THAN AN `Option`, because it is the identity of the one thing
    /// this is ever used for - `min` against a per-option ration - so the no-limit case and
    /// the ordinary one take the same line. An option would put a match at every use of it
    /// to say "and otherwise, do nothing".
    ///
    /// See [`LookAheadRequest::menu_time_budget_ms`] for what the limit is FOR, and
    /// [`answer_starts`] for how it is spent.
    pub fn menu_budget(&self) -> std::time::Duration {
        if self.menu_time_budget_ms == 0 {
            return std::time::Duration::MAX;
        }
        std::time::Duration::from_millis(self.menu_time_budget_ms)
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
        Self {
            answers: Vec::new(),
            error: Some(reason),
        }
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
    let mut found = Questions {
        conversations: group,
        ..Default::default()
    };
    let mut queries = HashSet::new();
    let mut items = HashSet::new();
    let mut tasks = HashSet::new();
    let mut thoughts = HashSet::new();
    let mut data = HashSet::new();

    for node in graph.nodes() {
        collect(
            &node.guard,
            &mut queries,
            &mut items,
            &mut tasks,
            &mut thoughts,
            &mut data,
        );

        found.entries.push(NodeRef::from(node.id));
        if node.kind != DialogueCheckKind::None {
            found.checks.push(NodeRef::from(node.id));
        }
    }

    // EVERY SUBJECT THE SEARCH IS SEEDED FROM, and not only what a guard names. `seed_state`
    // reads each slot from the world before the first state exists, and an item, task or
    // thought an action moves has a slot whether or not a guard mentions it. One left out
    // of the questions is never answered by the plugin, and reads as not held.
    let symbols = graph.symbols();
    for name in (0..symbols.count()).filter_map(|slot| symbols.name_of(slot)) {
        if let Some(item) = name.strip_prefix(ITEM_PREFIX) {
            items.insert(item.to_string());
        } else if let Some(task) = name.strip_prefix(TASK_PREFIX) {
            tasks.insert(task.to_string());
        } else if let Some(thought) = name.strip_prefix(THOUGHT_PREFIX) {
            thoughts.insert(thought.to_string());
        }
    }

    // THE GROUP'S VARIABLES, which are the only names the engine can read from the world -
    // see `VariableRef` - so asking for exactly these is asking for everything it reads.
    // Already sorted, which is the order their ids number them in.
    found.variables = symbols.variables().to_vec();

    // SORTED, and that is load-bearing rather than tidy. The plugin caches this list
    // against a conversation and answers it POSITIONALLY - see
    // `WorldSnapshot::variable_values` - so the order is the agreement between the two
    // sides, and a list that reordered itself between two calls would silently move every
    // answer onto the wrong question.
    found.queries = sorted(queries);
    found.items = sorted(items);
    found.tasks = sorted(tasks);
    found.thoughts = sorted(thoughts);
    // SORTED for the same reason the rest are: the answers come back positionally, so the
    // order is the agreement between the two sides.
    found.data = {
        let mut requests: Vec<DataRequest> = data.into_iter().collect();
        requests.sort_by(|a, b| (a.kind as i32, &a.subject).cmp(&(b.kind as i32, &b.subject)));
        requests
    };
    found
        .entries
        .sort_by_key(|node| (node.conversation, node.entry));
    found
        .checks
        .sort_by_key(|node| (node.conversation, node.entry));

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
    guard: &Guard,
    queries: &mut HashSet<String>,
    items: &mut HashSet<String>,
    tasks: &mut HashSet<String>,
    thoughts: &mut HashSet<String>,
    data: &mut HashSet<DataRequest>,
) {
    // A SWEEP RATHER THAN A WALK. Every node contributes wherever it sits, so this needs
    // the shape of one node at a time and never the shape between two.
    for node in guard.nodes() {
        match node.expression() {
            GuardExpression::Call(name, args) => {
                let subject = args.only().and_then(|only| match only.expression() {
                    GuardExpression::Literal(value) if value.kind() == GuardValueKind::Text => {
                        Some(value.text().to_string())
                    }
                    _ => None,
                });

                match (name, subject) {
                    ("CheckItem", Some(subject)) => {
                        items.insert(subject);
                    }
                    ("IsTaskActive", Some(subject)) => {
                        tasks.insert(subject);
                    }
                    ("IsTHCPresent", Some(subject)) => {
                        thoughts.insert(subject);
                    }
                    // `FlagSet(name)` and `FlagNotSet(name)` are `Variable[name]` written
                    // another way, and a flag named by a literal is one of the group's
                    // variables, which are asked for whole rather than here.
                    (flag, Some(_)) if crate::world::flag_query(flag).is_some() => {}
                    // An ACTION the database calls from a guard. Never asked of the plugin,
                    // because the plugin answers a query by RUNNING it - and running these
                    // finishes a task or awards experience in the player's save. Answered
                    // instead by `BoundContext::query`; see
                    // `modelling::ACTIONS_USED_AS_GUARDS`.
                    (other, _) if crate::core::modelling::is_action_used_as_guard(other) => {}
                    // A reputation question, answered from the reputation VARIABLES rather
                    // than asked as a call - the group declares the whole range it compares,
                    // so the plugin is already sending every amount it needs. Asked as a
                    // call it would be answered once from the world and go stale the moment
                    // the group's own ReputationGrows moved one.
                    (other, _) if crate::core::reputation::range_of(other).is_some() => {}
                    // THE CABINET'S NARROW QUESTIONS, answered from sets the plugin
                    // ENUMERATES rather than from a call per thought. Asked as calls these
                    // were three Lua runs per subject; asked as data they are at most two
                    // reads for the whole group, and the same two the save already records.
                    //
                    // THE SUBJECT IS ASKED ABOUT TOO, because the plugin cannot walk the
                    // cabinet whole and builds each set over the group's named thoughts. A
                    // thought left off that list is never looked at, and reads as not in
                    // the set - which closes a route the game opens.
                    (name, subject) if thought_state_kinds(name).is_some() => {
                        let kinds = thought_state_kinds(name).expect("just matched");
                        data.extend(kinds.iter().copied().map(DataRequest::set));
                        thoughts.extend(subject);
                    }
                    // WHAT IS WORN, read slot by slot rather than asked as a call per item:
                    // the answer is the equipment table itself, and the same few reads serve
                    // every item a group asks about. See `core::equipment`.
                    (equipment::CHECK_EQUIPPED, _) => {
                        data.extend(
                            equipment::SLOTS
                                .iter()
                                .map(|slot| DataRequest::about(DataKind::EquippedInSlot, slot)),
                        );
                    }
                    _ => {
                        // Only literal arguments can be answered ahead of time. A computed
                        // argument would have to be evaluated per state, which is exactly
                        // what a snapshot cannot do - so it is left out, reads Unknown, and
                        // the guard turns permissive.
                        let values: Option<Vec<GuardValue>> = args
                            .iter()
                            .map(|arg| match arg.expression() {
                                GuardExpression::Literal(value) => Some(value.clone()),
                                _ => None,
                            })
                            .collect();
                        if let Some(values) = values {
                            queries.insert(query_key(name, &values));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Answers one request, from the symbolic search.
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
    let mut conversations: Vec<i32> = request
        .starts
        .iter()
        .map(|start| start.conversation)
        .collect();
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
    let answers =
        isolated::on_its_own_thread_caught(|| answer_within(&graph, &world, request, &novelty));

    match answers {
        Ok(Some(answers)) => LookAheadResponse {
            answers,
            error: None,
        },
        // THE MACHINE, not the budget: the manager preallocates its node store and that
        // allocation aborts rather than failing, so it is asked for fallibly first - see
        // de-0a3a. Every option is answered "nothing established" rather than the request
        // failing, because a menu with no markers is what a mod without an engine draws
        // and the player has seen it before.
        Ok(None) => LookAheadResponse {
            answers: all_unanswered(request, "no-ram"),
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
                answers: all_unanswered(request, "crashed"),
                error: None,
            }
        }
    }
}

/// How high a counter is modelled before it saturates.
///
/// SIXTEEN, AND EVERY MEASUREMENT IN THE REPOSITORY WAS MADE AT IT -
/// `measurements/menu_matrix.rs` and the symbolic tests all use this number. Changing it
/// changes which states a search can tell apart, so a run measured under one cap says
/// nothing about a search under another.
pub const COUNTER_CAP: i32 = 16;

/// The answers for one request, from inside the thread that owns the diagram.
///
/// `None` where the diagram's memory did not stretch to a starting point: the machine
/// could not supply the node store, or it could and the seed still would not fit in it.
/// Either is a fact about the memory rather than about any option, so it is reported once
/// for all of them - and the caller draws the menu unmarked, which is what it does for a
/// missing engine too.
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
    let layout =
        DataLayout::for_group_entered_at(graph, world, COUNTER_CAP, Some(&entered_at_of(request)));
    let vars = DataVars::try_new(&layout, &symbols, request.diagram_budget())?;
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, world, &vars)?;

    // ONCE FOR THE MENU, like the manager and the compiler above. The parent map and the
    // SCC decomposition are facts about the LINKS - no start, no world, no budget - and
    // every option below wants the same ones. Each used to build its own: twenty-four
    // Tarjan passes over conversation 631's 4,514 entries for one answer, which
    // `measurements/per_start_setup.rs` priced at 246 ms a menu. See `GroupShape`.
    let shape = GroupShape::of(graph);

    Some(answer_starts(
        graph,
        world,
        request,
        novelty,
        &mut compiler,
        &seed,
        &shape,
    ))
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
///
/// ## EVERY REQUEST IS A MENU, and a menu of one option is still a menu
///
/// There was a second path here that answered the options one at a time, reached by a flag
/// on the request. The plugin never set it: a response menu is what the game asks about, and
/// the only callers that took the other path were the CLI and a handful of fixtures. So the
/// flag separated the product from its own tests, and every offline scenario was green
/// against code the game does not run - which is exactly how two defects in this path
/// survived until an in-game run found them. See de-0jsf.21.
///
/// A separate position and baseline is retained for each roll, so a check is two contestants.
pub fn answer_starts<'a, F: Fn(DialogueNodeId) -> Novelty>(
    graph: &LookAheadGraph,
    world: &dyn ILookAheadWorld,
    request: &LookAheadRequest,
    novelty: &F,
    compiler: &mut GuardCompiler<'a>,
    seed: &BDDFunction,
    shape: &GroupShape,
) -> Vec<LookAheadAnswer> {
    use crate::symbolic::menu::{self, Contestant};
    use crate::symbolic::search::Search;
    let began = std::time::Instant::now();
    let mut answers = Vec::new();
    let mut contestants = Vec::new();
    let mut indices = Vec::new();
    // LOCKED CHECKS, whose halves are answered apart from the menu - see below.
    let started = crate::core::state::seed_state(graph, world);
    let mut locked = Vec::new();
    for &start in &request.starts {
        let id = DialogueNodeId::from(start);
        let Some(node) = graph.get(id) else {
            answers.push(unanswered(start, "none"));
            continue;
        };
        let branches: &[StartBranch] = if node.is_rolled() {
            &[StartBranch::Pass, StartBranch::Fail]
        } else {
            &[StartBranch::Either]
        };
        // A LOCKED OPTION IS ONE CLOSED WHEN THE SEARCH STARTS FOR A REASON THE PLAYER COULD
        // LIFT: a check whose failure slot is already set - the save failed it and the game
        // will not offer it again - a price the purse cannot cover, or, for a red check's Pass
        // half alone, a thought that forces every red roll to fail. A locked half is searched
        // from a world with its locks lifted, so it can be answered as if it were open; the
        // locks stay in the ordinary world, so nothing else can route through it.
        let failed_check = (node.is_rolled()
            && node.failed_flag_slot >= 0
            && started.is_set(node.failed_flag_slot as usize))
        .then(|| graph.symbols().name_of(node.failed_flag_slot as usize))
        .flatten()
        .map(Lock::FailedCheck);
        let unaffordable =
            (!crate::oracle::can_afford(node, &started)).then_some(Lock::Unaffordable(node.cost));
        let locks: Vec<Lock> = failed_check.into_iter().chain(unaffordable).collect();
        let unlocked_seed = (!locks.is_empty()).then(|| {
            seed_of(
                graph,
                &Unlocked {
                    inner: world,
                    locks: &locks,
                },
                compiler.vars(),
            )
        });
        for &branch in branches {
            // THE RED LOCK IS READ DURING THE CRAWL rather than seeded, so a half it locks is
            // walked under the lifted world from its first step, not only started from it.
            let red_pass_forbidden = (branch == StartBranch::Pass
                && !crate::world::roll_may_succeed(node, world))
            .then_some(Lock::RedPassForbidden(id));
            let branch_locks: Vec<Lock> = locks.iter().copied().chain(red_pass_forbidden).collect();
            let lifted = Unlocked {
                inner: world,
                locks: &branch_locks,
            };
            let world_here: &dyn ILookAheadWorld = if branch_locks.is_empty() {
                world
            } else {
                &lifted
            };
            let seed_here = match &unlocked_seed {
                Some(Some(unlocked)) => unlocked,
                Some(None) => {
                    let mut result = unanswered(start, "memory");
                    result.branch = Some(branch_name(branch).to_string());
                    answers.push(result);
                    continue;
                }
                None => seed,
            };
            let mut from = novelty_search::Where::of(
                graph,
                id,
                branch,
                seed_here,
                compiler,
                world_here,
                COUNTER_CAP as u32,
            );
            let destinations = if branch == StartBranch::Either {
                vec![id]
            } else {
                from.destinations(graph, compiler, world_here, COUNTER_CAP as u32)
            };
            let baseline = destinations
                .iter()
                .map(|id| novelty(*id))
                .max()
                .unwrap_or(Novelty::SeenThisGame);
            let mut result = unanswered(start, "none");
            result.branch = if branch == StartBranch::Either {
                None
            } else {
                Some(branch_name(branch).to_string())
            };
            result.destination = baseline as i32;
            result.best = baseline as i32;
            if from.out_of_nodes() {
                result.stopped_by = "memory".to_string();
            } else {
                let contestant = Contestant {
                    position: from.position(id),
                    baseline,
                    landing: destinations,
                };
                if !branch_locks.is_empty() {
                    locked.push((answers.len(), contestant, branch_locks));
                } else {
                    indices.push(answers.len());
                    contestants.push(contestant);
                }
            }
            answers.push(result);
        }
    }
    let ration = request.search_budget();
    let wall = request.menu_budget().min(
        ration
            .overall
            .saturating_mul((contestants.len() + locked.len()) as u32),
    );
    // WHERE THE PLAYER HAS BEEN, as far as it bears on this menu - see [`passed_since_hub`].
    let encountered: Vec<DialogueNodeId> = request
        .encountered
        .iter()
        .map(|entry| DialogueNodeId::from(*entry))
        .collect();
    let returned = passed_since_hub(graph, shape, &encountered);

    // THE CHEAP QUESTION FIRST, AND THE EXACT MARKING WHERE IT MARKS NOTHING - see
    // [`mark_menu_as_shipped`], which the menu measurement calls too.
    let found = mark_menu_as_shipped(
        Search {
            graph,
            compiler: &mut *compiler,
            world,
            counter_cap: COUNTER_CAP as u32,
        },
        novelty,
        &contestants,
        &menu::Budget {
            wall: wall.saturating_sub(began.elapsed()),
            each: ration.each,
        },
        shape,
        &returned,
    );
    for (index, mark) in indices.into_iter().zip(&found.marks) {
        record(&mut answers[index], mark);
    }

    // THE LOCKED CHECKS, ANSWERED ONCE EVERY ORDINARY STAR IS SETTLED. Each half asks what it
    // WOULD reach if the check were open, with every option of this menu blocked - itself and
    // its siblings - so it cannot cycle back through the menu, and every entry an ordinary
    // star claimed blocked too. So a half is starred only for content no other option reaches,
    // and its word tells the player whether unlocking the check is worth a skill point.
    let mut blocked: HashSet<DialogueNodeId> = request
        .starts
        .iter()
        .map(|start| DialogueNodeId::from(*start))
        .collect();
    blocked.extend(found.marks.iter().filter_map(|mark| mark.witness));
    // THE ONWARD STARS, which name no entry: the onward question only establishes that an
    // option leads somewhere, so there is no witness to block. Each is kept with the class it
    // was starred for, since a star claims nothing of a class it was not hunting.
    let onward: Vec<(usize, Novelty)> = found
        .marks
        .iter()
        .enumerate()
        .filter(|(_, mark)| mark.round.is_some() && mark.witness.is_none())
        .map(|(index, mark)| (index, mark.best))
        .collect();
    let mut passes = found.passes;
    for (index, contestant, branch_locks) in locked {
        // AN OPTION ANSWERED WHOLE STARTS AT ITSELF, where a check's half starts past the
        // check - so its own entry stays walkable for its own search, or nothing could enter
        // it at all. A half keeps its check blocked, which is what stops it cycling back.
        let mut blocked_here = blocked.clone();
        if contestant
            .position
            .entries
            .contains(&contestant.position.option)
        {
            blocked_here.remove(&contestant.position.option);
        }
        // Walked under its own locks lifted, which is what lets a red lock - read at every
        // step - open this half and no other.
        let lifted = Unlocked {
            inner: world,
            locks: &branch_locks,
        };
        let alone = loop {
            let mut alone = menu::mark_menu_blocking(
                Search {
                    graph,
                    compiler: &mut *compiler,
                    world: &lifted,
                    counter_cap: COUNTER_CAP as u32,
                },
                novelty,
                std::slice::from_ref(&contestant),
                &menu::Budget {
                    wall: wall.saturating_sub(began.elapsed()),
                    each: ration.each,
                },
                shape,
                &blocked_here,
            );
            passes += alone.passes;

            // KEPT OFF WHAT AN ONWARD STAR ALREADY LEADS TO, one entry at a time. Where the half
            // lands on an entry an onward star of that class reaches - asked in the real world,
            // locks and all, the way the star was - the entry is blocked and the half asked
            // again, so it ends starred only for content no star leads to. The passes are spent
            // only on entries a half actually lands on, and only in a menu with a locked option.
            let Some(witness) = alone.marks[0].witness else {
                break alone;
            };
            let class = novelty(witness);
            let rivals: Vec<usize> = onward
                .iter()
                .filter(|(_, best)| *best == class)
                .map(|(rival, _)| *rival)
                .collect();
            if rivals.is_empty() {
                break alone;
            }
            match menu::reached_onward(
                Search {
                    graph,
                    compiler: &mut *compiler,
                    world,
                    counter_cap: COUNTER_CAP as u32,
                },
                &contestants,
                &rivals,
                witness,
                &menu::Budget {
                    wall: wall.saturating_sub(began.elapsed()),
                    each: ration.each,
                },
                shape,
                &returned,
            ) {
                Ok(false) => break alone,
                Ok(true) => {
                    blocked_here.insert(witness);
                }
                // NOT KNOWN WHETHER A STAR ALREADY LEADS THERE, so the half is not starred for
                // it, and says its search gave up rather than that there is nothing to find.
                Err((stopped_by, out_of_nodes)) => {
                    let mark = &mut alone.marks[0];
                    mark.best = contestant.baseline;
                    mark.distance = None;
                    mark.round = None;
                    mark.witness = None;
                    mark.complete = false;
                    mark.stopped_by = stopped_by;
                    mark.out_of_nodes = out_of_nodes;
                    break alone;
                }
            }
        };
        record(&mut answers[index], &alone.marks[0]);
    }

    // Menu work is shared, so report its cost once rather than multiplying by options.
    if let Some(first) = answers.first_mut() {
        first.elapsed_ms = began.elapsed().as_millis() as u64;
        first.nodes_reached = passes;
    }
    answers
}

/// Every start of a request answered as nothing established, for one reason.
///
/// What a caller draws is a menu with no markers, which is also what it draws without an
/// engine at all - so the shape is the same whatever stopped it, and only `stopped_by`
/// differs. Shared with [`crate::workspace`], which reaches the same wall a request later
/// rather than a request earlier.
pub(crate) fn all_unanswered(request: &LookAheadRequest, stopped_by: &str) -> Vec<LookAheadAnswer> {
    request
        .starts
        .iter()
        .map(|start| unanswered(*start, stopped_by))
        .collect()
}

/// What the player has passed since the hubs they are inside, which a menu's onward question cuts.
///
/// THE ONE PLACE A WALK BECOMES A CUT, for [`answer_starts`] and the menu measurement alike, so a
/// measurement that carries a walk pays for and gets exactly what a player's request does: the
/// group's hub candidates, kept by `shape` once found, and the stack followed along the walk - see
/// [`crate::symbolic::hub`]. `encountered` is every entry the conversation stepped through,
/// beginning at its start and including the ones it never displayed. An empty walk says nothing
/// about where the player is, and cuts nothing.
pub fn passed_since_hub(
    graph: &LookAheadGraph,
    shape: &crate::symbolic::known::GroupShape,
    encountered: &[DialogueNodeId],
) -> HashSet<DialogueNodeId> {
    if encountered.is_empty() {
        return HashSet::new();
    }
    crate::symbolic::hub::since_current_hub(shape.order(), shape.hubs(graph), encountered)
}

/// Marks a menu the way the product does.
///
/// THE ONE PLACE THE CHOICE IS MADE, for [`answer_starts`] and the menu measurement alike, so
/// a measurement taken by default measures what a player waits for. The cheap question is
/// asked first - which options lead to unread content without returning through the menu -
/// and the exact marking runs only where that marks nothing, each of its rounds a branch and
/// bound over single targets. See [`crate::symbolic::menu::mark_menu_hybrid`].
///
/// `returned` is what the player has passed since the hubs they are inside, cut beside the
/// siblings by the cheap question; empty for a caller that does not know where the player is.
pub fn mark_menu_as_shipped<F: Fn(DialogueNodeId) -> Novelty>(
    search: crate::symbolic::search::Search<'_, '_>,
    novelty: &F,
    contestants: &[crate::symbolic::menu::Contestant],
    budget: &crate::symbolic::menu::Budget,
    shape: &GroupShape,
    returned: &HashSet<DialogueNodeId>,
) -> crate::symbolic::menu::MenuAnswer {
    crate::symbolic::menu::mark_menu_hybrid(search, novelty, contestants, budget, shape, returned)
}

/// Writes what a marking settled about one start onto its answer.
fn record(answer: &mut LookAheadAnswer, mark: &crate::symbolic::menu::Marked) {
    answer.best = mark.best as i32;
    answer.witness = mark.witness.map(NodeRef::from);
    answer.complete = mark.complete;
    answer.stopped_by = if mark.out_of_nodes {
        "memory"
    } else {
        stopped_name(mark.stopped_by)
    }
    .to_string();
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

/// The name a stopped search crosses the wire under.
///
/// THE WORDS ARE THE WIRE'S, and they do not change, because the C# side and the harness read them
/// and what they MEAN has not changed: a ration ran out, and which one decides whether a
/// player can do anything about it. What has changed is which rations exist - a set-based
/// search has candidates and a clock where a state-at-a-time one would have states and bytes.
fn stopped_name(stopped: novelty_search::StoppedBy) -> &'static str {
    match stopped {
        novelty_search::StoppedBy::Nothing => "none",
        // A CENSUS HAVING WHAT IT CAME FOR, which is the only thing that sets this and is
        // not something the bridge ever runs. The arm is here because the match is total,
        // and "states" is the wire's nearest word for a caller's appetite running out.
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
/// refusal and the target are one fact: `None` is the refusal, and anything else is the
/// class the search is sent hunting.
///
/// NOTHING OUTRANKS THE TOP RUNG. Text no save has read is as novel as anything gets, so
/// there is nothing for a search to find and the answer is settled without walking at all,
/// this being decided before a search is begun.
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
/// The performance matrix claims to measure what the game runs, and it was calling
/// `answer::best_novelty` unconditionally where the game refuses to search at all. A
/// second copy of this predicate is exactly how that drifted, so it is exported rather than
/// reimplemented.
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
    use crate::test_graph::{Entry, GraphBuilder, node};
    use crate::world::test_world::TestWorld;

    /// One outcome of one start, asked through the path the game asks through.
    ///
    /// A MENU OF ONE OPTION, because that is the only kind of request there is. A rolled
    /// check comes back as two answers - one per outcome, told apart by `branch` - so this
    /// picks the one it was asked about.
    ///
    /// The subject of every caller is the ROLL rather than the competition between options,
    /// so a menu of one costs these tests nothing: with no siblings there is no competition
    /// to express. A test whose subject IS the competition must offer a real menu.
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
        answer_menu(graph, world, &[start], novelty)
            .into_iter()
            .find(|answer| answer.branch == branch_name(branch))
            .expect("the outcome asked about comes back")
    }

    /// Every answer for a menu of `starts`, over a layout that carries money wherever the
    /// group reads it - so a price is refused as the product refuses it.
    fn answer_menu<F>(
        graph: &LookAheadGraph,
        world: &TestWorld,
        starts: &[DialogueNodeId],
        novelty: F,
    ) -> Vec<LookAheadAnswer>
    where
        F: Fn(DialogueNodeId) -> Novelty,
    {
        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(
            graph,
            COUNTER_CAP,
            DataLayout::money_ceiling(graph, world.money()),
            false,
        );
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(world);
        let seed = seed_of(graph, world, &vars).expect("room for a seed");

        let request = LookAheadRequest {
            conversation: starts[0].conversation_id,
            starts: starts.iter().map(|start| NodeRef::from(*start)).collect(),
            ..Default::default()
        };
        answer_starts(
            graph,
            world,
            &request,
            &novelty,
            &mut compiler,
            &seed,
            &GroupShape::of(graph),
        )
    }

    /// What the wire calls an outcome, so a test can find the answer it wanted.
    fn branch_name(branch: StartBranch) -> Option<String> {
        match branch {
            StartBranch::Either => None,
            StartBranch::Pass => Some("pass".to_string()),
            StartBranch::Fail => Some("fail".to_string()),
        }
    }

    /// A check whose outcomes land on different rungs, both below the top one.
    ///
    /// 0 is the check. Passing opens 1, which this save has read, and 2 lies past it;
    /// failing opens 3. Nothing anywhere is unseen in any game, which is what makes the
    /// OPTION not worth searching while one of its OUTCOMES still is.
    fn check_landing_on_something_read() -> LookAheadGraph {
        GraphBuilder::new()
            .add(
                Entry::new(0)
                    .kind(DialogueCheckKind::White)
                    .flag("roll")
                    .links(&[1, 3]),
            )
            .add(
                Entry::new(1)
                    .guard(r#"Variable["roll"] == true"#)
                    .links(&[2]),
            )
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
            if id == node(1) {
                Novelty::SeenThisGame
            } else {
                Novelty::UnseenThisGame
            }
        };

        assert!(
            class_worth_hunting(&graph, &[node(0)], novelty(node(0)), novelty).is_none(),
            "the option should be refused: nothing outranks unseen-this-game here",
        );

        let pass = score_one(&graph, &world, node(0), StartBranch::Pass, novelty);
        assert_eq!(pass.branch.as_deref(), Some(PASS));
        assert_eq!(
            pass.destination,
            Novelty::SeenThisGame as i32,
            "passing opens 1"
        );
        assert_eq!(
            pass.best,
            Novelty::UnseenThisGame as i32,
            "the unread entry past 1 outranks where passing lands, and should be reported",
        );

        let fail = score_one(&graph, &world, node(0), StartBranch::Fail, novelty);
        assert_eq!(fail.branch.as_deref(), Some(FAIL));
        assert_eq!(
            fail.destination,
            Novelty::UnseenThisGame as i32,
            "failing opens 3"
        );
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
    /// would answer both the same way and name neither class - see `symbolic::answer`
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
            if id == node(2) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::UnseenThisGame
            }
        };
        assert_eq!(
            class_worth_hunting(&graph, &[node(0)], Novelty::UnseenThisGame, one_top_rung),
            Some(Novelty::UnseenAnyGame),
            "and the class it names is what the search is sent hunting",
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
            if id == node(0) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
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
            if id == node(2) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
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
        assert_eq!(
            class_worth_hunting(&graph, &[node(0)], Novelty::SeenThisGame, read),
            None
        );
    }

    /// An outcome's answer round-trips as JSON, naming which outcome it is.
    #[test]
    fn an_outcome_survives_the_wire() {
        let answer = LookAheadAnswer {
            start: NodeRef {
                conversation: 451,
                entry: 12,
            },
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
            start: NodeRef {
                conversation: 451,
                entry: 12,
            },
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
        assert!(
            !text.contains("branch"),
            "an absent outcome still crossed: {text}"
        );

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
        assert_eq!(
            answer.destination, 0,
            "an absent destination reads as the bottom rung"
        );
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

    /// The price the options below cost, one more than the player carries.
    const PRICE: i32 = 10;

    /// What an entry is worth to the tests below: 2 has never been read, everything else has.
    fn only_two_is_unread(id: DialogueNodeId) -> Novelty {
        if id == node(2) {
            Novelty::UnseenAnyGame
        } else {
            Novelty::SeenThisGame
        }
    }

    /// An option the purse cannot cover is answered for what buying it would open.
    ///
    /// 0 costs more than the player carries and opens 2, which nobody has read. No search can
    /// enter 0 from this purse, so an ordinary answer would say nothing; as a LOCKED option it
    /// is answered from a purse that covers the price, the way a failed white check is
    /// answered as if it were open.
    #[test]
    fn an_unaffordable_option_is_answered_for_what_buying_it_opens() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).cost(PRICE).links(&[2]))
            .add(Entry::new(2))
            .build();
        let world = TestWorld::new().with_money(PRICE - 1);

        let answer = score_one(
            &graph,
            &world,
            node(0),
            StartBranch::Either,
            only_two_is_unread,
        );
        assert_eq!(
            answer.best,
            Novelty::UnseenAnyGame as i32,
            "buying 0 opens 2, which nobody has read"
        );
    }

    /// And, like a failed white check, it is starred only for what no other option reaches.
    ///
    /// 1 is free and opens 2; 0 is unaffordable and opens 2 as well. Buying 0 would reach 2,
    /// but 1 already does, so 0 must not claim it.
    #[test]
    fn an_unaffordable_option_is_not_starred_for_what_a_sibling_reaches() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).cost(PRICE).links(&[2]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build();
        let world = TestWorld::new().with_money(PRICE - 1);

        let answers = answer_menu(&graph, &world, &[node(0), node(1)], only_two_is_unread);
        let best_of = |id: DialogueNodeId| {
            answers
                .iter()
                .find(|answer| answer.start == NodeRef::from(id))
                .expect("every option is answered")
                .best
        };

        assert_eq!(
            best_of(node(1)),
            Novelty::UnseenAnyGame as i32,
            "the free option reaches 2"
        );
        assert_eq!(
            best_of(node(0)),
            Novelty::SeenThisGame as i32,
            "the unaffordable one would reach only what the free one already does"
        );
    }

    /// But what no other option reaches is still the unaffordable one's to claim.
    ///
    /// 1 is free and opens 3; 0 is unaffordable and opens 2. Both lead somewhere nobody has
    /// read and neither reaches the other's, so both are starred - the free one by the onward
    /// question, the unaffordable one because no onward star leads where buying it would.
    #[test]
    fn an_unaffordable_option_is_starred_for_what_no_sibling_reaches() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).cost(PRICE).links(&[2]))
            .add(Entry::new(1).links(&[3]))
            .add(Entry::new(2))
            .add(Entry::new(3))
            .build();
        let world = TestWorld::new().with_money(PRICE - 1);
        let two_and_three_are_unread = |id: DialogueNodeId| {
            if id == node(2) || id == node(3) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        };

        let answers = answer_menu(
            &graph,
            &world,
            &[node(0), node(1)],
            two_and_three_are_unread,
        );
        let best_of = |id: DialogueNodeId| {
            answers
                .iter()
                .find(|answer| answer.start == NodeRef::from(id))
                .expect("every option is answered")
                .best
        };

        assert_eq!(
            best_of(node(1)),
            Novelty::UnseenAnyGame as i32,
            "the free option reaches 3"
        );
        assert_eq!(
            best_of(node(0)),
            Novelty::UnseenAnyGame as i32,
            "buying 0 opens 2, which the free option does not reach"
        );
    }

    /// A rolled check's flag and failure slot are asked for, though no guard names the slot.
    ///
    /// The engine closes a check whose failure slot is set and seeds that slot from the world,
    /// so the plugin has to answer it. Left out, a red check the save has failed reads as
    /// untried and its success branch is open to every crawl.
    #[test]
    fn a_rolled_checks_flag_and_failure_slot_are_asked_for() {
        let graph = GraphBuilder::new()
            .add(
                Entry::new(0)
                    .kind(DialogueCheckKind::Red)
                    .flag("roll")
                    .links(&[1, 2]),
            )
            .add(Entry::new(1).guard(r#"Variable["roll"] == true"#))
            .add(Entry::new(2).guard(r#"Variable["roll"] == false"#))
            .build();

        let asked = questions_of(&graph, Vec::new());
        assert!(
            asked.variables.contains(&"roll".to_string()),
            "{:?}",
            asked.variables
        );
        assert!(
            asked.variables.contains(&"roll_failed".to_string()),
            "{:?}",
            asked.variables
        );
    }

    /// A red check a thought forces to fail answers its Pass half like a locked check.
    ///
    /// 0 is a red check: passing opens 1, which nobody has read, and failing opens 2. With
    /// every red roll forced to fail the Pass half is still answered for what passing WOULD
    /// open, the Fail half is answered as it stands, and 3 - a sibling that leads into the
    /// check - cannot reach 1 through it.
    #[test]
    fn a_red_pass_forced_to_fail_is_answered_but_opens_nothing_else() {
        let graph = GraphBuilder::new()
            .add(
                Entry::new(0)
                    .kind(DialogueCheckKind::Red)
                    .flag("roll")
                    .links(&[1, 2]),
            )
            .add(Entry::new(1).guard(r#"Variable["roll"] == true"#))
            .add(Entry::new(2).guard(r#"Variable["roll"] == false"#))
            .add(Entry::new(3).links(&[0]))
            .build();
        let world = TestWorld::new().with_red_checks_failing(true);
        let only_one_is_unread = |id: DialogueNodeId| {
            if id == node(1) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        };

        let answers = answer_menu(&graph, &world, &[node(0), node(3)], only_one_is_unread);
        let best_of = |id: DialogueNodeId, branch: StartBranch| {
            answers
                .iter()
                .find(|answer| {
                    answer.start == NodeRef::from(id) && answer.branch == branch_name(branch)
                })
                .expect("every half is answered")
                .best
        };

        assert_eq!(
            best_of(node(0), StartBranch::Pass),
            Novelty::UnseenAnyGame as i32,
            "passing would open 1"
        );
        assert_eq!(
            best_of(node(0), StartBranch::Fail),
            Novelty::SeenThisGame as i32,
            "failing opens only 2"
        );
        assert_eq!(
            best_of(node(3), StartBranch::Either),
            Novelty::SeenThisGame as i32,
            "the sibling cannot pass the check to reach 1"
        );
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
    use crate::test_graph::{Entry, GraphBuilder};

    /// What a one-entry group guarded by `text` asks.
    fn asked(text: &str) -> Questions {
        let graph = GraphBuilder::new().add(Entry::new(0).guard(text)).build();
        questions_of(&graph, Vec::new())
    }

    /// What `world` answers for the variable `name`, declared so it can be asked for.
    fn read(world: &dyn ILookAheadWorld, name: &str) -> GuardValue {
        let mut symbols = StateSymbols::new();
        symbols.declare_variables([name.to_string()]);
        world.get_variable(symbols.variable_ref(name).expect("it was just declared"))
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
        let found =
            asked(r#"CheckItem("badge") and IsTaskActive("TASK.x") and IsTHCPresent("jamais_vu")"#);

        assert_eq!(found.items, vec!["badge".to_string()]);
        assert_eq!(found.tasks, vec!["TASK.x".to_string()]);
        assert_eq!(found.thoughts, vec!["jamais_vu".to_string()]);
        assert!(
            found.queries.is_empty(),
            "these must not also be asked as calls"
        );
    }

    /// A thought asked about only by a cabinet-state question is still named to the plugin,
    /// which builds the cooking and fixed sets over the thoughts it is given.
    #[test]
    fn a_cabinet_state_question_names_its_thought() {
        let found = asked(
            r#"IsTHCFixed("aces_high") or IsTHCCooking("jamais_vu") or IsTHCCookingOrFixed("honour")"#,
        );

        assert_eq!(
            found.thoughts,
            vec![
                "aces_high".to_string(),
                "honour".to_string(),
                "jamais_vu".to_string()
            ]
        );
        assert_eq!(
            found.data,
            vec![
                DataRequest::set(DataKind::ThoughtsCooking),
                DataRequest::set(DataKind::ThoughtsFixed)
            ]
        );
    }

    /// An action called from a guard is never asked of the plugin, because ASKING RUNS IT.
    ///
    /// Both of these return nothing and exist only for their effect - one closes a journal
    /// task, the other awards experience - and the plugin answers a query by running it as
    /// Lua in the live game. A name that reaches this list is a write to the player's save
    /// on every crawl of the group.
    #[test]
    fn an_action_called_from_a_guard_is_never_asked_for() {
        for guard in [
            r#"FinishTask("TASK.pissing_competition_done") == true"#,
            r#"XPStandardSetBool("XP.cuno_suspicion") == true"#,
        ] {
            let found = asked(guard);
            assert!(
                found.queries.is_empty(),
                "asking the plugin for this would run it: {guard} -> {:?}",
                found.queries
            );
        }
    }

    /// A flag is a dialogue variable written another way, and is asked for as one.
    #[test]
    fn a_flag_is_asked_for_as_a_variable() {
        let found = asked(r#"FlagSet("church.done")"#);
        assert_eq!(found.variables, vec!["church.done".to_string()]);
        assert!(found.queries.is_empty());
    }

    /// AND SO IS THE FLAG ASKED ABOUT THE OTHER WAY ROUND.
    ///
    /// `FlagNotSet` is `FlagSet` negated, so its flag is a variable too. Asked as a query
    /// instead, it would be answered from the snapshot - the value the crawl STARTED with -
    /// and `SetFlag` is a modelled write, so a group that raises a flag and then asks about
    /// it would read its own write as not having happened.
    #[test]
    fn a_flag_asked_about_negatively_is_also_asked_for_as_a_variable() {
        let found = asked(r#"FlagNotSet("village.said_the_cock_thing")"#);
        assert_eq!(
            found.variables,
            vec!["village.said_the_cock_thing".to_string()]
        );
        assert!(
            found.queries.is_empty(),
            "answered from the variable, not asked as a query: {:?}",
            found.queries
        );
    }

    /// A reputation question declares the WHOLE RANGE it compares, and is not asked as a
    /// call.
    ///
    /// The argument names what the answer is compared to, not what is read - the game walks
    /// four reputations and returns whichever is winning. A range member left undeclared is
    /// one the comparison cannot see, and asking the plugin for the call instead would
    /// answer it once from the world, where `ReputationGrows` is a modelled write.
    #[test]
    fn a_reputation_question_declares_the_range_it_compares() {
        let found = asked(r#"IsHighestPolitical("communist")"#);
        assert_eq!(
            found.variables,
            vec![
                "reputation.communist".to_string(),
                "reputation.moralist".to_string(),
                "reputation.revacholian_nationhood".to_string(),
                "reputation.ultraliberal".to_string(),
            ],
            "all four political reputations, sorted as the group declares them"
        );
        assert!(
            found.queries.is_empty(),
            "answered from the variables, not asked as a call: {:?}",
            found.queries
        );
    }

    #[test]
    fn an_ordinary_query_is_asked_for_by_its_key() {
        let found = asked(r#"WasGameBeatenInHardcoreMode() and IsKimHere()"#);
        assert_eq!(
            found.queries,
            vec![
                "IsKimHere()".to_string(),
                "WasGameBeatenInHardcoreMode()".to_string()
            ],
        );
    }

    /// The key the engine hands out is the key it later looks up. Written as a test
    /// because the two sides agreeing is the whole point of naming them here.
    #[test]
    fn the_key_asked_for_is_the_key_answered() {
        let found = asked(r#"IsKimHere()"#);
        let key = &found.queries[0];

        let mut world = WorldSnapshot::default();
        world
            .queries
            .insert(key.clone(), WireValue::Bool { value: true });
        let world = SnapshotWorld::new(world);

        let answer = world.query("IsKimHere", &[]);
        assert!(
            answer.boolean(),
            "the answer did not come back under the key given"
        );
    }

    /// `CheckEquipped` asks for every slot as DATA, and never as a call to run.
    #[test]
    fn check_equipped_reads_every_slot_rather_than_running_a_call() {
        let found = asked(r#"CheckEquipped("neck_tie")"#);
        assert!(
            found.queries.is_empty(),
            "asked as a call: {:?}",
            found.queries
        );
        assert_eq!(
            found.data.len(),
            equipment::SLOTS.len(),
            "one read per slot: {:?}",
            found.data
        );
        for slot in equipment::SLOTS {
            assert!(
                found
                    .data
                    .contains(&DataRequest::about(DataKind::EquippedInSlot, slot)),
                "{slot} is not read"
            );
        }
    }

    /// The slots answer `CheckEquipped`, and a slot nobody read leaves a missing item Unknown.
    #[test]
    fn check_equipped_is_answered_from_the_slots() {
        let found = asked(r#"CheckEquipped("neck_tie")"#);
        let world_with = |neck: Option<&str>| {
            let mut snapshot = WorldSnapshot::default();
            for request in &found.data {
                let answer = match (request.subject.as_str(), neck) {
                    ("NECK", None) => DataAnswer::default(),
                    ("NECK", Some(item)) => DataAnswer::of_value(WireValue::Text {
                        value: item.to_string(),
                    }),
                    _ => DataAnswer::of_value(WireValue::Text {
                        value: String::new(),
                    }),
                };
                snapshot.data.insert(request.clone(), answer);
            }
            SnapshotWorld::new(snapshot)
        };
        let tie = [GuardValue::from_text("neck_tie".to_string())];

        let worn = world_with(Some("neck_tie")).query("CheckEquipped", &tie);
        assert_eq!(worn.kind(), GuardValueKind::Boolean);
        assert!(worn.boolean());

        let bare = world_with(Some("")).query("CheckEquipped", &tie);
        assert_eq!(bare.kind(), GuardValueKind::Boolean);
        assert!(!bare.boolean());

        let unread = world_with(None).query("CheckEquipped", &tie);
        assert_eq!(unread.kind(), GuardValueKind::Unknown);
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
        snapshot
            .variables
            .insert("church.done".to_string(), WireValue::Unknown);
        // And one it did answer, which must not be overridden by the table's initial.
        snapshot.variables.insert(
            "jam.lorrymans_questioned".to_string(),
            WireValue::Number { value: 4.0 },
        );

        let world = SnapshotWorld::declaring(snapshot, Some(Arc::new(table)));

        assert_eq!(
            read(&world, "jam.lorrymans_questioned").try_as_number(),
            Some(4.0)
        );
        assert_eq!(read(&world, "church.done").kind(), GuardValueKind::Boolean);
        // Never named at all, and the table does not declare it either.
        assert_eq!(
            read(&world, "nothing.declares.this").kind(),
            GuardValueKind::Unknown
        );
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
        assert_eq!(
            read(&world, "pier.reporting_counter").try_as_number(),
            Some(0.0)
        );

        // And without the table it is Unknown, which is what it was before.
        let bare = SnapshotWorld::new(WorldSnapshot::default());
        assert_eq!(
            read(&bare, "pier.reporting_counter").kind(),
            GuardValueKind::Unknown
        );
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
        snapshot
            .resolve(&questions)
            .expect("the lists are the same length");

        let world = SnapshotWorld::new(snapshot);
        assert_eq!(read(&world, "a.first").try_as_number(), Some(4.0));
        assert!(read(&world, "b.second").boolean());
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
        snapshot
            .variables
            .insert("a.first".to_string(), WireValue::Number { value: 9.0 });
        snapshot
            .resolve(&questions)
            .expect("the lists are the same length");

        assert_eq!(
            read(&SnapshotWorld::new(snapshot), "a.first").try_as_number(),
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

        let refused = snapshot
            .resolve(&questions)
            .expect_err("it must be refused");
        assert!(
            refused.contains("1 variable answers came back for 2"),
            "{refused}"
        );
    }

    /// Anything unanswered reads Unknown, which is the permissive direction.
    #[test]
    fn an_unanswered_question_is_unknown_rather_than_false() {
        let world = SnapshotWorld::new(WorldSnapshot::default());

        assert_eq!(
            read(&world, "never.mentioned").kind(),
            GuardValueKind::Unknown
        );
        assert_eq!(
            world.query("IsKimHere", &[]).kind(),
            GuardValueKind::Unknown
        );
        assert_eq!(
            world.check_passes(DialogueNodeId::new(1, 2)),
            Ternary::Unknown
        );
    }

    /// All three check outcomes come across, and the third one is silence.
    #[test]
    fn a_check_answer_carries_all_three_outcomes() {
        let world = SnapshotWorld::new(WorldSnapshot {
            checks_pass: NodeSet::from_iter([NodeRef {
                conversation: 1,
                entry: 1,
            }]),
            checks_fail: NodeSet::from_iter([NodeRef {
                conversation: 1,
                entry: 2,
            }]),
            ..Default::default()
        });

        assert_eq!(world.check_passes(DialogueNodeId::new(1, 1)), Ternary::True);
        assert_eq!(
            world.check_passes(DialogueNodeId::new(1, 2)),
            Ternary::False
        );
        assert_eq!(
            world.check_passes(DialogueNodeId::new(1, 3)),
            Ternary::Unknown
        );
    }

    #[test]
    fn seen_entries_are_carried_across() {
        let world = SnapshotWorld::new(WorldSnapshot {
            seen: NodeSet::from_iter([NodeRef {
                conversation: 7,
                entry: 3,
            }]),
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
                .map(|entry| NodeRef {
                    conversation: 631,
                    entry,
                })
                .into_iter()
                .chain([NodeRef {
                    conversation: 636,
                    entry: 7,
                }]),
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
        let set = NodeSet::from_iter([-5, -4, -3, -1, 2, 3].map(|entry| NodeRef {
            conversation: 9,
            entry,
        }));

        let written = serde_json::to_string(&set).expect("it serialises");
        assert_eq!(written, r#"{"9":"-5--3,-1,2-3"}"#);

        let back: NodeSet = serde_json::from_str(&written).expect("it reads");
        assert_eq!(back, set);
    }

    #[test]
    fn an_entry_set_comes_back_from_its_runs() {
        let set: NodeSet = serde_json::from_str(r#"{"631":"0-3,5","636":"7"}"#).expect("it reads");

        assert_eq!(set.len(), 6);
        assert!(set.contains(&NodeRef {
            conversation: 631,
            entry: 3
        }));
        assert!(!set.contains(&NodeRef {
            conversation: 631,
            entry: 4
        }));
        assert!(set.contains(&NodeRef {
            conversation: 636,
            entry: 7
        }));
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
        assert!(set.contains(&NodeRef {
            conversation: 631,
            entry: 5
        }));
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
        let mut world = WorldSnapshot {
            money: 250,
            day_minutes: 720,
            ..Default::default()
        };
        world
            .variables
            .insert("x".to_string(), WireValue::Number { value: 3.0 });
        world
            .queries
            .insert("IsKimHere()".to_string(), WireValue::Bool { value: true });

        let request = LookAheadRequest {
            conversation: 631,
            starts: vec![NodeRef {
                conversation: 631,
                entry: 4,
            }],
            unseen_any_game: NodeSet::from_iter([NodeRef {
                conversation: 631,
                entry: 9,
            }]),
            unseen_this_game: NodeSet::default(),
            state_budget: 0,
            time_budget_ms: 0,
            menu_time_budget_ms: 0,
            memory_budget_mb: 0,
            encountered: vec![NodeRef {
                conversation: 631,
                entry: 2,
            }],
            world,
        };

        let text = serde_json::to_string(&request).expect("it serialises");
        let back: LookAheadRequest = serde_json::from_str(&text).expect("it comes back");

        assert_eq!(back.conversation, 631);
        assert_eq!(back.world.money, 250);
        assert_eq!(
            back.encountered,
            vec![NodeRef {
                conversation: 631,
                entry: 2,
            }]
        );
        assert!(back.unseen_any_game.contains(&NodeRef {
            conversation: 631,
            entry: 9
        }));
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
            menu_time_budget_ms: 0,
            memory_budget_mb: 64,
            encountered: Vec::new(),
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
            menu_time_budget_ms: 0,
            memory_budget_mb: 0,
            encountered: Vec::new(),
            world: WorldSnapshot::default(),
        };

        assert_eq!(
            request.diagram_budget().memory(),
            DiagramBudget::DEFAULT_MEMORY_BUDGET,
        );
        assert!(
            request.diagram_budget().memory() > 0,
            "the default turned the budget off"
        );
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
            menu_time_budget_ms: 0,
            memory_budget_mb: 0,
            encountered: Vec::new(),
            world: WorldSnapshot::default(),
        };

        let budget = request.search_budget();
        assert_eq!(budget.backwards, std::time::Duration::from_millis(250));
        assert!(
            budget.each <= budget.backwards,
            "a candidate may not outlast the search"
        );
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
            menu_time_budget_ms: 0,
            memory_budget_mb: 0,
            encountered: Vec::new(),
            world: WorldSnapshot::default(),
        };

        assert_eq!(
            request.diagram_budget().memory(),
            DiagramBudget::DEFAULT_MEMORY_BUDGET,
        );

        // No number of the player's, so every part of the search keeps its own pacing.
        assert_eq!(
            request.search_budget().backwards,
            answer::Budget::default().backwards
        );
        assert_eq!(request.search_budget().each, answer::Budget::default().each);
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
        let node = LookAheadNode {
            ..LookAheadNode::new(DialogueNodeId::new(1, 0))
        };
        let graph = LookAheadGraph::new(vec![node], symbols).unwrap();

        // Straight at the private path rather than through an index, because what is being
        // checked is the answer for a start the graph does not hold.
        assert!(graph.get(DialogueNodeId::new(1, 99)).is_none());
    }
}
