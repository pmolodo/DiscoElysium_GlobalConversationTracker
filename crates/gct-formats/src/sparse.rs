// SPDX-License-Identifier: MIT
//! The sparse representation of a save's Lua tables, read and written.
//!
//! ## Why this is not a Lua table, and not a `serde_json::Value` either
//!
//! The sparse form's property names are as often BOOKKEEPING as they are Lua keys - a key
//! range, a status name, a diff's `_changes`. So it does not get to be a Lua table, whose
//! invariants are what the dense form means.
//!
//! It is not a [`serde_json::Value`] because two of that type's properties are wrong here.
//! ORDER IS PART OF THE DOCUMENT: these files are read by a person and compared by a diff,
//! and a writer that sorted the keys would rewrite every one of them the first time it ran.
//! `serde_json`'s map sorts unless a crate-wide feature says otherwise, and turning that
//! feature on would change the ordering of every other JSON this crate writes.
//!
//! AND THERE ARE NO ARRAYS. The form has none, so one arriving means the file is not what
//! it claims to be, and saying so beats carrying a variant that can never be written.
//!
//! ## What a number is
//!
//! A whole number that fits in 32 bits is an INTEGER, so an id reads as `1` rather than
//! `1.0`; anything else stays a float. That is not cosmetic - it decides equality, and
//! equality is what a diff is made of. Normalising on the way IN means a document that
//! spelled an id `1.0` and one that spelled it `1` compare equal, which is what they mean.
//!
//! NEGATIVE ZERO IS THE EXCEPTION and stays a float: it is whole and it fits, but as an
//! integer it would be written back as `0` and the sign would be gone.

use std::fmt;

/// One value in the sparse tree.
#[derive(Debug, Clone, PartialEq)]
pub enum SparseValue {
    Null,
    Bool(bool),
    /// A whole number that fits in 32 bits. See the module note on why it is its own case.
    Int(i32),
    /// Everything else numeric.
    Float(f64),
    Text(String),
    Map(SparseMap),
}

/// An ordered JSON object in the sparse representation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SparseMap {
    entries: Vec<(String, SparseValue)>,
}

impl SparseMap {
    /// An empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a property.
    ///
    /// Appends rather than replaces, matching the writer this is a port of: a document
    /// with a repeated key is not something either side produces, and the last one read
    /// wins on the way in.
    pub fn add(&mut self, name: impl Into<String>, value: SparseValue) {
        self.entries.push((name.into(), value));
    }

    /// The value of a property, or nothing where it is absent.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&SparseValue> {
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    /// Whether a property is present.
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// The properties, in the order they will be written.
    #[must_use]
    pub fn entries(&self) -> &[(String, SparseValue)] {
        &self.entries
    }

    /// How many properties it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Why a document could not be read as a sparse tree.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SparseFault {
    /// It is not JSON, or it holds something the sparse form has no case for.
    #[error("{0} could not be read: {1}")]
    Unreadable(String, String),
    /// Its root is not an object.
    #[error("{0} is not a JSON object")]
    NotAnObject(String),
}

/// What an array is reported as, since the form has none.
const NO_ARRAYS: &str = "the sparse form has no arrays";

impl<'de> serde::Deserialize<'de> for SparseValue {
    /// Reads one value, IN DOCUMENT ORDER for an object.
    ///
    /// Deserialising straight from the parser rather than by way of a
    /// [`serde_json::Value`] is the whole reason this exists: that type's map sorts its
    /// keys unless a crate-wide feature says otherwise, so going through it would throw
    /// away the order before this ever saw it - and turning the feature on would change
    /// every other JSON this crate writes.
    fn deserialize<D: serde::Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        reader.deserialize_any(SparseVisitor)
    }
}

struct SparseVisitor;

impl<'de> serde::de::Visitor<'de> for SparseVisitor {
    type Value = SparseValue;

    fn expecting(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str("a null, a boolean, a number, a string or an object")
    }

    fn visit_unit<E>(self) -> Result<SparseValue, E> {
        Ok(SparseValue::Null)
    }

    fn visit_none<E>(self) -> Result<SparseValue, E> {
        Ok(SparseValue::Null)
    }

    fn visit_bool<E>(self, flag: bool) -> Result<SparseValue, E> {
        Ok(SparseValue::Bool(flag))
    }

    fn visit_i64<E>(self, whole: i64) -> Result<SparseValue, E> {
        Ok(whole_or_float(whole as f64))
    }

    fn visit_u64<E>(self, whole: u64) -> Result<SparseValue, E> {
        Ok(whole_or_float(whole as f64))
    }

    fn visit_f64<E>(self, float: f64) -> Result<SparseValue, E> {
        Ok(whole_or_float(float))
    }

    fn visit_str<E>(self, text: &str) -> Result<SparseValue, E> {
        Ok(SparseValue::Text(text.to_string()))
    }

    fn visit_string<E>(self, text: String) -> Result<SparseValue, E> {
        Ok(SparseValue::Text(text))
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, _: A) -> Result<SparseValue, A::Error> {
        Err(serde::de::Error::custom(NO_ARRAYS))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(
        self,
        mut entries: A,
    ) -> Result<SparseValue, A::Error> {
        let mut map = SparseMap::new();
        while let Some((name, value)) = entries.next_entry::<String, SparseValue>()? {
            map.add(name, value);
        }

        Ok(SparseValue::Map(map))
    }
}

/// A number, whole and 32-bit-sized where it can be. See the module note.
fn whole_or_float(float: f64) -> SparseValue {
    if float.fract() == 0.0
        && float >= f64::from(i32::MIN)
        && float <= f64::from(i32::MAX)
        && !is_negative_zero(float)
    {
        #[allow(clippy::cast_possible_truncation)]
        return SparseValue::Int(float as i32);
    }

    SparseValue::Float(float)
}

/// True for -0.0, which ordinary comparison cannot tell from 0.0.
fn is_negative_zero(value: f64) -> bool {
    value == 0.0 && value.is_sign_negative()
}

/// Reads a sparse tree from UTF-8 JSON.
///
/// `context` names the document in any fault, since a caller reading forty of them needs
/// to know which one.
///
/// # Errors
///
/// Where the text is not JSON, where it holds an array - which the sparse form has no case
/// for - or where its root is not an object.
pub fn read(text: &str, context: &str) -> Result<SparseMap, SparseFault> {
    let value: SparseValue = serde_json::from_str(text)
        .map_err(|error| SparseFault::Unreadable(context.to_string(), error.to_string()))?;

    match value {
        SparseValue::Map(map) => Ok(map),
        _ => Err(SparseFault::NotAnObject(context.to_string())),
    }
}

/// How far one level of nesting is indented.
const INDENT: usize = 2;

/// Writes a sparse tree as UTF-8 JSON, indented, with the trailing newline it is stored
/// with.
///
/// ## What this escapes, and it is as little as possible
///
/// A quote and a backslash, because JSON has no other way to carry them, and the control
/// characters, because JSON forbids them raw. NOTHING ELSE - not an apostrophe, not an
/// angle bracket, and not a single non-ASCII character, all of which are written as
/// themselves.
///
/// The committed diffs disagree with that, and deliberately: the 19 written by .NET's
/// default encoder carry 493 escapes, mixed hex casing and all, because that encoder is
/// built for pasting JSON into HTML. Nothing here does that, the escapes make the files
/// harder to read, and de-xz48.4 rewrites every one of them anyway.
#[must_use]
pub fn write(map: &SparseMap) -> String {
    let mut text = String::new();
    write_map(&mut text, map, 0);
    text.push('\n');
    text
}

fn write_map(out: &mut String, map: &SparseMap, depth: usize) {
    if map.is_empty() {
        out.push_str("{}");
        return;
    }

    out.push_str("{\n");
    for (at, (name, value)) in map.entries.iter().enumerate() {
        indent(out, depth + 1);
        write_text(out, name);
        out.push_str(": ");
        write_value(out, value, depth + 1);
        if at + 1 < map.entries.len() {
            out.push(',');
        }
        out.push('\n');
    }

    indent(out, depth);
    out.push('}');
}

fn write_value(out: &mut String, value: &SparseValue, depth: usize) {
    match value {
        SparseValue::Null => out.push_str("null"),
        SparseValue::Bool(true) => out.push_str("true"),
        SparseValue::Bool(false) => out.push_str("false"),
        SparseValue::Int(whole) => out.push_str(&whole.to_string()),
        SparseValue::Float(float) => match serde_json::Number::from_f64(*float) {
            // Shortest round-trip, and valid JSON, which is what serde_json's own writer
            // would produce for the same value.
            Some(number) => out.push_str(&number.to_string()),
            // JSON has no infinity and no NaN. Null is what every JSON writer that meets
            // one does, and it is what the C# writer refuses over - refusing here would
            // mean a save the mod cannot read rather than one it reads approximately.
            None => out.push_str("null"),
        },
        SparseValue::Text(text) => write_text(out, text),
        SparseValue::Map(map) => write_map(out, map, depth),
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth * INDENT {
        out.push(' ');
    }
}

fn write_text(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            control if control < ' ' => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

impl fmt::Display for SparseMap {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(&write(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_ok(text: &str) -> SparseMap {
        read(text, "a test document").expect("it reads")
    }

    #[test]
    fn a_document_keeps_the_order_it_was_written_in() {
        // Reverse alphabetical, so a writer that sorted would be visible.
        let text = "{\n  \"zebra\": 1,\n  \"mongoose\": 2,\n  \"dolphin\": 3\n}\n";

        let map = read_ok(text);

        assert_eq!(
            map.entries()
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            vec!["zebra", "mongoose", "dolphin"],
        );
        assert_eq!(write(&map), text);
    }

    #[test]
    fn every_value_the_form_has_survives_a_round_trip() {
        let text = concat!(
            "{\n",
            "  \"nothing\": null,\n",
            "  \"yes\": true,\n",
            "  \"no\": false,\n",
            "  \"whole\": 42,\n",
            "  \"negative\": -7,\n",
            "  \"fractional\": 2.5,\n",
            "  \"text\": \"blue\",\n",
            "  \"nested\": {\n",
            "    \"deeper\": {\n",
            "      \"still\": 1\n",
            "    }\n",
            "  },\n",
            "  \"empty\": {}\n",
            "}\n",
        );

        assert_eq!(write(&read_ok(text)), text);
    }

    /// A whole number reads as one however it was spelled, because equality turns on it.
    #[test]
    fn a_whole_number_is_an_integer_however_it_was_written() {
        let map = read_ok("{\"a\": 1, \"b\": 1.0, \"c\": 1e0}");

        assert_eq!(map.find("a"), Some(&SparseValue::Int(1)));
        assert_eq!(map.find("b"), Some(&SparseValue::Int(1)));
        assert_eq!(map.find("c"), Some(&SparseValue::Int(1)));
    }

    /// Negative zero is whole and it fits, and it still cannot be an integer.
    #[test]
    fn negative_zero_stays_a_float_so_its_sign_survives() {
        let map = read_ok("{\"below\": -0.0}");

        assert_eq!(map.find("below"), Some(&SparseValue::Float(-0.0)));
        assert!(write(&map).contains("-0.0"));
    }

    /// Beyond 32 bits it is not the kind of number an id is.
    #[test]
    fn a_number_too_large_for_an_id_stays_a_float() {
        let map = read_ok("{\"huge\": 4294967296}");

        assert_eq!(map.find("huge"), Some(&SparseValue::Float(4_294_967_296.0)));
    }

    /// As little escaping as JSON allows, which is what keeps these files readable.
    #[test]
    fn only_what_json_forbids_raw_is_escaped() {
        let mut map = SparseMap::new();
        map.add("quote", SparseValue::Text("say \"this\"".to_string()));
        map.add("apostrophe", SparseValue::Text("don't".to_string()));
        map.add("angle", SparseValue::Text("a > b".to_string()));
        map.add("accent", SparseValue::Text("émigré".to_string()));
        map.add("newline", SparseValue::Text("one\ntwo".to_string()));
        map.add("backslash", SparseValue::Text("a\\b".to_string()));

        let written = write(&map);

        assert!(written.contains(r#""say \"this\"""#), "{written}");
        assert!(written.contains("don't"), "an apostrophe is not escaped");
        assert!(written.contains("a > b"), "an angle bracket is not escaped");
        assert!(written.contains("émigré"), "non-ASCII is written as itself");
        assert!(written.contains(r#""one\ntwo""#), "{written}");
        assert!(written.contains(r#""a\\b""#), "{written}");
        assert_eq!(write(&read_ok(&written)), written, "and it reads back");
    }

    /// A control character has no raw spelling in JSON, so it gets the numeric one.
    #[test]
    fn a_control_character_is_escaped_because_json_forbids_it_raw() {
        let mut map = SparseMap::new();
        map.add("bell", SparseValue::Text("a\u{7}b".to_string()));

        let written = write(&map);

        assert!(written.contains(r"\u0007"), "{written}");
        assert_eq!(write(&read_ok(&written)), written);
    }

    /// The form has no arrays, so one is a document that is not what it claims to be.
    #[test]
    fn an_array_is_refused_rather_than_carried() {
        let refused = read("{\"listed\": [1, 2]}", "a test document").expect_err("refused");

        assert!(refused.to_string().contains("array"), "{refused}");
    }

    #[test]
    fn a_root_that_is_not_an_object_is_refused() {
        assert!(matches!(
            read("42", "a test document"),
            Err(SparseFault::NotAnObject(_)),
        ));
        assert!(matches!(
            read("not json at all", "a test document"),
            Err(SparseFault::Unreadable(_, _)),
        ));
    }

    #[test]
    fn a_fault_names_the_document_it_is_about() {
        let refused = read("[]", "Actor.json").expect_err("refused");

        assert!(refused.to_string().contains("Actor.json"), "{refused}");
    }
}
