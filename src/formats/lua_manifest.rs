// SPDX-License-Identifier: MIT
//! What the sparse form knows about the save's own data, and where it knows it.
//!
//! Two claims, both about tables named by PATH - `Conversation/*/Dialog`, where an asterisk
//! stands for one segment:
//!
//! - which tables can be written in a GROUPED shape, as one key list per distinct value
//!   rather than one object per key;
//! - which tables have dictionary keys that are not strings, since a JSON property name is
//!   always one and a number that went in has to come back a number.
//!
//! EVERY RULE HERE IS CHECKED BEFORE IT IS USED. A table that does not have the shape its
//! grouping claims is written out entry by entry instead, so a rule that goes stale costs
//! output size and never accuracy. The key-type rule is the other way round - it is a claim
//! the reader cannot verify from the file alone, so a wrong one is a wrong key, and the
//! canonical-spelling check in [`super::lua_sparse`] is what catches it.

/// A table whose children all hold one value under the same key.
///
/// Such a table can be written as one list of keys per distinct value instead of one object
/// per key, which is what makes a save's dialogue statuses affordable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grouping {
    /// The table's path pattern, where an asterisk stands for one segment.
    pub path: &'static str,
    /// The single key every child table holds.
    pub child_key: &'static str,
    /// The value carried by the great majority of children, which the grouped form leaves
    /// out entirely and the reader puts back.
    pub default_value: &'static str,
}

/// The dialogue-status map of a conversation.
///
/// In the template save this is 112,940 tables of exactly `{"SimStatus": ...}`, 94% of them
/// "Untouched" - about 6 MB of the 10.8 MB the entry-by-entry form takes.
pub const DIALOG_PATH: &str = "Conversation/*/Dialog";

/// The key every dialogue entry holds.
pub const SIM_STATUS_KEY: &str = "SimStatus";

/// The status an untouched dialogue entry carries.
pub const UNTOUCHED_STATUS: &str = "Untouched";

/// Tables written as one key list per distinct value.
pub const GROUPINGS: &[Grouping] = &[Grouping {
    path: DIALOG_PATH,
    child_key: SIM_STATUS_KEY,
    default_value: UNTOUCHED_STATUS,
}];

/// The grouping rule for a table path, or nothing where there is none.
#[must_use]
pub fn grouping_for(path: &str) -> Option<&'static Grouping> {
    GROUPINGS
        .iter()
        .find(|grouping| matches(grouping.path, path))
}

/// The wire type shared by every dictionary key in one Lua table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    /// UTF-8 string keys, which is every table not named below.
    Text,
    /// IEEE-754 double keys.
    Number,
    /// Boolean keys.
    Boolean,
}

impl KeyType {
    /// What a key of this type is called in a refusal.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Text => "string",
            Self::Number => "number",
            Self::Boolean => "boolean",
        }
    }
}

/// Table paths whose dictionary keys are not strings.
///
/// THE CONVERSATION TABLE IS NOT ONE OF THEM, which looks wrong and is not: its ids are a
/// complete run from 1, so a save holds every conversation in the table's LIST part and
/// stores no key for any of them. The list part's keys are its own indices and never pass
/// through a name. A save that did hold a conversation in the dictionary part would bring
/// its id back as the digits of one, which is the day this gains a second row.
pub const KEY_TYPES: &[(&str, KeyType)] = &[(DIALOG_PATH, KeyType::Number)];

/// The dictionary-key type a table path holds.
#[must_use]
pub fn expected_type(path: &str) -> KeyType {
    KEY_TYPES
        .iter()
        .find(|(pattern, _)| matches(pattern, path))
        .map_or(KeyType::Text, |(_, kind)| *kind)
}

/// Whether a table path matches a pattern, where an asterisk stands for one segment.
///
/// Shared by both claims above, so they name the same tables the same way.
#[must_use]
pub fn matches(pattern: &str, path: &str) -> bool {
    let mut expected = pattern.split('/');
    let mut actual = path.split('/');
    loop {
        match (expected.next(), actual.next()) {
            (None, None) => return true,
            (Some(want), Some(have)) if want == "*" || want == have => {}
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_asterisk_stands_for_exactly_one_segment() {
        assert!(matches(DIALOG_PATH, "Conversation/631/Dialog"));
        assert!(matches(DIALOG_PATH, "Conversation/anything at all/Dialog"));
        assert!(!matches(DIALOG_PATH, "Conversation/631/Dialog/4"));
        assert!(!matches(DIALOG_PATH, "Conversation/Dialog"));
        assert!(!matches(DIALOG_PATH, "Variable/631/Dialog"));
    }

    /// An empty segment is still a segment, so a stray slash does not match a wildcard away.
    #[test]
    fn an_empty_segment_does_not_vanish() {
        assert!(matches(DIALOG_PATH, "Conversation//Dialog"));
        assert!(!matches(DIALOG_PATH, "Conversation/631//Dialog"));
    }

    #[test]
    fn only_the_dialogue_map_is_grouped_and_only_it_has_number_keys() {
        assert_eq!(
            grouping_for("Conversation/631/Dialog").map(|rule| rule.child_key),
            Some(SIM_STATUS_KEY),
        );
        assert_eq!(grouping_for("Conversation/631"), None);

        assert_eq!(expected_type("Conversation/631/Dialog"), KeyType::Number);
        assert_eq!(expected_type("Conversation/631"), KeyType::Text);
        assert_eq!(expected_type("Variable"), KeyType::Text);
    }

    /// The two claims are about the same tables, and a mismatch would be silent.
    #[test]
    fn every_grouped_table_says_what_its_keys_are() {
        for grouping in GROUPINGS {
            assert!(
                KEY_TYPES
                    .iter()
                    .any(|(pattern, _)| *pattern == grouping.path),
                "{} is grouped and names no key type",
                grouping.path,
            );
        }
    }
}
