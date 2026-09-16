// SPDX-License-Identifier: MIT
//! Between the generated wire types and the engine's own.
//!
//! ## Why there are two sets of types at all
//!
//! [`crate::wire`] is generated from `proto/engine.proto` and is shaped by what encodes
//! well: an entry set is a list of runs, an unanswered question is an unset field, a
//! message field is an `Option` whether or not the thing is optional. [`crate::bridge`] is
//! shaped by what the engine needs to ask: an entry set is a `HashSet` you can test
//! membership in, an unanswered question is a value that answers "unknown" to everything.
//!
//! Collapsing them would mean the engine indexing into run lists, or the schema carrying
//! whatever a `HashSet` happens to serialise as. This module is the seam, and it is the
//! only place either shape has to know about the other.
//!
//! ## Absence is not an error here
//!
//! A protobuf message with a field missing is a valid message - that is the format's whole
//! compatibility story - so `None` arrives constantly and legitimately: a request with no
//! seen set, a snapshot with no checks. Every conversion below reads absence as the empty
//! or default value rather than refusing it, which is what the JSON wire's `#[serde(default)]`
//! did for the same fields.
//!
//! THE ONE EXCEPTION IS A RUN THAT RUNS BACKWARDS, which is not absence but nonsense, and
//! is refused. A set that is silently half-read is a world quietly answering "not seen" for
//! entries the player has read, with the marker then wrong and nothing to say so.

use std::collections::HashMap;

use crate::bridge::{
    DataAnswer, DataKind, FAIL, LookAheadAnswer, LookAheadRequest, LookAheadResponse, NodeRef,
    NodeSet, PASS, Questions, WireValue, WorldSnapshot,
};
use crate::wire;

/// What a wire message could not be read as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireError(String);

impl std::fmt::Display for WireError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(&self.0)
    }
}

impl std::error::Error for WireError {}

// ---------------------------------------------------------------------------
// Reading what arrived
// ---------------------------------------------------------------------------

impl From<wire::NodeRef> for NodeRef {
    fn from(node: wire::NodeRef) -> Self {
        Self {
            conversation: node.conversation,
            entry: node.entry,
        }
    }
}

impl From<NodeRef> for wire::NodeRef {
    fn from(node: NodeRef) -> Self {
        Self {
            conversation: node.conversation,
            entry: node.entry,
        }
    }
}

/// Expands the runs of a set, or says which one made no sense.
fn read_node_set(set: Option<wire::NodeSet>) -> Result<NodeSet, WireError> {
    let mut nodes = NodeSet::default();
    let Some(set) = set else {
        return Ok(nodes);
    };

    for conversation in set.conversations {
        for run in conversation.runs {
            if run.last < run.first {
                return Err(WireError(format!(
                    "conversation {}: the run {}-{} runs backwards",
                    conversation.conversation, run.first, run.last
                )));
            }

            for entry in run.first..=run.last {
                nodes.insert(NodeRef {
                    conversation: conversation.conversation,
                    entry,
                });
            }
        }
    }

    Ok(nodes)
}

impl From<wire::WireValue> for WireValue {
    /// An unset value is the unknowable one, which is what silence has to mean.
    fn from(value: wire::WireValue) -> Self {
        match value.value {
            Some(wire::wire_value::Value::Boolean(value)) => WireValue::Bool { value },
            Some(wire::wire_value::Value::Number(value)) => WireValue::Number { value },
            Some(wire::wire_value::Value::Text(value)) => WireValue::Text { value },
            None => WireValue::Unknown,
        }
    }
}

impl From<WireValue> for wire::WireValue {
    fn from(value: WireValue) -> Self {
        Self {
            value: match value {
                WireValue::Bool { value } => Some(wire::wire_value::Value::Boolean(value)),
                WireValue::Number { value } => Some(wire::wire_value::Value::Number(value)),
                WireValue::Text { value } => Some(wire::wire_value::Value::Text(value)),
                WireValue::Unknown => None,
            },
        }
    }
}

fn read_values(values: Vec<wire::WireValue>) -> Vec<WireValue> {
    values.into_iter().map(WireValue::from).collect()
}

fn read_named(values: HashMap<String, wire::WireValue>) -> HashMap<String, WireValue> {
    values
        .into_iter()
        .map(|(name, value)| (name, WireValue::from(value)))
        .collect()
}

/// Reads a snapshot, or says what about it could not be read.
pub fn read_snapshot(snapshot: Option<wire::WorldSnapshot>) -> Result<WorldSnapshot, WireError> {
    let Some(snapshot) = snapshot else {
        return Ok(WorldSnapshot::default());
    };

    Ok(WorldSnapshot {
        money: snapshot.money,
        day_minutes: snapshot.day_minutes,
        day_counter: snapshot.day_counter,
        clock_locked: snapshot.clock_locked,
        variables: {
            let mut variables = read_named(snapshot.variables);
            crate::bridge::lock_failed_white_checks(&mut variables, snapshot.failed_white_checks);
            variables
        },
        variable_values: read_values(snapshot.variable_values),
        queries: read_named(snapshot.queries),
        query_values: read_values(snapshot.query_values),
        items: snapshot.items.into_iter().collect(),
        tasks: snapshot.tasks.into_iter().collect(),
        thoughts: snapshot.thoughts.into_iter().collect(),
        checks_pass: read_node_set(snapshot.checks_pass)?,
        checks_fail: read_node_set(snapshot.checks_fail)?,
        seen: read_node_set(snapshot.seen)?,
        red_checks_fail: snapshot.red_checks_fail,
        // NAMED IS LEFT EMPTY and the positional list carries everything, because a wire
        // caller answers the engine's own request list in order. `WorldSnapshot::resolve`
        // moves these onto their requests and refuses a list of the wrong length.
        data: HashMap::new(),
        data_values: read_data(snapshot.data_values),
    })
}

/// The wire's spelling of a data kind.
///
/// Written out rather than derived, because the two enums are generated by different tools
/// from the same schema and nothing would catch them drifting apart: a match here fails to
/// compile the moment either side gains a value.
fn written_kind(kind: DataKind) -> wire::DataKind {
    match kind {
        DataKind::ThoughtsCooking => wire::DataKind::ThoughtsCooking,
        DataKind::ThoughtsFixed => wire::DataKind::ThoughtsFixed,
        DataKind::EquippedInSlot => wire::DataKind::EquippedInSlot,
        DataKind::TabHoldsItems => wire::DataKind::TabHoldsItems,
        DataKind::ItemsInGroup => wire::DataKind::ItemsInGroup,
        DataKind::HeldItemsInGroup => wire::DataKind::HeldItemsInGroup,
        DataKind::SceneIsOutside => wire::DataKind::SceneIsOutside,
        DataKind::SkillDamage => wire::DataKind::SkillDamage,
        DataKind::GameMode => wire::DataKind::GameMode,
        DataKind::HardcorePlaythroughCompleted => wire::DataKind::HardcorePlaythroughCompleted,
    }
}

/// Reads the answers to what the engine asked to have read.
fn read_data(answers: Vec<wire::DataAnswer>) -> Vec<DataAnswer> {
    answers
        .into_iter()
        .map(|answer| DataAnswer {
            value: answer
                .value
                .map(WireValue::from)
                .unwrap_or(WireValue::Unknown),
            names: answer.names,
            read: answer.read,
        })
        .collect()
}

/// Reads a look-ahead request, or says what about it could not be read.
///
/// The budgets are `u64` on the wire and two of them are `usize` here. On the platforms
/// this runs on those are the same width, and a value that did not fit would be a budget
/// nobody could mean, so it saturates rather than refusing: the request still gets an
/// answer, bounded by more memory or more states than the machine has.
pub fn read_look_ahead(request: wire::LookAheadRequest) -> Result<LookAheadRequest, WireError> {
    Ok(LookAheadRequest {
        conversation: request.conversation,
        starts: request.starts.into_iter().map(NodeRef::from).collect(),
        unseen_any_game: read_node_set(request.unseen_any_game)?,
        unseen_this_game: read_node_set(request.unseen_this_game)?,
        state_budget: usize::try_from(request.state_budget).unwrap_or(usize::MAX),
        time_budget_ms: request.time_budget_ms,
        menu_time_budget_ms: request.menu_time_budget_ms,
        memory_budget_mb: usize::try_from(request.memory_budget_mb).unwrap_or(usize::MAX),
        encountered: request.encountered.into_iter().map(NodeRef::from).collect(),
        world: read_snapshot(request.world)?,
    })
}

// ---------------------------------------------------------------------------
// Writing what goes back
// ---------------------------------------------------------------------------

/// Collapses a set into runs, the way it crosses.
///
/// Sorted by conversation and then by entry, so one set has one encoding. It has to: the
/// in-game suites and the measurements compare answers across runs, and a set whose bytes
/// depended on a hash order would differ from itself.
pub fn write_node_set(set: &NodeSet) -> wire::NodeSet {
    let mut by_conversation: std::collections::BTreeMap<i32, Vec<i32>> =
        std::collections::BTreeMap::new();
    for node in set.iter() {
        by_conversation
            .entry(node.conversation)
            .or_default()
            .push(node.entry);
    }

    let conversations = by_conversation
        .into_iter()
        .map(|(conversation, mut entries)| {
            entries.sort_unstable();
            wire::ConversationRuns {
                conversation,
                runs: collapse(&entries),
            }
        })
        .collect();

    wire::NodeSet { conversations }
}

/// Consecutive ids, as runs. The entries come from a set, so they strictly ascend.
fn collapse(entries: &[i32]) -> Vec<wire::NodeRun> {
    let mut runs = Vec::new();
    let mut index = 0;
    while index < entries.len() {
        let first = entries[index];
        let mut last = first;
        while index + 1 < entries.len() && entries[index + 1] == last + 1 {
            index += 1;
            last = entries[index];
        }

        runs.push(wire::NodeRun { first, last });
        index += 1;
    }

    runs
}

/// The novelty a rung number means.
///
/// An unknown number is the SEEN rung rather than a refusal. This is the engine's own
/// answer on its way out, so a number outside the three would be a bug here and not a
/// message from anywhere; answering with the least surprising rung keeps the menu drawn.
fn novelty_of(best: i32) -> wire::Novelty {
    match best {
        1 => wire::Novelty::UnseenThisGame,
        2 => wire::Novelty::UnseenAnyGame,
        _ => wire::Novelty::Seen,
    }
}

/// Which outcome of a rolled check an answer is about.
fn branch_of(branch: Option<&str>) -> wire::Branch {
    match branch {
        Some(PASS) => wire::Branch::Pass,
        Some(FAIL) => wire::Branch::Fail,
        // An ordinary option, which carries no branch at all.
        _ => wire::Branch::None,
    }
}

/// What stopped a search.
fn stopped_by_of(stopped_by: &str) -> wire::StoppedBy {
    match stopped_by {
        "states" => wire::StoppedBy::States,
        "time" => wire::StoppedBy::Time,
        _ => wire::StoppedBy::None,
    }
}

fn write_answer(answer: LookAheadAnswer) -> wire::LookAheadAnswer {
    wire::LookAheadAnswer {
        start: Some(answer.start.into()),
        branch: branch_of(answer.branch.as_deref()) as i32,
        destination: novelty_of(answer.destination) as i32,
        best: novelty_of(answer.best) as i32,
        witness: answer.witness.map(wire::NodeRef::from),
        complete: answer.complete,
        elapsed_ms: answer.elapsed_ms,
        states_explored: answer.states_explored as u64,
        nodes_reached: answer.nodes_reached as u64,
        stopped_by: stopped_by_of(&answer.stopped_by) as i32,
    }
}

/// Writes a look-ahead response the way it crosses.
pub fn write_look_ahead(response: LookAheadResponse) -> wire::LookAheadResponse {
    wire::LookAheadResponse {
        answers: response.answers.into_iter().map(write_answer).collect(),
        error: response.error,
    }
}

/// Writes the questions the way they cross.
pub fn write_questions(questions: Questions) -> wire::Questions {
    wire::Questions {
        conversations: questions.conversations,
        variables: questions.variables,
        queries: questions.queries,
        items: questions.items,
        tasks: questions.tasks,
        thoughts: questions.thoughts,
        checks: questions.checks.into_iter().map(Into::into).collect(),
        entries: questions.entries.into_iter().map(Into::into).collect(),
        data: questions
            .data
            .into_iter()
            .map(|request| wire::DataRequest {
                kind: written_kind(request.kind) as i32,
                subject: request.subject,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(conversation: i32, entry: i32) -> NodeRef {
        NodeRef {
            conversation,
            entry,
        }
    }

    /// A set is the same set after crossing, however its runs were spelled.
    #[test]
    fn an_entry_set_survives_being_collapsed_and_expanded() {
        let mut set = NodeSet::default();
        for entry in (0..=40).chain([42]).chain(50..=99) {
            set.insert(node(631, entry));
        }
        set.insert(node(636, 3));

        let crossed = write_node_set(&set);
        assert_eq!(read_node_set(Some(crossed)).expect("it reads"), set);
    }

    /// One set has one encoding, so answers can be compared across runs.
    #[test]
    fn a_set_collapses_the_same_way_every_time() {
        let build = || {
            let mut set = NodeSet::default();
            for entry in [7, 1, 2, 3, 99, 50] {
                set.insert(node(9, entry));
            }
            set.insert(node(4, 1));
            write_node_set(&set)
        };

        assert_eq!(build(), build());
        let runs = &build().conversations[0];
        assert_eq!(runs.conversation, 4, "conversations come back sorted");
        assert_eq!(
            build().conversations[1].runs,
            vec![
                wire::NodeRun { first: 1, last: 3 },
                wire::NodeRun { first: 7, last: 7 },
                wire::NodeRun {
                    first: 50,
                    last: 50
                },
                wire::NodeRun {
                    first: 99,
                    last: 99
                },
            ],
        );
    }

    /// Absence is how protobuf spells "nothing here", and it arrives constantly.
    #[test]
    fn a_message_with_nothing_in_it_reads_as_empty_rather_than_failing() {
        assert!(read_node_set(None).expect("it reads").is_empty());

        let snapshot = read_snapshot(None).expect("it reads");
        assert_eq!(snapshot.money, 0);
        assert!(snapshot.seen.is_empty());

        let request = read_look_ahead(wire::LookAheadRequest::default()).expect("it reads");
        assert!(request.starts.is_empty());
        assert!(request.world.seen.is_empty());
    }

    /// A run that runs backwards is nonsense rather than absence, and is refused.
    #[test]
    fn a_backwards_run_is_refused_rather_than_read_as_empty() {
        let set = wire::NodeSet {
            conversations: vec![wire::ConversationRuns {
                conversation: 9,
                runs: vec![wire::NodeRun { first: 50, last: 1 }],
            }],
        };

        let refused = read_node_set(Some(set)).expect_err("it is refused");
        assert!(refused.to_string().contains("runs backwards"), "{refused}");
    }

    #[test]
    fn an_unset_value_is_the_unknowable_one() {
        assert!(matches!(
            WireValue::from(wire::WireValue::default()),
            WireValue::Unknown
        ));
        assert_eq!(
            wire::WireValue::from(WireValue::Unknown),
            wire::WireValue::default()
        );
    }

    #[test]
    fn a_value_survives_crossing_in_either_direction() {
        for value in [
            WireValue::Bool { value: true },
            WireValue::Number { value: 2.5 },
            WireValue::Text {
                value: "raining".to_string(),
            },
            WireValue::Unknown,
        ] {
            let crossed = wire::WireValue::from(value.clone());
            let back = WireValue::from(crossed);
            assert_eq!(format!("{back:?}"), format!("{value:?}"));
        }
    }

    /// An ordinary option carries no branch, which is how the mod knows not to draw the line.
    #[test]
    fn only_a_rolled_check_names_an_outcome() {
        assert_eq!(branch_of(None), wire::Branch::None);
        assert_eq!(branch_of(Some(PASS)), wire::Branch::Pass);
        assert_eq!(branch_of(Some(FAIL)), wire::Branch::Fail);
    }

    #[test]
    fn what_stopped_a_search_crosses_as_itself() {
        assert_eq!(stopped_by_of("none"), wire::StoppedBy::None);
        assert_eq!(stopped_by_of("states"), wire::StoppedBy::States);
        assert_eq!(stopped_by_of("time"), wire::StoppedBy::Time);
    }

    #[test]
    fn the_three_rungs_cross_as_themselves() {
        assert_eq!(novelty_of(0), wire::Novelty::Seen);
        assert_eq!(novelty_of(1), wire::Novelty::UnseenThisGame);
        assert_eq!(novelty_of(2), wire::Novelty::UnseenAnyGame);
    }

    /// The budgets are wider on the wire than in the engine on a 32-bit build.
    #[test]
    fn a_budget_too_large_to_hold_saturates_rather_than_refusing() {
        let request = read_look_ahead(wire::LookAheadRequest {
            memory_budget_mb: u64::MAX,
            state_budget: u64::MAX,
            ..Default::default()
        })
        .expect("it reads");

        assert_eq!(request.memory_budget_mb, usize::MAX);
        assert_eq!(request.state_budget, usize::MAX);
    }
}
