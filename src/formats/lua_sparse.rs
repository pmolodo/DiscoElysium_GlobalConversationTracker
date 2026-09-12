// SPDX-License-Identifier: MIT
//! Between a save's Lua table and the sparse tree it is stored as.
//!
//! ## What the sparse form keeps, and what it does not
//!
//! It keeps EVERY KEY AND EVERY VALUE. It does not keep the LAYOUT: where a table's Lua
//! list part ended and its dictionary part began, and what order the entries came in.
//! Reading back picks one canonical answer to both.
//!
//! So a sparse round trip is not byte for byte, and the blob it produces is not the blob it
//! read. The bet is that the game does not care where the boundary fell or what order a
//! table's entries were stored in, which is what Lua semantics say and what an actual load
//! has to confirm. [`super::lua_blob`] is the form that does keep the layout, and it is the
//! one a packed save is written from.
//!
//! ## Grouped, or entry by entry
//!
//! Only tables named by [`super::lua_manifest`] are restructured, and only when they really
//! have the shape it claims; everything else is written out entry by entry. A grouped table
//! says which keys it stands for in `_keys` and then names one key list per distinct value,
//! leaving out the value the great majority of its children carry.
//!
//! ## The SimX derivation is REFUSED, not skipped
//!
//! A `Variable` table may leave out the variables that merely repeat the `Conversation`
//! table and name what it left out in a `_derived_simx` header. Rebuilding those is
//! de-xz48.6.3.2.2 and is not here yet, so a document carrying that header is turned away.
//! Reading it and ignoring the header would produce a Variable table missing the variables
//! it names, which is a save that loads and is wrong.

use std::collections::{HashMap, HashSet};

use super::header::{self, Expected};
use super::lua_blob::{self, LuaTable, LuaValue};
use super::lua_manifest::{self, Grouping, KeyType};
use super::runs::{self, RunFault};
use super::sparse::{SparseMap, SparseValue};

/// What a sparse table document is called, and the version of it this build writes.
pub const FORMAT: Expected = Expected {
    format: "sparse",
    version: 1,
};

/// Marks a grouped table and lists the keys it stands for.
pub const KEYS_KEY: &str = "_keys";

/// The layout form's list boundary.
///
/// This writer never emits it - the reader works the boundary out - but the reader honours
/// it, so a hand-written file can pin the split where it has a reason to.
pub const LIST_COUNT_KEY: &str = "_num_list_entries";

/// Where a `Variable` table names the variables it left out. See the module note.
pub const DERIVED_SIMX_KEY: &str = "_derived_simx";

/// A name for a table's entry order that nothing writes.
///
/// Refused rather than ignored, so a file that recorded an order fails loudly instead of
/// quietly losing it.
const RETIRED_REORDER_KEY: &str = "_reorder";

/// The table whose variables mirror another table's data.
pub const VARIABLE_TABLE: &str = "Variable";

/// The table those variables mirror.
pub const CONVERSATION_TABLE: &str = "Conversation";

/// Property names this format uses for itself rather than for a Lua key.
///
/// Each means what it says only where it belongs - the two header fields and the list
/// boundary leading a table, `_keys` in a grouped one, `_derived_simx` in a `Variable`. The
/// name is reserved EVERYWHERE ELSE too, so a table cannot carry an entry that would be
/// read as bookkeeping the next time the file is opened.
const BOOKKEEPING: [&str; 6] = [
    KEYS_KEY,
    LIST_COUNT_KEY,
    DERIVED_SIMX_KEY,
    RETIRED_REORDER_KEY,
    header::FORMAT_KEY,
    header::VERSION_KEY,
];

/// Why a Lua table could not be written as a sparse tree, or read back from one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LuaSparseFault {
    /// What should have been a table is not a JSON object.
    #[error("{0} must be a JSON object")]
    NotAnObject(String),
    /// A table carries an entry named after this format's own bookkeeping.
    #[error("table '{path}' cannot name an entry '{name}'; that name is reserved")]
    Reserved { path: String, name: String },
    /// Two properties of one object have the same name.
    #[error("table '{path}' has two '{name}' properties")]
    Duplicate { path: String, name: String },
    /// A list entry is not named after its own 1-based index.
    #[error("{path} list entry {index} must be named '{index}', not '{name}'")]
    ListName {
        path: String,
        index: usize,
        name: String,
    },
    /// The pinned list boundary is not a count, or is longer than the table.
    #[error("{path}.{LIST_COUNT_KEY} must be a whole number no larger than the table")]
    ListCount { path: String },
    /// A dictionary key is not of the type this table's keys are.
    #[error("{path} dictionary key '{name}' is not a valid {kind}")]
    Key {
        path: String,
        name: String,
        kind: &'static str,
    },
    /// A dictionary key reads as a value that would be written back differently.
    #[error("{path} dictionary key '{name}' must be spelled '{canonical}'")]
    KeySpelling {
        path: String,
        name: String,
        canonical: String,
    },
    /// A Lua key that has no spelling as a property name.
    #[error("{path} has a table key that is not a string, a number or a boolean")]
    UnwritableKey { path: String },
    /// A grouped table's property is not a key range.
    #[error("{path}.{name} must be a key range")]
    NotARange { path: String, name: String },
    /// A key range could not be read.
    #[error("{0}")]
    Range(#[from] RunFault),
    /// A grouped table's `_keys` does not climb, so it says nothing usable about the order.
    #[error("{path}.{KEYS_KEY} must list its keys in ascending order, without repeats")]
    KeysNotAscending { path: String },
    /// A grouped table gives one key two values.
    #[error("{path} lists key {key} under both '{first}' and '{second}'")]
    Overlap {
        path: String,
        key: i64,
        first: String,
        second: String,
    },
    /// A grouped table gives a value to a key it does not claim.
    #[error("{path} groups key {key}, which {KEYS_KEY} omits")]
    Ungrouped { path: String, key: i64 },
    /// It leaves out variables that only the Conversation table can rebuild.
    #[error(
        "{path} carries a {DERIVED_SIMX_KEY} header, and rebuilding what it names is not \
         implemented (de-xz48.6.3.2.2)"
    )]
    NeedsSimX { path: String },
}

/// Turns one table into its sparse tree.
///
/// `path` names the table, as `Conversation/631/Dialog` - it is what the manifests match
/// against and what a fault quotes.
///
/// # Errors
///
/// Where a table holds a key that has no spelling as a JSON property name.
pub fn encode(table: &LuaTable, path: &str) -> Result<SparseMap, LuaSparseFault> {
    if let Some(grouping) = lua_manifest::grouping_for(path)
        && let Some(grouped) = try_encode_grouped(table, grouping)
    {
        return Ok(grouped);
    }

    encode_dense(table, path)
}

/// Turns a sparse tree back into the table it came from.
///
/// # Errors
///
/// Where the document is not an object, where it names an entry after this format's own
/// bookkeeping, where a key is not of the type the table's keys are or is spelled a way
/// this would not write it, where a grouped table's ranges disagree with each other, or
/// where it leaves out variables that only the Conversation table can rebuild.
pub fn decode(node: &SparseValue, path: &str) -> Result<LuaTable, LuaSparseFault> {
    let SparseValue::Map(map) = node else {
        return Err(LuaSparseFault::NotAnObject(path.to_string()));
    };

    decode_map(map, path)
}

/// Every entry of a table, keyed the way the sparse form names it.
///
/// The list part's keys are its own 1-based indices, which the table does not store, and
/// the dictionary part's are what it holds.
fn named_entries<'a>(
    table: &'a LuaTable,
    path: &str,
) -> Result<Vec<(String, &'a LuaValue)>, LuaSparseFault> {
    let mut named = Vec::with_capacity(table.list.len() + table.dict.len());
    for (at, value) in table.list.iter().enumerate() {
        named.push(((at + 1).to_string(), value));
    }

    for (key, value) in &table.dict {
        let name = key_name(key).ok_or_else(|| LuaSparseFault::UnwritableKey {
            path: path.to_string(),
        })?;
        named.push((name, value));
    }

    Ok(named)
}

fn encode_dense(table: &LuaTable, path: &str) -> Result<SparseMap, LuaSparseFault> {
    let mut map = SparseMap::new();
    for (name, value) in named_entries(table, path)? {
        let encoded = encode_value(value, path, &name)?;
        map.add(name, encoded);
    }

    Ok(map)
}

fn encode_value(value: &LuaValue, path: &str, name: &str) -> Result<SparseValue, LuaSparseFault> {
    Ok(match value {
        LuaValue::Nil => SparseValue::Null,
        LuaValue::Bool(flag) => SparseValue::Bool(*flag),
        LuaValue::Int(whole) => SparseValue::Int(*whole),
        LuaValue::Float(float) => SparseValue::Float(*float),
        LuaValue::Text(text) => SparseValue::Text(text.clone()),
        LuaValue::Table(child) => SparseValue::Map(encode(child, &below(path, name))?),
    })
}

/// The path of a table's child.
fn below(path: &str, name: &str) -> String {
    format!("{path}/{name}")
}

/// Writes a table of same-shaped children as one key list per distinct value.
///
/// Nothing where the table does not have that shape after all, which leaves the caller to
/// write it entry by entry. EVERY CONDITION IS CHECKED rather than assumed, so a manifest
/// rule that goes stale costs output size and never accuracy.
fn try_encode_grouped(table: &LuaTable, grouping: &Grouping) -> Option<SparseMap> {
    let child_key = LuaValue::Text(grouping.child_key.to_string());
    let default = LuaValue::Text(grouping.default_value.to_string());

    let mut keys = Vec::with_capacity(table.list.len() + table.dict.len());
    let mut by_value: Vec<(&str, Vec<i64>)> = Vec::new();
    for (key, value) in integer_keyed(table)? {
        let LuaValue::Table(child) = value else {
            return None;
        };
        if !child.list.is_empty() || child.dict.len() != 1 {
            return None;
        }

        let (held, carried) = &child.dict[0];
        if *held != child_key || matches!(carried, LuaValue::Table(_)) {
            return None;
        }

        keys.push(key);
        if *carried == default {
            continue;
        }

        // The value becomes a property name, so it has to be one this format would read
        // back as a value rather than as its own bookkeeping.
        let LuaValue::Text(name) = carried else {
            return None;
        };
        if name.is_empty() || BOOKKEEPING.contains(&name.as_str()) {
            return None;
        }

        match by_value.iter_mut().find(|(held, _)| *held == name) {
            Some((_, bucket)) => bucket.push(key),
            None => by_value.push((name, vec![key])),
        }
    }

    let mut grouped = SparseMap::new();
    keys.sort_unstable();
    grouped.add(KEYS_KEY, SparseValue::Text(runs::pack(&keys)));
    for (name, mut bucket) in by_value {
        bucket.sort_unstable();
        grouped.add(name, SparseValue::Text(runs::pack(&bucket)));
    }

    Some(grouped)
}

/// Every entry of a table with its key as a whole number, or nothing where one is not.
fn integer_keyed(table: &LuaTable) -> Option<Vec<(i64, &LuaValue)>> {
    let mut entries = Vec::with_capacity(table.list.len() + table.dict.len());
    for (at, value) in table.list.iter().enumerate() {
        entries.push((at as i64 + 1, value));
    }

    for (key, value) in &table.dict {
        let LuaValue::Int(whole) = key else {
            return None;
        };
        entries.push((i64::from(*whole), value));
    }

    Some(entries)
}

fn decode_map(map: &SparseMap, path: &str) -> Result<LuaTable, LuaSparseFault> {
    if map.has(DERIVED_SIMX_KEY) {
        return Err(LuaSparseFault::NeedsSimX {
            path: path.to_string(),
        });
    }

    match lua_manifest::grouping_for(path) {
        Some(grouping) if map.has(KEYS_KEY) => decode_grouped(map, path, grouping),
        _ => decode_dense(map, path),
    }
}

fn decode_dense(map: &SparseMap, path: &str) -> Result<LuaTable, LuaSparseFault> {
    let mut table = LuaTable::default();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut list_count = 0;
    let mut still_list = true;
    let mut boundary_given = false;

    for (name, value) in map.entries() {
        let so_far = table.list.len() + table.dict.len();
        if so_far == 0 {
            // LEADING ONLY: what the document is, which version of it, and where its list
            // part ends. Deeper in, each of these is an ordinary name and reserved.
            if name == header::FORMAT_KEY || name == header::VERSION_KEY {
                // The caller has already acted on both.
                continue;
            }
            if name == LIST_COUNT_KEY {
                list_count = as_count(value, path)?;
                boundary_given = true;
                continue;
            }
        }

        if BOOKKEEPING.contains(&name.as_str()) {
            return Err(LuaSparseFault::Reserved {
                path: path.to_string(),
                name: name.clone(),
            });
        }
        if !seen.insert(name) {
            return Err(LuaSparseFault::Duplicate {
                path: path.to_string(),
                name: name.clone(),
            });
        }

        let index = so_far + 1;
        let is_list_entry = if boundary_given {
            so_far < list_count
        } else {
            still_list && *name == index.to_string()
        };
        still_list = is_list_entry;

        let decoded = decode_value(value, path, name)?;
        if is_list_entry {
            if *name != index.to_string() {
                return Err(LuaSparseFault::ListName {
                    path: path.to_string(),
                    index,
                    name: name.clone(),
                });
            }
            table.list.push(decoded);
            if !boundary_given {
                list_count = index;
            }
        } else {
            let key = parse_dictionary_key(name, lua_manifest::expected_type(path), path)?;
            table.dict.push((key, decoded));
        }
    }

    // A pinned boundary past the end of the table is a file that disagrees with itself,
    // and the table it would build is short of the entries the boundary promised.
    if table.list.len() < list_count {
        return Err(LuaSparseFault::ListCount {
            path: path.to_string(),
        });
    }

    Ok(table)
}

fn decode_value(value: &SparseValue, path: &str, name: &str) -> Result<LuaValue, LuaSparseFault> {
    Ok(match value {
        SparseValue::Null => LuaValue::Nil,
        SparseValue::Bool(flag) => LuaValue::Bool(*flag),
        SparseValue::Int(whole) => LuaValue::Int(*whole),
        SparseValue::Float(float) => LuaValue::Float(*float),
        SparseValue::Text(text) => LuaValue::Text(text.clone()),
        SparseValue::Map(child) => LuaValue::Table(decode_map(child, &below(path, name))?),
    })
}

fn decode_grouped(
    map: &SparseMap,
    path: &str,
    grouping: &Grouping,
) -> Result<LuaTable, LuaSparseFault> {
    let mut claimed = "";
    let mut by_key: HashMap<i64, &str> = HashMap::new();
    for (name, value) in map.entries() {
        let SparseValue::Text(range) = value else {
            return Err(LuaSparseFault::NotARange {
                path: path.to_string(),
                name: name.clone(),
            });
        };

        if name == KEYS_KEY {
            claimed = range;
            continue;
        }

        for key in runs::unpack(range, path)? {
            if let Some(first) = by_key.insert(key, name) {
                return Err(LuaSparseFault::Overlap {
                    path: path.to_string(),
                    key,
                    first: first.to_string(),
                    second: name.clone(),
                });
            }
        }
    }

    let ascending = ascending_keys(claimed, path)?;
    let held: HashSet<i64> = ascending.iter().copied().collect();
    for key in by_key.keys() {
        if !held.contains(key) {
            return Err(LuaSparseFault::Ungrouped {
                path: path.to_string(),
                key: *key,
            });
        }
    }

    // THE ORDER THE KEYS COME BACK IN: the run 1, 2, 3, ... as the list part, then whatever
    // is left, ascending. The save this came from may well have had them in some other
    // order and its own split between the two parts; neither is recorded, and neither is
    // reproduced. See the module note.
    let mut listed = 0;
    while ascending.get(listed).copied() == Some(listed as i64 + 1) {
        listed += 1;
    }

    let mut table = LuaTable::default();
    for key in &ascending {
        let carried = by_key
            .get(key)
            .map_or(grouping.default_value, |value| *value);
        let child = LuaValue::Table(LuaTable {
            list: Vec::new(),
            dict: vec![(
                LuaValue::Text(grouping.child_key.to_string()),
                LuaValue::Text(carried.to_string()),
            )],
        });

        if table.list.len() < listed {
            table.list.push(child);
        } else {
            table.dict.push((lua_blob::number(*key as f64), child));
        }
    }

    Ok(table)
}

/// The keys a grouped table claims, which must climb.
///
/// The writer sorts them, so a list that does not climb is one nothing here wrote - and the
/// table it would build has its list part cut in the wrong place, or a key twice. Neither
/// says so on its own, which is why this does.
fn ascending_keys(claimed: &str, path: &str) -> Result<Vec<i64>, LuaSparseFault> {
    let mut keys = Vec::new();
    for (first, last) in runs::bounds(claimed, path)? {
        if first > last || keys.last().is_some_and(|previous| *previous >= first) {
            return Err(LuaSparseFault::KeysNotAscending {
                path: path.to_string(),
            });
        }
        runs::expand(first, last, &mut keys);
    }

    Ok(keys)
}

fn as_count(value: &SparseValue, path: &str) -> Result<usize, LuaSparseFault> {
    match value {
        SparseValue::Int(whole) if *whole >= 0 => Ok(*whole as usize),
        _ => Err(LuaSparseFault::ListCount {
            path: path.to_string(),
        }),
    }
}

/// The property name a Lua key is written as, or nothing where it has none.
fn key_name(key: &LuaValue) -> Option<String> {
    Some(match key {
        LuaValue::Text(text) => text.clone(),
        LuaValue::Int(whole) => whole.to_string(),
        LuaValue::Float(float) => float.to_string(),
        LuaValue::Bool(flag) => flag.to_string(),
        LuaValue::Nil | LuaValue::Table(_) => return None,
    })
}

/// The Lua key a property name stands for, given the type this table's keys have.
///
/// PARSING IS LOOSER THAN WRITING - `1e0` and `1` are the same number - so the result is
/// held to the one spelling this would write, or an edited key would come back changed on
/// the next round trip and the file would no longer be its own fixed point.
fn parse_dictionary_key(name: &str, kind: KeyType, path: &str) -> Result<LuaValue, LuaSparseFault> {
    let unreadable = || LuaSparseFault::Key {
        path: path.to_string(),
        name: name.to_string(),
        kind: kind.describe(),
    };

    let key = match kind {
        KeyType::Text => LuaValue::Text(name.to_string()),
        KeyType::Number => lua_blob::number(name.parse().map_err(|_| unreadable())?),
        KeyType::Boolean => LuaValue::Bool(name.parse().map_err(|_| unreadable())?),
    };

    let canonical = key_name(&key).expect("a key built here is one of the writable kinds");
    if canonical == name {
        Ok(key)
    } else {
        Err(LuaSparseFault::KeySpelling {
            path: path.to_string(),
            name: name.to_string(),
            canonical,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::lua_manifest::{SIM_STATUS_KEY, UNTOUCHED_STATUS};
    use crate::formats::sparse;

    /// A dialogue-status child, which is what a grouped table is made of.
    fn status(value: &str) -> LuaValue {
        LuaValue::Table(LuaTable {
            list: Vec::new(),
            dict: vec![(
                LuaValue::Text(SIM_STATUS_KEY.to_string()),
                LuaValue::Text(value.to_string()),
            )],
        })
    }

    fn text(value: &str) -> LuaValue {
        LuaValue::Text(value.to_string())
    }

    fn read(document: &str) -> SparseMap {
        sparse::read(document, "a test document").expect("it reads")
    }

    fn decode_ok(document: &str, path: &str) -> LuaTable {
        decode(&SparseValue::Map(read(document)), path).expect("it decodes")
    }

    fn round_trip(table: &LuaTable, path: &str) -> LuaTable {
        let encoded = encode(table, path).expect("it encodes");
        decode(&SparseValue::Map(encoded), path).expect("it decodes")
    }

    #[test]
    fn a_table_of_ordinary_entries_is_written_entry_by_entry() {
        let table = LuaTable {
            list: vec![text("first"), text("second")],
            dict: vec![
                (text("Alert"), LuaValue::Text(String::new())),
                (text("count"), LuaValue::Int(7)),
                (text("flag"), LuaValue::Bool(false)),
                (text("nothing"), LuaValue::Nil),
            ],
        };

        let written = sparse::write(&encode(&table, VARIABLE_TABLE).expect("it encodes"));

        assert_eq!(
            written,
            concat!(
                "{\n",
                "  \"1\": \"first\",\n",
                "  \"2\": \"second\",\n",
                "  \"Alert\": \"\",\n",
                "  \"count\": 7,\n",
                "  \"flag\": false,\n",
                "  \"nothing\": null\n",
                "}\n",
            ),
        );
        assert_eq!(round_trip(&table, VARIABLE_TABLE), table);
    }

    /// The boundary is not written, and the reader finds it where the names stop counting.
    #[test]
    fn the_list_boundary_is_worked_out_rather_than_recorded() {
        let table = decode_ok(r#"{"1": "a", "2": "b", "x": "c"}"#, VARIABLE_TABLE);

        assert_eq!(table.list, vec![text("a"), text("b")]);
        assert_eq!(table.dict, vec![(text("x"), text("c"))]);
    }

    /// A name that is not the next index ends the list part, even when it is a number.
    #[test]
    fn a_number_out_of_sequence_is_a_dictionary_key() {
        let table = decode_ok(r#"{"1": "a", "3": "b"}"#, VARIABLE_TABLE);

        assert_eq!(table.list, vec![text("a")]);
        assert_eq!(table.dict, vec![(text("3"), text("b"))]);
    }

    /// A hand-written file may pin the split, and then the names have to agree with it.
    #[test]
    fn a_pinned_boundary_is_honoured_and_held_to_the_names() {
        let table = decode_ok(
            r#"{"_num_list_entries": 1, "1": "a", "2": "b"}"#,
            VARIABLE_TABLE,
        );

        assert_eq!(table.list, vec![text("a")]);
        assert_eq!(table.dict, vec![(text("2"), text("b"))]);

        let refused = decode(
            &SparseValue::Map(read(r#"{"_num_list_entries": 2, "1": "a", "x": "b"}"#)),
            VARIABLE_TABLE,
        )
        .expect_err("refused");
        assert!(
            matches!(refused, LuaSparseFault::ListName { .. }),
            "{refused}"
        );
    }

    #[test]
    fn a_boundary_longer_than_the_table_is_refused() {
        let refused = decode(
            &SparseValue::Map(read(r#"{"_num_list_entries": 3, "1": "a"}"#)),
            VARIABLE_TABLE,
        )
        .expect_err("refused");

        assert!(
            matches!(refused, LuaSparseFault::ListCount { .. }),
            "{refused}"
        );
    }

    /// The two header fields lead a document and are the caller's business, not a key.
    #[test]
    fn the_header_is_passed_over_where_it_belongs_and_refused_where_it_does_not() {
        let table = decode_ok(
            r#"{"_format": "sparse", "_formatVersion": 1, "Alert": ""}"#,
            VARIABLE_TABLE,
        );
        assert_eq!(table.dict.len(), 1);

        let refused = decode(
            &SparseValue::Map(read(r#"{"Alert": "", "_format": "sparse"}"#)),
            VARIABLE_TABLE,
        )
        .expect_err("refused");
        assert!(
            matches!(refused, LuaSparseFault::Reserved { .. }),
            "{refused}"
        );
    }

    /// Nothing writes `_reorder`, and a file that did recorded something worth not losing.
    #[test]
    fn a_retired_bookkeeping_name_is_refused_rather_than_ignored() {
        let refused = decode(
            &SparseValue::Map(read(r#"{"_reorder": "0-3"}"#)),
            VARIABLE_TABLE,
        )
        .expect_err("refused");

        assert!(
            matches!(refused, LuaSparseFault::Reserved { .. }),
            "{refused}"
        );
    }

    #[test]
    fn a_table_named_twice_is_refused() {
        let mut map = SparseMap::new();
        map.add("Alert", SparseValue::Text(String::new()));
        map.add("Alert", SparseValue::Int(1));

        let refused = decode(&SparseValue::Map(map), VARIABLE_TABLE).expect_err("refused");

        assert!(
            matches!(refused, LuaSparseFault::Duplicate { .. }),
            "{refused}"
        );
    }

    #[test]
    fn a_root_that_is_not_an_object_is_refused() {
        let refused = decode(&SparseValue::Int(4), VARIABLE_TABLE).expect_err("refused");

        assert!(refused.to_string().contains(VARIABLE_TABLE), "{refused}");
    }

    /// The dialogue map is 94% one value, and this is what makes a save affordable.
    #[test]
    fn a_dialogue_map_is_written_as_one_key_list_per_status() {
        let table = LuaTable {
            list: Vec::new(),
            dict: vec![
                (LuaValue::Int(0), status(UNTOUCHED_STATUS)),
                (LuaValue::Int(1), status("WasDisplayed")),
                (LuaValue::Int(2), status(UNTOUCHED_STATUS)),
                (LuaValue::Int(3), status("WasDisplayed")),
                (LuaValue::Int(4), status("WasOffered")),
            ],
        };
        let path = "Conversation/631/Dialog";

        let written = sparse::write(&encode(&table, path).expect("it encodes"));

        assert_eq!(
            written,
            concat!(
                "{\n",
                "  \"_keys\": \"0-4\",\n",
                "  \"WasDisplayed\": \"1,3\",\n",
                "  \"WasOffered\": \"4\"\n",
                "}\n",
            ),
        );
        assert_eq!(round_trip(&table, path), table);
    }

    /// A table the manifest names, which does not have the shape it claims, still writes.
    #[test]
    fn a_table_that_is_not_the_shape_its_rule_claims_is_written_entry_by_entry() {
        let path = "Conversation/631/Dialog";
        for odd in [
            LuaValue::Text("not a table".to_string()),
            LuaValue::Table(LuaTable {
                list: Vec::new(),
                dict: vec![(text("Something Else"), text(UNTOUCHED_STATUS))],
            }),
            LuaValue::Table(LuaTable {
                list: Vec::new(),
                dict: vec![
                    (text(SIM_STATUS_KEY), text("WasDisplayed")),
                    (text("extra"), LuaValue::Int(1)),
                ],
            }),
        ] {
            let table = LuaTable {
                list: Vec::new(),
                dict: vec![(LuaValue::Int(0), odd.clone())],
            };

            let encoded = encode(&table, path).expect("it encodes");
            assert!(!encoded.has(KEYS_KEY), "{odd} was grouped");
            assert_eq!(round_trip(&table, path), table, "{odd}");
        }
    }

    /// A status that would be read back as bookkeeping is not allowed to become a name.
    #[test]
    fn a_status_named_after_this_formats_own_bookkeeping_is_not_grouped() {
        let table = LuaTable {
            list: Vec::new(),
            dict: vec![(LuaValue::Int(0), status(KEYS_KEY))],
        };

        let encoded = encode(&table, "Conversation/631/Dialog").expect("it encodes");

        assert_eq!(encoded.entries().len(), 1, "it was written entry by entry");
        assert!(encoded.has("0"), "under its own key");
    }

    /// The grouped form does not record the split either, and the reader picks the run.
    #[test]
    fn a_grouped_table_whose_keys_start_at_one_comes_back_as_a_list() {
        let table = decode_ok(r#"{"_keys": "1-3,7"}"#, "Conversation/631/Dialog");

        assert_eq!(table.list.len(), 3);
        assert_eq!(table.dict.len(), 1);
        assert_eq!(table.dict[0].0, LuaValue::Int(7));
        assert_eq!(table.dict[0].1, status(UNTOUCHED_STATUS));
    }

    #[test]
    fn a_grouped_table_whose_keys_start_at_zero_is_all_dictionary() {
        let table = decode_ok(r#"{"_keys": "0-2"}"#, "Conversation/631/Dialog");

        assert!(table.list.is_empty());
        assert_eq!(table.dict.len(), 3);
    }

    #[test]
    fn a_grouped_table_that_contradicts_itself_is_refused() {
        let path = "Conversation/631/Dialog";
        let refused =
            |document: &str| decode(&SparseValue::Map(read(document)), path).expect_err("refused");

        assert!(matches!(
            refused(r#"{"_keys": "0-2", "A": "1", "B": "1"}"#),
            LuaSparseFault::Overlap { key: 1, .. },
        ));
        assert!(matches!(
            refused(r#"{"_keys": "0-2", "A": "5"}"#),
            LuaSparseFault::Ungrouped { key: 5, .. },
        ));
        assert!(matches!(
            refused(r#"{"_keys": "2-0"}"#),
            LuaSparseFault::KeysNotAscending { .. },
        ));
        assert!(matches!(
            refused(r#"{"_keys": "0-2,1"}"#),
            LuaSparseFault::KeysNotAscending { .. },
        ));
        assert!(matches!(
            refused(r#"{"_keys": "0-x"}"#),
            LuaSparseFault::Range(_),
        ));
        assert!(matches!(
            refused(r#"{"_keys": "0-2", "A": 5}"#),
            LuaSparseFault::NotARange { .. },
        ));
    }

    /// A dialogue key is a number, and it has to come back one rather than as its digits.
    #[test]
    fn a_key_comes_back_the_type_the_manifest_says_it_is() {
        let path = "Conversation/631/Dialog";
        let table = decode_ok(r#"{"0": {"SimStatus": "Untouched"}}"#, path);

        assert_eq!(table.dict[0].0, LuaValue::Int(0));

        let elsewhere = decode_ok(r#"{"0": "a"}"#, VARIABLE_TABLE);
        assert_eq!(elsewhere.dict[0].0, text("0"));
    }

    /// Parsing is looser than writing, so a key that would come back changed is refused.
    #[test]
    fn a_key_spelled_a_way_this_would_not_write_is_refused() {
        let path = "Conversation/631/Dialog";
        for spelling in ["1.0", "1e0", " 1", "+1"] {
            let mut map = SparseMap::new();
            map.add(spelling, SparseValue::Map(SparseMap::new()));

            let refused = decode(&SparseValue::Map(map), path).expect_err("refused");
            assert!(
                matches!(
                    refused,
                    LuaSparseFault::KeySpelling { .. } | LuaSparseFault::Key { .. },
                ),
                "'{spelling}': {refused}",
            );
        }
    }

    /// Rebuilding what the header names is the next ticket, and half-reading it is a save
    /// that loads and is wrong.
    #[test]
    fn a_table_that_leaves_out_its_derived_variables_is_refused() {
        let refused = decode(
            &SparseValue::Map(read(
                r#"{"_derived_simx": {"_conversations": "1-3"}, "Alert": ""}"#,
            )),
            VARIABLE_TABLE,
        )
        .expect_err("refused");

        assert!(
            matches!(refused, LuaSparseFault::NeedsSimX { .. }),
            "{refused}"
        );
    }

    /// Nested tables carry the path down, which is what the manifests match against.
    ///
    /// The conversations sit in the table's LIST part, which is where a save holds them:
    /// their ids are a complete run from 1, so the blob stores none of them as a key.
    #[test]
    fn a_nested_dialogue_map_is_found_by_its_path() {
        let conversations = LuaTable {
            list: vec![LuaValue::Table(LuaTable {
                list: Vec::new(),
                dict: vec![(
                    text("Dialog"),
                    LuaValue::Table(LuaTable {
                        list: Vec::new(),
                        dict: vec![(LuaValue::Int(0), status("WasDisplayed"))],
                    }),
                )],
            })],
            dict: Vec::new(),
        };

        let encoded = encode(&conversations, CONVERSATION_TABLE).expect("it encodes");

        assert!(
            sparse::write(&encoded).contains("\"_keys\": \"0\""),
            "{}",
            sparse::write(&encoded),
        );
        assert_eq!(
            round_trip(&conversations, CONVERSATION_TABLE),
            conversations
        );
    }

    /// A key that has no spelling as a property name is a table this cannot write.
    #[test]
    fn a_table_key_with_no_name_is_refused() {
        let table = LuaTable {
            list: Vec::new(),
            dict: vec![(LuaValue::Table(LuaTable::default()), LuaValue::Int(1))],
        };

        let refused = encode(&table, VARIABLE_TABLE).expect_err("refused");

        assert!(
            matches!(refused, LuaSparseFault::UnwritableKey { .. }),
            "{refused}"
        );
    }
}
