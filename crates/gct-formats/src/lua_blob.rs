// SPDX-License-Identifier: MIT
//! The binary blob a save carries its five Lua tables in.
//!
//! ## The format
//!
//! Five consecutive values - Actor, Item, Location, Variable, Conversation - followed by
//! whatever else the file holds, which this does not interpret. Each value begins with a
//! one-byte ASCII marker saying what it is:
//!
//! - `T` a table: a little-endian `i32` count and that many LIST entries, then another
//!   count and that many KEY-VALUE pairs.
//! - `S` a string: its byte length, seven bits at a time, then that many UTF-8 bytes.
//! - `N` a number: eight bytes of little-endian `f64`.
//! - `B` a boolean: one byte, zero for false.
//! - `X` nil, carrying nothing.
//!
//! It is what .NET's `BinaryWriter` produces, because that is what wrote it: the game's
//! dialogue system stores its persistent data this way.
//!
//! ## Why the round trip has to be exact
//!
//! A blob written slightly wrong is not a save that loads slightly wrong. The game reads
//! the whole archive or ignores it: the main menu comes up with no Continue and Load Game
//! greyed out, and nothing anywhere says why. So the test over the packed saves an in-game
//! run leaves behind is byte-for-byte, and everything here is built for that: ORDER IS
//! KEPT, the list and dictionary halves stay where they were, and a number is written back
//! from the bits it was read as.
//!
//! ## What a number is
//!
//! Read as `f64` always, because that is what the file holds. A whole one that fits in 32
//! bits is kept as an integer so an id reads as `1` rather than `1.0` - the same rule the
//! sparse form follows, for the same reason: equality. Writing converts back, which is
//! exact, so the bytes are the ones that were read.
//!
//! NEGATIVE ZERO AND NOT-A-NUMBER stay floats. Both are whole by the arithmetic and
//! neither survives being made an integer: the first would lose its sign and the second is
//! not a value integers have.

use std::fmt;

/// The markers, which are single ASCII bytes.
mod marker {
    pub const TABLE: u8 = b'T';
    pub const STRING: u8 = b'S';
    pub const NUMBER: u8 = b'N';
    pub const BOOLEAN: u8 = b'B';
    pub const NIL: u8 = b'X';
}

/// The five top-level tables, in the order the blob holds them.
pub const TABLE_NAMES: [&str; 5] = ["Actor", "Item", "Location", "Variable", "Conversation"];

/// One Lua value, as the blob stores it.
#[derive(Debug, Clone, PartialEq)]
pub enum LuaValue {
    Nil,
    Bool(bool),
    /// A whole number that fits in 32 bits. See the module note.
    Int(i32),
    /// Everything else numeric, including negative zero and not-a-number.
    Float(f64),
    Text(String),
    Table(LuaTable),
}

/// A Lua table: a list part and a dictionary part, in the order the blob holds them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LuaTable {
    /// The list part, whose Lua keys are 1, 2, 3 and are not stored.
    pub list: Vec<LuaValue>,
    /// The dictionary part, in file order, which is not sorted and must not be.
    pub dict: Vec<(LuaValue, LuaValue)>,
}

impl LuaTable {
    /// The value under a Lua key, LOOKING IN BOTH HALVES.
    ///
    /// The list part's keys are its own 1-based indices and are not stored, so a caller
    /// asking for key 3 must not have to know which half the table happened to keep it in -
    /// that split is the blob's business and nothing else's.
    ///
    /// The dictionary half is scanned in order, because it is a table in file order rather
    /// than a map. A save's tables are small enough for that, or are keyed by indices that
    /// land in the list half.
    #[must_use]
    pub fn get(&self, key: &LuaValue) -> Option<&LuaValue> {
        if let LuaValue::Int(whole) = key
            && let Ok(at) = usize::try_from(*whole)
            && (1..=self.list.len()).contains(&at)
        {
            return self.list.get(at - 1);
        }

        self.dict
            .iter()
            .find(|(held, _)| held == key)
            .map(|(_, value)| value)
    }
}

/// A whole blob: the five tables, and whatever followed them.
#[derive(Debug, Clone, PartialEq)]
pub struct Blob {
    /// The five top-level values, in [`TABLE_NAMES`] order.
    pub tables: Vec<LuaValue>,
    /// What the file held after them, carried through untouched.
    ///
    /// The dialogue system calls it "extra data" and stores length-prefixed Lua source in
    /// it. Nothing here reads it, and a writer that dropped it would produce a save missing
    /// something the game put there.
    pub trailing: Vec<u8>,
}

/// Why a blob could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BlobFault {
    /// It ended in the middle of something.
    #[error("the blob ends at offset {0}, mid-value")]
    Truncated(usize),
    /// A marker is not one this build knows.
    #[error("offset {0}: '{1}' is not a value marker")]
    Marker(usize, char),
    /// A count says a table holds a negative number of things.
    #[error("offset {0}: a table claims {1} entries")]
    Count(usize, i32),
    /// A string's bytes are not UTF-8.
    ///
    /// Refused rather than replaced. A replacement character written back is a different
    /// save, and the point of reading this at all is to write it back unchanged.
    #[error("offset {0}: a string is not UTF-8")]
    NotUtf8(usize),
    /// A length seven bits at a time that never ends.
    #[error("offset {0}: a length runs past five bytes")]
    Length(usize),
    /// A table key that is nil, which Lua has no such thing as.
    #[error("offset {0}: a table key is nil")]
    NilKey(usize),
}

/// Reads a blob.
///
/// # Errors
///
/// Where the bytes run out mid-value, where a marker or a count is not one this format
/// has, or where a string is not UTF-8.
pub fn read(bytes: &[u8]) -> Result<Blob, BlobFault> {
    let mut reader = Reader { bytes, at: 0 };

    let mut tables = Vec::with_capacity(TABLE_NAMES.len());
    for _ in 0..TABLE_NAMES.len() {
        tables.push(reader.value()?);
    }

    Ok(Blob {
        tables,
        trailing: bytes[reader.at..].to_vec(),
    })
}

/// Writes a blob back.
#[must_use]
pub fn write(blob: &Blob) -> Vec<u8> {
    let mut out = Vec::new();
    for value in &blob.tables {
        write_value(&mut out, value);
    }

    out.extend_from_slice(&blob.trailing);
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], BlobFault> {
        let end = self
            .at
            .checked_add(count)
            .ok_or(BlobFault::Truncated(self.at))?;
        let taken = self
            .bytes
            .get(self.at..end)
            .ok_or(BlobFault::Truncated(self.at))?;
        self.at = end;
        Ok(taken)
    }

    fn byte(&mut self) -> Result<u8, BlobFault> {
        Ok(self.take(1)?[0])
    }

    fn int32(&mut self) -> Result<i32, BlobFault> {
        let bytes: [u8; 4] = self.take(4)?.try_into().expect("four bytes were taken");
        Ok(i32::from_le_bytes(bytes))
    }

    fn double(&mut self) -> Result<f64, BlobFault> {
        let bytes: [u8; 8] = self.take(8)?.try_into().expect("eight bytes were taken");
        Ok(f64::from_le_bytes(bytes))
    }

    /// A length written seven bits at a time, low group first, high bit meaning "more".
    ///
    /// Five groups at most, which is what thirty-two bits takes. A sixth is a length
    /// nobody wrote, and reading on would be reading the string's own bytes as its length.
    fn length(&mut self) -> Result<usize, BlobFault> {
        let began = self.at;
        let mut value: u32 = 0;
        for group in 0..5 {
            let byte = self.byte()?;
            value |= u32::from(byte & 0x7F) << (group * 7);
            if byte & 0x80 == 0 {
                return Ok(value as usize);
            }
        }

        Err(BlobFault::Length(began))
    }

    fn value(&mut self) -> Result<LuaValue, BlobFault> {
        let at = self.at;
        match self.byte()? {
            marker::NIL => Ok(LuaValue::Nil),
            marker::BOOLEAN => Ok(LuaValue::Bool(self.byte()? != 0)),
            marker::NUMBER => Ok(number(self.double()?)),
            marker::STRING => {
                let length = self.length()?;
                let at = self.at;
                let bytes = self.take(length)?;
                String::from_utf8(bytes.to_vec())
                    .map(LuaValue::Text)
                    .map_err(|_| BlobFault::NotUtf8(at))
            }
            marker::TABLE => self.table(),
            other => Err(BlobFault::Marker(at, other as char)),
        }
    }

    fn table(&mut self) -> Result<LuaValue, BlobFault> {
        let at = self.at;
        let listed = self.int32()?;
        let listed = usize::try_from(listed).map_err(|_| BlobFault::Count(at, listed))?;

        let mut list = Vec::with_capacity(listed.min(self.bytes.len()));
        for _ in 0..listed {
            list.push(self.value()?);
        }

        let at = self.at;
        let paired = self.int32()?;
        let paired = usize::try_from(paired).map_err(|_| BlobFault::Count(at, paired))?;

        let mut dict = Vec::with_capacity(paired.min(self.bytes.len()));
        for _ in 0..paired {
            // A NIL KEY IS A FORMAT ERROR rather than a value, and the marker is the only
            // place it shows. Lua has no nil key, so a blob claiming one is a blob that was
            // read at the wrong offset.
            let at = self.at;
            if self.bytes.get(at) == Some(&marker::NIL) {
                return Err(BlobFault::NilKey(at));
            }

            let key = self.value()?;
            let value = self.value()?;
            dict.push((key, value));
        }

        Ok(LuaValue::Table(LuaTable { list, dict }))
    }
}

/// A number, whole and 32-bit-sized where it can be. See the module note.
///
/// Public because the sparse form has to reach the same answer: a key that went into a save
/// as the number 1 has to come back out of `"1"` as the same value, and "the same" is what
/// this decides.
#[must_use]
pub fn number(float: f64) -> LuaValue {
    if float.fract() == 0.0
        && float >= f64::from(i32::MIN)
        && float <= f64::from(i32::MAX)
        && !(float == 0.0 && float.is_sign_negative())
    {
        #[allow(clippy::cast_possible_truncation)]
        return LuaValue::Int(float as i32);
    }

    LuaValue::Float(float)
}

fn write_value(out: &mut Vec<u8>, value: &LuaValue) {
    match value {
        LuaValue::Nil => out.push(marker::NIL),
        LuaValue::Bool(flag) => {
            out.push(marker::BOOLEAN);
            out.push(u8::from(*flag));
        }
        LuaValue::Int(whole) => {
            out.push(marker::NUMBER);
            out.extend_from_slice(&f64::from(*whole).to_le_bytes());
        }
        LuaValue::Float(float) => {
            out.push(marker::NUMBER);
            out.extend_from_slice(&float.to_le_bytes());
        }
        LuaValue::Text(text) => {
            out.push(marker::STRING);
            write_length(out, text.len());
            out.extend_from_slice(text.as_bytes());
        }
        LuaValue::Table(table) => {
            out.push(marker::TABLE);
            write_count(out, table.list.len());
            for entry in &table.list {
                write_value(out, entry);
            }

            write_count(out, table.dict.len());
            for (key, entry) in &table.dict {
                write_value(out, key);
                write_value(out, entry);
            }
        }
    }
}

/// A count, which the format stores as a plain little-endian `i32`.
///
/// A table with more than two billion entries is not something the game writes and not
/// something this could hold anyway; saturating rather than wrapping means such a thing
/// would produce a refused save rather than a silently truncated one.
fn write_count(out: &mut Vec<u8>, count: usize) {
    #[allow(clippy::cast_possible_truncation)]
    let count = i32::try_from(count).unwrap_or(i32::MAX);
    out.extend_from_slice(&count.to_le_bytes());
}

/// A length, seven bits at a time, low group first.
fn write_length(out: &mut Vec<u8>, length: usize) {
    let mut left = u32::try_from(length).unwrap_or(u32::MAX);
    while left >= 0x80 {
        #[allow(clippy::cast_possible_truncation)]
        out.push((left as u8) | 0x80);
        left >>= 7;
    }

    #[allow(clippy::cast_possible_truncation)]
    out.push(left as u8);
}

impl fmt::Display for LuaValue {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nil => out.write_str("nil"),
            Self::Bool(flag) => write!(out, "{flag}"),
            Self::Int(whole) => write!(out, "{whole}"),
            Self::Float(float) => write!(out, "{float}"),
            Self::Text(text) => write!(out, "{text:?}"),
            Self::Table(table) => {
                write!(
                    out,
                    "{{{} listed, {} paired}}",
                    table.list.len(),
                    table.dict.len()
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The five top-level tables a blob must open with, as the smallest legal one.
    fn five_empty_tables() -> Vec<u8> {
        let mut bytes = Vec::new();
        for _ in 0..TABLE_NAMES.len() {
            bytes.push(marker::TABLE);
            bytes.extend_from_slice(&0i32.to_le_bytes());
            bytes.extend_from_slice(&0i32.to_le_bytes());
        }

        bytes
    }

    /// A blob whose first table holds `value` under the key "k".
    fn blob_holding(value: LuaValue) -> Blob {
        let mut tables = vec![LuaValue::Table(LuaTable {
            list: Vec::new(),
            dict: vec![(LuaValue::Text("k".to_string()), value)],
        })];
        while tables.len() < TABLE_NAMES.len() {
            tables.push(LuaValue::Table(LuaTable::default()));
        }

        Blob {
            tables,
            trailing: Vec::new(),
        }
    }

    fn round_trip(blob: &Blob) -> Blob {
        read(&write(blob)).expect("what this wrote, it reads")
    }

    #[test]
    fn the_smallest_legal_blob_is_five_empty_tables() {
        let blob = read(&five_empty_tables()).expect("it reads");

        assert_eq!(blob.tables.len(), 5);
        assert_eq!(blob.trailing, Vec::<u8>::new());
        assert_eq!(write(&blob), five_empty_tables());
    }

    #[test]
    fn every_kind_of_value_survives_the_round_trip() {
        for value in [
            LuaValue::Nil,
            LuaValue::Bool(true),
            LuaValue::Bool(false),
            LuaValue::Int(0),
            LuaValue::Int(-7),
            LuaValue::Int(i32::MAX),
            LuaValue::Float(2.5),
            LuaValue::Float(-0.5),
            LuaValue::Text(String::new()),
            LuaValue::Text("blue".to_string()),
            LuaValue::Text("émigré, and a \" in it".to_string()),
        ] {
            let blob = blob_holding(value.clone());
            assert_eq!(round_trip(&blob), blob, "{value}");
        }
    }

    /// A table's two halves stay where they were, and in the order they were in.
    #[test]
    fn a_tables_list_and_dictionary_halves_keep_their_order() {
        let table = LuaValue::Table(LuaTable {
            list: vec![
                LuaValue::Text("first".to_string()),
                LuaValue::Text("second".to_string()),
            ],
            // Reverse alphabetical, so anything that sorted would be visible.
            dict: vec![
                (LuaValue::Text("zebra".to_string()), LuaValue::Int(1)),
                (LuaValue::Text("mongoose".to_string()), LuaValue::Int(2)),
                (LuaValue::Text("dolphin".to_string()), LuaValue::Int(3)),
            ],
        });

        let blob = blob_holding(table);
        assert_eq!(round_trip(&blob), blob);
        assert_eq!(write(&round_trip(&blob)), write(&blob));
    }

    #[test]
    fn a_nested_table_survives_the_round_trip() {
        let inner = LuaValue::Table(LuaTable {
            list: vec![LuaValue::Int(1)],
            dict: vec![(LuaValue::Int(7), LuaValue::Bool(true))],
        });
        let blob = blob_holding(LuaValue::Table(LuaTable {
            list: vec![inner.clone()],
            dict: vec![(LuaValue::Text("in".to_string()), inner)],
        }));

        assert_eq!(round_trip(&blob), blob);
    }

    /// A whole number reads as an integer and writes back as the bits it came from.
    #[test]
    fn a_whole_number_is_an_integer_and_still_writes_back_as_a_double() {
        let blob = blob_holding(LuaValue::Int(42));
        let bytes = write(&blob);

        assert_eq!(read(&bytes).expect("it reads"), blob);
        assert!(
            bytes.windows(8).any(|eight| eight == 42f64.to_le_bytes()),
            "the number is stored as eight bytes of double",
        );
    }

    /// Negative zero is whole and it fits, and it still cannot be an integer.
    #[test]
    fn negative_zero_stays_a_float_so_its_bits_survive() {
        let blob = blob_holding(LuaValue::Float(-0.0));
        let back = round_trip(&blob);

        let LuaValue::Table(table) = &back.tables[0] else {
            panic!("the first table");
        };
        let LuaValue::Float(float) = table.dict[0].1 else {
            panic!("it stayed a float");
        };
        assert!(float.is_sign_negative(), "and kept its sign");
        assert_eq!(write(&back), write(&blob));
    }

    /// Not-a-number is written back from the bits it was read as, payload and all.
    #[test]
    fn not_a_number_survives_as_the_bits_it_arrived_in() {
        let odd = f64::from_bits(0x7FF8_0000_0000_0007);
        let blob = blob_holding(LuaValue::Float(odd));
        let bytes = write(&blob);

        assert_eq!(write(&read(&bytes).expect("it reads")), bytes);
    }

    /// A length of 128 or more takes two groups, which is where the encoding earns itself.
    #[test]
    fn a_long_string_survives_its_multi_byte_length() {
        for length in [0, 1, 127, 128, 300, 20_000] {
            let blob = blob_holding(LuaValue::Text("x".repeat(length)));
            assert_eq!(round_trip(&blob), blob, "a string of {length}");
        }
    }

    /// Whatever followed the five tables is carried through untouched.
    #[test]
    fn what_follows_the_tables_is_kept_rather_than_read() {
        let mut bytes = five_empty_tables();
        bytes.extend_from_slice(b"\x01\x02 not a value \xFF");

        let blob = read(&bytes).expect("it reads");

        assert_eq!(blob.trailing, b"\x01\x02 not a value \xFF".to_vec());
        assert_eq!(write(&blob), bytes, "and written back where it was");
    }

    #[test]
    fn a_blob_that_ends_mid_value_is_refused() {
        let whole = five_empty_tables();
        for cut in 1..whole.len() {
            assert!(
                read(&whole[..cut]).is_err(),
                "a blob cut at {cut} bytes should not read",
            );
        }
    }

    #[test]
    fn a_marker_this_build_does_not_know_is_refused() {
        let refused = read(b"Q").expect_err("refused");

        assert!(matches!(refused, BlobFault::Marker(0, 'Q')), "{refused}");
    }

    #[test]
    fn a_negative_count_is_refused_rather_than_read_as_enormous() {
        let mut bytes = vec![marker::TABLE];
        bytes.extend_from_slice(&(-1i32).to_le_bytes());

        assert!(matches!(read(&bytes), Err(BlobFault::Count(_, -1))));
    }

    #[test]
    fn a_string_that_is_not_utf8_is_refused_rather_than_replaced() {
        let mut bytes = vec![marker::STRING, 2, 0xFF, 0xFE];
        bytes.extend_from_slice(&five_empty_tables()[1..]);

        assert!(matches!(read(&bytes), Err(BlobFault::NotUtf8(_))));
    }

    /// Lua has no nil key, so a blob claiming one was read at the wrong offset.
    #[test]
    fn a_nil_table_key_is_refused() {
        let mut bytes = vec![marker::TABLE];
        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes.extend_from_slice(&1i32.to_le_bytes());
        bytes.push(marker::NIL);
        bytes.push(marker::NIL);

        assert!(matches!(read(&bytes), Err(BlobFault::NilKey(_))));
    }

    /// A length that never terminates would otherwise read the string as its own length.
    #[test]
    fn a_length_that_runs_on_is_refused() {
        let bytes = vec![marker::STRING, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80];

        assert!(matches!(read(&bytes), Err(BlobFault::Length(_))));
    }
}
