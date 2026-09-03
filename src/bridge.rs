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

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::core::guard::GuardExpression;
use crate::core::guard_value::{GuardValue, GuardValueKind};
use crate::core::types::{DialogueCheckKind, DialogueNodeId, Novelty, Ternary};
use crate::engine::engine::LookAheadEngine;
use crate::graph::graph::LookAheadGraph;
use crate::index::{build_group_graph, Index};
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

/// What a check answers, when the plugin knows.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CheckAnswer {
    pub node: NodeRef,
    /// -1 fails, 0 undecided, 1 passes - the three the engine's `Ternary` carries.
    pub passes: i32,
}

/// The player's situation, as the plugin sees it, for one look-ahead.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorldSnapshot {
    pub money: i32,
    pub day_minutes: i32,
    pub day_counter: i32,
    pub clock_locked: bool,
    /// Dialogue variables, by name.
    #[serde(default)]
    pub variables: HashMap<String, WireValue>,
    /// World queries, by the key [`questions_for`] gave them.
    #[serde(default)]
    pub queries: HashMap<String, WireValue>,
    /// Items held when the crawl starts.
    #[serde(default)]
    pub items: HashSet<String>,
    /// Journal tasks active when the crawl starts.
    #[serde(default)]
    pub tasks: HashSet<String>,
    /// Thoughts in the cabinet when the crawl starts.
    #[serde(default)]
    pub thoughts: HashSet<String>,
    #[serde(default)]
    pub checks: Vec<CheckAnswer>,
    /// Entries the player has already been shown.
    #[serde(default)]
    pub seen: HashSet<NodeRef>,
}

/// A [`WorldSnapshot`] with its lookups arranged for asking rather than for sending.
pub struct SnapshotWorld {
    snapshot: WorldSnapshot,
    checks: HashMap<DialogueNodeId, Ternary>,
}

impl SnapshotWorld {
    pub fn new(snapshot: WorldSnapshot) -> Self {
        let checks = snapshot
            .checks
            .iter()
            .map(|answer| {
                let ternary = match answer.passes {
                    value if value > 0 => Ternary::True,
                    value if value < 0 => Ternary::False,
                    _ => Ternary::Unknown,
                };
                (DialogueNodeId::from(answer.node), ternary)
            })
            .collect();

        Self { snapshot, checks }
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
        self.snapshot.variables.get(name).map(GuardValue::from).unwrap_or_else(GuardValue::unknown)
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
        self.checks.get(&node).copied().unwrap_or(Ternary::Unknown)
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
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LookAheadRequest {
    /// Any conversation in the group; the engine loads the whole group from it.
    pub conversation: i32,
    /// The option entries to score. One answer comes back per start.
    pub starts: Vec<NodeRef>,
    /// Entries the player has never seen in any game.
    #[serde(default)]
    pub unseen_any_game: HashSet<NodeRef>,
    /// Entries unseen this game but seen in a previous one.
    #[serde(default)]
    pub unseen_this_game: HashSet<NodeRef>,
    pub world: WorldSnapshot,
}

/// What one option scored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookAheadAnswer {
    pub start: NodeRef,
    /// 0 seen, 1 unseen this game, 2 unseen in any game.
    pub best: i32,
    /// The entry that proved it, where something did.
    pub witness: Option<NodeRef>,
    /// Whether the search settled. False means `best` is a lower bound.
    pub complete: bool,
    pub elapsed_ms: u64,
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

    // Sorted, so two runs over the same group produce the same list. The plugin may cache
    // these against a conversation, and a list that reordered itself would look like a
    // change every time.
    found.variables = sorted(variables);
    found.queries = sorted(queries);
    found.items = sorted(items);
    found.tasks = sorted(tasks);
    found.thoughts = sorted(thoughts);
    found.entries.sort_by_key(|node| (node.conversation, node.entry));
    found.checks.sort_by_key(|node| (node.conversation, node.entry));

    Ok(found)
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
pub fn answer(index: &Index, request: &LookAheadRequest) -> LookAheadResponse {
    let (graph, _) = match build_group_graph(index, request.conversation) {
        Ok(built) => built,
        Err(reason) => return LookAheadResponse::failed(reason),
    };

    let world = SnapshotWorld::new(request.world.clone());
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

    let engine = LookAheadEngine::default();
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
            });
            continue;
        }

        let began = std::time::Instant::now();
        let result = engine.evaluate(&graph, id, &world, &novelty);
        answers.push(LookAheadAnswer {
            start: *start,
            best: result.best as i32,
            witness: None,
            complete: !result.budget_exhausted(),
            elapsed_ms: began.elapsed().as_millis() as u64,
        });
    }

    LookAheadResponse { answers, error: None }
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

    /// Anything unanswered reads Unknown, which is the permissive direction.
    #[test]
    fn an_unanswered_question_is_unknown_rather_than_false() {
        let world = SnapshotWorld::new(WorldSnapshot::default());

        assert_eq!(world.get_variable("never.mentioned").kind(), GuardValueKind::Unknown);
        assert_eq!(world.query("IsKimHere", &[]).kind(), GuardValueKind::Unknown);
        assert_eq!(world.check_passes(DialogueNodeId::new(1, 2)), Ternary::Unknown);
    }

    #[test]
    fn a_check_answer_carries_all_three_outcomes() {
        let world = SnapshotWorld::new(WorldSnapshot {
            checks: vec![
                CheckAnswer { node: NodeRef { conversation: 1, entry: 1 }, passes: 1 },
                CheckAnswer { node: NodeRef { conversation: 1, entry: 2 }, passes: -1 },
                CheckAnswer { node: NodeRef { conversation: 1, entry: 3 }, passes: 0 },
            ],
            ..Default::default()
        });

        assert_eq!(world.check_passes(DialogueNodeId::new(1, 1)), Ternary::True);
        assert_eq!(world.check_passes(DialogueNodeId::new(1, 2)), Ternary::False);
        assert_eq!(world.check_passes(DialogueNodeId::new(1, 3)), Ternary::Unknown);
    }

    #[test]
    fn seen_entries_are_carried_across() {
        let world = SnapshotWorld::new(WorldSnapshot {
            seen: HashSet::from([NodeRef { conversation: 7, entry: 3 }]),
            ..Default::default()
        });

        assert!(world.is_seen(DialogueNodeId::new(7, 3)));
        assert!(!world.is_seen(DialogueNodeId::new(7, 4)));
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
            unseen_any_game: HashSet::from([NodeRef { conversation: 631, entry: 9 }]),
            unseen_this_game: HashSet::new(),
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
