// SPDX-License-Identifier: MIT
//! The archive the game wrote, written out as a directory a repository can hold.
//!
//! ## What this is the other half of
//!
//! [`super::packed_save::pack`] turns an expanded save back into the archive the game
//! loads. This is the direction that produces one in the first place, which happens when
//! somebody adds a scenario: a save comes out of the game as a zip, and what gets committed
//! is the directory.
//!
//! ## A save is written as a CHANGE to another one wherever it can be
//!
//! Given a base, every member is compared with the base's member of the same suffix and
//! only the difference is written - a `json-diff` for the JSON members, a unified diff for
//! the rest, nothing at all for a member that did not change, and a sparse diff per Lua
//! table. A save that differs in one number therefore costs one small file rather than a
//! copy of everything, which is what makes a dozen scenarios affordable to commit.
//!
//! BOTH SIDES ARE DECODED AND RE-ENCODED before their tables are compared. The sparse form
//! does not record a grouped table's property order, so a tree read off disk can spell the
//! same table differently from one this writer would produce; comparing what is on disk
//! with what would be written would report those spellings as changes. Decoding the base
//! and encoding it again puts both sides in this writer's spelling, and what is left is the
//! data.
//!
//! ## Sparse, and only sparse
//!
//! There is no layout-preserving form here and no flag asking for one. A save's Lua blob
//! round-trips byte for byte through [`super::lua_blob`], which is a stronger promise than
//! a round trip through JSON, and nothing this repository commits is written any other way.

use std::path::Path;

use super::expanded_save::{
    self, EXPANDED_SUFFIX, Files, Manifest, Member, MemberKind, Members, SaveFault, Written, shown,
};
use super::lua_blob::{self, Blob, BlobFault};
use super::lua_parts::{self, PARTS_SUFFIX, PartsFault};
use super::lua_simx::Orders;
use super::packed_save::{self, UnpackFault, Unpacked};
use super::{json_diff, text_diff};

/// What a member written as a JSON diff is named after.
const JSON_SUFFIX: &str = ".json";

/// What a member written as a text diff has appended to its name.
const TEXT_DIFF_SUFFIX: &str = ".diff";

/// Why a save could not be expanded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExpandFault {
    /// The archive would not open, or is not one.
    #[error("{0}")]
    Unpacking(#[from] UnpackFault),
    /// The Lua blob inside it would not read.
    #[error("{0}")]
    Blob(#[from] BlobFault),
    /// The tables would not encode, or a base's would not read.
    #[error("{0}")]
    Tables(#[from] PartsFault),
    /// A base's pass-through members would not resolve.
    #[error("{0}")]
    BaseMembers(#[from] SaveFault),
    /// A member's name does not begin with the save's own.
    #[error("'{0}' is not prefixed with the save's name, '{1}'")]
    Unprefixed(String, String),
    /// A member that is not JSON is not text either, so there is no diff to take of it.
    #[error("'{0}' is neither JSON nor UTF-8 text, so it cannot be written as a change")]
    Undiffable(String),
    /// A member named like JSON is not JSON.
    #[error("'{0}' is not JSON: {1}")]
    NotJson(String, String),
    /// The base is not somewhere a save can be read from.
    #[error("{0} is neither a packed save nor an expanded one")]
    NotASave(String),
}

/// A save's two halves, however it arrived.
#[derive(Debug, Clone, PartialEq)]
pub struct Save {
    /// Its pass-through members, by the suffix that names them.
    pub members: Members,
    /// Its Lua tables, as the blob they are.
    pub blob: Blob,
}

/// Reads a save, whether it is packed or already expanded.
///
/// `orders` is what a save's derived variables are rebuilt from, and only an expanded save
/// that left any out needs it - a packed one carries every variable.
///
/// # Errors
///
/// Where the path is neither a packed save nor an expanded one, or where either half of
/// what is there will not read.
pub fn read(files: &impl Files, path: &Path, orders: Option<&Orders>) -> Result<Save, ExpandFault> {
    if path.is_dir() {
        return Ok(Save {
            members: expanded_save::members_of(files, path)?,
            blob: lua_parts::document(files, path, orders)?,
        });
    }

    if !path.is_file() {
        return Err(ExpandFault::NotASave(shown(path)));
    }

    let packed = packed_save::unpack(path)?;
    Ok(Save {
        blob: lua_blob::read(&packed.lua)?,
        members: members_of(&packed)?,
    })
}

/// The files a packed save expands into, before any of them is on disk.
///
/// `directory` is where the save goes, and is what the base is named relative to. With a
/// `base` the save is written as a change to it; without one, whole.
///
/// TAKES THE SAVE RATHER THAN A PATH TO ONE, so that what a save expands to can be worked
/// out from a save that is not on disk - which is what lets the writer be held against the
/// committed saves without an archive to read first.
///
/// # Errors
///
/// Where the blob or the base will not read, where a member is not prefixed with the save's
/// own name, or where a member that has to be diffed is neither JSON nor text.
pub fn expansion(
    files: &impl Files,
    packed: &Unpacked,
    directory: &Path,
    base: Option<&Path>,
    orders: Option<&Orders>,
) -> Result<Vec<Written>, ExpandFault> {
    let parts = lua_parts::encode(&lua_blob::read(&packed.lua)?, orders)?;

    // NAMED AFTER THE BLOB INSIDE THE ARCHIVE rather than after the directory, because at
    // this point they need not agree: a save comes out of the game carrying a timestamp in
    // its name and is often expanded into a directory named without one.
    let tables = directory.join(format!("{}{EXPANDED_SUFFIX}{PARTS_SUFFIX}", packed.stem()));

    let Some(base) = base else {
        let mut written: Vec<Written> = packed
            .members
            .iter()
            .map(|entry| Written {
                path: directory.join(&entry.name),
                bytes: entry.bytes.clone(),
            })
            .collect();
        written.extend(lua_parts::files(&tables, &parts, None));
        return Ok(written);
    };

    let beneath = read(files, base, orders)?;
    let mut written = Vec::new();
    let mut members = Vec::with_capacity(packed.members.len());
    for entry in &packed.members {
        let (member, diff) = change(packed, entry, &beneath.members)?;
        if let Some(diff) = diff {
            written.push(Written {
                path: directory.join(member.diff.as_deref().unwrap_or_default()),
                bytes: diff,
            });
        }
        members.push(member);
    }

    let manifest = Manifest {
        base: expanded_save::relative(directory, base),
        members,
    };
    written.insert(
        0,
        Written {
            path: directory.join(expanded_save::MANIFEST_NAME),
            bytes: expanded_save::write_manifest(&manifest).into_bytes(),
        },
    );

    let beneath = lua_parts::encode(&beneath.blob, orders)?;
    written.extend(lua_parts::files(&tables, &parts, Some(&beneath)));
    Ok(written)
}

/// What one member of a save becomes against the base's member of the same suffix.
///
/// A MEMBER THE BASE DOES NOT HAVE IS AN ADDITION, diffed against nothing, which is how a
/// save that carries a member no earlier one did is still written as a change.
fn change(
    packed: &Unpacked,
    entry: &packed_save::Entry,
    beneath: &Members,
) -> Result<(Member, Option<Vec<u8>>), ExpandFault> {
    let suffix = suffix_of(&entry.name, packed.stem())?;
    let was = beneath
        .iter()
        .find(|(named, _)| *named == suffix)
        .map(|(_, bytes)| bytes.as_slice());

    let inherited = |name: &str| Member {
        name: name.to_string(),
        suffix: suffix.clone(),
        kind: MemberKind::Inherit,
        diff: None,
    };

    if entry.name.ends_with(JSON_SUFFIX) {
        let target = as_json(&entry.bytes, &entry.name)?;
        let baseline = match was {
            Some(bytes) => as_json(bytes, &entry.name)?,
            None => serde_json::Value::Null,
        };

        let Some(patch) = json_diff::create(&baseline, &target) else {
            return Ok((inherited(&entry.name), None));
        };

        let text = serde_json::to_string_pretty(&patch).expect("a diff is plain JSON") + "\n";
        return Ok((
            Member {
                name: entry.name.clone(),
                suffix,
                kind: MemberKind::Json,
                diff: Some(entry.name.clone()),
            },
            Some(text.into_bytes()),
        ));
    }

    let target = as_text(&entry.bytes, &entry.name)?;
    let baseline = match was {
        Some(bytes) => as_text(bytes, &entry.name)?,
        None => String::new(),
    };

    let Some(patch) = text_diff::create(&entry.name, &baseline, &target) else {
        return Ok((inherited(&entry.name), None));
    };

    Ok((
        Member {
            name: entry.name.clone(),
            suffix,
            kind: MemberKind::Text,
            diff: Some(entry.name.clone() + TEXT_DIFF_SUFFIX),
        },
        Some(patch.into_bytes()),
    ))
}

/// A packed save's pass-through members, by the suffix that names them.
fn members_of(packed: &Unpacked) -> Result<Members, ExpandFault> {
    packed
        .members
        .iter()
        .map(|entry| Ok((suffix_of(&entry.name, packed.stem())?, entry.bytes.clone())))
        .collect()
}

/// What is left of a member's name once the save's own name is taken off it.
fn suffix_of(name: &str, stem: &str) -> Result<String, ExpandFault> {
    name.strip_prefix(stem)
        .map(str::to_string)
        .ok_or_else(|| ExpandFault::Unprefixed(name.to_string(), stem.to_string()))
}

fn as_json(bytes: &[u8], context: &str) -> Result<serde_json::Value, ExpandFault> {
    serde_json::from_slice(bytes)
        .map_err(|why| ExpandFault::NotJson(context.to_string(), why.to_string()))
}

fn as_text(bytes: &[u8], context: &str) -> Result<String, ExpandFault> {
    String::from_utf8(bytes.to_vec()).map_err(|_| ExpandFault::Undiffable(context.to_string()))
}
