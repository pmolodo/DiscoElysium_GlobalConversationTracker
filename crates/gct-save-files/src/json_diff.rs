// SPDX-License-Identifier: MIT
//! A recursive overlay for general JSON documents: what one document changes in another.
//!
//! THE MERGE RULE IS THE WHOLE FORMAT. Two objects merge key by key, and anything else -
//! a number, a string, an array, a null - REPLACES what it lands on. A diff therefore
//! carries only the leaves it changes, and what counts as a leaf is decided by the document
//! rather than by this code: a conversation's entries are one run-encoded string, so
//! changing any of them restates that conversation's run; a list of orbs is one array, so
//! adding one restates the list. What a diff never restates is everything it did not touch,
//! which is the whole of the saving.
//!
//! REMOVAL IS SEPARATE, because a merge cannot express it: a key that is absent from the
//! changes means "unchanged", which is the common case and the one worth spelling cheaply.
//! What a diff removes is named in `_remove`, as JSON Pointer paths - `/conversations/29` -
//! which is also why the escaping below exists rather than a plain join.

use serde_json::{Map, Value};

use super::header::{BASE_KEY, Expected};

/// What this format is called, and the version of it this build writes.
pub const FORMAT: Expected = Expected {
    format: "json-diff",
    version: 1,
};

/// Where a diff's changes live.
pub const CHANGES_KEY: &str = "_changes";

/// Where a diff's removals live.
pub const REMOVE_KEY: &str = "_remove";

/// Why a diff could not be applied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DiffFault {
    /// It is not a diff of the current version, or not a diff at all.
    #[error("{0}")]
    Header(#[from] super::header::HeaderFault),
    /// Its removals are not a list of paths.
    #[error("its {REMOVE_KEY} is not a list of paths")]
    Removals,
}

/// What a diff says it is a diff of, or nothing where it says nothing.
pub fn base_of(patch: &Value) -> Option<&str> {
    patch.get(BASE_KEY).and_then(Value::as_str)
}

/// Applies a diff to the document it is a diff of.
///
/// # Errors
///
/// Where the patch is not a current-version diff, or its removals are not paths.
pub fn apply(baseline: &Value, patch: &Value) -> Result<Value, DiffFault> {
    FORMAT.check_document(patch)?;

    let removed = removals(patch)?;
    // ABSENT AND PRESENT-BUT-EMPTY ARE THE SAME THING HERE, and both mean "changes
    // nothing": a diff states only what it does, so a diff that only removes carries no
    // changes at all rather than an empty half for a reader to look past.
    let changes = patch.get(CHANGES_KEY).cloned().unwrap_or_else(|| {
        if patch.get(CHANGES_KEY).is_some() {
            Value::Null
        } else {
            Value::Object(Map::new())
        }
    });

    Ok(merge(baseline, &changes, "", &removed))
}

/// The paths a diff removes.
fn removals(patch: &Value) -> Result<Vec<String>, DiffFault> {
    let Some(listed) = patch.get(REMOVE_KEY) else {
        return Ok(Vec::new());
    };

    let listed = listed.as_array().ok_or(DiffFault::Removals)?;
    listed
        .iter()
        .map(|path| path.as_str().map(str::to_string).ok_or(DiffFault::Removals))
        .collect()
}

/// One document overlaid on another, with the named paths dropped on the way.
///
/// A REMOVAL CAN SIT BENEATH A KEY THE CHANGES NEVER MENTION, so an unchanged object is
/// still walked where some removal path lies under it. Copying it whole instead would keep
/// exactly what the diff says to drop.
fn merge(baseline: &Value, changes: &Value, path: &str, removed: &[String]) -> Value {
    let (Some(old), Some(changed)) = (baseline.as_object(), changes.as_object()) else {
        return changes.clone();
    };

    let mut merged = Map::new();
    for (name, value) in old {
        let child = child_path(path, name);
        if removed.contains(&child) {
            continue;
        }

        let beneath = format!("{child}/");
        merged.insert(
            name.clone(),
            match changed.get(name) {
                Some(change) => merge(value, change, &child, removed),
                None if value.is_object() && removed.iter().any(|at| at.starts_with(&beneath)) => {
                    merge(value, &Value::Object(Map::new()), &child, removed)
                }
                None => value.clone(),
            },
        );
    }

    for (name, value) in changed {
        if !old.contains_key(name) {
            merged.insert(name.clone(), value.clone());
        }
    }

    Value::Object(merged)
}

/// The diff that turns one document into another, or nothing where they are already equal.
pub fn create(baseline: &Value, target: &Value) -> Option<Value> {
    let mut removed = Vec::new();
    let (changes, changed) = difference(baseline, target, "", &mut removed);

    if !changed && removed.is_empty() {
        return None;
    }

    let mut patch = FORMAT.stamp();
    if !removed.is_empty() {
        patch.insert(
            REMOVE_KEY.to_string(),
            Value::Array(removed.into_iter().map(Value::String).collect()),
        );
    }
    if changed {
        patch.insert(CHANGES_KEY.to_string(), changes);
    }

    Some(Value::Object(patch))
}

/// What one document changes in another, and whether it changes anything.
fn difference(
    baseline: &Value,
    target: &Value,
    path: &str,
    removed: &mut Vec<String>,
) -> (Value, bool) {
    let (Some(old), Some(new)) = (baseline.as_object(), target.as_object()) else {
        return (target.clone(), baseline != target);
    };

    let mut changes = Map::new();
    for name in old.keys() {
        if !new.contains_key(name) {
            removed.push(child_path(path, name));
        }
    }

    for (name, value) in new {
        let Some(was) = old.get(name) else {
            changes.insert(name.clone(), value.clone());
            continue;
        };

        let before = removed.len();
        let child = child_path(path, name);
        let (changed, differs) = difference(was, value, &child, removed);
        if differs || removed.len() > before {
            changes.insert(name.clone(), changed);
        }
    }

    let any = !changes.is_empty();
    (Value::Object(changes), any)
}

/// One step of a JSON Pointer, escaped as the pointer grammar wants.
fn child_path(parent: &str, name: &str) -> String {
    format!("{parent}/{}", name.replace('~', "~0").replace('/', "~1"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// What a document and its diff do together: the diff of two documents rebuilds one.
    fn round_trip(baseline: Value, target: Value) {
        let patch = create(&baseline, &target).expect("the two differ");

        assert_eq!(apply(&baseline, &patch).expect("it applies"), target);
    }

    #[test]
    fn a_changed_leaf_is_the_only_thing_a_diff_carries() {
        let baseline = json!({ "a": { "b": 1, "c": 2 }, "d": 3 });
        let target = json!({ "a": { "b": 9, "c": 2 }, "d": 3 });

        let patch = create(&baseline, &target).expect("the two differ");

        assert_eq!(patch[CHANGES_KEY], json!({ "a": { "b": 9 } }));
        round_trip(baseline, target);
    }

    #[test]
    fn an_array_is_a_leaf_and_is_restated_whole() {
        let baseline = json!({ "orbs": ["one", "two"] });
        let target = json!({ "orbs": ["one", "two", "three"] });

        let patch = create(&baseline, &target).expect("the two differ");

        assert_eq!(
            patch[CHANGES_KEY],
            json!({ "orbs": ["one", "two", "three"] })
        );
        round_trip(baseline, target);
    }

    #[test]
    fn a_key_that_is_gone_is_removed_by_path_rather_than_merged_away() {
        let baseline = json!({ "a": 1, "b": 2 });
        let target = json!({ "a": 1 });

        let patch = create(&baseline, &target).expect("the two differ");

        assert_eq!(patch[REMOVE_KEY], json!(["/b"]));
        round_trip(baseline, target);
    }

    /// A removal is honoured even beneath a key the changes say nothing about - the shape a
    /// hand-written diff takes when it changes one branch of an object and prunes another.
    #[test]
    fn a_removal_beneath_an_unchanged_key_is_still_removed() {
        let baseline = json!({
            "holder": { "changed": 1, "cache": { "gone": true, "kept": true } },
        });
        let patch = json!({
            "_format": "json-diff",
            "_formatVersion": 1,
            CHANGES_KEY: { "holder": { "changed": 2 } },
            REMOVE_KEY: ["/holder/cache/gone"],
        });

        let merged = apply(&baseline, &patch).expect("it applies");

        assert_eq!(
            merged,
            json!({ "holder": { "changed": 2, "cache": { "kept": true } } })
        );
    }

    #[test]
    fn a_key_with_a_slash_or_a_tilde_in_it_survives_the_pointer() {
        let baseline = json!({ "a/b": 1, "c~d": 2, "keep": 3 });
        let target = json!({ "keep": 3 });

        let patch = create(&baseline, &target).expect("the two differ");
        let paths = patch[REMOVE_KEY].as_array().expect("a list").clone();

        assert!(paths.contains(&json!("/a~1b")));
        assert!(paths.contains(&json!("/c~0d")));
        round_trip(baseline, target);
    }

    #[test]
    fn two_documents_that_are_the_same_have_no_diff_at_all() {
        assert_eq!(create(&json!({ "a": 1 }), &json!({ "a": 1 })), None);
    }

    #[test]
    fn a_diff_of_another_format_is_refused() {
        let patch = json!({ "_format": "sparse-diff", "_formatVersion": 1 });

        assert!(apply(&json!({}), &patch).is_err());
    }

    #[test]
    fn a_diff_with_no_header_is_refused_rather_than_applied() {
        let patch = json!({ CHANGES_KEY: { "a": 1 } });

        assert!(apply(&json!({}), &patch).is_err());
    }

    #[test]
    fn a_diff_that_only_removes_carries_no_changes() {
        let patch = create(&json!({ "a": 1, "b": 2 }), &json!({ "a": 1 })).expect("they differ");

        assert!(patch.get(CHANGES_KEY).is_none());
    }

    #[test]
    fn what_a_diff_does_not_mention_is_left_alone() {
        let baseline = json!({ "a": { "deep": { "kept": 1 } }, "b": 2 });
        let patch = json!({
            "_format": "json-diff",
            "_formatVersion": 1,
            CHANGES_KEY: { "b": 3 },
        });

        let merged = apply(&baseline, &patch).expect("it applies");

        assert_eq!(merged["a"], json!({ "deep": { "kept": 1 } }));
        assert_eq!(merged["b"], json!(3));
    }

    #[test]
    fn a_diff_names_what_it_is_a_diff_of_where_it_names_one() {
        let patch = json!({ "_base": "beneath.json" });

        assert_eq!(base_of(&patch), Some("beneath.json"));
        assert_eq!(base_of(&json!({})), None);
    }
}
