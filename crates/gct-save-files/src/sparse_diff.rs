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
//! ## A CHANGED RUN SAYS WHICH IDS MOVED
//!
//! A save records its dialogue statuses as one run of entry ids per status - `"0,3,5-9"` -
//! and a save that has shown one more line differs in one id. Writing the new run out whole
//! says what the run BECAME; it does not say what changed, and a reader is left diffing two
//! walls of digits by eye. So a changed run is written as the ids that joined it and the
//! ids that left:
//!
//! ```text
//! "Dialog": { "_changes": { "WasDisplayed": "7" },
//!             "_remove":  { "WasOffered":   "7" } }
//! ```
//!
//! THE SAME TWO WORDS this format already reserves at its top level, one level down, which
//! is one rule reused rather than a second one invented. Both halves are needed because a
//! status only ever goes UP: an entry moving from offered to displayed leaves one run and
//! joins another.
//!
//! ALWAYS, rather than only where it is shorter. The point is that a diff should read as a
//! change rather than as a result, and a rule that switched on size would leave the big
//! diffs unreadable and fix only the small ones. It is not about the bytes - the worst case
//! it saves is about a kilobyte, measured by `tools/survey-run-diff-cost.py`.
//!
//! ## What counts as a run, and how a reader can be sure
//!
//! Both sides must parse as runs, re-encode to exactly the string they came from, and be
//! ASCENDING. The first two make this safe without a table of which paths hold runs: a
//! value that merely reads like `"1-5"` is treated as one only if `"1-5"` is also how this
//! repository would have written that set.
//!
//! The third is what makes a delta LOSSLESS, and it is not a formality. A delta is set
//! arithmetic, and not every run here is a set. `_derived_simx` records the order its
//! variables are rebuilt in, so its `_conversations` is a SEQUENCE - and one save spells it
//! `"1499-1159,1156-488,486-306,304-254,250-1,1500-1501"`, which holds exactly the ids the
//! save it is a diff of holds, in another order. Put through a set it comes back ascending,
//! which is a save that rebuilds its variables in the wrong order with nothing saying so.
//! For an ascending run the set determines the string exactly; anything else is written
//! whole, and a reordering says everything it has to say by being written out.
//!
//! Of the 3,669 runs this repository holds, 3,667 ascend and those two do not, measured by
//! `tools/survey-run-order.py`. Neither of the two changes its ids, only their order, so
//! nothing is lost by spelling them in full. See de-vz8v.2 for the ordered delta an id
//! moving WITHIN one of them would need.
//!
//! AN EMPTIED RUN IS AN EMPTY RUN, not an absent key. Removing a key is what the document's
//! own `_remove` is for, and a delta that quietly did it as well would be two operations
//! wearing one name. `_keys` is what makes this load-bearing: it holds a run of key ids, so
//! it is delta'd like any other, and an empty `_keys` means something an absent one does
//! not.
//!
//! ## A key really called `_changes`
//!
//! THE GAME NAMES THINGS FREELY. One of its actors is `_Smallest_Church_in_Saint_Saëns_`, so
//! nothing may be assumed from the shape of a name and a table could hold a key called
//! `_changes`. A change to a key named after one of this format's own three words is
//! therefore written under [`RESERVED_KEY_VALUES`], where it cannot be mistaken for the
//! word it shares a name with.
//!
//! WRITTEN ONLY WHERE ONE EXISTS, which is nowhere in this repository today. The
//! alternative - escaping every underscore - would cost every diff its readability to guard
//! against something that has never happened.
//!
//! ## A path is a JSON Pointer
//!
//! `/conversations/29`, with `~` written `~0` and `/` written `~1`, so a key containing
//! either is still one path segment. The escaping is what makes a path unambiguous rather
//! than a plain join, and the game's keys do contain slashes.

use std::collections::BTreeSet;

use super::header::{self, Expected};
use super::runs;
use super::sparse::{SparseMap, SparseValue};

/// What this format is called, and the version of it this build writes.
///
/// VERSION 2 SAYS WHICH IDS MOVED IN A RUN, where version 1 wrote the run out again. See
/// the note on run deltas at the top of this module.
pub const FORMAT: Expected = Expected {
    format: "sparse-diff",
    version: 2,
};

/// Where a diff's changes live.
pub const CHANGES_KEY: &str = "_changes";

/// Where a diff's removals live.
pub const REMOVE_KEY: &str = "_remove";

/// Where a change to a key whose NAME is one of this format's own words lives.
///
/// THE GAME NAMES THINGS FREELY. One of its actors is called
/// `_Smallest_Church_in_Saint_Saëns_`, so a key beginning with an underscore is ordinary
/// data and nothing may be assumed from the shape of a name. A table could therefore hold a
/// key called `_changes`, and a diff that wrote it beside its own `_changes` would be a
/// diff nobody could read back.
///
/// So a change to a key named `_changes`, `_remove` or `_reserved_key_values` is written
/// under this one instead. It costs nothing when no such key exists, which is every file
/// this repository holds today - and the alternative, escaping every underscore, would make
/// every diff harder to read to guard against something that has never happened.
pub const RESERVED_KEY_VALUES: &str = "_reserved_key_values";

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
    let mut colliding = SparseMap::new();
    let mut joined = SparseMap::new();
    let mut left = SparseMap::new();

    for (key, _) in baseline.entries() {
        if target.find(key).is_none() {
            removed.push(child_path(path, key));
        }
    }

    // THE WALK IS ITS OWN SCOPE, because the closure below borrows two of the maps and they
    // are read again once it has ended.
    {
        // A KEY WHOSE NAME IS ONE OF THIS FORMAT'S OWN goes in the container instead, or a
        // reader could not tell it from the thing it is named after.
        let mut wrote = |key: &String, value: SparseValue| {
            if reserved(key) {
                colliding.add(key.clone(), value);
            } else {
                changes.add(key.clone(), value);
            }
        };

        for (key, value) in target.entries() {
            match baseline.find(key) {
                None => wrote(key, value.clone()),
                Some(SparseValue::Map(was)) => {
                    let SparseValue::Map(now) = value else {
                        wrote(key, value.clone());
                        continue;
                    };

                    // A CHILD THAT ONLY REMOVES STILL HAS TO APPEAR, or the removal would
                    // be recorded with nothing carrying it. Counting the removals before
                    // and after is how a child whose whole change is a removal keeps its
                    // place.
                    let before = removed.len();
                    let child = diff_map(was, now, &child_path(path, key), removed);
                    if !child.is_empty() || removed.len() > before {
                        wrote(key, SparseValue::Map(child));
                    }
                }
                // A RUN SAYS WHICH IDS MOVED. A key named after one of this format's own
                // words cannot, since the delta is itself keyed by name - so it is
                // replaced whole.
                //
                // A DELTA THAT NAMES NO ID IS NOT ONE. Two runs holding the same ids
                // differ only in their ORDER, which set arithmetic has nothing to say
                // about, and writing neither half would drop the change instead of
                // describing it.
                Some(was) if was != value => match moved(was, value)
                    .filter(|_| !reserved(key))
                    .filter(|(gained, lost)| !gained.is_empty() || !lost.is_empty())
                {
                    Some((gained, lost)) => {
                        if !gained.is_empty() {
                            joined.add(key.clone(), SparseValue::Text(runs::pack(&gained)));
                        }
                        if !lost.is_empty() {
                            left.add(key.clone(), SparseValue::Text(runs::pack(&lost)));
                        }
                    }
                    None => wrote(key, value.clone()),
                },
                Some(_) => {}
            }
        }
    }

    // AHEAD OF WHAT ELSE CHANGED, because what moved is what a reader came to find out.
    if !colliding.is_empty() {
        changes.lead(RESERVED_KEY_VALUES, SparseValue::Map(colliding));
    }
    if !left.is_empty() {
        changes.lead(REMOVE_KEY, SparseValue::Map(left));
    }
    if !joined.is_empty() {
        changes.lead(CHANGES_KEY, SparseValue::Map(joined));
    }

    changes
}

/// Whether a name means something to a change object rather than being one of its keys.
///
/// THE GAME NAMES THINGS FREELY, and one of its actors really is called
/// `_Smallest_Church_in_Saint_Saëns_` - so "a key beginning with an underscore is ours" is
/// not a rule this format may have. Only these three words are reserved, and a data key
/// that collides with one is written in [`RESERVED_KEY_VALUES`] instead.
fn reserved(key: &str) -> bool {
    matches!(key, CHANGES_KEY | REMOVE_KEY | RESERVED_KEY_VALUES)
}

/// Which ids joined a run and which left it, where both sides are runs at all.
///
/// Nothing where either side is not one, and the caller then writes the new value whole -
/// which is what every value that is not a run needs anyway.
fn moved(was: &SparseValue, now: &SparseValue) -> Option<(Vec<i64>, Vec<i64>)> {
    let (SparseValue::Text(was), SparseValue::Text(now)) = (was, now) else {
        return None;
    };

    let before: BTreeSet<i64> = canonical(was)?.into_iter().collect();
    let after: BTreeSet<i64> = canonical(now)?.into_iter().collect();

    Some((
        after.difference(&before).copied().collect(),
        before.difference(&after).copied().collect(),
    ))
}

/// The ids a string holds, where the string is a run whose ORDER carries nothing.
///
/// Two conditions, and the second is the one that matters.
///
/// RE-ENCODED AND COMPARED, so a value that merely reads like a run is only treated as one
/// if the run encoding agrees this is how it would have spelled that set.
///
/// AND ASCENDING, because a delta is set arithmetic and set arithmetic cannot keep an order.
/// Not every run here is a set: `_derived_simx` names which conversations' variables were
/// left out, in the order they sat in, and that order is what pairs them with the positions
/// beside them. Feeding it through a set gives back the same ids ascending, which is a save
/// that rebuilds its variables at the wrong offsets and nothing saying so. For an ascending
/// run the set determines the string exactly, so the delta is lossless; for any other, the
/// value is written whole. See de-vz8v.2.
fn canonical(text: &str) -> Option<Vec<i64>> {
    let ids = runs::unpack(text, "a run").ok()?;
    let ascending = ids.windows(2).all(|pair| pair[0] < pair[1]);

    (ascending && runs::pack(&ids) == text).then_some(ids)
}

fn merge_map(baseline: &SparseMap, changes: &SparseMap, path: &str, removed: &[&str]) -> SparseMap {
    let mut merged = SparseMap::new();

    // THE RUN DELTAS FOR THIS OBJECT'S OWN CHILDREN, which is what `_changes` and `_remove`
    // mean anywhere but the top of the document, and the changes to keys whose names those
    // words took. Everything else here is a key of the object itself.
    let joined = nested(changes, CHANGES_KEY);
    let left = nested(changes, REMOVE_KEY);
    let colliding = nested(changes, RESERVED_KEY_VALUES);
    let change_to = |key: &str| {
        if reserved(key) {
            colliding.and_then(|held| held.find(key))
        } else {
            changes.find(key)
        }
    };

    for (key, value) in baseline.entries() {
        let child = child_path(path, key);
        if removed.iter().any(|gone| *gone == child) {
            continue;
        }

        let delta = !reserved(key)
            && (joined.is_some_and(|held| held.has(key)) || left.is_some_and(|held| held.has(key)));
        if delta && let Some(run) = with_delta(value, run_at(joined, key), run_at(left, key)) {
            merged.add(key.clone(), SparseValue::Text(run));
            continue;
        }

        match (value, change_to(key)) {
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
    for (key, value) in changes
        .entries()
        .iter()
        .filter(|(key, _)| !reserved(key))
        .chain(colliding.map_or(&[][..], SparseMap::entries))
    {
        if baseline.find(key).is_none() {
            merged.add(key.clone(), value.clone());
        }
    }

    // A DELTA AGAINST A RUN THE BASELINE DOES NOT HAVE is an addition, which nothing here
    // writes - the writer states a new run whole - but which reads the only way it can.
    for (key, _) in joined.map_or(&[][..], SparseMap::entries) {
        if !reserved(key)
            && baseline.find(key).is_none()
            && let Some(run) =
                with_delta(&SparseValue::Text(String::new()), run_at(joined, key), None)
        {
            merged.add(key.clone(), SparseValue::Text(run));
        }
    }

    merged
}

/// One of the two run-delta halves of a change object, where it is there and is an object.
fn nested<'a>(changes: &'a SparseMap, key: &str) -> Option<&'a SparseMap> {
    match changes.find(key) {
        Some(SparseValue::Map(held)) => Some(held),
        _ => None,
    }
}

/// The ids one half names for one key.
fn run_at<'a>(half: Option<&'a SparseMap>, key: &str) -> Option<&'a str> {
    match half?.find(key) {
        Some(SparseValue::Text(text)) => Some(text.as_str()),
        _ => None,
    }
}

/// A run with ids taken out and ids put in, or nothing where it is not a run to begin with.
///
/// A SIDE THAT WILL NOT PARSE IS NOT DELTA'D. This is the reading end of a format whose
/// writer only ever produces runs here, so a value that is not one means a hand-edited file
/// - and leaving it to the ordinary merge is better than inventing a value nobody wrote.
///
/// A RUN EMPTIED OF EVERY ID IS AN EMPTY RUN, not an absent key. Removing a key is what the
/// document's own `_remove` is for, and a delta that quietly did it as well would be two
/// operations wearing one name. Nothing the writer produces reaches this: it only writes a
/// delta where both sides are runs, and it never writes an empty one.
fn with_delta(value: &SparseValue, joined: Option<&str>, left: Option<&str>) -> Option<String> {
    let SparseValue::Text(was) = value else {
        return None;
    };

    let mut ids: BTreeSet<i64> = if was.is_empty() {
        BTreeSet::new()
    } else {
        canonical(was)?.into_iter().collect()
    };

    for gone in left.and_then(canonical).unwrap_or_default() {
        ids.remove(&gone);
    }
    ids.extend(joined.and_then(canonical).unwrap_or_default());

    Some(runs::pack(&ids.into_iter().collect::<Vec<_>>()))
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

    /// The whole point: a run that gained one id says which one, not what it became.
    #[test]
    fn a_run_that_gained_an_id_says_which_id() {
        let patch = create(
            &tree(r#"{"Dialog": {"WasDisplayed": "0,3,5-9"}}"#),
            &tree(r#"{"Dialog": {"WasDisplayed": "0,3,5-9,12"}}"#),
            "beneath.json",
        )
        .expect("they differ");

        assert_eq!(
            sparse::write(&patch).trim_end(),
            concat!(
                "{\n",
                "  \"_format\": \"sparse-diff\",\n",
                "  \"_formatVersion\": 2,\n",
                "  \"_base\": \"beneath.json\",\n",
                "  \"_changes\": {\n",
                "    \"Dialog\": {\n",
                "      \"_changes\": {\n",
                "        \"WasDisplayed\": \"12\"\n",
                "      }\n",
                "    }\n",
                "  }\n",
                "}",
            ),
        );
    }

    /// And what it says applies back to what it was taken against.
    #[test]
    fn a_run_delta_applies_to_the_run_it_was_taken_against() {
        let was = tree(r#"{"Dialog": {"WasDisplayed": "0,3,5-9"}}"#);
        let now = tree(r#"{"Dialog": {"WasDisplayed": "0,3,5-9,12"}}"#);

        let patch = create(&was, &now, "beneath.json").expect("they differ");

        assert_eq!(apply(&was, &patch).expect("it applies"), now);
    }

    /// A status only ever goes up, so an entry leaves one run as it joins another.
    #[test]
    fn an_id_moving_from_one_run_to_another_is_a_removal_and_an_addition() {
        let was = tree(r#"{"Dialog": {"WasOffered": "3,7", "WasDisplayed": "1"}}"#);
        let now = tree(r#"{"Dialog": {"WasOffered": "3", "WasDisplayed": "1,7"}}"#);

        let patch = create(&was, &now, "beneath.json").expect("they differ");
        let text = sparse::write(&patch);

        assert!(text.contains(r#""WasOffered": "7""#), "{text}");
        assert!(text.contains(r#""WasDisplayed": "7""#), "{text}");
        assert_eq!(apply(&was, &patch).expect("it applies"), now);
    }

    /// A status nothing is at any more has its key removed, by the document's own `_remove`.
    #[test]
    fn a_status_nothing_is_at_any_more_loses_its_key() {
        let was = tree(r#"{"Dialog": {"WasOffered": "7", "WasDisplayed": "1"}}"#);
        let now = tree(r#"{"Dialog": {"WasDisplayed": "1,7"}}"#);

        let patch = create(&was, &now, "beneath.json").expect("they differ");

        assert_eq!(apply(&was, &patch).expect("it applies"), now);
    }

    /// THE ONE THAT COST EIGHT SAVES, before an emptied run gave back an empty run rather
    /// than nothing. `_keys` holds a run of key ids, so it is delta'd like any other - and
    /// emptying it has to leave the empty run the reader expects, not an absent key.
    #[test]
    fn a_run_of_key_ids_emptied_by_a_delta_is_an_empty_run() {
        let was = tree(r#"{"Dialog": {"_keys": "0-1", "WasDisplayed": "1"}}"#);
        let now = tree(r#"{"Dialog": {"_keys": "", "WasDisplayed": "1"}}"#);

        let patch = create(&was, &now, "beneath.json").expect("they differ");

        assert!(sparse::write(&patch).contains(r#""_keys": "0-1""#));
        assert_eq!(apply(&was, &patch).expect("it applies"), now);
    }

    /// A run that does not ascend is an ordered sequence, and a set cannot carry an order.
    ///
    /// THE ONE THAT COST EIGHT SAVES A SECOND TIME. `_derived_simx` names the conversations
    /// whose variables were left out, in the order they sat in, and that order is the
    /// pairing with the positions beside it. A delta over it gave back the same ids
    /// ascending, which is a save that rebuilds its variables at the wrong offsets.
    #[test]
    fn a_run_that_is_not_ascending_is_written_whole() {
        let was = tree(r#"{"_conversations": "1499-1159,1156-488"}"#);
        let now = tree(r#"{"_conversations": "1499-1159,1156-487"}"#);

        let patch = create(&was, &now, "beneath.json").expect("they differ");
        let text = sparse::write(&patch);

        assert!(
            text.contains(r#""_conversations": "1499-1159,1156-487""#),
            "{text}"
        );
        assert_eq!(apply(&was, &patch).expect("it applies"), now);
    }

    /// The corpus's own case: the same ids in another order, which is no delta at all.
    ///
    /// Both saves that hold an unordered run hold one of these - every id the save they are
    /// a diff of holds, rearranged. A delta would name nothing in either half, so the run is
    /// written out, and the new order is the whole of what it has to say. See de-vz8v.2.
    #[test]
    fn a_run_holding_the_same_ids_in_another_order_is_written_whole() {
        let was = tree(r#"{"_conversations": "1-250,254-304"}"#);
        let now = tree(r#"{"_conversations": "304-254,250-1"}"#);

        let patch = create(&was, &now, "beneath.json").expect("they differ");

        let SparseValue::Map(changes) = patch.find(CHANGES_KEY).expect("it changes something")
        else {
            panic!("the changes are an object");
        };
        assert_eq!(
            changes.find("_conversations"),
            Some(&SparseValue::Text("304-254,250-1".to_string())),
            "the run itself, not a delta of it",
        );
        assert_eq!(apply(&was, &patch).expect("it applies"), now);
    }

    /// A key really called `_changes` is data, and goes where it cannot be mistaken for the
    /// word it shares a name with.
    ///
    /// THE GAME NAMES THINGS FREELY - one of its actors is `_Smallest_Church_in_Saint_Saëns_`
    /// - so this is a shape the format has to have rather than one it can rule out.
    #[test]
    fn a_key_named_after_one_of_this_formats_own_words_is_held_apart() {
        let was = tree(r#"{"_changes": "one", "_remove": "two", "other": 1}"#);
        let now = tree(r#"{"_changes": "ONE", "_remove": "TWO", "other": 2}"#);

        let patch = create(&was, &now, "beneath.json").expect("they differ");
        let text = sparse::write(&patch);

        assert!(text.contains(RESERVED_KEY_VALUES), "{text}");
        assert_eq!(apply(&was, &patch).expect("it applies"), now);
    }

    /// And a diff of anything else does not carry the container at all.
    #[test]
    fn nothing_carries_the_container_unless_a_key_needs_it() {
        let patch = create(
            &tree(r#"{"Dialog": {"WasDisplayed": "1"}}"#),
            &tree(r#"{"Dialog": {"WasDisplayed": "1,7"}}"#),
            "beneath.json",
        )
        .expect("they differ");

        assert!(!sparse::write(&patch).contains(RESERVED_KEY_VALUES));
    }

    /// A key called `_reserved_key_values` is data too, and holds itself.
    #[test]
    fn even_the_containers_own_name_is_a_key_something_could_have() {
        let was = tree(r#"{"_reserved_key_values": 1}"#);
        let now = tree(r#"{"_reserved_key_values": 2}"#);

        let patch = create(&was, &now, "beneath.json").expect("they differ");

        assert_eq!(apply(&was, &patch).expect("it applies"), now);
    }

    /// A key of that name being ADDED, rather than changed, lands in the same place.
    #[test]
    fn a_key_named_after_one_of_those_words_can_be_added_too() {
        let was = tree(r#"{"other": 1}"#);
        let now = tree(r#"{"other": 1, "_remove": "mine"}"#);

        let patch = create(&was, &now, "beneath.json").expect("they differ");

        assert_eq!(apply(&was, &patch).expect("it applies"), now);
    }

    /// A value that merely reads like a run is written whole, because it may not be one.
    #[test]
    fn a_string_this_format_would_not_have_written_as_a_run_is_replaced_whole() {
        // "1,2,3" is a set this format spells "1-3", so it is not a run it wrote.
        let was = tree(r#"{"note": "1,2,3"}"#);
        let now = tree(r#"{"note": "1,2,3,4"}"#);

        let patch = create(&was, &now, "beneath.json").expect("they differ");
        let text = sparse::write(&patch);

        assert!(text.contains(r#""note": "1,2,3,4""#), "{text}");
        assert_eq!(apply(&was, &patch).expect("it applies"), now);
    }

    /// And a string that is not a run at all is untouched by any of this.
    #[test]
    fn a_string_that_is_not_a_run_is_replaced_whole() {
        let was = tree(r#"{"who": "Kim"}"#);
        let now = tree(r#"{"who": "Harry"}"#);

        let patch = create(&was, &now, "beneath.json").expect("they differ");

        assert!(sparse::write(&patch).contains(r#""who": "Harry""#));
        assert_eq!(apply(&was, &patch).expect("it applies"), now);
    }

    /// A bulk change is a delta too, since the rule does not switch on size.
    #[test]
    fn a_run_that_changed_a_great_deal_is_still_a_delta() {
        let was = tree(r#"{"Dialog": {"WasDisplayed": "1-100"}}"#);
        let now = tree(r#"{"Dialog": {"WasDisplayed": "1-50,200-250"}}"#);

        let patch = create(&was, &now, "beneath.json").expect("they differ");
        let text = sparse::write(&patch);

        assert!(text.contains(r#""WasDisplayed": "200-250""#), "{text}");
        assert!(text.contains(r#""WasDisplayed": "51-100""#), "{text}");
        assert_eq!(apply(&was, &patch).expect("it applies"), now);
    }

    /// A version 1 diff is refused, since its runs mean the other thing.
    #[test]
    fn a_diff_of_the_older_version_is_refused() {
        let older = tree(
            r#"{"_format": "sparse-diff", "_formatVersion": 1,
                "_changes": {"a": 1}}"#,
        );

        let refused = apply(&tree("{}"), &older).expect_err("refused");

        assert!(matches!(refused, SparseDiffFault::Header(_)), "{refused}");
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
        assert_eq!(patch.find(header::VERSION_KEY), Some(&SparseValue::Int(2)),);
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
        let patch = tree(r#"{"_format": "sparse-diff", "_formatVersion": 2, "_changes": 7}"#);

        assert_eq!(
            apply(&tree(r#"{"a": 1}"#), &patch),
            Err(SparseDiffFault::NotAnObject("changes")),
        );
    }

    /// Absent and present-but-empty mean the same thing, which is "changes nothing".
    #[test]
    fn a_diff_with_neither_half_changes_nothing() {
        let patch = tree(r#"{"_format": "sparse-diff", "_formatVersion": 2}"#);

        assert_eq!(
            apply(&tree(r#"{"a": 1}"#), &patch).expect("it applies"),
            tree(r#"{"a": 1}"#),
        );
    }
}
