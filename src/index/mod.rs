// SPDX-License-Identifier: MIT
//! Reading the extracted conversation index, and building a graph out of it.
//!
//! The only reader of `conversation_index.jsonl`, written by
//! `dotnet run --project tools/DialogueExtract -- conversation-index`. One JSON object
//! per line, one conversation each.
//!
//! In the library rather than in the binary because two other things need it - the tests
//! that check this agrees with the C# builder, and any symbolic encoding, which is sized
//! by the graph this produces.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::action::DialogueAction;
use crate::core::guard::Guard;
use crate::core::skill_movers::SkillMoves;
use crate::core::state::{ONCE_PREFIX, SEEN_PREFIX, StateSymbols};
use crate::core::types::{DialogueCheckKind, DialogueNodeId};
use crate::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::parser::action_parser::parse_actions_with_journal;
use crate::parser::guard_parser::parse_guard;
use crate::symbolic::data_layout::DataLayout;

/// Field names as the asset spells them.
pub(crate) const ACTOR_FIELD: &str = "Actor";
const PASSIVE_FIELD: &str = "DifficultyPass";
/// Marks a passive check that fires when it FAILS.
const ANTIPASSIVE_FIELD: &str = "Antipassive";
/// The articy id of the skill a rolled or fake check tests (`CheckNodeUtil.GetSkillType`).
pub(crate) const SKILL_TYPE_FIELD: &str = "SkillType";
const RED_FIELD: &str = "DifficultyRed";
const WHITE_FIELD: &str = "DifficultyWhite";
const FAKE_FIELD: &str = "DifficultyAtmo";
/// Whether a fake check's forced roll succeeds (`FakeCheckNode.TransformCheck`).
const ALWAYS_SUCCEED_FIELD: &str = "AlwaysSucceed";
const TEST_FIELD: &str = "HiddenTest";
const KIM_WATCH_FIELD: &str = "kim_watch";
const BOOLEAN_ONLY_FIELD: &str = "boolean_only";
const FLAG_NAME_FIELD: &str = "FlagName";

/// What a check's failure slot is called: its flag's name with this after it.
///
/// PUBLIC because the slot is read like any other variable, so a world that knows a check is
/// already failed - the game keeps locked white checks in a table of its own, not in Lua -
/// answers this name as true, and the check starts closed.
pub const FAILED_FLAG_SUFFIX: &str = "_failed";
const CLICK_COST_FIELD: &str = "ClickCost";
const COST_ONCE_FIELD: &str = "CostOnce";
const HIDDEN_NOT_ENOUGH_FIELD: &str = "HiddenNotEnough";

/// What an entry's presentation is scheduled by: the Dialogue System's sequence.
///
/// WHETHER A LINE HOLDS THE SCREEN IS IN HERE AND NOWHERE ELSE. A walk decides whether a line
/// waits for a continue from what links out of it - a line behind it waits, a menu behind it
/// does not - and measured in game that is right almost everywhere and wrong where a sequence
/// RUNS: an animation, or a command scheduled with `@`, keeps its line up and takes a continue
/// the link shape does not predict. Two entries of conversation 1467 are the measured case, and
/// `tools/sequence-holds.py` counts the population they belong to. See de-oaaq.
pub(crate) const SEQUENCE_FIELD: &str = "Sequence";

/// Sequence commands that keep their line on screen until the player dismisses it.
///
/// SMALLER THAN THE EVIDENCE, ON PURPOSE. Only what has been measured holds; everything else
/// keeps the behaviour it has today, so this can move the entries it was measured for and no
/// others. `FocusCamera` is deliberately absent - 82 entries use it before a menu and nothing
/// has measured it either way, and calling it a holder on a hunch would be claiming a
/// measurement nobody took. See de-oaaq, and `tools/sequence-holds.py` for the population.
const HOLDING_COMMANDS: [&str; 7] = [
    "PlayAnimation",
    "SetTriggerAnimation",
    "FadeToBlack",
    "TotalBlack",
    "SemiBlack",
    "PostFX",
    "BanishInterface",
];

/// Whether an entry's sequence keeps its line on screen, so leaving it costs a continue.
///
/// ## Why a line's links do not answer this
///
/// A walk charges a continue to leave a line with another LINE behind it, and none to leave one
/// with a MENU behind it, because the menu composes beside the line. Measured in game that is
/// right almost everywhere - and wrong on the entries whose sequence RUNS, which stay up until
/// they are dismissed whatever is behind them.
///
/// A COMMAND SCHEDULED WITH `@` HOLDS BY CONSTRUCTION, since the sequence is not over until the
/// last thing in it has happened; `1467:177` schedules work at 1.5 and 2 seconds and waits. The
/// named commands are the rest of what has been measured, `1467:17`'s animation among them.
///
/// An order that fires and forgets does NOT hold: `LuaRun`, `SetAreaState` and `TravelTo` were
/// each watched in game with the menu composing beside them. That is why the test is not
/// "carries a sequence" - most of the game's entries carry one, and 95.9% of those say only
/// `Continue()`.
pub(crate) fn sequence_holds_the_screen(sequence: &str) -> bool {
    if sequence.is_empty() {
        return false;
    }
    // A scheduled command: '@' then a time. Anything else after '@' is not one.
    let scheduled = sequence
        .match_indices('@')
        .any(|(at, _)| sequence[at + 1..].starts_with(|c: char| c.is_ascii_digit()));
    scheduled || HOLDING_COMMANDS.iter().any(|name| sequence.contains(name))
}

/// The actor an entry names when the player speaks it, as [`ACTOR_FIELD`] spells it.
///
/// A number rather than a name because that is what the field holds. `tests/shipped_index.rs`
/// pins it to the actor table, where 396 is "You".
pub const PLAYER_ACTOR: &str = "396";

/// Every field this module reads out of an entry, and the only ones a shipped index needs
/// to carry.
///
/// ## Why this is public
///
/// The index the mod ships is TRIMMED - the full one is 50 MB and nine tenths of it is
/// prose, titles and articy ids that no search consults. Trimming means someone, somewhere,
/// deciding which fields survive, and that list has to be this list.
///
/// It is not a hypothetical risk. The first attempt at the trim guessed these names -
/// `IsPassiveCheck`, `IsRedCheck`, `IsWhiteCheck` and so on - and every one was wrong. A
/// trimmed index built on that guess would have dropped every check kind, and NOTHING
/// WOULD HAVE FAILED: entries would simply have stopped being checks, the search would have
/// walked straight through them, and the markers would have been quietly wrong.
///
/// The extractor is C# and cannot share a constant with this, so `tests/shipped_index.rs`
/// checks the two against each other instead: for every name here, an entry that has it in
/// the full index must still have it in the trimmed one.
pub const ENTRY_FIELDS_READ: [&str; 16] = [
    ACTOR_FIELD,
    PASSIVE_FIELD,
    ANTIPASSIVE_FIELD,
    RED_FIELD,
    WHITE_FIELD,
    FAKE_FIELD,
    ALWAYS_SUCCEED_FIELD,
    TEST_FIELD,
    KIM_WATCH_FIELD,
    BOOLEAN_ONLY_FIELD,
    FLAG_NAME_FIELD,
    SKILL_TYPE_FIELD,
    CLICK_COST_FIELD,
    COST_ONCE_FIELD,
    HIDDEN_NOT_ENOUGH_FIELD,
    SEQUENCE_FIELD,
];

/// The header line's version property, as the extractor writes it.
///
/// Must match `ShippedIndex.FormatProperty`, which writes the same line.
pub const FORMAT_PROPERTY: &str = "format";

/// The index format this build understands.
///
/// Must match `ShippedIndex.FormatVersion`. A content hash answers "is this the same
/// game"; this answers "is this an index this engine can read" - and an index from an
/// older build would pass its content hash while missing fields the engine has since
/// started reading, which is a cache hit on a file that cannot answer the question.
pub const FORMAT_VERSION: i32 = 4;

/// A shipped index's header, which is its first line.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct IndexHeader {
    #[serde(rename = "format")]
    pub format: i32,
}

/// One conversation, as one line of the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub id: i32,
    /// What this conversation's content reduces to, where the writer computed one.
    ///
    /// Empty for the FULL index, which is a build intermediate that nothing validates
    /// against anything. The shipped index carries one per conversation, because it is a
    /// cache of a database the plugin can also read for itself and this is what makes the
    /// two comparable. Never computed here - see `ConversationHasher`, which is the one
    /// routine that reduces a conversation, and which this engine is deliberately not a
    /// third writer of.
    #[serde(default)]
    pub hash: String,
    /// The conversation's own fields the engine reads - a journal task's conditions, named in
    /// [`conversation_fields_read`] - and empty for every other conversation.
    #[serde(default)]
    pub fields: HashMap<String, String>,
    #[serde(default)]
    pub entries: Vec<EntryRecord>,
}

/// How many subtasks a journal task can have: `JournalImporter.MAX_NR_OF_SUBTASKS`.
pub const JOURNAL_SUBTASK_LIMIT: usize = 12;

/// Every CONVERSATION field the engine reads, which is a journal task's conditions: the main
/// task's display, done and cancel, then the same for each subtask in order.
///
/// Must match `IndexFields.ConversationRead`, which builds the same list the same way;
/// `tests/shipped_index.rs` checks the trimmed index keeps every one.
pub fn conversation_fields_read() -> Vec<String> {
    let mut names: Vec<String> = journal::JOURNAL_ROLES
        .iter()
        .map(|role| format!("{role}_condition_main"))
        .collect();
    for subtask in 1..=JOURNAL_SUBTASK_LIMIT {
        names.extend(
            journal::JOURNAL_ROLES
                .iter()
                .map(|role| format!("{role}_subtask_{subtask:02}")),
        );
    }
    names
}

pub mod journal;
pub mod price;

/// One dialogue entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryRecord {
    pub id: i32,
    #[serde(default)]
    pub group: bool,
    #[serde(default)]
    pub guard: String,
    #[serde(default)]
    pub script: String,
    /// Destination ENTRY ids, paired positionally with `to_conversation`.
    #[serde(default)]
    pub to: Vec<i32>,
    /// Destination CONVERSATION ids, one per `to` entry.
    ///
    /// Absent or short on purpose: an entry all of whose links stay inside its own
    /// conversation does not carry the key, so a missing element means "this
    /// conversation", which [`links_of`] fills in.
    #[serde(default)]
    pub to_conversation: Vec<i32>,
    #[serde(default)]
    pub fields: HashMap<String, String>,
}

/// The index, by conversation id.
pub type Index = HashMap<i32, ConversationRecord>;

/// Reads `conversation_index.jsonl`, discarding its header if it has one.
pub fn read_index(path: &Path) -> anyhow::Result<Index> {
    read_index_with_header(path).map(|(index, _)| index)
}

/// The same, keeping what the header said.
///
/// A shipped index opens with `{"format":4}`; the full index has no header at all, and
/// then there is no version and no per-conversation hash, so nothing can be validated
/// against it. That is not an error - it is the mod shipping a build intermediate, and it
/// works exactly as well as it did before there was such a thing as validation.
///
/// A header naming a version this build does not understand IS an error. An index the
/// engine half-understands is worse than none: it would pass a content check while missing
/// fields the engine has since started reading.
pub fn read_index_with_header(path: &Path) -> anyhow::Result<(Index, Option<IndexHeader>)> {
    let mut index = Index::new();
    let mut header = None;

    for (number, line) in BufReader::new(File::open(path)?).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        if number == 0
            && let Some(found) = parse_header(&line)
        {
            if found.format != FORMAT_VERSION {
                anyhow::bail!(
                    "{} is a version {} index; this build reads version {}",
                    path.display(),
                    found.format,
                    FORMAT_VERSION,
                );
            }

            header = Some(found);
            continue;
        }

        let conversation: ConversationRecord = serde_json::from_str(&line)?;
        index.insert(conversation.id, conversation);
    }

    Ok((index, header))
}

/// The header, if this line is one.
///
/// Told apart by the properties rather than by the shape of the text, because getting it
/// wrong is silent: a header read as a conversation is a record with id 0 and no entries,
/// which looks like a real, empty conversation and would answer every question about it
/// with "nothing there".
fn parse_header(line: &str) -> Option<IndexHeader> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let object = value.as_object()?;
    if !object.contains_key(FORMAT_PROPERTY) || object.contains_key("id") {
        return None;
    }

    serde_json::from_value(value).ok()
}

/// One variable, as the database declares it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VariableRecord {
    pub name: String,
    /// "Boolean", "Number", or empty where the database declares none.
    #[serde(rename = "type")]
    pub declared: String,
    /// The value it starts at, as the database writes it.
    pub initial: String,
}

/// What the database says its variables are, so a world need not guess.
///
/// ## Why guessing was not good enough
///
/// A variable nobody has written reads BOOLEAN FALSE, which is what the game does - an
/// unset Lua variable is nil and nil is falsy - and is right for the great majority of
/// guards, including the 5,994 of 13,059 distinct ones ending in `== false`. It is wrong
/// for a counter: `>= 3` against a boolean cannot be evaluated at all, because
/// `try_as_number` gives nothing for one, so the guard turns undecidable and the branch
/// stays open. Answering number zero for everything instead is a worse bug, since
/// `GuardValue::equals` is kind-sensitive and all 5,994 of those would start answering
/// false.
///
/// The declared type settles it, and there are only 142 numbers among 10,645 variables -
/// so the guessing was wrong about one variable in seventy-five, and undecidable on
/// exactly the comparisons that count things.
///
/// THE INITIAL VALUE MATTERS TOO, and is not always zero: `apt.smoker_second_departure`
/// starts at 9999 and `apt.apt_for_rent_door_closed_counter` at -1. A fixture that
/// assumed zero was wrong about those before it was wrong about anything else.
#[derive(Debug, Clone, Default)]
pub struct VariableTable {
    initial: HashMap<String, crate::core::guard_value::GuardValue>,
    numbers: usize,
}

impl VariableTable {
    /// What the extractor calls the file.
    pub const FILE_NAME: &'static str = "variables.jsonl";

    /// Reads `variables.jsonl`, as `dotnet run --project tools/DialogueExtract -- variables`
    /// writes it.
    pub fn read(path: &Path) -> anyhow::Result<Self> {
        let mut table = Self::default();
        for line in BufReader::new(File::open(path)?).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let record: VariableRecord = serde_json::from_str(&line)?;
            table.add(&record);
        }

        Ok(table)
    }

    /// Records one variable at its declared type.
    ///
    /// Public so a test can build a table without a file - the reader above is the only
    /// other caller.
    pub fn add(&mut self, record: &VariableRecord) {
        use crate::core::guard_value::GuardValue;

        let value = match record.declared.as_str() {
            "Number" => {
                self.numbers += 1;
                // A declared number whose initial value will not parse is still a number;
                // zero is the honest reading of "it starts unset", and it keeps the KIND
                // right, which is the half that decides whether a comparison can be
                // answered at all.
                GuardValue::from_number(record.initial.trim().parse::<f64>().unwrap_or(0.0))
            }
            "Boolean" => {
                GuardValue::from_boolean(record.initial.trim().eq_ignore_ascii_case("true"))
            }
            // Anything else is carried as text rather than guessed at. Nothing in the
            // shipped database is anything else, so this is a door rather than a path.
            _ => GuardValue::from_text(record.initial.clone()),
        };

        self.initial.insert(record.name.clone(), value);
    }

    /// What the database says this variable starts as, if it declares it at all.
    pub fn initial(&self, name: &str) -> Option<&crate::core::guard_value::GuardValue> {
        self.initial.get(name)
    }

    /// How many variables the table holds.
    pub fn len(&self) -> usize {
        self.initial.len()
    }

    pub fn is_empty(&self) -> bool {
        self.initial.is_empty()
    }

    /// How many of them are declared numbers - the counters.
    pub fn numbers(&self) -> usize {
        self.numbers
    }
}

/// Every conversation reachable from `start` by following links, `start` included.
///
/// Sorted, and that is not cosmetic: the order conversations are visited decides the
/// order their entries intern symbols, which decides the slot numbering - and a slot
/// number is a decision-diagram variable number. An unordered walk would give the same
/// graph a different variable order on different runs, and make any symbolic measurement
/// unrepeatable.
///
/// ## If this is ever CACHED, do not store a group of one
///
/// Computed on demand today, and nothing anywhere persists a group. Whoever changes that
/// should leave the singletons out and read a miss as "the group is just this
/// conversation" (de-wncd.2): a stored group of one is a stored default.
///
/// MEASURED, and it is most of the table. `performance/group_census.rs`, 2026-09-07:
/// 1,372 of the 1,422 distinct groups are a single conversation, and the other 50 carry
/// fifty-five per cent of the entries. So omitting the singletons takes a group store from
/// fourteen hundred rows to FIFTY, which is the difference between an artefact worth
/// arguing about and one that is obviously free.
///
/// THE INDEX ALREADY DOES EXACTLY THIS one level down - see `EntryRecord::to_conversation`,
/// which is absent or short precisely so that a missing element means "this conversation".
/// A group cache would be the same rule applied to the walk rather than to a link.
///
/// The one distinction that has to survive is between a conversation that is a singleton
/// and one THE INDEX DOES NOT HOLD. The second is already an error at every call site, and
/// a miss that quietly meant "a group of one" would turn a bad conversation id into a
/// plausible-looking answer.
pub fn discover_group(index: &Index, start: i32) -> Vec<i32> {
    let mut group = HashSet::new();
    let mut pending = VecDeque::new();
    group.insert(start);
    pending.push_back(start);

    while let Some(id) = pending.pop_front() {
        let Some(conversation) = index.get(&id) else {
            continue;
        };
        for entry in &conversation.entries {
            for &destination in &entry.to_conversation {
                // A link to a conversation the index does not hold ends the branch
                // rather than being an error, matching the C# builder: the caller
                // decides how wide the index is.
                if index.contains_key(&destination) && group.insert(destination) {
                    pending.push_back(destination);
                }
            }
        }
    }

    let mut ids: Vec<i32> = group.into_iter().collect();
    ids.sort_unstable();
    ids
}

/// Builds the graph for the whole group `start` belongs to.
///
/// Returns the graph and the conversations it spans, in the order they were built - the
/// order that fixed the slot numbering.
pub fn build_group_graph(index: &Index, start: i32) -> Result<(LookAheadGraph, Vec<i32>), String> {
    if !index.contains_key(&start) {
        return Err(format!("Conversation {} is not in the index.", start));
    }

    let group = discover_group(index, start);
    let mut symbols = StateSymbols::new();
    let mut nodes = Vec::new();
    // THE JOURNAL, which lives in task conversations that are no part of any group - so it is
    // read from the whole index, for every group. A scan of the conversation fields only.
    let journal = journal::Journal::from_index(index);

    for &conversation_id in &group {
        for entry in &index[&conversation_id].entries {
            // The conversation id comes from the conversation, not the entry: entry ids
            // restart at 0 in every conversation.
            let node_id = DialogueNodeId::new(conversation_id, entry.id);

            let guard = parse_guard(&entry.guard)
                .map(|guard| journal.with_tasks_as_variables(&guard))
                .unwrap_or_else(|_| Guard::always_true());
            let mut actions = parse_actions_with_journal(&entry.script, &mut symbols, &journal);

            let kind = determine_kind(&entry.fields);
            actions.extend(passive_success_actions(&entry.fields, kind, &mut symbols));
            let failure_actions = check_failure_actions(&entry.fields, kind, &mut symbols);
            let (cost, cost_once, hidden_when_unaffordable) = parse_cost(&entry.fields);
            let cost = cost.max(0);
            let price_scale = if cost > 0 {
                price::scale_of(&index[&conversation_id].entries, entry)
            } else {
                None
            };
            let (flag_slot, failed_flag_slot) = parse_flags(&entry.fields, &mut symbols, kind);
            let boolean_only = read_boolean(&entry.fields, BOOLEAN_ONLY_FIELD);
            let closes_once_seen = kind == DialogueCheckKind::Fake
                || (kind == DialogueCheckKind::KimSwitch && !boolean_only);
            let seen_slot = if closes_once_seen {
                symbols.seen(node_id) as i32
            } else {
                -1
            };

            let mut node = LookAheadNode {
                is_group: entry.group,
                kind,
                guard,
                skill_moves: SkillMoves::of(actions.iter().chain(&failure_actions), &symbols),
                actions,
                failure_actions,
                links: links_of(entry, conversation_id),
                cost,
                click_cost: cost,
                price_scale,
                cost_once,
                hidden_when_unaffordable,
                flag_slot,
                failed_flag_slot,
                boolean_only,
                seen_slot,
                ..LookAheadNode::new(node_id)
            };
            node.player = entry
                .fields
                .get(ACTOR_FIELD)
                .is_some_and(|actor| actor == PLAYER_ACTOR);
            node.holds_the_screen = entry
                .fields
                .get(SEQUENCE_FIELD)
                .is_some_and(|sequence| sequence_holds_the_screen(sequence));
            nodes.push(node);
        }
    }

    let (nodes, symbols) = keeping_only_read_slots(nodes, symbols);

    Ok((LookAheadGraph::new(nodes, symbols)?, group))
}

/// Drops every slot no guard in the group reads, renumbering what is left.
///
/// ## Why it is exact
///
/// A conversation group is closed under links, so a search over it only ever evaluates
/// guards belonging to it. A slot no guard in the group reads cannot change which entries
/// are reachable, whatever an action writes to it - it is write-only for the length of
/// any search. So this removes no information: it removes carrying.
///
/// ## Why it is worth doing
///
/// Between a quarter and nearly a half of a group's slots are like this - measured, in
/// `performance/unread_slots.rs` - and a state-at-a-time search's cost is dominated by copying and
/// comparing the slot vector, about seventy per cent of the per-state cost on the widest
/// group. The slots are written by actions, copied for every state, hashed and compared,
/// and can never change an answer.
///
/// The symbolic side has done this since it existed, in
/// [`DataLayout::keeping_only_read`], which is where the rule comes from. This puts it
/// one level lower, so both engines get a graph that never mentions the dropped slots
/// rather than each trimming its own copy.
///
/// ## What is kept
///
/// - Every name any guard reads, which [`DataLayout::read_by_nodes`] computes, including
///   the subjects of the queries answered from search state and a rolled check's own pass
///   and fail flags.
/// - The engine's bookkeeping, `seen:` and `once:`. No guard mentions either and every
///   search depends on both - a seen marker is what closes a once-only check, a once
///   marker is what stops a purchase being charged twice.
///
/// `once:` slots do not exist yet at this point - [`LookAheadGraph::new`] interns them,
/// after this - so the prefix guards nothing today. It is here because the rule is "the
/// engine's own bookkeeping stays", not "seen stays", and a later change that moved the
/// interning earlier should not quietly cost every once slot in the game.
///
/// ## Where a mistake would show
///
/// In ANSWERS, not in performance: a slot index is baked into the nodes, so a
/// misnumbering sends a read somewhere else rather than degrading quietly. Every search
/// test asserts on what a search finds, `tests/corpus.rs` runs the builders over the whole
/// shipped database, and `tests/backward_oracle.rs` compares this search against the
/// symbolic search, which numbers its own variables independently.
fn keeping_only_read_slots(
    nodes: Vec<LookAheadNode>,
    symbols: StateSymbols,
) -> (Vec<LookAheadNode>, StateSymbols) {
    let reads = DataLayout::read_by_nodes(nodes.iter(), &symbols);

    // AND WHAT SOMETHING WRITES, which is the other half of the same rule.
    //
    // A slot a guard reads but nothing in the group writes cannot change during a search:
    // the seed puts the world's value in it and nothing ever moves it. Carrying it costs a
    // column in every state vector and a variable in every diagram, to hold a constant.
    //
    // WRITTEN MEANS WRITTEN BY ANYTHING, NOT BY AN ACTION. Getting that wrong is what broke
    // the first two attempts at this trim, and it broke them silently in ANSWERS: a rolled
    // check records its own result in `flag_slot` and `failed_flag_slot`, and the ENGINE
    // writes those, not any parsed action. Counting only actions classified them as never
    // written, dropped them, and `renumber` turned them into -1 - so the check could no
    // longer record whether it had passed, the pass branch resolved somewhere else, and
    // five scenarios in tests/branch_shapes.rs drew the wrong colour.
    //
    // The same is true of `seen_slot`. `once_slot` is -1 at this point - those are interned
    // later, by `LookAheadGraph::new` - and is included so that moving the interning earlier
    // cannot quietly reintroduce the same bug.
    let mut written = vec![false; symbols.count()];
    let mark = |slot: i32, written: &mut Vec<bool>| {
        if let Ok(slot) = usize::try_from(slot)
            && slot < written.len()
        {
            written[slot] = true;
        }
    };
    for node in &nodes {
        mark(node.flag_slot, &mut written);
        mark(node.failed_flag_slot, &mut written);
        mark(node.seen_slot, &mut written);
        mark(node.once_slot, &mut written);
        for action in node.all_actions() {
            // Money, clock and unmodelled actions carry no slot.
            if action.writes_slot() {
                mark(action.slot(), &mut written);
            }
        }
    }

    // A CONDITIONAL WRITE WHOSE TESTED SLOT NOTHING WRITES tests the world's value, which is
    // constant for a search - so the condition is settled when the graph is fitted rather than
    // carried as a slot the search splits on. Dropping those slots took conversation 368's menu
    // from 986 ms to 610 ms and its diagram from 1.42 to 0.84 million nodes.
    let settle = |action: DialogueAction| match action.unless() {
        Some(tested) if !written.get(tested).copied().unwrap_or(false) => {
            let variable = symbols
                .name_of(tested)
                .expect("a tested slot has a name")
                .to_string();
            action.settled_by_world(&variable)
        }
        _ => action,
    };
    let nodes: Vec<LookAheadNode> = nodes
        .into_iter()
        .map(|mut node| {
            node.actions = node.actions.into_iter().map(&settle).collect();
            node.failure_actions = node.failure_actions.into_iter().map(&settle).collect();
            node
        })
        .collect();

    // WHAT A CONDITIONAL WRITE STILL TESTS, which the group writes too: kept whatever else reads
    // it, since the action has no other way to ask it.
    let tested: HashSet<String> = nodes
        .iter()
        .flat_map(|node| DataLayout::tested_by_actions(node, &symbols))
        .collect();

    let keep: Vec<bool> = (0..symbols.count())
        .map(|slot| match symbols.name_of(slot) {
            Some(name) => {
                name.starts_with(SEEN_PREFIX)
                    || name.starts_with(ONCE_PREFIX)
                    || tested.contains(name)
                    || (reads.contains(name) && written[slot])
            }
            // A slot with no name is one this table never interned, so there is nothing
            // to keep and nothing pointing at it.
            None => false,
        })
        .collect();

    let (kept, map) = symbols.retaining(&keep);

    let renumber = |slot: i32| -> i32 {
        usize::try_from(slot)
            .ok()
            .and_then(|slot| map.get(slot).copied())
            .unwrap_or(-1)
    };

    let nodes = nodes
        .into_iter()
        .map(|mut node| {
            node.flag_slot = renumber(node.flag_slot);
            node.failed_flag_slot = renumber(node.failed_flag_slot);
            node.seen_slot = renumber(node.seen_slot);
            // An action whose slot has gone is REMOVED rather than renumbered - see
            // `DialogueAction::renumbered` for why writing -1 would be a different thing
            // entirely.
            let renumbered = |actions: Vec<DialogueAction>| {
                actions
                    .into_iter()
                    .filter_map(|action| action.renumbered(&map))
                    .collect()
            };
            node.actions = renumbered(node.actions);
            node.failure_actions = renumbered(node.failure_actions);
            node
        })
        .collect();

    (nodes, kept)
}

/// An entry's outgoing links, pairing each destination entry with its conversation.
pub fn links_of(entry: &EntryRecord, conversation_id: i32) -> Vec<DialogueNodeId> {
    entry
        .to
        .iter()
        .enumerate()
        .map(|(i, &destination_entry)| {
            // Short or absent means the link stays in this conversation.
            let destination_conversation = entry
                .to_conversation
                .get(i)
                .copied()
                .unwrap_or(conversation_id);
            DialogueNodeId::new(destination_conversation, destination_entry)
        })
        .collect()
}

/// Which special node type an entry is, if any.
///
/// A fixed precedence, not whatever order the fields come in: `fields` is a map, so an
/// entry carrying two of these would otherwise get a different kind on different runs.
/// The order matches the C# `ConversationIndex.KindOf`.
pub fn determine_kind(fields: &HashMap<String, String>) -> DialogueCheckKind {
    if fields.contains_key(PASSIVE_FIELD) {
        return DialogueCheckKind::Passive;
    }
    if fields.contains_key(RED_FIELD) {
        return DialogueCheckKind::Red;
    }
    if fields.contains_key(WHITE_FIELD) {
        return DialogueCheckKind::White;
    }
    if fields.contains_key(FAKE_FIELD) {
        return DialogueCheckKind::Fake;
    }
    if fields.contains_key(TEST_FIELD) {
        return DialogueCheckKind::Test;
    }
    if fields.contains_key(KIM_WATCH_FIELD) {
        return DialogueCheckKind::KimSwitch;
    }
    DialogueCheckKind::None
}

/// A boolean field, read case-insensitively as the C# `bool.TryParse` does.
fn read_boolean(fields: &HashMap<String, String>, name: &str) -> bool {
    fields
        .get(name)
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

/// An entry's cost, whether it is charged once, and whether poverty hides it.
pub fn parse_cost(fields: &HashMap<String, String>) -> (i32, bool, bool) {
    let cost = fields
        .get(CLICK_COST_FIELD)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    (
        cost,
        read_boolean(fields, COST_ONCE_FIELD),
        read_boolean(fields, HIDDEN_NOT_ENOUGH_FIELD),
    )
}

/// What a passive check's success adds while a thought is fixed, as once actions on the entry -
/// see [`crate::core::thought_effects`]. Empty for anything that is not a passive check paying
/// that price.
pub(crate) fn passive_success_actions(
    fields: &HashMap<String, String>,
    kind: DialogueCheckKind,
    symbols: &mut StateSymbols,
) -> Vec<DialogueAction> {
    use crate::core::thought_effects::passive_success_effect;

    if kind != DialogueCheckKind::Passive || fields.contains_key(ANTIPASSIVE_FIELD) {
        return Vec::new();
    }
    fields
        .get(ACTOR_FIELD)
        .and_then(|actor| passive_success_effect(actor))
        .map(|(thought, effect)| effect.action(thought, symbols, PASSIVE_PRICE.to_string()))
        .into_iter()
        .collect()
}

/// What a check's failing branch adds while a thought is fixed - see
/// [`crate::core::thought_effects`]. Empty for a check whose skill names no ability, for a fake
/// check forced to succeed, and for anything that is not a rolled or fake check.
pub(crate) fn check_failure_actions(
    fields: &HashMap<String, String>,
    kind: DialogueCheckKind,
    symbols: &mut StateSymbols,
) -> Vec<DialogueAction> {
    use crate::core::thought_effects::{RolledKind, ability_of_skill_id, failure_effects};

    let rolled = match kind {
        DialogueCheckKind::White => RolledKind::White,
        DialogueCheckKind::Red => RolledKind::Red,
        DialogueCheckKind::Fake if !read_boolean(fields, ALWAYS_SUCCEED_FIELD) => RolledKind::Red,
        _ => return Vec::new(),
    };
    let Some(ability) = fields
        .get(SKILL_TYPE_FIELD)
        .and_then(|id| ability_of_skill_id(id))
    else {
        return Vec::new();
    };
    // Built as once actions like every thought effect; a failing branch applies them with no
    // once slot, since a check fails at most once and its flag already says so.
    failure_effects(rolled, ability)
        .into_iter()
        .map(|(thought, effect)| effect.action(thought, symbols, CHECK_RESULT.to_string()))
        .collect()
}

/// What the actions a check's result adds are named in reports.
const PASSIVE_PRICE: &str = "CheckAlterant.PassiveCheckSuccessPrice";
const CHECK_RESULT: &str = "CheckAlterant";

/// The success and failure flag slots of a rolled check, or -1 for anything else.
pub fn parse_flags(
    fields: &HashMap<String, String>,
    symbols: &mut StateSymbols,
    kind: DialogueCheckKind,
) -> (i32, i32) {
    if kind != DialogueCheckKind::Red && kind != DialogueCheckKind::White {
        return (-1, -1);
    }

    match fields.get(FLAG_NAME_FIELD) {
        Some(flag) if !flag.trim().is_empty() => {
            let passed = symbols.variable(flag);
            let failed = symbols.variable(&format!("{flag}{FAILED_FLAG_SUFFIX}"));
            (passed as i32, failed as i32)
        }
        _ => (-1, -1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i32, to: Vec<i32>, to_conversation: Vec<i32>) -> EntryRecord {
        EntryRecord {
            id,
            group: false,
            guard: String::new(),
            script: String::new(),
            to,
            to_conversation,
            fields: HashMap::new(),
        }
    }

    fn conversation(id: i32, entries: Vec<EntryRecord>) -> ConversationRecord {
        ConversationRecord {
            id,
            hash: String::new(),
            fields: HashMap::new(),
            entries,
        }
    }

    fn index_of(conversations: Vec<ConversationRecord>) -> Index {
        conversations.into_iter().map(|c| (c.id, c)).collect()
    }

    /// A journal write whose tested variable nothing in the group writes is settled by the
    /// world: the tested slot is dropped, and whether the write fires is decided when the graph
    /// is fitted - so a task is revealed only where the world has not cancelled it.
    #[test]
    fn a_conditional_write_on_an_unwritten_variable_is_settled_by_the_world() {
        use crate::core::action::DialogueActionKind;
        use crate::core::guard_value::GuardValue;
        use crate::graph::Fitting;
        use crate::world::test_world::TestWorld;

        let mut task = conversation(7, Vec::new());
        task.fields = [
            ("display_condition_main", r#"Variable["TASK.wall"]"#),
            ("done_condition_main", r#"Variable["TASK.wall_done"]"#),
            (
                "cancel_condition_main",
                r#"Variable["TASK.wall_cancelled"]"#,
            ),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
        let mut gain = entry(1, vec![2], vec![]);
        gain.script = r#"GainTask("TASK.wall")"#.to_string();
        let mut reader = entry(2, vec![], vec![]);
        reader.guard = r#"Variable["TASK.wall"]"#.to_string();
        let index = index_of(vec![
            task,
            conversation(1, vec![entry(0, vec![1], vec![]), gain, reader]),
        ]);

        let (graph, _) = build_group_graph(&index, 1).expect("the group builds");
        assert!(graph.symbols().find("TASK.wall_cancelled").is_none());
        let reveal = &graph
            .get(DialogueNodeId::new(1, 1))
            .expect("the entry")
            .actions[0];
        assert_eq!(reveal.kind(), DialogueActionKind::Assign);
        assert_eq!(reveal.unset_variable(), Some("TASK.wall_cancelled"));

        let reaches = |cancelled: bool| {
            let world = TestWorld::new()
                .set_variable("TASK.wall", GuardValue::from_boolean(false))
                .set_variable("TASK.wall_cancelled", GuardValue::from_boolean(cancelled));
            let mut fitted = graph.clone();
            fitted.fit(&Fitting::read(&fitted, &world));
            crate::oracle::walk(
                &fitted,
                DialogueNodeId::new(1, 0),
                &world,
                crate::oracle::COUNTER_CAP,
            )
            .reached(DialogueNodeId::new(1, 2))
        };
        assert!(reaches(false));
        assert!(!reaches(true));
    }

    #[test]
    fn a_link_with_no_conversation_stays_in_its_own() {
        let e = entry(0, vec![7, 8], vec![]);
        assert_eq!(
            links_of(&e, 42),
            vec![DialogueNodeId::new(42, 7), DialogueNodeId::new(42, 8)]
        );
    }

    #[test]
    fn a_short_conversation_list_only_covers_its_own_links() {
        // Two destinations, one named conversation: the second falls back.
        let e = entry(0, vec![7, 8], vec![99]);
        assert_eq!(
            links_of(&e, 42),
            vec![DialogueNodeId::new(99, 7), DialogueNodeId::new(42, 8)]
        );
    }

    #[test]
    fn the_group_follows_links_transitively() {
        let index = index_of(vec![
            conversation(1, vec![entry(0, vec![0], vec![2])]),
            conversation(2, vec![entry(0, vec![0], vec![3])]),
            conversation(3, vec![entry(0, vec![], vec![])]),
            conversation(4, vec![entry(0, vec![], vec![])]),
        ]);

        // 1 reaches 2 reaches 3. Nothing reaches 4.
        assert_eq!(discover_group(&index, 1), vec![1, 2, 3]);
        assert_eq!(discover_group(&index, 4), vec![4]);
    }

    #[test]
    fn a_link_out_of_the_index_ends_the_branch() {
        let index = index_of(vec![conversation(1, vec![entry(0, vec![0], vec![404])])]);

        assert_eq!(discover_group(&index, 1), vec![1]);
    }

    #[test]
    fn a_cycle_between_conversations_terminates() {
        let index = index_of(vec![
            conversation(1, vec![entry(0, vec![0], vec![2])]),
            conversation(2, vec![entry(0, vec![0], vec![1])]),
        ]);

        assert_eq!(discover_group(&index, 1), vec![1, 2]);
    }

    #[test]
    fn the_check_kind_precedence_does_not_depend_on_field_order() {
        let mut fields = HashMap::new();
        fields.insert(RED_FIELD.to_string(), "10".to_string());
        fields.insert(PASSIVE_FIELD.to_string(), "8".to_string());

        // Passive wins over Red however the map happens to iterate.
        assert_eq!(determine_kind(&fields), DialogueCheckKind::Passive);
    }

    #[test]
    fn booleans_are_read_case_insensitively() {
        let mut fields = HashMap::new();
        fields.insert(COST_ONCE_FIELD.to_string(), "true".to_string());
        fields.insert(HIDDEN_NOT_ENOUGH_FIELD.to_string(), "True".to_string());
        fields.insert(CLICK_COST_FIELD.to_string(), "50".to_string());

        assert_eq!(parse_cost(&fields), (50, true, true));
    }

    #[test]
    fn a_missing_cost_is_free_and_a_junk_cost_is_not_an_error() {
        let mut fields = HashMap::new();
        fields.insert(CLICK_COST_FIELD.to_string(), "not a number".to_string());

        assert_eq!(parse_cost(&fields), (0, false, false));
        assert_eq!(parse_cost(&HashMap::new()), (0, false, false));
    }

    /// A file with a header reads as its conversations, and says what version it was.
    #[test]
    fn a_shipped_index_reports_its_format() {
        let path = written(&format!(
            "{{\"format\":{FORMAT_VERSION}}}\n{{\"id\":7,\"hash\":\"abc\",\"entries\":[]}}\n"
        ));

        let (index, header) = read_index_with_header(path.path()).expect("it reads");
        assert_eq!(header.map(|h| h.format), Some(FORMAT_VERSION));
        assert_eq!(index.len(), 1);
        assert_eq!(index[&7].hash, "abc");
    }

    /// The full index has no header, and that is not an error - it cannot be validated.
    #[test]
    fn an_index_with_no_header_reads_with_no_format_and_no_hash() {
        let path = written("{\"id\":7,\"entries\":[]}\n");

        let (index, header) = read_index_with_header(path.path()).expect("it reads");
        assert!(header.is_none());
        assert!(index[&7].hash.is_empty());
    }

    /// A version this build does not read is refused, not half-understood.
    ///
    /// The failure being prevented: an older index passes its CONTENT hash - it really is
    /// the same game - while missing fields the engine has since started reading, so the
    /// cache hits on a file that cannot answer the question.
    #[test]
    fn an_index_from_another_format_is_refused() {
        let path = written(concat!("{\"format\":99}\n", "{\"id\":7,\"entries\":[]}\n",));

        let refused = read_index_with_header(path.path()).expect_err("it must be refused");
        assert!(refused.to_string().contains("version 99"), "{refused}");
    }

    /// A header is told apart by its properties, not by where it is.
    ///
    /// Getting this wrong is silent: a header read as a conversation is a record with id 0
    /// and no entries, which looks like a real, empty conversation.
    #[test]
    fn a_header_is_never_mistaken_for_a_conversation() {
        let path = written(&format!(
            "{{\"format\":{FORMAT_VERSION}}}\n{{\"id\":7,\"entries\":[]}}\n"
        ));
        let (index, _) = read_index_with_header(path.path()).expect("it reads");

        assert!(!index.contains_key(&0), "the header became conversation 0");
        assert_eq!(index.len(), 1);
    }

    /// A file written to a temporary path, removed when it goes out of scope.
    struct Written(std::path::PathBuf);

    impl Written {
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Written {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn written(contents: &str) -> Written {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);

        let path = std::env::temp_dir().join(format!(
            "gct-index-{}-{}.jsonl",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::write(&path, contents).expect("the fixture writes");
        Written(path)
    }
}
