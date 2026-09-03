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

use crate::core::guard::GuardExpression;
use crate::core::state::StateSymbols;
use crate::core::types::{DialogueCheckKind, DialogueNodeId};
use crate::graph::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::parser::action_parser::parse_actions;
use crate::parser::guard_parser::parse_guard;

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

/// One conversation, as one line of the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub id: i32,
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

/// Reads `conversation_index.jsonl`.
pub fn read_index(path: &Path) -> anyhow::Result<Index> {
    let mut index = Index::new();
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let conversation: ConversationRecord = serde_json::from_str(&line)?;
        index.insert(conversation.id, conversation);
    }

    Ok(index)
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
    fn add(&mut self, record: &VariableRecord) {
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

    Ok((LookAheadGraph::new(nodes, symbols)?, group))
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
        ConversationRecord { id, entries }
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
}
