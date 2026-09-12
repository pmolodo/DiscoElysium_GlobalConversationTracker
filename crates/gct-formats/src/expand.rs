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
    self, EXPANDED_SUFFIX, Files, JSON_SUFFIX, Manifest, Member, MemberKind, Members, SaveFault,
    TEXT_DIFF_SUFFIX, Written, shown,
};
use super::header;
use super::lua_blob::{self, Blob, BlobFault, TABLE_NAMES};
use super::lua_parts::{self, PARTS_SUFFIX, PartsFault};
use super::lua_simx::Orders;
use super::packed_save::{self, LUA_SUFFIX, UnpackFault, Unpacked};
use super::{json_diff, text_diff};

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
    /// A member has no counterpart anywhere beneath the base.
    #[error("nothing beneath the base answers for '{0}', so it cannot be written as a change")]
    Unmatched(String),
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

/// A save already expanded, put back into the shape it left the game in.
///
/// Every diff beneath it is resolved, so what comes back is the WHOLE save rather than the
/// change - which is what an archive holds, and therefore what [`expansion`] has to be
/// given. The name comes off the manifest where there is one, since that is the name the
/// members were written for.
///
/// # Errors
///
/// Where either half of the save will not resolve.
pub fn as_packed(
    files: &impl Files,
    save: &Path,
    orders: Option<&Orders>,
) -> Result<Unpacked, ExpandFault> {
    let links = expanded_save::chain(files, save)?;
    let stem = links.last().expect("a chain ends at the save").stem();
    let whole = read(files, save, orders)?;

    Ok(Unpacked {
        lua_name: format!("{stem}{LUA_SUFFIX}"),
        lua: lua_blob::write(&whole.blob),
        members: whole
            .members
            .into_iter()
            .map(|(suffix, bytes)| packed_save::Entry {
                name: format!("{stem}{suffix}"),
                bytes,
            })
            .collect(),
    })
}

/// What a save already expanded would be written as by THIS build.
///
/// What the standing rule needs at every version bump: only the latest version of a format
/// is committed, so a bump regenerates every file of it, and this is what regenerates one.
/// A save written whole comes back whole, and one written as a change comes back as a
/// change to the same base.
///
/// # Errors
///
/// As [`as_packed`] and [`expansion`].
pub fn rewrite(
    files: &impl Files,
    save: &Path,
    orders: Option<&Orders>,
) -> Result<Vec<Written>, ExpandFault> {
    let links = expanded_save::chain(files, save)?;
    let here = links.last().expect("a chain ends at the save");
    let base = here
        .manifest
        .as_ref()
        .map(|manifest| here.directory.join(&manifest.base));

    let packed = as_packed(files, save, orders)?;
    expansion(files, &packed, save, base.as_deref(), orders)
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
        let suffix = suffix_of(&entry.name, packed.stem())?;
        let was = beneath
            .members
            .iter()
            .find(|(named, _)| *named == suffix)
            .map(|(_, bytes)| bytes.as_slice());

        // WHERE THE FILE IT IS A DIFF OF ACTUALLY IS, which need not be in the base: a base
        // that inherits this member holds no file for it, and nor may its own base.
        let names = expanded_save::member_beneath(files, base, &suffix)?
            .map(|path| expanded_save::relative(directory, &path));

        let (member, diff) = change(entry, suffix, was, names.as_deref())?;
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

    // WHERE EACH TABLE IT IS A DIFF OF ACTUALLY IS, on the same footing as the members: a
    // base that changes no table has no split directory, so the file can be links further
    // up. Named from the split directory, since that is where the diff will stand.
    let mut named = Vec::with_capacity(TABLE_NAMES.len());
    for table in TABLE_NAMES {
        let file = format!("{table}{JSON_SUFFIX}");
        let path = expanded_save::table_beneath(files, base, &file, lua_parts::directory_in)?
            .ok_or_else(|| ExpandFault::Unmatched(file))?;
        named.push(expanded_save::relative(&tables, &path));
    }

    let beneath = lua_parts::encode(&beneath.blob, orders)?;
    written.extend(lua_parts::files(
        &tables,
        &parts,
        Some(&lua_parts::Beneath {
            tables: &beneath,
            named: &named,
        }),
    ));
    Ok(written)
}

/// What one member of a save becomes against the base's member of the same suffix.
///
/// `names` is where that member's file is, spelled from the directory this save is written
/// into, and every diff written here carries it - a `_base` in the JSON kinds, and the
/// first line in the text one. A MEMBER THE BASE DOES NOT HOLD IS REFUSED rather than
/// written as a diff of nothing: a diff that names no base is one only the manifest beside
/// it can read, which is the arrangement this ticket exists to end.
fn change(
    entry: &packed_save::Entry,
    suffix: String,
    was: Option<&[u8]>,
    names: Option<&str>,
) -> Result<(Member, Option<Vec<u8>>), ExpandFault> {
    let (Some(was), Some(names)) = (was, names) else {
        return Err(ExpandFault::Unmatched(entry.name.clone()));
    };

    let inherited = Member {
        name: entry.name.clone(),
        suffix: suffix.clone(),
        kind: MemberKind::Inherit,
        diff: None,
    };

    if entry.name.ends_with(JSON_SUFFIX) {
        let target = as_json(&entry.bytes, &entry.name)?;
        let baseline = as_json(was, &entry.name)?;

        let Some(mut patch) = json_diff::create(&baseline, &target) else {
            return Ok((inherited, None));
        };

        patch
            .as_object_mut()
            .expect("a diff is an object")
            .insert(header::BASE_KEY.to_string(), names.into());

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
    let Some(patch) = text_diff::create(names, &entry.name, &as_text(was, &entry.name)?, &target)
    else {
        return Ok((inherited, None));
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
