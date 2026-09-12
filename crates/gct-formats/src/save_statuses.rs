// SPDX-License-Identifier: MIT
//! What a save says it has already shown, read out of the blob the game wrote.
//!
//! ## The one thing anybody asks a save
//!
//! Every reader of a save in this repository wants the same thing out of it: which dialogue
//! entries it has offered and displayed. The mod asks on every save load, to refill the
//! dialogue half of the current playthrough; the union check asks to hold the state file
//! against real saves. Both used to have their own walk of the same tables, in two
//! languages, so this is the walk.
//!
//! ## Why the answer is a [`GlobalState`]
//!
//! Because that is what the answer IS. A save's statuses and the mod's own record of them
//! are the same kind of fact - a status per entry, merged upward - and the state type
//! already drops `Untouched`, which is exactly right: a real save holds around 113,000
//! entries of which about 1,500 are above it, and nothing downstream wants the rest.
//!
//! ## What it costs, measured rather than assumed
//!
//! Reading the blob builds the whole table, which is 30 MB held and 62 ms over a real 8.1 MB
//! save - see `crates/gct-state-check/examples/save_read_cost.rs`. The alternative is a
//! streaming walk that never builds one, which is what the C# this replaces did, and which
//! would be a SECOND reader of the blob format beside [`super::lua_blob`]. A format read
//! twice is what this crate exists to end, and the cost lands on a save load, where the
//! player is already waiting seconds.

use super::global_state::{GlobalState, Status};
use super::lua_blob::{self, LuaTable, LuaValue, TABLE_NAMES};
use super::lua_simx::CONVERSATION_TABLE;

/// The per-conversation field holding the dialogue entry map.
pub const DIALOG_FIELD: &str = "Dialog";

/// The per-entry field holding the status name.
pub const SIM_STATUS_FIELD: &str = "SimStatus";

/// Why a save's statuses could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StatusFault {
    /// The blob will not read.
    #[error("{0}")]
    Blob(#[from] lua_blob::BlobFault),
    /// It holds something other than a table where the conversations should be.
    #[error("a save holds no {CONVERSATION_TABLE} table")]
    NoConversations,
    /// An entry names a status this build does not know.
    ///
    /// REFUSED RATHER THAN SKIPPED. An unknown status means an assumption about the save
    /// format is wrong, and dropping it would quietly leave history out of whatever is
    /// built from this.
    #[error("conversation {0}, entry {1} has status '{2}', which is not one")]
    UnknownStatus(i32, i32, String),
}

/// Where the conversations sit among a blob's five tables.
#[must_use]
pub fn at_conversations() -> usize {
    TABLE_NAMES
        .iter()
        .position(|name| *name == CONVERSATION_TABLE)
        .expect("the conversations are one of the five")
}

/// Every dialogue status the blob the game wrote records.
///
/// # Errors
///
/// Where the blob will not read, holds no conversations, or names a status this build does
/// not know.
pub fn in_blob(blob: &[u8]) -> Result<GlobalState, StatusFault> {
    let read = lua_blob::read(blob)?;
    let at = at_conversations();
    let Some(LuaValue::Table(conversations)) = read.tables.get(at) else {
        return Err(StatusFault::NoConversations);
    };

    in_conversations(conversations)
}

/// The same, given the `Conversation` table itself.
///
/// SEPARATE FROM [`in_blob`] because an expanded save does not have a blob: its tables are
/// already trees on disk, and decoding the one that matters is cheaper than rebuilding the
/// binary form to read it back.
///
/// # Errors
///
/// Where an entry names a status this build does not know.
pub fn in_conversations(conversations: &LuaTable) -> Result<GlobalState, StatusFault> {
    let mut state = GlobalState::new();

    for (conversation, value) in numbered(conversations) {
        let LuaValue::Table(held) = value else {
            continue;
        };
        let Some(LuaValue::Table(dialog)) = field(held, DIALOG_FIELD) else {
            continue;
        };

        for (entry, value) in numbered(dialog) {
            let LuaValue::Table(held) = value else {
                continue;
            };
            let Some(LuaValue::Text(named)) = field(held, SIM_STATUS_FIELD) else {
                continue;
            };

            let status = Status::parse(named)
                .ok_or_else(|| StatusFault::UnknownStatus(conversation, entry, named.clone()))?;
            state.merge(conversation, entry, status);
        }
    }

    Ok(state)
}

/// Every entry of a table whose key is an id, from BOTH HALVES of it.
///
/// A Lua table keeps a list part whose keys are its own 1-based indices and are not stored,
/// and a dictionary part that is. Which half a conversation landed in is the blob's business
/// and not this one's, so both are walked - reading only the dictionary would read a save
/// short with nothing saying so.
fn numbered(table: &LuaTable) -> Vec<(i32, &LuaValue)> {
    let mut found: Vec<(i32, &LuaValue)> = table
        .list
        .iter()
        .enumerate()
        .filter_map(|(at, value)| i32::try_from(at + 1).ok().map(|id| (id, value)))
        .collect();

    found.extend(
        table
            .dict
            .iter()
            .filter_map(|(key, value)| id_of(key).map(|id| (id, value))),
    );

    found
}

/// One named field of a table, which is a dictionary key spelled as a string.
fn field<'a>(table: &'a LuaTable, name: &str) -> Option<&'a LuaValue> {
    table.get(&LuaValue::Text(name.to_string()))
}

/// A key as the id it may be, whether the blob wrote it as a number or as text.
fn id_of(key: &LuaValue) -> Option<i32> {
    match key {
        LuaValue::Int(whole) => Some(*whole),
        LuaValue::Text(text) => text.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table with a dictionary half only, which is how a save writes its conversations.
    fn keyed(entries: Vec<(LuaValue, LuaValue)>) -> LuaTable {
        LuaTable {
            list: Vec::new(),
            dict: entries,
        }
    }

    fn text(what: &str) -> LuaValue {
        LuaValue::Text(what.to_string())
    }

    /// One conversation holding one entry at one status.
    fn one(conversation: &str, entry: &str, status: &str) -> LuaTable {
        keyed(vec![(
            text(conversation),
            LuaValue::Table(keyed(vec![(
                text(DIALOG_FIELD),
                LuaValue::Table(keyed(vec![(
                    text(entry),
                    LuaValue::Table(keyed(vec![(text(SIM_STATUS_FIELD), text(status))])),
                )])),
            )])),
        )])
    }

    #[test]
    fn a_conversation_with_a_displayed_entry_reads_as_one() {
        let state = in_conversations(&one("10", "5", "WasDisplayed")).expect("it reads");

        assert_eq!(state.status_of(10, 5), Status::WasDisplayed);
        assert_eq!(state.len(), 1);
    }

    /// The state never records `Untouched`, which is most of a real save.
    #[test]
    fn an_untouched_entry_is_not_recorded() {
        let state = in_conversations(&one("10", "5", "Untouched")).expect("it reads");

        assert_eq!(state.len(), 0);
    }

    /// A status nothing knows means an assumption is wrong, so nothing is read.
    #[test]
    fn a_status_this_build_does_not_know_stops_the_read() {
        let why = in_conversations(&one("10", "5", "WasEaten")).expect_err("refused");

        assert!(matches!(why, StatusFault::UnknownStatus(10, 5, _)), "{why}");
    }

    /// The list half's keys are its own indices and are not stored beside it.
    #[test]
    fn a_conversation_in_the_list_half_is_found_by_its_index() {
        let dialog = LuaValue::Table(keyed(vec![(
            text(DIALOG_FIELD),
            LuaValue::Table(keyed(vec![(
                text("7"),
                LuaValue::Table(keyed(vec![(text(SIM_STATUS_FIELD), text("WasOffered"))])),
            )])),
        )]));
        let conversations = LuaTable {
            list: vec![dialog],
            dict: Vec::new(),
        };

        let state = in_conversations(&conversations).expect("it reads");

        assert_eq!(state.status_of(1, 7), Status::WasOffered);
    }

    /// A key that is not an id is not a conversation, and is passed over rather than read.
    #[test]
    fn a_key_that_is_not_an_id_is_not_a_conversation() {
        let odd = keyed(vec![(
            text("notanid"),
            LuaValue::Table(LuaTable::default()),
        )]);

        assert_eq!(in_conversations(&odd).expect("it reads").len(), 0);
    }

    /// An entry with no status says nothing, which is not the same as saying Untouched.
    #[test]
    fn an_entry_with_no_status_field_is_passed_over() {
        let held = keyed(vec![(
            text("10"),
            LuaValue::Table(keyed(vec![(
                text(DIALOG_FIELD),
                LuaValue::Table(keyed(vec![(
                    text("5"),
                    LuaValue::Table(LuaTable::default()),
                )])),
            )])),
        )]);

        assert_eq!(in_conversations(&held).expect("it reads").len(), 0);
    }

    /// A blob that is not one says so rather than reading as an empty save.
    #[test]
    fn a_blob_that_will_not_read_is_refused() {
        assert!(matches!(in_blob(b"not a blob"), Err(StatusFault::Blob(_))));
    }
}
