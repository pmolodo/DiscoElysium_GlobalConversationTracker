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

use crate::core::action::DialogueActionKind;
use crate::core::guard::GuardExpression;
use crate::core::state::{StateSymbols, ONCE_PREFIX, SEEN_PREFIX};
use crate::core::types::{DialogueCheckKind, DialogueNodeId};
use crate::graph::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::parser::action_parser::parse_actions;
use crate::parser::guard_parser::parse_guard;
use crate::symbolic::data_layout::DataLayout;

/// Field names as the asset spells them.
const PASSIVE_FIELD: &str = "DifficultyPass";
const RED_FIELD: &str = "DifficultyRed";
const WHITE_FIELD: &str = "DifficultyWhite";
const FAKE_FIELD: &str = "DifficultyAtmo";
const TEST_FIELD: &str = "HiddenTest";
const KIM_WATCH_FIELD: &str = "kim_watch";
const BOOLEAN_ONLY_FIELD: &str = "boolean_only";
const FLAG_NAME_FIELD: &str = "FlagName";
const CLICK_COST_FIELD: &str = "ClickCost";
const COST_ONCE_FIELD: &str = "CostOnce";
const HIDDEN_NOT_ENOUGH_FIELD: &str = "HiddenNotEnough";

/// Every field this module reads out of an entry, and the only ones a shipped index needs
/// to carry.
///
/// ## Why this is public
///
/// The index the mod ships is TRIMMED - the full one is 50 MB and nine tenths of it is
/// prose, titles and articy ids that no crawl consults. Trimming means someone, somewhere,
/// deciding which fields survive, and that list has to be this list.
///
/// It is not a hypothetical risk. The first attempt at the trim guessed these names -
/// `IsPassiveCheck`, `IsRedCheck`, `IsWhiteCheck` and so on - and every one was wrong. A
/// trimmed index built on that guess would have dropped every check kind, and NOTHING
/// WOULD HAVE FAILED: entries would simply have stopped being checks, the crawl would have
/// walked straight through them, and the markers would have been quietly wrong.
///
/// The extractor is C# and cannot share a constant with this, so `tests/shipped_index.rs`
/// checks the two against each other instead: for every name here, an entry that has it in
/// the full index must still have it in the trimmed one.
pub const ENTRY_FIELDS_READ: [&str; 11] = [
    PASSIVE_FIELD,
    RED_FIELD,
    WHITE_FIELD,
    FAKE_FIELD,
    TEST_FIELD,
    KIM_WATCH_FIELD,
    BOOLEAN_ONLY_FIELD,
    FLAG_NAME_FIELD,
    CLICK_COST_FIELD,
    COST_ONCE_FIELD,
    HIDDEN_NOT_ENOUGH_FIELD,
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
pub const FORMAT_VERSION: i32 = 1;

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
    #[serde(default)]
    pub entries: Vec<EntryRecord>,
}

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
/// A shipped index opens with `{"format":1}`; the full index has no header at all, and
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

        if number == 0 {
            if let Some(found) = parse_header(&line) {
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
            "Boolean" => GuardValue::from_boolean(record.initial.trim().eq_ignore_ascii_case("true")),
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
pub fn discover_group(index: &Index, start: i32) -> Vec<i32> {
    let mut group = HashSet::new();
    let mut pending = VecDeque::new();
    group.insert(start);
    pending.push_back(start);

    while let Some(id) = pending.pop_front() {
        let Some(conversation) = index.get(&id) else { continue };
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
pub fn build_group_graph(
    index: &Index,
    start: i32,
) -> Result<(LookAheadGraph, Vec<i32>), String> {
    if !index.contains_key(&start) {
        return Err(format!("Conversation {} is not in the index.", start));
    }

    let group = discover_group(index, start);
    let mut symbols = StateSymbols::new();
    let mut nodes = Vec::new();

    for &conversation_id in &group {
        for entry in &index[&conversation_id].entries {
            // The conversation id comes from the conversation, not the entry: entry ids
            // restart at 0 in every conversation.
            let node_id = DialogueNodeId::new(conversation_id, entry.id);

            let guard =
                parse_guard(&entry.guard).unwrap_or_else(|_| GuardExpression::always_true());
            let actions = parse_actions(&entry.script, &mut symbols);

            let kind = determine_kind(&entry.fields);
            let (cost, cost_once, hidden_when_unaffordable) = parse_cost(&entry.fields);
            let (flag_slot, failed_flag_slot) = parse_flags(&entry.fields, &mut symbols, kind);
            let boolean_only = read_boolean(&entry.fields, BOOLEAN_ONLY_FIELD);
            let closes_once_seen = kind == DialogueCheckKind::Fake
                || (kind == DialogueCheckKind::KimSwitch && !boolean_only);
            let seen_slot = if closes_once_seen { symbols.seen(node_id) as i32 } else { -1 };

            nodes.push(LookAheadNode::new(
                node_id,
                entry.group,
                kind,
                guard,
                actions,
                links_of(entry, conversation_id),
                cost.max(0),
                cost_once,
                hidden_when_unaffordable,
                flag_slot,
                failed_flag_slot,
                boolean_only,
                seen_slot,
            ));
        }
    }

    let (nodes, symbols) = keeping_only_read_slots(nodes, symbols);

    Ok((LookAheadGraph::new(nodes, symbols)?, group))
}

/// Drops every slot no guard in the group reads, renumbering what is left.
///
/// ## Why it is exact
///
/// A conversation group is closed under links, so a crawl over it only ever evaluates
/// guards belonging to it. A slot no guard in the group reads cannot change which entries
/// are reachable, whatever an action writes to it - it is write-only for the length of
/// any crawl. So this removes no information: it removes carrying.
///
/// ## Why it is worth doing
///
/// Between a quarter and nearly a half of a group's slots are like this - measured, in
/// `tests/unread_slots.rs` - and the explicit crawl's cost is dominated by copying and
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
///   the subjects of the queries answered from crawl state and a rolled check's own pass
///   and fail flags.
/// - The engine's bookkeeping, `seen:` and `once:`. No guard mentions either and every
///   crawl depends on both - a seen marker is what closes a once-only check, a once
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
/// misnumbering sends a read somewhere else rather than degrading quietly. Every crawl
/// test asserts on what a search finds, `tests/corpus.rs` runs the builders over the whole
/// shipped database, and `tests/backward_oracle.rs` compares this crawl against the
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
    let mut mark = |slot: i32, written: &mut Vec<bool>| {
        if let Ok(slot) = usize::try_from(slot) {
            if slot < written.len() {
                written[slot] = true;
            }
        }
    };
    for node in &nodes {
        mark(node.flag_slot, &mut written);
        mark(node.failed_flag_slot, &mut written);
        mark(node.seen_slot, &mut written);
        mark(node.once_slot, &mut written);
        for action in &node.actions {
            // Only these two carry a slot; money, clock and unmodelled actions do not.
            if matches!(
                action.kind(),
                DialogueActionKind::Assign | DialogueActionKind::Increment
            ) {
                mark(action.slot(), &mut written);
            }
        }
    }

    let keep: Vec<bool> = (0..symbols.count())
        .map(|slot| match symbols.name_of(slot) {
            Some(name) => {
                name.starts_with(SEEN_PREFIX)
                    || name.starts_with(ONCE_PREFIX)
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
            node.actions = node
                .actions
                .into_iter()
                .filter_map(|action| action.renumbered(&map))
                .collect();
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
            let destination_conversation =
                entry.to_conversation.get(i).copied().unwrap_or(conversation_id);
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
    if fields.contains_key(PASSIVE_FIELD) { return DialogueCheckKind::Passive; }
    if fields.contains_key(RED_FIELD) { return DialogueCheckKind::Red; }
    if fields.contains_key(WHITE_FIELD) { return DialogueCheckKind::White; }
    if fields.contains_key(FAKE_FIELD) { return DialogueCheckKind::Fake; }
    if fields.contains_key(TEST_FIELD) { return DialogueCheckKind::Test; }
    if fields.contains_key(KIM_WATCH_FIELD) { return DialogueCheckKind::KimSwitch; }
    DialogueCheckKind::None
}

/// A boolean field, read case-insensitively as the C# `bool.TryParse` does.
fn read_boolean(fields: &HashMap<String, String>, name: &str) -> bool {
    fields.get(name).is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

/// An entry's cost, whether it is charged once, and whether poverty hides it.
pub fn parse_cost(fields: &HashMap<String, String>) -> (i32, bool, bool) {
    let cost = fields.get(CLICK_COST_FIELD).and_then(|v| v.parse().ok()).unwrap_or(0);
    (
        cost,
        read_boolean(fields, COST_ONCE_FIELD),
        read_boolean(fields, HIDDEN_NOT_ENOUGH_FIELD),
    )
}

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
            let failed = symbols.variable(&format!("{}_failed", flag));
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
        ConversationRecord { id, hash: String::new(), entries }
    }

    fn index_of(conversations: Vec<ConversationRecord>) -> Index {
        conversations.into_iter().map(|c| (c.id, c)).collect()
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
        let path = written(concat!(
            "{\"format\":1}\n",
            "{\"id\":7,\"hash\":\"abc\",\"entries\":[]}\n",
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
        let path = written(concat!(
            "{\"format\":99}\n",
            "{\"id\":7,\"entries\":[]}\n",
        ));

        let refused = read_index_with_header(path.path()).expect_err("it must be refused");
        assert!(refused.to_string().contains("version 99"), "{refused}");
    }

    /// A header is told apart by its properties, not by where it is.
    ///
    /// Getting this wrong is silent: a header read as a conversation is a record with id 0
    /// and no entries, which looks like a real, empty conversation.
    #[test]
    fn a_header_is_never_mistaken_for_a_conversation() {
        let path = written("{\"format\":1}\n{\"id\":7,\"entries\":[]}\n");
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
