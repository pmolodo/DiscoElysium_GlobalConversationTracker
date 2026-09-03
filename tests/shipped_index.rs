// SPDX-License-Identifier: MIT
//! Does the index the mod ships still say everything the engine reads?
//!
//! The shipped index is trimmed - 47.7 MB down to 14.3 MB - and trimming means a list,
//! written in C# in `ShippedIndex.KeptFields`, of what survives. The engine's own list is
//! `index::ENTRY_FIELDS_READ`, written in Rust. Two lists that must agree and cannot share
//! a constant.
//!
//! ## Why this test exists specifically
//!
//! The first draft of the trim guessed those names - `IsPassiveCheck`, `IsRedCheck`,
//! `IsWhiteCheck` and the rest - and every one of the eleven was wrong. Nothing would have
//! failed: the trimmed index would simply have contained no skill checks at all, every
//! `determine_kind` would have answered `None`, the crawl would have walked through every
//! check as though it were ordinary dialogue, and the markers would have been quietly
//! wrong in the direction that shows content the player cannot reach.
//!
//! So this compares the two indexes rather than the two lists, which is the stronger
//! check: it fails if a field the engine names is present in the full index and missing
//! from the trimmed one, whatever either list happens to say.

use std::collections::HashMap;

use lookahead_engine::index::{build_group_graph, read_index, ENTRY_FIELDS_READ};

mod common;

/// The groups every other measurement uses, so a difference here is comparable with the
/// numbers elsewhere.
const EXPENSIVE: [i32; 5] = [368, 631, 14, 28, 1030];

#[test]
fn the_trimmed_index_keeps_every_field_the_engine_reads() {
    let (Some(full), Some(trimmed)) = (common::conversation_index(), common::shipped_index())
    else {
        return;
    };

    let full = read_index(&full).expect("the full index reads");
    let trimmed = read_index(&trimmed).expect("the trimmed index reads");

    assert_eq!(
        full.len(),
        trimmed.len(),
        "the trim changed how many conversations there are",
    );

    let mut checked = 0usize;
    let mut kept: HashMap<&str, usize> = HashMap::new();

    for (id, conversation) in &full {
        let other = trimmed.get(id).unwrap_or_else(|| panic!("conversation {id} was dropped"));
        assert_eq!(
            conversation.entries.len(),
            other.entries.len(),
            "conversation {id} lost entries",
        );

        for (entry, slim) in conversation.entries.iter().zip(&other.entries) {
            assert_eq!(entry.id, slim.id, "conversation {id} reordered its entries");

            // The parts the engine deserialises, which must survive whole.
            assert_eq!(entry.guard, slim.guard, "{id}:{} lost its guard", entry.id);
            assert_eq!(entry.script, slim.script, "{id}:{} lost its script", entry.id);
            assert_eq!(entry.to, slim.to, "{id}:{} lost its links", entry.id);
            assert_eq!(
                entry.to_conversation, slim.to_conversation,
                "{id}:{} lost its cross-conversation links", entry.id,
            );
            assert_eq!(entry.group, slim.group, "{id}:{} lost its group flag", entry.id);

            for name in ENTRY_FIELDS_READ {
                match entry.fields.get(name) {
                    Some(value) => {
                        assert_eq!(
                            slim.fields.get(name),
                            Some(value),
                            "{id}:{} lost the field '{name}', which the engine reads",
                            entry.id,
                        );
                        *kept.entry(name).or_default() += 1;
                    }
                    // Absent in the full index is fine; the trim cannot invent one.
                    None => assert!(
                        !slim.fields.contains_key(name),
                        "{id}:{} gained a field '{name}' it did not have",
                        entry.id,
                    ),
                }
            }

            checked += 1;
        }
    }

    println!("{checked} entries checked across {} conversations", full.len());
    let mut rows: Vec<(&&str, &usize)> = kept.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (name, count) in rows {
        println!("  {count:>6}  {name}");
    }

    // A trim that kept nothing would pass every assertion above, because every one of them
    // is conditional on the full index having the field. This is what makes it mean
    // something.
    assert!(
        kept.len() >= 5,
        "only {} of the engine's fields were seen at all; is the trim keeping anything?",
        kept.len(),
    );
}

/// And the graph built from the trimmed index is the graph built from the full one.
///
/// The end of the argument. The field comparison above says the bytes survived; this says
/// the thing the engine actually constructs out of them is the same - the check kinds, the
/// costs, the flags, the links.
#[test]
fn a_group_built_from_the_trimmed_index_is_the_same_group() {
    let (Some(full), Some(trimmed)) = (common::conversation_index(), common::shipped_index())
    else {
        return;
    };

    let full = read_index(&full).expect("the full index reads");
    let trimmed = read_index(&trimmed).expect("the trimmed index reads");

    for conversation in EXPENSIVE {
        let Ok((one, group)) = build_group_graph(&full, conversation) else { continue };
        let (other, other_group) =
            build_group_graph(&trimmed, conversation).expect("the trimmed group builds");

        assert_eq!(group, other_group, "conversation {conversation}: the group changed");
        assert_eq!(
            one.count(),
            other.count(),
            "conversation {conversation}: the entry count changed",
        );

        let mut compared = 0;
        for node in one.nodes() {
            let twin = other
                .get(node.id)
                .unwrap_or_else(|| panic!("{} is missing from the trimmed graph", node.id));

            assert_eq!(node.kind, twin.kind, "{}: the check kind changed", node.id);
            assert_eq!(node.cost, twin.cost, "{}: the cost changed", node.id);
            assert_eq!(node.cost_once, twin.cost_once, "{}: cost_once changed", node.id);
            assert_eq!(node.is_group, twin.is_group, "{}: the group flag changed", node.id);
            assert_eq!(node.links, twin.links, "{}: the links changed", node.id);
            assert_eq!(
                node.boolean_only, twin.boolean_only,
                "{}: boolean_only changed", node.id,
            );
            assert_eq!(
                node.guard.to_string(),
                twin.guard.to_string(),
                "{}: the guard changed",
                node.id,
            );
            assert_eq!(
                node.actions.len(),
                twin.actions.len(),
                "{}: the actions changed",
                node.id,
            );
            compared += 1;
        }

        println!("conversation {conversation}: {compared} entries identical");
    }
}
