// SPDX-License-Identifier: MIT
//! The journal: every task and subtask, by the dialogue variables that stand for it.
//!
//! ## The game's definition
//!
//! From the pre-final-cut export (Assets/Scripts/Assembly-CSharp), `JournalImporter.Populate`:
//! a conversation is a task when it has `display_condition_main`, and each of its subtasks
//! `NN`, for `NN` from 01 to 12, exists while `done_subtask_NN` is assigned and non-empty.
//!
//! ```text
//! string showCondition = Field.LookupValue(conversation.fields, "display_condition_main");
//! string doneCondition = Field.LookupValue(conversation.fields, "done_condition_main");
//! string cancelCondition = Field.LookupValue(conversation.fields, "cancel_condition_main");
//! ...
//! for (int i = 1; i <= 12; i++)
//! {
//!     if (!Field.IsFieldAssigned(conversation.fields, "done_subtask_" + text2)) break;
//!     ...
//!     if (!string.IsNullOrEmpty(text4)) journalTask.AddSubtask(..., showCondition2, text4, cancelCondition2, ...);
//! }
//! ```
//!
//! Each condition is a Lua expression naming one variable, which `Completeable` reduces with
//! `ArticyBridge.GetVariableFromLuaExpression`:
//!
//! ```text
//! int num = variable.IndexOf('[');
//! int num2 = variable.IndexOf(']');
//! if (num > 0 && num2 > num) return variable.Substring(num + 2, num2 - num - 3);
//! return variable;
//! ```
//!
//! `JournalModel.GetByConditionVariable` finds a part by ANY of its three variables, which is
//! why a script may finish a task by naming its done variable.

use std::collections::HashMap;

use super::{Index, JOURNAL_SUBTASK_LIMIT};
use crate::core::guard::{Guard, GuardExpression, GuardRef};
use crate::core::guard_value::{GuardValue, GuardValueKind};

/// The three conditions a part has, in the order the index fields name them.
pub const JOURNAL_ROLES: [&str; 3] = ["display", "done", "cancel"];

/// The field that makes a conversation a task.
const TASK_FIELD: &str = "display_condition_main";

/// One task or subtask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalPart {
    /// The variable set when it is revealed.
    pub show: String,
    /// The variable set when it is done.
    pub done: String,
    /// The variable set when it is cancelled, where it has one.
    pub cancel: Option<String>,
    /// The task a subtask belongs to, as an index into [`Journal::parts`]; `None` for a task.
    pub parent: Option<usize>,
}

/// Every part, and which part each condition variable names.
#[derive(Debug, Clone, Default)]
pub struct Journal {
    parts: Vec<JournalPart>,
    by_variable: HashMap<String, usize>,
}

impl Journal {
    /// The journal the index's task conversations describe.
    pub fn from_index(index: &Index) -> Self {
        let mut journal = Self::default();
        let mut ids: Vec<i32> = index
            .values()
            .filter(|record| record.fields.contains_key(TASK_FIELD))
            .map(|record| record.id)
            .collect();
        ids.sort_unstable();

        for id in ids {
            let fields = &index[&id].fields;
            let condition = |name: String| {
                fields
                    .get(&name)
                    .map(|value| variable_of(value))
                    .filter(|variable| !variable.is_empty())
            };

            let Some(task) = journal.add(
                condition("display_condition_main".to_string()),
                condition("done_condition_main".to_string()),
                condition("cancel_condition_main".to_string()),
                None,
            ) else {
                continue;
            };

            for subtask in 1..=JOURNAL_SUBTASK_LIMIT {
                if !fields.contains_key(&format!("done_subtask_{subtask:02}")) {
                    break;
                }
                journal.add(
                    condition(format!("display_subtask_{subtask:02}")),
                    condition(format!("done_subtask_{subtask:02}")),
                    condition(format!("cancel_subtask_{subtask:02}")),
                    Some(task),
                );
            }
        }

        journal
    }

    /// Adds a part, where it has the show and done variables every part needs.
    fn add(
        &mut self,
        show: Option<String>,
        done: Option<String>,
        cancel: Option<String>,
        parent: Option<usize>,
    ) -> Option<usize> {
        let (show, done) = (show?, done?);
        let at = self.parts.len();
        for variable in [Some(&show), Some(&done), cancel.as_ref()]
            .into_iter()
            .flatten()
        {
            self.by_variable.entry(variable.clone()).or_insert(at);
        }
        self.parts.push(JournalPart {
            show,
            done,
            cancel,
            parent,
        });
        Some(at)
    }

    /// Every part.
    pub fn parts(&self) -> &[JournalPart] {
        &self.parts
    }

    /// The part a condition variable names, as its index and itself.
    pub fn part_named(&self, variable: &str) -> Option<(usize, &JournalPart)> {
        let at = *self.by_variable.get(variable)?;
        Some((at, &self.parts[at]))
    }

    /// `guard` with every `IsTaskActive("name")` replaced by what it means over variables.
    ///
    /// `JournalModel.IsTaskActive`, from the pre-final-cut export:
    ///
    /// ```text
    /// if (byConditionVariable == null) { Debug.LogErrorFormat(...); return false; }
    /// if (byConditionVariable is JournalTask)
    /// {
    ///     if (GainedTasks.Contains(task) && !byConditionVariable.IsCanceled) return !byConditionVariable.IsDone;
    ///     return false;
    /// }
    /// JournalTask parent = ((JournalSubtask)byConditionVariable).parent;
    /// if (parent.GainedSubtasks.Contains(subtask) && !IsCanceled && !IsDone && !parent.IsCanceled)
    ///     return !parent.IsDone;
    /// return false;
    /// ```
    ///
    /// A part is gained when it is revealed, and revealing sets its show variable; done and
    /// cancelled set theirs, and `JournalWatchman.CheckForChanges` makes all three agree with
    /// the variables on every load. So the question is `show and not done and not cancel`, and
    /// for a subtask its parent neither done nor cancelled.
    ///
    /// REWRITTEN RATHER THAN ANSWERED, so every place a guard is read - the graph's declared
    /// variables, the request, the search's own state, the compiler - reads a task the way it
    /// reads any variable, and a journal action is an ordinary write to one.
    ///
    /// A literal naming no part is `false`, as the game answers. A computed argument names
    /// nothing that can be looked up, and stays a call.
    pub fn with_tasks_as_variables(&self, guard: &Guard) -> Guard {
        if !guard.nodes().any(|node| {
            matches!(node.expression(), GuardExpression::Call(name, _) if name == IS_TASK_ACTIVE)
        }) {
            return guard.clone();
        }
        self.rewritten(guard.as_ref())
    }

    fn rewritten(&self, node: GuardRef<'_>) -> Guard {
        match node.expression() {
            GuardExpression::Literal(value) => Guard::literal(value.clone()),
            GuardExpression::Variable(name) => Guard::variable(name),
            GuardExpression::Not(inner) => Guard::not(self.rewritten(inner)),
            GuardExpression::And(left, right) => {
                Guard::and(self.rewritten(left), self.rewritten(right))
            }
            GuardExpression::Or(left, right) => {
                Guard::or(self.rewritten(left), self.rewritten(right))
            }
            GuardExpression::Comparison(op, left, right) => {
                Guard::comparison(op, self.rewritten(left), self.rewritten(right))
            }
            GuardExpression::Call(name, arguments) => {
                let subject = arguments.only().and_then(|only| match only.expression() {
                    GuardExpression::Literal(value) if value.kind() == GuardValueKind::Text => {
                        Some(value.text().to_string())
                    }
                    _ => None,
                });
                match (name == IS_TASK_ACTIVE, subject) {
                    (true, Some(subject)) => self.active(&subject),
                    _ => Guard::call(name, arguments.iter().map(|a| self.rewritten(a)).collect()),
                }
            }
        }
    }

    /// Whether the part `variable` names is active, over its variables.
    fn active(&self, variable: &str) -> Guard {
        let Some((_, part)) = self.part_named(variable) else {
            return Guard::literal(GuardValue::from_boolean(false));
        };
        let mut holds = Guard::and(Guard::variable(&part.show), open(part));
        if let Some(parent) = part.parent {
            holds = Guard::and(holds, open(&self.parts[parent]));
        }
        holds
    }
}

/// The call the journal answers.
pub const IS_TASK_ACTIVE: &str = "IsTaskActive";

/// `not done and not cancel`, for a part.
fn open(part: &JournalPart) -> Guard {
    let not_done = Guard::not(Guard::variable(&part.done));
    match &part.cancel {
        Some(cancel) => Guard::and(not_done, Guard::not(Guard::variable(cancel))),
        None => not_done,
    }
}

/// `ArticyBridge.GetVariableFromLuaExpression`: the name inside `Variable["name"]`, or the
/// expression itself where it has no brackets.
fn variable_of(expression: &str) -> String {
    match (expression.find('['), expression.find(']')) {
        (Some(open), Some(close)) if open > 0 && close > open + 2 => {
            expression[open + 2..close - 1].to_string()
        }
        _ => expression.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::ConversationRecord;

    fn task(id: i32, fields: &[(&str, &str)]) -> ConversationRecord {
        ConversationRecord {
            id,
            hash: String::new(),
            fields: fields
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            entries: Vec::new(),
        }
    }

    #[test]
    fn a_condition_names_its_variable() {
        assert_eq!(variable_of(r#"Variable["TASK.x_done"]"#), "TASK.x_done");
        assert_eq!(variable_of("TASK.x"), "TASK.x");
    }

    /// Any of a part's variables finds it, and a subtask knows its task.
    #[test]
    fn a_part_is_found_by_any_of_its_variables() {
        let mut index = Index::new();
        index.insert(
            7,
            task(
                7,
                &[
                    ("display_condition_main", r#"Variable["TASK.wall"]"#),
                    ("done_condition_main", r#"Variable["TASK.wall_done"]"#),
                    (
                        "cancel_condition_main",
                        r#"Variable["TASK.wall_cancelled"]"#,
                    ),
                    ("display_subtask_01", r#"Variable["TASK.oil"]"#),
                    ("done_subtask_01", r#"Variable["TASK.oil_done"]"#),
                    ("cancel_subtask_01", ""),
                ],
            ),
        );
        let journal = Journal::from_index(&index);

        let (wall, found) = journal.part_named("TASK.wall_done").expect("the task");
        assert_eq!(found.show, "TASK.wall");
        assert_eq!(journal.part_named("TASK.wall_cancelled").unwrap().0, wall);

        let (_, oil) = journal.part_named("TASK.oil").expect("the subtask");
        assert_eq!(oil.parent, Some(wall));
        assert_eq!(oil.cancel, None, "an empty condition is no variable");
        assert!(journal.part_named("TASK.nothing").is_none());
    }

    /// `IsTaskActive` reads as its part's variables, a subtask's parent included, and a name
    /// the journal does not know reads false.
    #[test]
    fn a_task_question_is_rewritten_over_its_variables() {
        let mut index = Index::new();
        index.insert(
            7,
            task(
                7,
                &[
                    ("display_condition_main", r#"Variable["TASK.wall"]"#),
                    ("done_condition_main", r#"Variable["TASK.wall_done"]"#),
                    (
                        "cancel_condition_main",
                        r#"Variable["TASK.wall_cancelled"]"#,
                    ),
                    ("display_subtask_01", r#"Variable["TASK.oil"]"#),
                    ("done_subtask_01", r#"Variable["TASK.oil_done"]"#),
                ],
            ),
        );
        let journal = Journal::from_index(&index);
        let asked = |name: &str| {
            let guard = Guard::not(Guard::call(
                IS_TASK_ACTIVE,
                vec![Guard::literal(GuardValue::from_text(name.to_string()))],
            ));
            journal.with_tasks_as_variables(&guard).to_string()
        };

        let task = asked("TASK.wall_done");
        assert!(!task.contains(IS_TASK_ACTIVE), "{task}");
        for variable in ["TASK.wall", "TASK.wall_done", "TASK.wall_cancelled"] {
            assert!(task.contains(variable), "{task} does not read {variable}");
        }
        let subtask = asked("TASK.oil");
        for variable in [
            "TASK.oil",
            "TASK.oil_done",
            "TASK.wall_done",
            "TASK.wall_cancelled",
        ] {
            assert!(
                subtask.contains(variable),
                "{subtask} does not read {variable}"
            );
        }
        assert!(asked("TASK.nothing").contains("false"));
    }

    /// Subtasks stop at the first number with no done condition, as the importer's loop does.
    #[test]
    fn subtasks_stop_at_the_first_gap() {
        let mut index = Index::new();
        index.insert(
            1,
            task(
                1,
                &[
                    ("display_condition_main", r#"Variable["TASK.a"]"#),
                    ("done_condition_main", r#"Variable["TASK.a_done"]"#),
                    ("display_subtask_02", r#"Variable["TASK.b"]"#),
                    ("done_subtask_02", r#"Variable["TASK.b_done"]"#),
                ],
            ),
        );
        assert_eq!(Journal::from_index(&index).parts().len(), 1);
    }
}
