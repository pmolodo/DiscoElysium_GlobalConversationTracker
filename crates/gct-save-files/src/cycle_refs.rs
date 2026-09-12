// SPDX-License-Identifier: MIT
//! FullSerializer's cycle references, put back in an order the game can read.
//!
//! ## What they are
//!
//! The game's world state is written by FullSerializer, which handles an object that
//! appears in two places by writing it out once under an `$id` and writing
//! `{"$ref": "..."}` wherever it appears again. The pair is object identity: the two sites
//! are the same object, and which of them carries the body says nothing beyond which one
//! the writer reached first.
//!
//! ## Why that has to be repaired rather than preserved
//!
//! THE READER RESOLVES IN DOCUMENT ORDER. A `$ref` whose `$id` has not been read yet is not
//! looked up later - it throws, with "Object definition has not been encountered for object
//! with id=N; have you reordered or modified the serialized data?", and the load is
//! abandoned from there down.
//!
//! Reordering is exactly what a round trip through this crate does. `serde_json`'s object
//! is a `BTreeMap`, so a document that passes through a diff and a merge comes back with its
//! keys in alphabetical order - which for one committed save moved an `Interactable/...`
//! key holding a `$ref` above the `Patrol Cloak Placeholder Prefab(Clone)/...` key holding
//! its `$id`. That save then loaded its dialogue half and threw its world half away: no
//! character sheet, no money, no inventory, nothing in the game's own log but a stack trace
//! it carried on past, and every passive check in the run decided against whatever
//! character the game already had.
//!
//! Keeping the writer's order instead would not be enough on its own, because the committed
//! saves have already been through the sort and the order that would have to be kept is
//! gone. Moving the definition works from where the data actually is, and it needs no order
//! to be right: it makes every id defined before it is used by construction.
//!
//! ## What it does
//!
//! Walks the document the way the reader will, and makes the FIRST site that mentions an id
//! the one that carries the body. A site that carried the body but is no longer first
//! becomes a `$ref`. An object that refers to itself keeps its inner reference, since its
//! id is registered before its body is read - which is the case the format exists for.

use serde_json::{Map, Value};

/// The member naming an object so others can point at it.
const ID: &str = "$id";

/// The member pointing at one.
const REF: &str = "$ref";

/// Moves every cycle definition to the first place its id is mentioned.
///
/// A document with no `$id` anywhere in it is left exactly as it was.
pub fn normalise(document: &mut Value) {
    let mut bodies = Map::new();
    lift(document, &mut bodies);
    if bodies.is_empty() {
        return;
    }

    let mut defined = Vec::new();
    place(document, &mut bodies, &mut defined);
}

/// Takes every definition's body away, leaving a reference in its place.
///
/// Every site then says the same thing - "the object with this id belongs here" - which is
/// what makes "whichever site comes first carries it" one rule rather than two.
fn lift(value: &mut Value, bodies: &mut Map<String, Value>) {
    match value {
        Value::Object(members) => {
            let Some(id) = named(members, ID) else {
                for member in members.values_mut() {
                    lift(member, bodies);
                }
                return;
            };

            let mut body = Value::Object(std::mem::take(members));
            if let Value::Object(inner) = &mut body {
                inner.remove(ID);
            }

            lift(&mut body, bodies);
            bodies.insert(id.clone(), body);
            members.insert(REF.to_string(), Value::String(id));
        }
        Value::Array(items) => {
            for item in items {
                lift(item, bodies);
            }
        }
        _ => {}
    }
}

/// Puts each body back at the first site that mentions its id.
///
/// The id is recorded as defined BEFORE its body is walked, so an object that points at
/// itself keeps that reference instead of being unrolled forever.
fn place(value: &mut Value, bodies: &mut Map<String, Value>, defined: &mut Vec<String>) {
    match value {
        Value::Object(members) => {
            let Some(id) = named(members, REF) else {
                for member in members.values_mut() {
                    place(member, bodies, defined);
                }
                return;
            };

            if defined.iter().any(|seen| *seen == id) {
                return;
            }

            // A REFERENCE TO AN ID NOTHING DEFINES, left as it stands rather than invented.
            // The game will refuse it, which is the honest outcome for a save that is
            // genuinely broken; quietly dropping it would hide that.
            let Some(mut body) = bodies.remove(&id) else {
                return;
            };

            defined.push(id.clone());
            place(&mut body, bodies, defined);

            let Value::Object(mut body) = body else {
                return;
            };
            members.clear();
            members.insert(ID.to_string(), Value::String(id));
            members.append(&mut body);
        }
        Value::Array(items) => {
            for item in items {
                place(item, bodies, defined);
            }
        }
        _ => {}
    }
}

/// The string an object's `$id` or `$ref` member holds, where it has one.
fn named(members: &Map<String, Value>, key: &str) -> Option<String> {
    members
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

/// The id every `$ref` in a document points at that no `$id` above it has defined.
///
/// Empty is what the game needs. Written here rather than in a test so that the check on a
/// real save and the check on a made-up one are the same code.
#[must_use]
pub fn unresolvable(document: &Value) -> Vec<String> {
    let mut defined = Vec::new();
    let mut missing = Vec::new();
    walk(document, &mut defined, &mut missing);
    missing
}

fn walk(value: &Value, defined: &mut Vec<String>, missing: &mut Vec<String>) {
    match value {
        Value::Object(members) => {
            if let Some(id) = named(members, ID) {
                defined.push(id);
            } else if let Some(id) = named(members, REF)
                && !defined.contains(&id)
            {
                missing.push(id);
            }

            for member in members.values() {
                walk(member, defined, missing);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk(item, defined, missing);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A reference above its definition swaps places with it.
    #[test]
    fn a_forward_reference_becomes_the_definition() {
        let mut document = json!({
            "a": {"$ref": "7"},
            "b": {"$id": "7", "value": 3},
        });

        assert_eq!(unresolvable(&document), vec!["7".to_string()]);
        normalise(&mut document);

        assert_eq!(
            document,
            json!({
                "a": {"$id": "7", "value": 3},
                "b": {"$ref": "7"},
            })
        );
        assert!(unresolvable(&document).is_empty());
    }

    /// One already in order is left as it stands.
    #[test]
    fn a_document_already_in_order_is_untouched() {
        let before = json!({
            "a": {"$id": "7", "value": 3},
            "b": {"$ref": "7"},
        });

        let mut document = before.clone();
        normalise(&mut document);

        assert_eq!(document, before);
    }

    /// An object pointing at itself keeps its inner reference.
    #[test]
    fn a_self_reference_is_left_alone() {
        let before = json!({"$id": "7", "again": {"$ref": "7"}});

        let mut document = before.clone();
        normalise(&mut document);

        assert_eq!(document, before);
        assert!(unresolvable(&document).is_empty());
    }

    /// A body that moves brings its own references with it, and they still resolve.
    #[test]
    fn a_hoisted_body_takes_its_own_references_with_it() {
        let mut document = json!({
            "a": {"$ref": "1"},
            "b": {"$id": "1", "inner": {"$ref": "2"}},
            "c": {"$id": "2", "value": 9},
        });

        normalise(&mut document);

        assert_eq!(
            document,
            json!({
                "a": {"$id": "1", "inner": {"$id": "2", "value": 9}},
                "b": {"$ref": "1"},
                "c": {"$ref": "2"},
            })
        );
        assert!(unresolvable(&document).is_empty());
    }

    /// Several mentions of one id leave the later ones pointing at the first.
    #[test]
    fn only_the_first_mention_carries_the_body() {
        let mut document = json!({
            "a": {"$ref": "4"},
            "b": {"$ref": "4"},
            "c": {"$id": "4", "value": 1},
        });

        normalise(&mut document);

        assert_eq!(
            document,
            json!({
                "a": {"$id": "4", "value": 1},
                "b": {"$ref": "4"},
                "c": {"$ref": "4"},
            })
        );
    }

    /// A definition inside an array is found like any other.
    #[test]
    fn a_definition_inside_an_array_moves_too() {
        let mut document = json!({
            "a": [{"$ref": "2"}],
            "b": [{"$id": "2", "value": 5}],
        });

        normalise(&mut document);

        assert_eq!(
            document,
            json!({
                "a": [{"$id": "2", "value": 5}],
                "b": [{"$ref": "2"}],
            })
        );
    }

    /// A reference nothing defines is left where it is rather than guessed at.
    #[test]
    fn a_reference_with_no_definition_is_left_alone() {
        let before = json!({"a": {"$ref": "5"}, "b": {"$id": "6", "value": 0}});

        let mut document = before.clone();
        normalise(&mut document);

        assert_eq!(document, before);
        assert_eq!(unresolvable(&document), vec!["5".to_string()]);
    }

    /// A document with no cycle references at all is not rewritten.
    #[test]
    fn a_document_without_cycles_is_untouched() {
        let before = json!({"a": [1, 2, {"b": "c"}], "d": {"e": false}});

        let mut document = before.clone();
        normalise(&mut document);

        assert_eq!(document, before);
    }
}
