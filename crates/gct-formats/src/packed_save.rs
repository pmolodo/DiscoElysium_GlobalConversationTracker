// SPDX-License-Identifier: MIT
//! The archive the game loads a save from, written out of an expanded one.
//!
//! ## What is in it
//!
//! A zip holding the Lua blob - [`super::lua_blob`], rebuilt from the split directory
//! [`super::lua_parts`] reads - and beside it the pass-through members
//! [`super::expanded_save`] resolves along the same chain. That is the whole archive; there
//! is nothing in a save this does not write.
//!
//! ## The naming rule, which is the part that is impossible to diagnose
//!
//! EVERY ENTRY IN A SAVE IS PREFIXED WITH THE SAVE'S OWN NAME, and the game IGNORES an
//! archive whose entries disagree with it. Not "loads it wrongly" - ignores it, so the main
//! menu comes up with no Continue and Load Game greyed out, and nothing anywhere says why.
//! Renaming the outer file alone produces exactly that.
//!
//! So the name the archive is written under decides the entry names, rather than the
//! directory the save was expanded from. The two are usually the same and the rule only
//! shows itself when they are not.
//!
//! ## And why a name gets a timestamp appended
//!
//! The game's own saves carry one - `Autosave1(9_5_2026 3-51-18 AM)` - so a name without one
//! is given one, and the archive is written under the name that results. A caller therefore
//! gets back the path that was written rather than assuming the one it asked for.

use std::io::Write;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use super::expanded_save::{self, EXPANDED_SUFFIX, Files, SaveFault, shown};
use super::lua_blob;
use super::lua_parts::{self, PartsFault};
use super::lua_simx::Orders;

/// What a packed save's filename ends in.
pub const ZIP_SUFFIX: &str = ".ntwtf.zip";

/// What the Lua blob inside one is called, after the save's own name.
pub const LUA_SUFFIX: &str = ".ntwtf.lua";

/// Why a save could not be packed.
#[derive(Debug, thiserror::Error)]
pub enum PackFault {
    /// The source is not an expanded save.
    #[error("an expanded save's directory must end in '{EXPANDED_SUFFIX}': {0}")]
    NotExpanded(String),
    /// The output is not named like a packed save.
    #[error("a packed save must end in '{ZIP_SUFFIX}': {0}")]
    NotPacked(String),
    /// The output is named nothing but the extension.
    #[error("a packed save needs a name before '{ZIP_SUFFIX}': {0}")]
    Unnamed(String),
    /// The pass-through members would not resolve.
    #[error("{0}")]
    Members(#[from] SaveFault),
    /// The Lua tables would not read.
    #[error("{0}")]
    Tables(#[from] PartsFault),
    /// The archive would not be written.
    #[error("{0}: {1}")]
    Unwritable(String, String),
}

/// Why a save could not be unpacked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnpackFault {
    /// It is not a zip archive, or not a readable one.
    #[error("{0} will not open as an archive: {1}")]
    Unopenable(String, String),
    /// It does not hold exactly one Lua blob.
    ///
    /// NEVER PICK ONE. Which blob a save's tables are in is not a guess worth making: the
    /// wrong choice reads as a save that loads and is somebody else's.
    #[error("{0} holds {1} '*{LUA_SUFFIX}' entries, and a save holds exactly one")]
    Blobs(String, usize),
    /// It holds something that is not a file at the top of the archive.
    #[error("{0} holds '{1}', and a save's members are files beside each other")]
    NotFlat(String, String),
    /// An entry is there and will not read.
    #[error("{0}: '{1}' will not read: {2}")]
    Unreadable(String, String, String),
}

/// One entry of a packed save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What it is called inside the archive, prefixed with the save's own name.
    pub name: String,
    /// What it holds.
    pub bytes: Vec<u8>,
}

/// An archive, before it is a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packed {
    /// Where it goes, which is not always where the caller asked for it.
    pub path: PathBuf,
    /// What it holds, the Lua blob first.
    pub entries: Vec<Entry>,
}

/// The moment a save is stamped with, where its name needs one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub year: i32,
    /// January is 1.
    pub month: u8,
    pub day: u8,
    /// On a 24-hour clock; the name is written on a 12-hour one.
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl Stamp {
    /// Now, where the machine's own offset can be had, and now in UTC where it cannot.
    ///
    /// A save stamped in the wrong zone reads oddly; one that refuses to pack stops a run.
    #[must_use]
    pub fn now() -> Self {
        let now =
            time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());

        Self {
            year: now.year(),
            month: now.month() as u8,
            day: now.day(),
            hour: now.hour(),
            minute: now.minute(),
            second: now.second(),
        }
    }

    /// How a save's name spells it.
    #[must_use]
    pub fn as_name(self) -> String {
        let (clock, meridiem) = match self.hour {
            0 => (12, "AM"),
            1..=11 => (self.hour, "AM"),
            12 => (12, "PM"),
            other => (other - 12, "PM"),
        };

        format!(
            "({month}_{day}_{year} {clock}-{minute:02}-{second:02} {meridiem})",
            month = self.month,
            day = self.day,
            year = self.year,
            minute = self.minute,
            second = self.second,
        )
    }
}

/// What a packed save holds, read back out of the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unpacked {
    /// What the Lua blob is called inside the archive, prefixed with the save's own name.
    pub lua_name: String,
    /// The blob itself, still binary.
    pub lua: Vec<u8>,
    /// Everything else, in the order the archive holds it.
    pub members: Vec<Entry>,
}

impl Unpacked {
    /// The save's own name, which every one of its entries is prefixed with.
    #[must_use]
    pub fn stem(&self) -> &str {
        self.lua_name
            .strip_suffix(LUA_SUFFIX)
            .unwrap_or(&self.lua_name)
    }
}

/// Reads the archive the game wrote.
///
/// # Errors
///
/// Where the file is not a readable archive, where it does not hold exactly one Lua blob,
/// where it holds anything but files beside each other, or where an entry will not read.
pub fn unpack(path: &Path) -> Result<Unpacked, UnpackFault> {
    let context = shown(path);
    let unopenable =
        |why: &dyn std::fmt::Display| UnpackFault::Unopenable(context.clone(), why.to_string());

    let file = std::fs::File::open(path).map_err(|why| unopenable(&why))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|why| unopenable(&why))?;

    let mut lua: Option<(String, Vec<u8>)> = None;
    let mut blobs = 0;
    let mut members = Vec::new();

    for at in 0..archive.len() {
        let mut entry = archive.by_index(at).map_err(|why| unopenable(&why))?;
        let name = entry.name().to_string();
        if entry.is_dir() || name.contains('/') || name.contains('\\') || name.is_empty() {
            return Err(UnpackFault::NotFlat(context, name));
        }

        let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or_default());
        std::io::Read::read_to_end(&mut entry, &mut bytes).map_err(|why| {
            UnpackFault::Unreadable(context.clone(), name.clone(), why.to_string())
        })?;

        if name.ends_with(LUA_SUFFIX) {
            blobs += 1;
            lua = Some((name, bytes));
        } else {
            members.push(Entry { name, bytes });
        }
    }

    match lua {
        Some((lua_name, lua)) if blobs == 1 => Ok(Unpacked {
            lua_name,
            lua,
            members,
        }),
        _ => Err(UnpackFault::Blobs(context, blobs)),
    }
}

/// Writes the archive the game loads, and says what it was actually called.
///
/// # Errors
///
/// Where either path is not named like what it is, where the save will not read, or where
/// the archive will not be written.
pub fn pack(
    files: &impl Files,
    source: &Path,
    output: &Path,
    orders: Option<&Orders>,
    stamp: Stamp,
) -> Result<PathBuf, PackFault> {
    let packed = contents(files, source, output, orders, stamp)?;
    write(&packed)?;
    Ok(packed.path)
}

/// What the archive for an expanded save holds, and what it has to be called.
///
/// # Errors
///
/// Where either path is not named like what it is, or where the save will not read.
pub fn contents(
    files: &impl Files,
    source: &Path,
    output: &Path,
    orders: Option<&Orders>,
    stamp: Stamp,
) -> Result<Packed, PackFault> {
    if !ends_with(source, EXPANDED_SUFFIX) {
        return Err(PackFault::NotExpanded(shown(source)));
    }

    let (archive_name, path) = named(output, stamp)?;

    let blob = lua_parts::document(files, source, orders)?;
    let mut entries = vec![Entry {
        name: format!("{archive_name}{LUA_SUFFIX}"),
        bytes: lua_blob::write(&blob),
    }];

    for (suffix, bytes) in expanded_save::members_of(files, source)? {
        entries.push(Entry {
            name: format!("{archive_name}{suffix}"),
            bytes,
        });
    }

    Ok(Packed { path, entries })
}

/// Writes one out.
///
/// # Errors
///
/// Where the directory, the file or any entry will not be written.
pub fn write(packed: &Packed) -> Result<(), PackFault> {
    let unwritable = |path: &Path| {
        let path = path.to_path_buf();
        move |why: &dyn std::fmt::Display| PackFault::Unwritable(shown(&path), why.to_string())
    };

    if let Some(parent) = packed.path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|why| unwritable(parent)(&why))?;
    }

    let failed = unwritable(&packed.path);
    let file = std::fs::File::create(&packed.path).map_err(|why| failed(&why))?;
    let mut archive = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for entry in &packed.entries {
        archive
            .start_file(entry.name.as_str(), options)
            .map_err(|why| failed(&why))?;
        archive
            .write_all(&entry.bytes)
            .map_err(|why| failed(&why))?;
    }

    archive.finish().map_err(|why| failed(&why))?;
    Ok(())
}

/// The name every entry is prefixed with, and the path the archive goes to.
fn named(output: &Path, stamp: Stamp) -> Result<(String, PathBuf), PackFault> {
    let file = output
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let name = file
        .strip_suffix(ZIP_SUFFIX)
        .ok_or_else(|| PackFault::NotPacked(shown(output)))?;
    if name.is_empty() {
        return Err(PackFault::Unnamed(shown(output)));
    }

    if stamped(name) {
        return Ok((name.to_string(), output.to_path_buf()));
    }

    let name = format!("{name}{}", stamp.as_name());
    let path = output.with_file_name(format!("{name}{ZIP_SUFFIX}"));
    Ok((name, path))
}

/// Whether a save's name already ends in a timestamp, so a second is not appended.
fn stamped(name: &str) -> bool {
    let Some(name) = name.strip_suffix(')') else {
        return false;
    };
    let Some(at) = name.rfind('(') else {
        return false;
    };

    let Some((date, rest)) = name[at + 1..].split_once(' ') else {
        return false;
    };
    let Some((clock, meridiem)) = rest.split_once(' ') else {
        return false;
    };
    if meridiem != "AM" && meridiem != "PM" {
        return false;
    }

    let date: Vec<&str> = date.split('_').collect();
    let clock: Vec<&str> = clock.split('-').collect();
    matches!(date.as_slice(), [month, day, year]
        if digits(month, 1..=2) && digits(day, 1..=2) && digits(year, 4..=4))
        && matches!(clock.as_slice(), [hour, minute, second]
            if digits(hour, 1..=2) && digits(minute, 2..=2) && digits(second, 2..=2))
}

/// Whether a field is that many digits and nothing else.
fn digits(text: &str, width: RangeInclusive<usize>) -> bool {
    width.contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether a path's last part ends in something, which `Path::ends_with` does not answer.
fn ends_with(path: &Path, suffix: &str) -> bool {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .ends_with(suffix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expanded_save::Held;
    use crate::lua_blob::TABLE_NAMES;
    use crate::lua_parts::{PARTS_SUFFIX, TRAILING_NAME};

    /// A stamp with every field a different width, so a misplaced one shows.
    const STAMP: Stamp = Stamp {
        year: 2026,
        month: 8,
        day: 31,
        hour: 20,
        minute: 13,
        second: 30,
    };

    /// What [`STAMP`] is called in a save's name.
    const STAMPED: &str = "(8_31_2026 8-13-30 PM)";

    /// A whole expanded save: five tables, the bytes after them, and one member beside.
    fn save(name: &str) -> Held {
        let mut held = Held::default();
        for table in TABLE_NAMES {
            held = held.with(
                &format!(
                    "{name}{EXPANDED_SUFFIX}/{name}{EXPANDED_SUFFIX}{PARTS_SUFFIX}/{table}.json"
                ),
                r#"{"_format": "sparse", "_formatVersion": 1}"#,
            );
        }

        held.with(
            &format!(
                "{name}{EXPANDED_SUFFIX}/{name}{EXPANDED_SUFFIX}{PARTS_SUFFIX}/{TRAILING_NAME}"
            ),
            "",
        )
        .with(
            &format!("{name}{EXPANDED_SUFFIX}/{name}.states.lua"),
            "state",
        )
    }

    fn names_in(packed: &Packed) -> Vec<&str> {
        packed
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect()
    }

    fn packed_at(source: &str, output: &str) -> Packed {
        contents(
            &save(source),
            Path::new(&format!("{source}{EXPANDED_SUFFIX}")),
            Path::new(output),
            None,
            STAMP,
        )
        .expect("it packs")
    }

    #[test]
    fn the_blob_comes_first_and_the_members_follow_it() {
        let packed = packed_at("chosen", &format!("out/chosen{STAMPED}{ZIP_SUFFIX}"));

        assert_eq!(
            names_in(&packed),
            vec![
                format!("chosen{STAMPED}{LUA_SUFFIX}"),
                format!("chosen{STAMPED}.states.lua"),
            ],
        );
        assert_eq!(packed.entries[1].bytes, b"state".to_vec());
    }

    /// The entry names follow the name the archive is written under. A save whose entries
    /// disagree with it is one the game ignores outright.
    #[test]
    fn the_entries_are_named_for_the_output_and_not_for_the_source() {
        let packed = packed_at("chosen", &format!("out/GCT-chosen{STAMPED}{ZIP_SUFFIX}"));

        for name in names_in(&packed) {
            assert!(name.starts_with(&format!("GCT-chosen{STAMPED}")), "{name}");
        }
    }

    /// A name with no timestamp is given one, and the archive goes to the name that makes.
    #[test]
    fn a_name_without_a_timestamp_is_given_one() {
        let packed = packed_at("chosen", &format!("out/chosen{ZIP_SUFFIX}"));

        assert_eq!(
            packed.path,
            Path::new("out").join(format!("chosen{STAMPED}{ZIP_SUFFIX}")),
        );
        assert_eq!(names_in(&packed)[0], format!("chosen{STAMPED}{LUA_SUFFIX}"));
    }

    /// And one that has a timestamp keeps the name that was asked for.
    #[test]
    fn a_name_that_carries_a_timestamp_is_left_as_it_is() {
        let asked = format!("out/chosen(1_2_2026 3-04-05 AM){ZIP_SUFFIX}");

        let packed = packed_at("chosen", &asked);

        assert_eq!(packed.path, Path::new(&asked));
    }

    #[test]
    fn the_game_s_own_save_names_are_read_as_carrying_a_timestamp() {
        assert!(stamped("Autosave1(9_5_2026 3-51-18 AM)"));
        assert!(stamped("chosen(12_31_2026 11-59-59 PM)"));

        // A near miss is not one: the wrong field widths, no meridiem, nothing in brackets.
        assert!(!stamped("chosen(9_5_26 3-51-18 AM)"));
        assert!(!stamped("chosen(9_5_2026 3-51-18)"));
        assert!(!stamped("chosen(9_5_2026 3-5-18 AM)"));
        assert!(!stamped("chosen"));
        assert!(!stamped("chosen()"));
    }

    #[test]
    fn midnight_and_noon_are_twelve_rather_than_zero() {
        let at = |hour| Stamp { hour, ..STAMP }.as_name();

        assert_eq!(at(0), "(8_31_2026 12-13-30 AM)");
        assert_eq!(at(12), "(8_31_2026 12-13-30 PM)");
        assert_eq!(at(13), "(8_31_2026 1-13-30 PM)");
    }

    #[test]
    fn a_source_that_is_not_an_expanded_save_is_refused() {
        let why = contents(
            &save("chosen"),
            Path::new("chosen"),
            Path::new(&format!("out/chosen{ZIP_SUFFIX}")),
            None,
            STAMP,
        )
        .expect_err("it is refused");

        assert!(matches!(why, PackFault::NotExpanded(_)), "{why}");
    }

    #[test]
    fn an_output_that_is_not_named_like_a_packed_save_is_refused() {
        let why = contents(
            &save("chosen"),
            Path::new(&format!("chosen{EXPANDED_SUFFIX}")),
            Path::new("out/chosen.zip"),
            None,
            STAMP,
        )
        .expect_err("it is refused");

        assert!(matches!(why, PackFault::NotPacked(_)), "{why}");
    }

    #[test]
    fn an_output_that_is_nothing_but_the_extension_is_refused() {
        let why = contents(
            &save("chosen"),
            Path::new(&format!("chosen{EXPANDED_SUFFIX}")),
            Path::new(&format!("out/{ZIP_SUFFIX}")),
            None,
            STAMP,
        )
        .expect_err("it is refused");

        assert!(matches!(why, PackFault::Unnamed(_)), "{why}");
    }

    /// A file in an expanded save that is not one of its members would be dropped without
    /// anything saying so, which is a save that packs and is short.
    #[test]
    fn a_file_that_is_not_a_member_of_the_save_is_refused() {
        let files = save("chosen").with("chosen.ntwtf/stray.json", "{}");

        let why = contents(
            &files,
            Path::new(&format!("chosen{EXPANDED_SUFFIX}")),
            Path::new(&format!("out/chosen{ZIP_SUFFIX}")),
            None,
            STAMP,
        )
        .expect_err("it is refused");

        assert!(
            matches!(&why, PackFault::Members(SaveFault::Unprefixed(_, name, _)) if name == "stray.json"),
            "{why}",
        );
    }
}
