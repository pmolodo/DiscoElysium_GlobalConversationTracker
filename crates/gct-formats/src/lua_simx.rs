// SPDX-License-Identifier: MIT
//! The `Variable` table's entries that are a second copy of the `Conversation` table.
//!
//! ## What they are
//!
//! One variable per conversation, named `Conversation_SimX_<articy id>`, holding
//! `"<dialogue articy id>;<code>;..."` - one pair per dialogue entry, where the code is the
//! same `SimStatus` that conversation's `Dialog` map already holds. In the template save
//! that is 1,494 strings and 2.44 MB, a quarter of the whole expansion, carrying nothing
//! the `Dialog` maps do not.
//!
//! So the sparse form leaves them out and names what it left out in a `_derived_simx`
//! header, which says which conversations they belonged to and where among the table's
//! entries they sat.
//!
//! ## What that costs, stated rather than discovered
//!
//! THE VARIABLE TABLE LOSES ITS INDEPENDENCE. It cannot be read without the `Conversation`
//! table beside it and without [`Orders`], which is built from a file that is not committed.
//! A reader without either REFUSES: the alternative is a Variable table missing the
//! variables its own header names, which is a save that loads and is wrong.
//!
//! Editing a status now changes both places at once, which is the other half of the same
//! trade and the reason it is worth having: the two copies can no longer disagree.
//!
//! ## What it does not cost: the data
//!
//! Every string is REBUILT DURING ENCODING and compared against the original pair for pair.
//! Only a string that matches is left out, so one that does not follow the pattern is
//! written verbatim rather than lost.
//!
//! THE COMPARISON IGNORES THE ORDER the pairs came in, and a rebuilt string uses one fixed
//! order instead. A save's own order is the game's Lua table order, which is not stable even
//! between saves of one playthrough, and it carries nothing: an id that appears more than
//! once always carries the same status, so the string says only which status each dialogue
//! entry has.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

use super::lua_blob::{LuaTable, LuaValue};
use super::lua_manifest::{SIM_STATUS_KEY, UNTOUCHED_STATUS};
use super::runs;
use super::sparse::{SparseMap, SparseValue};

/// The prefix of every variable this handles.
pub const VARIABLE_PREFIX: &str = "Conversation_SimX_";

/// The property that stands in for the variables left out.
pub const HEADER_KEY: &str = "_derived_simx";

/// Which conversations those variables belonged to, in the order they sat in.
pub const CONVERSATIONS_KEY: &str = "_conversations";

/// Where among the table's entries they sat.
pub const POSITIONS_KEY: &str = "_at";

/// The field of a conversation holding the id its variable is named for.
pub const ARTICY_ID_KEY: &str = "Articy_Id";

/// The file [`Orders`] is built from, which this repository does not commit.
pub const ORDERS_FILE_NAME: &str = "articy_ids_final_cut.json";

/// Between an id and its code, and between one pair and the next.
const PAIR_SEPARATOR: char = ';';

/// The one-letter code each status is written as.
const STATUS_CODES: [(&str, char); 3] = [
    (UNTOUCHED_STATUS, 'u'),
    ("WasDisplayed", 'd'),
    ("WasOffered", 'o'),
];

/// Why the derived variables could not be left out or put back.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SimXFault {
    /// The id map is not the document it should be.
    #[error("{ORDERS_FILE_NAME} could not be read: {0}")]
    Unreadable(String),
    /// The header's two halves are of different lengths.
    #[error("{path}.{HEADER_KEY} names {named} conversations and {placed} positions")]
    Mismatched {
        path: String,
        named: usize,
        placed: usize,
    },
    /// Either half of the header is missing, or is not a key range.
    #[error("{path}.{HEADER_KEY}.{name} must be a key range")]
    NotARange { path: String, name: &'static str },
    /// A key range in the header could not be read.
    #[error("{0}")]
    Range(#[from] runs::RunFault),
    /// The header names a conversation whose entries cannot be rebuilt.
    #[error(
        "{path}.{HEADER_KEY} names conversation {conversation}, whose dialogue statuses \
         cannot be rebuilt from the {CONVERSATION_TABLE} table and {ORDERS_FILE_NAME}"
    )]
    Unrebuildable { path: String, conversation: i64 },
    /// The header names a conversation the Conversation table cannot name back.
    #[error(
        "{path} needs conversation {conversation}'s {ARTICY_ID_KEY}, which the \
         {CONVERSATION_TABLE} table does not have"
    )]
    Unnamed { path: String, conversation: i64 },
    /// The header puts a variable somewhere the table has no room for it.
    #[error("{path}.{HEADER_KEY} puts a variable at {at}, and the table holds {entries} entries")]
    Position {
        path: String,
        at: i64,
        entries: usize,
    },
}

/// The table the derived variables repeat.
pub const CONVERSATION_TABLE: &str = "Conversation";

/// What a derived variable is rebuilt from.
#[derive(Clone, Copy)]
pub struct Derivation<'a> {
    /// The `Conversation` table, whose dialogue statuses the variables repeat.
    pub conversations: &'a LuaTable,
    /// Which dialogue entries each conversation has.
    pub orders: &'a Orders,
}

/// Which dialogue entries belong to a conversation, and what each conversation is called.
///
/// All a rebuild needs is the SET of dialogue entries per conversation. THE ORDER THEY CAME
/// IN IS NOT RECORDED AND NOT REPRODUCED - see the module note - so this sorts them once,
/// by dialogue index and then by id, and every rebuilt string follows that order.
pub struct Orders {
    conversations: HashMap<String, i32>,
    sequences: HashMap<i32, Vec<(String, i32)>>,
}

/// The id map as it is written.
#[derive(Deserialize)]
struct RawOrders {
    /// Articy id to conversation index.
    conversations: HashMap<String, i32>,
    /// Articy id to the conversation it is in and the dialogue indices it sits at.
    dialogue_entries: HashMap<String, (i32, Vec<i32>)>,
}

impl Orders {
    /// Reads the id map.
    ///
    /// # Errors
    ///
    /// Where the text is not the document [`ORDERS_FILE_NAME`] holds.
    pub fn read(text: &str) -> Result<Self, SimXFault> {
        let raw: RawOrders =
            serde_json::from_str(text).map_err(|why| SimXFault::Unreadable(why.to_string()))?;

        let mut sequences: HashMap<i32, Vec<(String, i32)>> = HashMap::new();
        for (articy_id, (conversation, indices)) in raw.dialogue_entries {
            let held = sequences.entry(conversation).or_default();
            for index in indices {
                held.push((articy_id.clone(), index));
            }
        }

        // ONE ORDER, USED BY EVERY REBUILD. Any total order would do; this one reads
        // sensibly, following the dialogue entries as they are numbered.
        for held in sequences.values_mut() {
            held.sort_unstable_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)));
        }

        Ok(Self {
            conversations: raw.conversations,
            sequences,
        })
    }

    /// The conversation an articy id names, or nothing where it names none.
    #[must_use]
    pub fn conversation_of(&self, articy_id: &str) -> Option<i32> {
        self.conversations.get(articy_id).copied()
    }

    /// A conversation's dialogue entries, in the order a rebuilt string lists them.
    #[must_use]
    pub fn sequence_of(&self, conversation: i32) -> Option<&[(String, i32)]> {
        self.sequences
            .get(&conversation)
            .map(std::vec::Vec::as_slice)
    }

    /// How many conversations it knows, which is what tells a loaded map from an empty one.
    #[must_use]
    pub fn len(&self) -> usize {
        self.conversations.len()
    }

    /// Whether it knows none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.conversations.is_empty()
    }
}

/// The variables that can be left out, as the position each sat at and what it belonged to.
///
/// EVERY ONE IS REBUILT AND COMPARED before it is named here, so a string that does not
/// follow the pattern is simply not in the result and is written out verbatim.
#[must_use]
pub fn derivable(variables: &LuaTable, derivation: &Derivation<'_>) -> BTreeMap<usize, i32> {
    let mut derived = BTreeMap::new();
    for (at, (key, value)) in variables.dict.iter().enumerate() {
        let (LuaValue::Text(name), LuaValue::Text(actual)) = (key, value) else {
            continue;
        };
        let Some(articy_id) = name.strip_prefix(VARIABLE_PREFIX) else {
            continue;
        };
        let Some(conversation) = derivation.orders.conversation_of(articy_id) else {
            continue;
        };

        if rebuild(conversation, derivation).is_some_and(|built| same_pairs(&built, actual)) {
            derived.insert(variables.list.len() + at, conversation);
        }
    }

    derived
}

/// The header naming what was left out, in the order it was left out.
#[must_use]
pub fn header(derived: &BTreeMap<usize, i32>) -> SparseMap {
    let conversations: Vec<i64> = derived.values().map(|held| i64::from(*held)).collect();
    let positions: Vec<i64> = derived.keys().map(|at| *at as i64).collect();

    let mut header = SparseMap::new();
    header.add(
        CONVERSATIONS_KEY,
        SparseValue::Text(runs::pack(&conversations)),
    );
    header.add(POSITIONS_KEY, SparseValue::Text(runs::pack(&positions)));
    header
}

/// Puts the left-out variables back, given what was written and the header describing what
/// is missing.
///
/// # Errors
///
/// Where the header's halves disagree in length, where either is not a key range, where a
/// conversation it names cannot be rebuilt or cannot be named back, or where a position it
/// gives is not one the table has.
pub fn restore(
    variables: &mut LuaTable,
    header: &SparseMap,
    derivation: &Derivation<'_>,
    path: &str,
) -> Result<(), SimXFault> {
    let owners = key_range(header, CONVERSATIONS_KEY, path)?;
    let positions = key_range(header, POSITIONS_KEY, path)?;
    if owners.len() != positions.len() {
        return Err(SimXFault::Mismatched {
            path: path.to_string(),
            named: owners.len(),
            placed: positions.len(),
        });
    }

    for (conversation, at) in owners.into_iter().zip(positions) {
        let narrowed = i32::try_from(conversation).ok();
        let rebuilt = narrowed
            .and_then(|held| rebuild(held, derivation))
            .ok_or_else(|| SimXFault::Unrebuildable {
                path: path.to_string(),
                conversation,
            })?;
        let articy_id = narrowed
            .and_then(|held| text_field(derivation.conversations, held, ARTICY_ID_KEY))
            .ok_or_else(|| SimXFault::Unnamed {
                path: path.to_string(),
                conversation,
            })?;

        // THE POSITION IS AN INDEX INTO THE WHOLE TABLE, list part first, and a derived
        // variable's key is a string - so it belongs in the dictionary part, past the list.
        let entries = variables.list.len() + variables.dict.len();
        let into = usize::try_from(at)
            .ok()
            .and_then(|held| held.checked_sub(variables.list.len()))
            .filter(|held| *held <= variables.dict.len())
            .ok_or_else(|| SimXFault::Position {
                path: path.to_string(),
                at,
                entries,
            })?;

        variables.dict.insert(
            into,
            (
                LuaValue::Text(format!("{VARIABLE_PREFIX}{articy_id}")),
                LuaValue::Text(rebuilt),
            ),
        );
    }

    Ok(())
}

/// One half of the header, expanded.
fn key_range(header: &SparseMap, name: &'static str, path: &str) -> Result<Vec<i64>, SimXFault> {
    let Some(SparseValue::Text(range)) = header.find(name) else {
        return Err(SimXFault::NotARange {
            path: path.to_string(),
            name,
        });
    };

    Ok(runs::unpack(range, path)?)
}

/// The string a conversation's dialogue statuses spell out, or nothing where they cannot.
fn rebuild(conversation: i32, derivation: &Derivation<'_>) -> Option<String> {
    let sequence = derivation.orders.sequence_of(conversation)?;
    let LuaValue::Table(held) = derivation.conversations.get(&LuaValue::Int(conversation))? else {
        return None;
    };
    let LuaValue::Table(dialog) = held.get(&LuaValue::Text("Dialog".to_string()))? else {
        return None;
    };

    let mut text = String::new();
    for (articy_id, index) in sequence {
        let LuaValue::Table(entry) = dialog.get(&LuaValue::Int(*index))? else {
            return None;
        };
        let LuaValue::Text(status) = entry.get(&LuaValue::Text(SIM_STATUS_KEY.to_string()))? else {
            return None;
        };

        let code = STATUS_CODES
            .iter()
            .find(|(known, _)| known == status)
            .map(|(_, code)| *code)?;
        if !text.is_empty() {
            text.push(PAIR_SEPARATOR);
        }
        text.push_str(articy_id);
        text.push(PAIR_SEPARATOR);
        text.push(code);
    }

    Some(text)
}

/// A string field of one conversation.
fn text_field(conversations: &LuaTable, conversation: i32, name: &str) -> Option<String> {
    let LuaValue::Table(held) = conversations.get(&LuaValue::Int(conversation))? else {
        return None;
    };
    match held.get(&LuaValue::Text(name.to_string()))? {
        LuaValue::Text(text) => Some(text.clone()),
        _ => None,
    }
}

/// Whether two strings say the same thing: the same pairs, in whatever order each lists them.
fn same_pairs(rebuilt: &str, actual: &str) -> bool {
    let mut left = pairs_of(rebuilt);
    let mut right = pairs_of(actual);
    if left.len() != right.len() {
        return false;
    }

    left.sort_unstable();
    right.sort_unstable();
    left == right
}

/// The id-and-code pairs of a string, as they are written.
///
/// An ODD number of fields leaves a trailing one, which is dropped here and so cannot match
/// anything - which is what should happen: such a string does not follow the pattern, and
/// the length check above turns it away.
fn pairs_of(text: &str) -> Vec<(&str, &str)> {
    let fields: Vec<&str> = text.split(PAIR_SEPARATOR).collect();
    fields
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| (pair[0], pair[1]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest id map that says anything: two conversations, four dialogue entries.
    fn orders() -> Orders {
        Orders::read(
            r#"{
                "conversations": {"0xAA": 7, "0xBB": 8},
                "dialogue_entries": {
                    "0xAA-1": [7, [1]],
                    "0xAA-0": [7, [0]],
                    "0xBB-0": [8, [0]],
                    "0xTWICE": [7, [2, 3]]
                }
            }"#,
        )
        .expect("it reads")
    }

    fn text(value: &str) -> LuaValue {
        LuaValue::Text(value.to_string())
    }

    /// The same, as the header writes it.
    fn range(value: &str) -> SparseValue {
        SparseValue::Text(value.to_string())
    }

    /// One dialogue entry, which is a table holding only its status.
    fn status(value: &str) -> LuaValue {
        LuaValue::Table(LuaTable {
            list: Vec::new(),
            dict: vec![(text(SIM_STATUS_KEY), text(value))],
        })
    }

    /// A conversation with the statuses given, keyed by dialogue index.
    fn conversation(articy_id: &str, statuses: &[(i32, &str)]) -> LuaValue {
        LuaValue::Table(LuaTable {
            list: Vec::new(),
            dict: vec![
                (text(ARTICY_ID_KEY), text(articy_id)),
                (
                    text("Dialog"),
                    LuaValue::Table(LuaTable {
                        list: Vec::new(),
                        dict: statuses
                            .iter()
                            .map(|(index, held)| (LuaValue::Int(*index), status(held)))
                            .collect(),
                    }),
                ),
            ],
        })
    }

    /// Conversations 7 and 8, in the list part where a save holds them - so the table's
    /// first six entries stand in for the conversations before them.
    fn conversations() -> LuaTable {
        let mut table = LuaTable::default();
        for _ in 1..7 {
            table.list.push(LuaValue::Table(LuaTable::default()));
        }
        table.list.push(conversation(
            "0xAA",
            &[
                (0, UNTOUCHED_STATUS),
                (1, "WasDisplayed"),
                (2, "WasOffered"),
                (3, "WasOffered"),
            ],
        ));
        table
            .list
            .push(conversation("0xBB", &[(0, "WasDisplayed")]));
        table
    }

    /// The string conversation 7's statuses spell out, in the one order a rebuild uses.
    const SEVENS_STRING: &str = "0xAA-0;u;0xAA-1;d;0xTWICE;o;0xTWICE;o";

    fn variables(entries: &[(&str, &str)]) -> LuaTable {
        LuaTable {
            list: Vec::new(),
            dict: entries
                .iter()
                .map(|(name, value)| (text(name), text(value)))
                .collect(),
        }
    }

    #[test]
    fn a_variable_that_repeats_its_conversation_can_be_left_out() {
        let held = conversations();
        let derivation = Derivation {
            conversations: &held,
            orders: &orders(),
        };
        let table = variables(&[
            ("Alert", ""),
            ("Conversation_SimX_0xAA", SEVENS_STRING),
            ("Conversation_SimX_0xBB", "0xBB-0;d"),
        ]);

        assert_eq!(
            derivable(&table, &derivation),
            BTreeMap::from([(1, 7), (2, 8)]),
        );
    }

    /// The pairs are a map, and the order a save happened to write them in carries nothing.
    #[test]
    fn the_order_the_pairs_came_in_does_not_matter() {
        let held = conversations();
        let derivation = Derivation {
            conversations: &held,
            orders: &orders(),
        };
        let shuffled = "0xTWICE;o;0xAA-1;d;0xTWICE;o;0xAA-0;u";
        let table = variables(&[("Conversation_SimX_0xAA", shuffled)]);

        assert_ne!(shuffled, SEVENS_STRING, "it really is a different order");
        assert_eq!(derivable(&table, &derivation), BTreeMap::from([(0, 7)]));
    }

    /// A string that says something else is KEPT, which is why leaving one out is safe.
    #[test]
    fn a_variable_that_says_something_else_is_not_left_out() {
        let held = conversations();
        let derivation = Derivation {
            conversations: &held,
            orders: &orders(),
        };
        for odd in [
            // A status that disagrees with the Dialog map.
            "0xAA-0;d;0xAA-1;d;0xTWICE;o;0xTWICE;o",
            // A pair missing.
            "0xAA-0;u;0xAA-1;d;0xTWICE;o",
            // A pair too many.
            "0xAA-0;u;0xAA-1;d;0xTWICE;o;0xTWICE;o;0xEXTRA;u",
            // Not pairs at all.
            "nothing of the sort",
            "",
        ] {
            let table = variables(&[("Conversation_SimX_0xAA", odd)]);

            assert!(
                derivable(&table, &derivation).is_empty(),
                "'{odd}' was left out",
            );
        }
    }

    /// A conversation the id map does not know is one nothing can be rebuilt from.
    #[test]
    fn a_variable_naming_an_unknown_conversation_is_not_left_out() {
        let held = conversations();
        let derivation = Derivation {
            conversations: &held,
            orders: &orders(),
        };
        let table = variables(&[("Conversation_SimX_0xZZ", "0xZZ-0;u")]);

        assert!(derivable(&table, &derivation).is_empty());
    }

    #[test]
    fn what_was_left_out_is_named_and_comes_back_where_it_was() {
        let held = conversations();
        let derivation = Derivation {
            conversations: &held,
            orders: &orders(),
        };
        let whole = variables(&[
            ("Alert", ""),
            ("Conversation_SimX_0xAA", SEVENS_STRING),
            ("later", "kept"),
            ("Conversation_SimX_0xBB", "0xBB-0;d"),
        ]);

        let derived = derivable(&whole, &derivation);
        assert_eq!(derived, BTreeMap::from([(1, 7), (3, 8)]));

        let named = header(&derived);
        assert_eq!(named.find(CONVERSATIONS_KEY), Some(&range("7-8")));
        assert_eq!(named.find(POSITIONS_KEY), Some(&range("1,3")));

        let mut written = LuaTable {
            list: Vec::new(),
            dict: whole
                .dict
                .iter()
                .enumerate()
                .filter(|(at, _)| !derived.contains_key(at))
                .map(|(_, entry)| entry.clone())
                .collect(),
        };
        restore(&mut written, &named, &derivation, "Variable").expect("it restores");

        assert_eq!(written, whole);
    }

    /// A variable is rebuilt in the one fixed order, which need not be the one it had.
    #[test]
    fn a_restored_variable_uses_the_order_every_rebuild_uses() {
        let held = conversations();
        let derivation = Derivation {
            conversations: &held,
            orders: &orders(),
        };
        let mut table = LuaTable::default();
        let named = header(&BTreeMap::from([(0, 7)]));

        restore(&mut table, &named, &derivation, "Variable").expect("it restores");

        assert_eq!(
            table.dict,
            vec![(text("Conversation_SimX_0xAA"), text(SEVENS_STRING))],
        );
    }

    #[test]
    fn a_header_that_contradicts_itself_is_refused() {
        let held = conversations();
        let derivation = Derivation {
            conversations: &held,
            orders: &orders(),
        };
        let refused = |conversations: &str, positions: &str| {
            let mut named = SparseMap::new();
            named.add(CONVERSATIONS_KEY, range(conversations));
            named.add(POSITIONS_KEY, range(positions));
            let mut table = LuaTable::default();
            restore(&mut table, &named, &derivation, "Variable").expect_err("refused")
        };

        assert!(matches!(
            refused("7-8", "0"),
            SimXFault::Mismatched {
                named: 2,
                placed: 1,
                ..
            },
        ));
        assert!(matches!(
            refused("9", "0"),
            SimXFault::Unrebuildable {
                conversation: 9,
                ..
            },
        ));
        assert!(matches!(
            refused("7", "4"),
            SimXFault::Position {
                at: 4,
                entries: 0,
                ..
            },
        ));
        assert!(matches!(refused("7", "x"), SimXFault::Range(_)));
    }

    #[test]
    fn a_header_missing_a_half_is_refused() {
        let held = conversations();
        let derivation = Derivation {
            conversations: &held,
            orders: &orders(),
        };
        let mut named = SparseMap::new();
        named.add(CONVERSATIONS_KEY, range("7"));
        let mut table = LuaTable::default();

        let refused = restore(&mut table, &named, &derivation, "Variable").expect_err("refused");

        assert!(matches!(
            refused,
            SimXFault::NotARange {
                name: POSITIONS_KEY,
                ..
            },
        ));
    }

    #[test]
    fn an_id_map_that_is_not_one_is_refused() {
        assert!(matches!(
            Orders::read("{\"conversations\": 4}"),
            Err(SimXFault::Unreadable(_)),
        ));
        assert!(matches!(Orders::read("["), Err(SimXFault::Unreadable(_))));
    }

    /// An id at several dialogue indices is listed once per index, in index order.
    #[test]
    fn a_repeated_id_is_listed_once_per_place_it_sits() {
        let known = orders();

        assert_eq!(
            known.sequence_of(7),
            Some(
                &[
                    ("0xAA-0".to_string(), 0),
                    ("0xAA-1".to_string(), 1),
                    ("0xTWICE".to_string(), 2),
                    ("0xTWICE".to_string(), 3),
                ][..]
            ),
        );
        assert_eq!(known.sequence_of(9), None);
        assert_eq!(known.conversation_of("0xBB"), Some(8));
        assert_eq!(known.conversation_of("0xZZ"), None);
    }
}
