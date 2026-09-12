// SPDX-License-Identifier: MIT
//! What one sparse tree changes in another.
//!
//! ## The merge rule, which is the whole format
//!
//! Two objects merge key by key, and anything else - a number, a string, a null -
//! REPLACES what it lands on. A diff therefore carries only the leaves it changes.
//!
//! REMOVAL IS SEPARATE, because a merge cannot express it: a key absent from the changes
//! means "unchanged", which is the common case and the one worth spelling cheaply. What a
//! diff removes is named in `_remove`, whose keys are JSON Pointer paths.
//!
//! ## Why `_remove` is an object here and a list in the JSON diff
//!
//! [`crate::json_diff`] carries its removals as a list of paths and this carries
//! them as an object whose values are all `true`. That is not a considered difference, it
//! is two formats written at different times - but it is a difference in FILES THAT EXIST,
//! so it is described rather than tidied. A set spelled as an object is what the sparse
//! form can hold anyway: it has no arrays.
//!
//! ## A path is a JSON Pointer
//!
//! `/conversations/29`, with `~` written `~0` and `/` written `~1`, so a key containing
//! either is still one path segment. The escaping is what makes a path unambiguous rather
//! than a plain join, and the game's keys do contain slashes.

use super::header::{self, Expected};
use super::sparse::{SparseMap, SparseValue};

/// What this format is called, and the version of it this build writes.
pub const FORMAT: Expected = Expected {
    format: "sparse-diff",
    version: 1,
};

/// Where a diff's changes live.
pub const CHANGES_KEY: &str = "_changes";

/// Where a diff's removals live.
pub const REMOVE_KEY: &str = "_remove";

/// Why a sparse diff could not be applied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SparseDiffFault {
    /// It is not a diff of the current version, or not a diff at all.
    #[error("{0}")]
    Header(#[from] header::HeaderFault),
    /// One of its two halves is present and is not an object.
    #[error("its {0} is not an object")]
    NotAnObject(&'static str),
}

/// The difference between two trees, or nothing where they are the same.
///
/// `base` is where the tree this is a diff of lives, relative to where the diff itself
/// will. IT IS AN ARGUMENT RATHER THAN SOMETHING A CALLER ADDS AFTERWARDS, because a diff
/// that does not name its base is only readable through whatever manifest happens to sit
/// beside it - so there is no way to write one here that leaves it out.
///
/// The result carries the whole header, so what comes back is a finished document rather
/// than something a caller has to remember to stamp.
#[must_use]
pub fn create(baseline: &SparseMap, target: &SparseMap, base: &str) -> Option<SparseMap> {
    let mut removed = Vec::new();
    let changes = diff_map(baseline, target, "", &mut removed);
    if removed.is_empty() && changes.is_empty() {
        return None;
    }

    let mut patch = SparseMap::new();
    patch.add(
        header::FORMAT_KEY,
        SparseValue::Text(FORMAT.format.to_string()),
    );
    #[allow(clippy::cast_possible_wrap)]
    patch.add(header::VERSION_KEY, SparseValue::Int(FORMAT.version as i32));
    patch.add(header::BASE_KEY, SparseValue::Text(base.to_string()));

    // EACH HALF IS LEFT OUT WHEN EMPTY, so a diff that only changes a value reads as that
    // value rather than as a value and an empty promise about removals.
    if !removed.is_empty() {
        let mut removals = SparseMap::new();
        for path in removed {
            removals.add(path, SparseValue::Bool(true));
        }
        patch.add(REMOVE_KEY, SparseValue::Map(removals));
    }

    if !changes.is_empty() {
        patch.add(CHANGES_KEY, SparseValue::Map(changes));
    }

    Some(patch)
}

/// What a diff says it is a diff of, or nothing where it says nothing.
///
/// A path relative to the diff itself, which is the same spelling every other `_base` in
/// this repository uses.
#[must_use]
pub fn base_of(patch: &SparseMap) -> Option<&str> {
    match patch.find(header::BASE_KEY) {
        Some(SparseValue::Text(text)) => Some(text.as_str()),
        _ => None,
    }
}

/// Applies a diff to the tree it is a diff of.
///
/// # Errors
///
/// Where the patch is not a current-version sparse diff, or where either of its halves is
/// present and is not an object.
pub fn apply(baseline: &SparseMap, patch: &SparseMap) -> Result<SparseMap, SparseDiffFault> {
    let format = match patch.find(header::FORMAT_KEY) {
        Some(SparseValue::Text(text)) => Some(text.as_str()),
        _ => None,
    };
    let version = match patch.find(header::VERSION_KEY) {
        Some(SparseValue::Int(number)) => u32::try_from(*number).ok(),
        _ => None,
    };
    FORMAT.check(format, version)?;

    // ABSENT AND PRESENT-BUT-EMPTY ARE THE SAME THING, and both mean "changes nothing": a
    // diff states only what it does.
    let removals = optional_map(patch, REMOVE_KEY, "removals")?;
    let changes = optional_map(patch, CHANGES_KEY, "changes")?;

    let removed: Vec<&str> = removals
        .map(|map| map.entries().iter().map(|(key, _)| key.as_str()).collect())
        .unwrap_or_default();

    // A half that is not there stands in as an empty one, which is what it means.
    let nothing = SparseMap::new();
    Ok(merge_map(
        baseline,
        changes.unwrap_or(&nothing),
        "",
        &removed,
    ))
}

/// One half of a diff, where it is there and is an object.
fn optional_map<'a>(
    patch: &'a SparseMap,
    key: &str,
    what: &'static str,
) -> Result<Option<&'a SparseMap>, SparseDiffFault> {
    match patch.find(key) {
        None => Ok(None),
        Some(SparseValue::Map(map)) => Ok(Some(map)),
        Some(_) => Err(SparseDiffFault::NotAnObject(what)),
    }
}

fn diff_map(
    baseline: &SparseMap,
    target: &SparseMap,
    path: &str,
    removed: &mut Vec<String>,
) -> SparseMap {
    let mut changes = SparseMap::new();

    for (key, _) in baseline.entries() {
        if target.find(key).is_none() {
            removed.push(child_path(path, key));
        }
    }

    for (key, value) in target.entries() {
        match baseline.find(key) {
            None => changes.add(key.clone(), value.clone()),
            Some(SparseValue::Map(was)) => {
                let SparseValue::Map(now) = value else {
                    changes.add(key.clone(), value.clone());
                    continue;
                };

                // A CHILD THAT ONLY REMOVES STILL HAS TO APPEAR, or the removal would be
                // recorded with nothing carrying it. Counting the removals before and
                // after is how a child whose whole change is a removal keeps its place.
                let before = removed.len();
                let child = diff_map(was, now, &child_path(path, key), removed);
                if !child.is_empty() || removed.len() > before {
                    changes.add(key.clone(), SparseValue::Map(child));
                }
            }
            Some(was) if was != value => changes.add(key.clone(), value.clone()),
            Some(_) => {}
        }
    }

    changes
}

fn merge_map(baseline: &SparseMap, changes: &SparseMap, path: &str, removed: &[&str]) -> SparseMap {
    let mut merged = SparseMap::new();

    for (key, value) in baseline.entries() {
        let child = child_path(path, key);
        if removed.iter().any(|gone| *gone == child) {
            continue;
        }

        match (value, changes.find(key)) {
            (SparseValue::Map(was), Some(SparseValue::Map(change))) => {
                merged.add(
                    key.clone(),
                    SparseValue::Map(merge_map(was, change, &child, removed)),
                );
            }
            (_, Some(change)) => merged.add(key.clone(), change.clone()),
            (_, None) => merged.add(key.clone(), value.clone()),
        }
    }

    // What the changes add, in the order they were written, after what the baseline held.
    for (key, value) in changes.entries() {
        if baseline.find(key).is_none() {
            merged.add(key.clone(), value.clone());
        }
    }

    merged
}

/// One step of a JSON Pointer, with the two characters a pointer reserves escaped.
fn child_path(parent: &str, name: &str) -> String {
    format!("{parent}/{}", name.replace('~', "~0").replace('/', "~1"))
}

#[cfg(test)]
mod tests {
    use super::super::sparse;
    use super::*;

    fn tree(text: &str) -> SparseMap {
        sparse::read(text, "a test document").expect("it reads")
    }

    /// A diff between two trees, applied, is the second tree.
    fn round_trip(baseline: &str, target: &str) -> SparseMap {
        let (was, now) = (tree(baseline), tree(target));
        match create(&was, &now, "beneath.json") {
            None => was,
            Some(patch) => apply(&was, &patch).expect("its own diff applies"),
        }
    }

    #[test]
    fn two_trees_that_are_the_same_have_no_diff() {
        assert_eq!(
            create(&tree(r#"{"a": 1}"#), &tree(r#"{"a": 1}"#), "beneath.json"),
            None
        );
    }

    #[test]
    fn a_changed_leaf_is_the_only_thing_carried() {
        let patch = create(
            &tree(r#"{"a": 1, "b": 2, "c": 3}"#),
            &tree(r#"{"a": 1, "b": 9, "c": 3}"#),
            "beneath.json",
        )
        .expect("they differ");

        let SparseValue::Map(changes) = patch.find(CHANGES_KEY).expect("it changes something")
        else {
            panic!("the changes are an object");
        };
        assert_eq!(changes.len(), 1, "only the leaf that changed");
        assert_eq!(changes.find("b"), Some(&SparseValue::Int(9)));
        assert!(!patch.has(REMOVE_KEY), "and nothing was removed");
    }

    #[test]
    fn a_nested_change_carries_only_the_path_to_it() {
        let patch = create(
            &tree(r#"{"one": {"deep": {"x": 1, "y": 2}}, "two": {"z": 3}}"#),
            &tree(r#"{"one": {"deep": {"x": 1, "y": 9}}, "two": {"z": 3}}"#),
            "beneath.json",
        )
        .expect("they differ");

        let written = sparse::write(&patch);
        assert!(written.contains("\"y\": 9"), "{written}");
        assert!(
            !written.contains("\"two\""),
            "an untouched branch: {written}"
        );
        assert!(!written.contains("\"x\""), "an untouched leaf: {written}");
    }

    #[test]
    fn a_removed_key_is_named_as_a_path() {
        let patch = create(
            &tree(r#"{"a": 1, "b": 2}"#),
            &tree(r#"{"a": 1}"#),
            "beneath.json",
        )
        .expect("they differ");

        let SparseValue::Map(removals) = patch.find(REMOVE_KEY).expect("something was removed")
        else {
            panic!("the removals are an object");
        };
        assert_eq!(removals.entries().len(), 1);
        assert_eq!(removals.entries()[0].0, "/b");
        assert!(!patch.has(CHANGES_KEY), "and nothing changed");
    }

    /// A key with a slash or a tilde in it is still ONE path segment.
    #[test]
    fn a_key_that_looks_like_a_path_is_escaped_into_one_segment() {
        let patch = create(
            &tree(r#"{"a/b": 1, "c~d": 2, "keep": 3}"#),
            &tree(r#"{"keep": 3}"#),
            "beneath.json",
        )
        .expect("they differ");

        let SparseValue::Map(removals) = patch.find(REMOVE_KEY).expect("removals") else {
            panic!("the removals are an object");
        };
        let paths: Vec<&str> = removals
            .entries()
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        assert_eq!(paths, vec!["/a~1b", "/c~0d"]);
    }

    /// A child whose whole change is a removal still has to appear in the changes.
    #[test]
    fn a_branch_that_only_loses_a_key_still_appears() {
        let patch = create(
            &tree(r#"{"one": {"gone": 1, "kept": 2}}"#),
            &tree(r#"{"one": {"kept": 2}}"#),
            "beneath.json",
        )
        .expect("they differ");

        let SparseValue::Map(changes) = patch.find(CHANGES_KEY).expect("changes") else {
            panic!("the changes are an object");
        };
        assert!(changes.has("one"), "the branch carrying the removal");

        let merged = round_trip(
            r#"{"one": {"gone": 1, "kept": 2}}"#,
            r#"{"one": {"kept": 2}}"#,
        );
        assert_eq!(merged, tree(r#"{"one": {"kept": 2}}"#));
    }

    /// An object merges key by key; everything else replaces what it lands on.
    #[test]
    fn an_object_merges_and_a_leaf_replaces() {
        assert_eq!(
            round_trip(r#"{"a": {"x": 1, "y": 2}}"#, r#"{"a": {"x": 1, "y": 9}}"#),
            tree(r#"{"a": {"x": 1, "y": 9}}"#),
            "an object merges",
        );
        assert_eq!(
            round_trip(r#"{"a": {"x": 1, "y": 2}}"#, r#"{"a": 7}"#),
            tree(r#"{"a": 7}"#),
            "a number replaces the object it lands on",
        );
        assert_eq!(
            round_trip(r#"{"a": 7}"#, r#"{"a": {"x": 1}}"#),
            tree(r#"{"a": {"x": 1}}"#),
            "and an object replaces the number",
        );
    }

    /// A key the baseline did not have is added after the ones it did.
    #[test]
    fn an_added_key_comes_after_the_ones_that_were_already_there() {
        let merged = round_trip(r#"{"a": 1, "b": 2}"#, r#"{"a": 1, "b": 2, "c": 3}"#);

        let keys: Vec<&str> = merged
            .entries()
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    /// The baseline's own order survives a merge, because these files are read by people.
    #[test]
    fn the_baselines_order_survives_a_merge() {
        let merged = round_trip(
            r#"{"zebra": 1, "mongoose": 2, "dolphin": 3}"#,
            r#"{"zebra": 1, "mongoose": 9, "dolphin": 3}"#,
        );

        let keys: Vec<&str> = merged
            .entries()
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        assert_eq!(keys, vec!["zebra", "mongoose", "dolphin"]);
    }

    /// A diff carries its own header, so what comes back is a whole document.
    #[test]
    fn a_diff_says_what_it_is() {
        let patch = create(&tree(r#"{"a": 1}"#), &tree(r#"{"a": 2}"#), "beneath.json")
            .expect("they differ");

        assert_eq!(
            patch.find(header::FORMAT_KEY),
            Some(&SparseValue::Text(FORMAT.format.to_string())),
        );
        assert_eq!(patch.find(header::VERSION_KEY), Some(&SparseValue::Int(1)));
        assert_eq!(
            patch.entries()[0].0,
            header::FORMAT_KEY,
            "and says it first, so a reader finds it without parsing the rest",
        );
    }

    /// A document that does not say it is a diff is not applied as one.
    #[test]
    fn something_that_is_not_a_diff_is_refused() {
        let refused = apply(&tree(r#"{"a": 1}"#), &tree(r#"{"a": 1}"#)).expect_err("refused");

        assert!(matches!(refused, SparseDiffFault::Header(_)), "{refused}");
    }

    /// A half that is present and is not an object is refused rather than skipped.
    #[test]
    fn a_half_that_is_not_an_object_is_refused() {
        let patch = tree(r#"{"_format": "sparse-diff", "_formatVersion": 1, "_changes": 7}"#);

        assert_eq!(
            apply(&tree(r#"{"a": 1}"#), &patch),
            Err(SparseDiffFault::NotAnObject("changes")),
        );
    }

    /// Absent and present-but-empty mean the same thing, which is "changes nothing".
    #[test]
    fn a_diff_with_neither_half_changes_nothing() {
        let patch = tree(r#"{"_format": "sparse-diff", "_formatVersion": 1}"#);

        assert_eq!(
            apply(&tree(r#"{"a": 1}"#), &patch).expect("it applies"),
            tree(r#"{"a": 1}"#),
        );
    }
}
