// SPDX-License-Identifier: MIT
//! Does the index the mod ships still say everything the engine reads?
//!
//! The shipped index is trimmed - 47.7 MB down to 16.9 MB - and trimming means a list,
//! written in C# in `ShippedIndex.KeptFields`, of what survives. The engine's own list is
//! `index::ENTRY_FIELDS_READ`, written in Rust. Two lists that must agree and cannot share
//! a constant.
//!
//! ## Why this test exists specifically
//!
//! The first draft of the trim guessed those names - `IsPassiveCheck`, `IsRedCheck`,
//! `IsWhiteCheck` and the rest - and every one of the eleven was wrong. Nothing would have
//! failed: the trimmed index would simply have contained no skill checks at all, every
//! `determine_kind` would have answered `None`, the search would have walked through every
//! check as though it were ordinary dialogue, and the markers would have been quietly
//! wrong in the direction that shows content the player cannot reach.
//!
//! So this compares the two indexes rather than the two lists, which is the stronger
//! check: it fails if a field the engine names is present in the full index and missing
//! from the trimmed one, whatever either list happens to say.

use std::collections::HashMap;

use lookahead_engine::core::types::DialogueCheckKind;
use lookahead_engine::index::journal::Journal;
use lookahead_engine::index::{
    ENTRY_FIELDS_READ, MODIFIER_SLOTS, build_group_graph, conversation_fields_read, determine_kind,
    discover_group, modifier_bonus_field, modifier_expression_field, read_index,
};

use gct_measure::common;

/// The groups every other measurement uses, so a difference here is comparable with the
/// numbers elsewhere.
const EXPENSIVE: [i32; 5] = [368, 631, 14, 28, 1030];

#[test]
fn the_player_actor_is_you() {
    let path = common::actors().expect("actor table is required");
    let text = std::fs::read_to_string(path).unwrap();
    let actor = text
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|actor| {
            actor["id"].as_i64().unwrap().to_string() == lookahead_engine::index::PLAYER_ACTOR
        })
        .expect("player actor exists");
    assert_eq!(actor["name"], "You");
}

/// The speakers whose passive checks pay a thought's price are the skills they are named for.
#[test]
fn the_passive_price_actors_are_their_skills() {
    let path = common::actors().expect("actor table is required");
    let text = std::fs::read_to_string(path).unwrap();
    let names: HashMap<String, String> = text
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .map(|actor| {
            (
                actor["id"].as_i64().unwrap().to_string(),
                actor["name"].as_str().unwrap().to_string(),
            )
        })
        .collect();

    for (actor, skill) in lookahead_engine::core::thought_effects::PASSIVE_PRICE_ACTORS {
        assert_eq!(
            names.get(actor).map(|name| name.to_uppercase()).as_deref(),
            Some(skill),
            "actor {actor}",
        );
    }
}

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
        let other = trimmed
            .get(id)
            .unwrap_or_else(|| panic!("conversation {id} was dropped"));
        assert_eq!(
            conversation.entries.len(),
            other.entries.len(),
            "conversation {id} lost entries",
        );
        assert_eq!(
            conversation.fields, other.fields,
            "conversation {id} changed the journal fields the engine reads",
        );

        for (entry, slim) in conversation.entries.iter().zip(&other.entries) {
            assert_eq!(entry.id, slim.id, "conversation {id} reordered its entries");

            // The parts the engine deserialises, which must survive whole.
            assert_eq!(entry.guard, slim.guard, "{id}:{} lost its guard", entry.id);
            assert_eq!(
                entry.script, slim.script,
                "{id}:{} lost its script",
                entry.id
            );
            assert_eq!(entry.to, slim.to, "{id}:{} lost its links", entry.id);
            assert_eq!(
                entry.to_conversation, slim.to_conversation,
                "{id}:{} lost its cross-conversation links",
                entry.id,
            );
            assert_eq!(
                entry.group, slim.group,
                "{id}:{} lost its group flag",
                entry.id
            );

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

    println!(
        "{checked} entries checked across {} conversations",
        full.len()
    );
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

/// The shipped index carries every white check's target modifiers.
///
/// THE NAMES ARE THE WHOLE RISK, which is the failure this file was written for: a field name
/// spelled wrong is a field no entry has, and a test that only compares the two indexes
/// against each other passes on it happily, since there is nothing on either side to compare.
/// So this asks the database's own population instead - every white check has at least one
/// modifier, and the bonus beside it is a number.
///
/// 126 white checks in the database this was written against, and not one without a modifier.
#[test]
fn the_shipped_index_carries_the_white_checks_modifiers() {
    let Some(trimmed) = common::shipped_index() else {
        return;
    };
    let trimmed = read_index(&trimmed).expect("the trimmed index reads");

    let mut checks = 0usize;
    let mut modifiers = 0usize;
    for (id, conversation) in &trimmed {
        for entry in &conversation.entries {
            if determine_kind(&entry.fields) != DialogueCheckKind::White {
                continue;
            }
            checks += 1;

            let mut carried = 0usize;
            for slot in 1..=MODIFIER_SLOTS {
                let Some(expression) = entry.fields.get(&modifier_expression_field(slot)) else {
                    continue;
                };
                if expression.trim().is_empty() {
                    continue;
                }
                carried += 1;

                let bonus = entry
                    .fields
                    .get(&modifier_bonus_field(slot))
                    .map(|text| text.trim().to_string())
                    .unwrap_or_default();
                assert!(
                    bonus.parse::<i32>().is_ok(),
                    "{id}:{} modifier {slot} is worth '{bonus}', which is not a number",
                    entry.id,
                );
            }

            assert!(
                carried > 0,
                "{id}:{} is a white check carrying no modifier, so either the database has \
                 gained one of those or the field names here no longer match it",
                entry.id,
            );
            modifiers += carried;
        }
    }

    assert!(
        checks > 100,
        "only {checks} white checks in the shipped index, which is too few to be the game's",
    );
    eprintln!("{checks} white checks carry {modifiers} modifiers");
}

/// The shipped index carries the whole journal, and only the fields the engine reads for it.
///
/// A task's conditions are CONVERSATION fields, which an index long carried none of - so an
/// index missing them reads as a game with no journal at all, and every journal action would
/// resolve to nothing without a word. 337 parts, 139 of them tasks, in the database this was
/// written against.
#[test]
fn the_shipped_index_carries_the_journal() {
    let Some(trimmed) = common::shipped_index() else {
        return;
    };
    let trimmed = read_index(&trimmed).expect("the trimmed index reads");

    let read = conversation_fields_read();
    for (id, conversation) in &trimmed {
        for name in conversation.fields.keys() {
            assert!(
                read.contains(name),
                "conversation {id} carries '{name}', which the engine does not read",
            );
        }
    }

    let journal = Journal::from_index(&trimmed);
    let tasks = journal
        .parts()
        .iter()
        .filter(|p| p.parent.is_none())
        .count();
    println!(
        "{} journal parts, {tasks} of them tasks",
        journal.parts().len()
    );
    assert!(
        journal.parts().len() >= JOURNAL_PARTS_AT_LEAST,
        "only {} journal parts; is the index carrying the task conditions?",
        journal.parts().len(),
    );
}

/// A floor well under the 337 parts the database has, so a content update that drops a few
/// does not fail the check while an index that carries none still does.
const JOURNAL_PARTS_AT_LEAST: usize = 300;

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
        let Ok((one, group)) = build_group_graph(&full, conversation) else {
            continue;
        };
        let (other, other_group) =
            build_group_graph(&trimmed, conversation).expect("the trimmed group builds");

        assert_eq!(
            group, other_group,
            "conversation {conversation}: the group changed"
        );
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

            assert_eq!(node.player, twin.player, "{}: the speaker changed", node.id);
            assert_eq!(
                node.choice, twin.choice,
                "{}: the choice flag changed",
                node.id
            );
            assert_eq!(node.kind, twin.kind, "{}: the check kind changed", node.id);
            assert_eq!(node.cost, twin.cost, "{}: the cost changed", node.id);
            assert_eq!(
                node.cost_once, twin.cost_once,
                "{}: cost_once changed",
                node.id
            );
            assert_eq!(
                node.is_group, twin.is_group,
                "{}: the group flag changed",
                node.id
            );
            assert_eq!(node.links, twin.links, "{}: the links changed", node.id);
            assert_eq!(
                node.boolean_only, twin.boolean_only,
                "{}: boolean_only changed",
                node.id,
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

        // THE SYMBOL TABLE TOO, in order and not only in size. It is what `DataLayout`
        // lays out and what a slot's index means, so two graphs that agreed on every entry
        // and disagreed here would build different diagrams out of the same group - and
        // every measurement that prices a layout would be pricing a different one.
        let symbols = one.symbols();
        let others = other.symbols();
        assert_eq!(
            symbols.count(),
            others.count(),
            "conversation {conversation}: the symbol count changed",
        );
        for index in 0..symbols.count() {
            assert_eq!(
                symbols.name_of(index),
                others.name_of(index),
                "conversation {conversation}: symbol {index} changed",
            );
        }

        println!(
            "conversation {conversation}: {compared} entries and {} symbols identical",
            symbols.count(),
        );
    }
}

/// And the WHOLE GAME's group list is the same list, group for group.
///
/// ## Why the two tests above are not enough for a caller that measures every group
///
/// They compare five groups, and `crates/gct-measure/examples/menu_matrix.rs` enumerates all 1,422 of them
/// before a whole-game run measures any - `group_list`, which is `discover_group` over every
/// conversation in the index, canonicalised by the set of conversations it reaches. That
/// list decides which rows a whole-game run HAS. A trim that dropped a cross-conversation
/// link in a group nobody has measured would leave both tests above green and silently
/// split one group into two, which is a different run rather than a slower one.
///
/// It is also the last thing between the menu matrix and the trimmed index. A row is a
/// function of the graph, the world, the budget, the cap and the profile, and only the first
/// comes out of the index - so identical graphs and an identical group list is the whole
/// argument.
#[test]
fn the_whole_games_group_list_survives_the_trim() {
    let (Some(full), Some(trimmed)) = (common::conversation_index(), common::shipped_index())
    else {
        return;
    };

    let full = read_index(&full).expect("the full index reads");
    let trimmed = read_index(&trimmed).expect("the trimmed index reads");

    let ours = groups_of(&full);
    let theirs = groups_of(&trimmed);

    assert_eq!(
        ours.len(),
        theirs.len(),
        "the trim changed how many groups there are"
    );
    assert_eq!(
        ours, theirs,
        "the trim changed which conversations reach which"
    );

    println!("{} groups identical across the whole index", ours.len());
}

/// Every conversation, and the set of conversations its group reaches.
///
/// The same walk `menu_matrix::group_list` makes, kept as a plain map rather than
/// canonicalised into starts: a map says WHICH conversation's group changed where a list of
/// starts would only say that one did.
fn groups_of(
    index: &lookahead_engine::index::Index,
) -> std::collections::BTreeMap<i32, std::collections::BTreeSet<i32>> {
    index
        .keys()
        .map(|conversation| {
            (
                *conversation,
                discover_group(index, *conversation).into_iter().collect(),
            )
        })
        .collect()
}
