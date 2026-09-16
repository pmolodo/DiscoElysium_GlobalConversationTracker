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
