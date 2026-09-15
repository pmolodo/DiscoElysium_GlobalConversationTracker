// SPDX-License-Identifier: MIT
//! An expanded save written as a change to another one, and the members that come of it.
//!
//! ## What an expanded save is
//!
//! A directory holding the files a packed save's archive holds: a Lua blob, split into a
//! `<name>.lua.parts` directory, and a handful of PASS-THROUGH members beside it - the two
//! JSON documents the game writes, a fog-of-war file, the area states. Every one of them
//! is prefixed with the save's own name, which is what a suffix is stripped from and what
//! [`Manifest::stem`] reads back.
//!
//! ## What `_archive.json` adds
//!
//! A save written as a CHANGE to another one carries this manifest instead of its members.
//! It names a `base`, relative to itself, and then says of each member whether it is
//! inherited whole, or is a diff, and of which kind. A save that differs in one number
//! therefore costs one small file rather than a copy of everything.
//!
//! THE BASE MAY ITSELF BE A DIFF, which is what lets several saves that share a setup state
//! it once: an intermediate names the shared changes and each save beyond it carries only
//! what makes it different. Resolution is recursive, and a chain that returns to itself is
//! refused rather than followed forever.
//!
//! ## What is here, and what is not
//!
//! The manifest and the pass-through members. The LUA BLOB is not: reaching it means the
//! game's own binary format and the sparse encoding over it, which is de-xz48.6.3 and about
//! twice the size of everything else in this module put together.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::header::{self, Expected};
use super::{json_diff, text_diff};

/// What this format is called, and the version of it this build writes.
pub const FORMAT: Expected = Expected {
    format: "expanded-save-diff",
    version: 1,
};

/// What an expanded save's directory name ends in.
pub const EXPANDED_SUFFIX: &str = ".ntwtf";

/// What a manifest is called, inside the directory it describes.
pub const MANIFEST_NAME: &str = "_archive.json";

/// Where a manifest names what it is a change to.
pub const BASE_KEY: &str = "base";

/// Where a manifest lists what the save holds.
pub const MEMBERS_KEY: &str = "members";

/// What a member written as a JSON diff is named after.
pub const JSON_SUFFIX: &str = ".json";

/// What a member written as a unified diff has appended to its name.
///
/// HOW A READER TELLS A DIFF FROM WHAT IT IS A DIFF OF, for the members that are not JSON:
/// a `json-diff` says what it is in its own header, and a text file has nowhere to put one,
/// so its name carries the distinction instead.
pub const TEXT_DIFF_SUFFIX: &str = ".diff";

/// How one member of a save relates to the same member of the base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    /// The base's member, unchanged. There is no diff file.
    Inherit,
    /// A JSON document, changed by a `json-diff` beside the manifest.
    Json,
    /// A text file, changed by a unified diff beside the manifest.
    Text,
}

impl MemberKind {
    /// What the manifest spells this kind as.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::Json => "json",
            Self::Text => "text",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "inherit" => Some(Self::Inherit),
            "json" => Some(Self::Json),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
}

/// One member of a save, as the manifest describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The file's name in this save, prefixed with this save's own name.
    pub name: String,
    /// What is left of the name once the save's own name is taken off it.
    ///
    /// HOW A MEMBER IS MATCHED TO THE BASE'S, because the two saves have different names
    /// and therefore different filenames for the same thing. The suffix is what they share.
    pub suffix: String,
    /// How it relates to the base's member of the same suffix.
    pub kind: MemberKind,
    /// The diff beside the manifest, where the kind has one.
    pub diff: Option<String>,
}

/// What a save's `_archive.json` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Where the base is, relative to the directory holding this manifest.
    pub base: String,
    /// What the save holds.
    pub members: Vec<Member>,
}

impl Manifest {
    /// The save's own name, which every member's is prefixed with.
    ///
    /// READ OFF THE MEMBERS rather than off the directory name: a directory can be renamed
    /// without its contents, and the manifest already records both halves of each member -
    /// strip a member's suffix from its name and what is left is the save. A manifest
    /// cannot disagree with itself.
    #[must_use]
    pub fn stem(&self) -> Option<&str> {
        let first = self.members.first()?;
        first.name.strip_suffix(&first.suffix)
    }
}

/// Why a save could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SaveFault {
    /// A file is not there.
    #[error("{0} is not there")]
    Missing(String),
    /// A file is there and will not read.
    #[error("{0} will not read: {1}")]
    Unreadable(String, String),
    /// A manifest is not one.
    #[error("{0}: {1}")]
    Header(String, #[source] header::HeaderFault),
    /// A manifest is missing something it must say.
    #[error("{0} {1}")]
    Incomplete(String, &'static str),
    /// A member names a kind this build has no case for.
    #[error("{0}: member '{1}' has an unknown kind '{2}'")]
    UnknownKind(String, String, String),
    /// A diff would not apply to the member it is a diff of.
    #[error("{0}: {1}")]
    Unapplicable(String, String),
    /// A member inherits from a base that does not have it.
    #[error("{0}: '{1}' inherits a member the base does not have")]
    NothingToInherit(String, String),
    /// A whole save holds a file that is not one of its members.
    #[error("{0}: '{1}' is not prefixed with the save's name, '{2}'")]
    Unprefixed(String, String, String),
    /// A base chain that returns to itself.
    #[error("the bases run in a circle, through {0}")]
    Circular(String),
    /// A diff that says nothing about what it is a diff of.
    #[error("{0} is a diff and names no {} to be a diff of", header::BASE_KEY)]
    Baseless(String),
    /// A file would not be written.
    #[error("{0} will not be written: {1}")]
    Unwritable(String, String),
    /// A save would be written where one already is.
    #[error("{0} is already there; remove it before writing a save into it")]
    Occupied(String),
}

/// One file of a save on its way to disk.
///
/// PLANNED BEFORE ANY OF IT IS WRITTEN, the way [`super::packed_save::contents`] plans an
/// archive: what a save expands to is then a value a test can read, rather than something
/// only a directory afterwards can be asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// Where it goes.
    pub path: PathBuf,
    /// What it holds.
    pub bytes: Vec<u8>,
}

/// Writes a planned save out, into a directory that is not already there.
///
/// REFUSED WHERE THE DIRECTORY EXISTS rather than written over. A save expanded on top of
/// an older one keeps whatever members the older one had and this one does not, and the
/// result reads as a save nobody wrote - a directory that is a mixture is worse than one
/// that is missing.
///
/// # Errors
///
/// Where the directory is already there, or where anything will not be written.
pub fn write_all(directory: &Path, files: &[Written]) -> Result<(), SaveFault> {
    if directory.exists() {
        return Err(SaveFault::Occupied(shown(directory)));
    }

    let unwritable = |path: &Path| {
        let path = path.to_path_buf();
        move |why: &dyn std::fmt::Display| SaveFault::Unwritable(shown(&path), why.to_string())
    };

    for file in files {
        if let Some(parent) = file.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|why| unwritable(parent)(&why))?;
        }

        std::fs::write(&file.path, &file.bytes).map_err(|why| unwritable(&file.path)(&why))?;
    }

    Ok(())
}

/// Reads a manifest.
///
/// # Errors
///
/// Where the text is not JSON, is not a current-version manifest, or leaves out something a
/// manifest must say.
pub fn read_manifest(text: &str, context: &str) -> Result<Manifest, SaveFault> {
    let document: serde_json::Value = serde_json::from_str(text)
        .map_err(|why| SaveFault::Unreadable(context.to_string(), why.to_string()))?;

    // BEFORE ANYTHING IS READ OFF IT. A manifest from a newer build may describe members
    // this one would resolve wrongly, and what reads it goes on to write a save.
    FORMAT
        .check_document(&document)
        .map_err(|why| SaveFault::Header(context.to_string(), why))?;

    let base = document
        .get(BASE_KEY)
        .and_then(serde_json::Value::as_str)
        .ok_or(SaveFault::Incomplete(
            context.to_string(),
            "names no base to be a change to",
        ))?
        .to_string();

    let listed = document
        .get(MEMBERS_KEY)
        .and_then(serde_json::Value::as_array)
        .ok_or(SaveFault::Incomplete(
            context.to_string(),
            "lists no members",
        ))?;

    let mut members = Vec::with_capacity(listed.len());
    for entry in listed {
        members.push(read_member(entry, context)?);
    }

    if members.is_empty() {
        return Err(SaveFault::Incomplete(
            context.to_string(),
            "lists no members",
        ));
    }

    Ok(Manifest { base, members })
}

/// Writes a manifest, indented, with the trailing newline it is stored with.
///
/// KEY ORDER IS ALPHABETICAL and comes free: a JSON object here is a sorted map, and the
/// four names a manifest uses happen to sort into the order they read best in - what the
/// document is, which version of it, what it is a change to, and then what it holds.
#[must_use]
pub fn write_manifest(manifest: &Manifest) -> String {
    let members: Vec<serde_json::Value> = manifest
        .members
        .iter()
        .map(|member| {
            serde_json::json!({
                "diff": member.diff,
                "kind": member.kind.as_str(),
                "name": member.name,
                "suffix": member.suffix,
            })
        })
        .collect();

    let mut document = FORMAT.stamp();
    document.insert(BASE_KEY.to_string(), manifest.base.clone().into());
    document.insert(MEMBERS_KEY.to_string(), members.into());

    let mut text = serde_json::to_string_pretty(&document).expect("a manifest is plain JSON");
    text.push('\n');
    text
}

fn read_member(entry: &serde_json::Value, context: &str) -> Result<Member, SaveFault> {
    let text_at = |key: &str| entry.get(key).and_then(serde_json::Value::as_str);
    let incomplete = |what| SaveFault::Incomplete(context.to_string(), what);

    let name = text_at("name").ok_or_else(|| incomplete("has a member with no name"))?;
    let suffix = text_at("suffix").ok_or_else(|| incomplete("has a member with no suffix"))?;
    let kind = text_at("kind").ok_or_else(|| incomplete("has a member with no kind"))?;

    if !name.ends_with(suffix) {
        return Err(incomplete(
            "has a member whose name does not end in its suffix",
        ));
    }

    let kind = MemberKind::parse(kind).ok_or_else(|| {
        SaveFault::UnknownKind(context.to_string(), name.to_string(), kind.to_string())
    })?;

    // A CHANGED MEMBER MUST NAME ITS DIFF, and an inherited one must not have used it.
    // `null` is how the writer spells the second, so absent and null are the same thing.
    let diff = text_at("diff").map(str::to_string);
    if kind != MemberKind::Inherit && diff.is_none() {
        return Err(incomplete("has a changed member with no diff filename"));
    }

    Ok(Member {
        name: name.to_string(),
        suffix: suffix.to_string(),
        kind,
        diff,
    })
}

/// What a save's pass-through members hold, by the suffix that names them.
pub type Members = Vec<(String, Vec<u8>)>;

/// Whatever a directory can be asked for.
///
/// A trait so the chain walk can be tested without a filesystem, and so the eventual host
/// verb can hand it a packed archive's entries rather than a directory. Reading a save is
/// the same walk either way; only where the bytes come from differs.
///
/// EVERY READ GOES THROUGH IT, listing included. A walk that asked the trait for files and
/// the filesystem for their names would work against a directory and quietly find nothing
/// anywhere else, which is the kind of half-abstraction that passes its own tests.
pub trait Files {
    /// One file's bytes, or nothing where it is not there.
    fn read(&self, path: &Path) -> Option<Vec<u8>>;

    /// The file names directly inside a directory, in any order.
    fn list(&self, directory: &Path) -> Vec<String>;
}

/// A directory on disk.
pub struct OnDisk;

impl Files for OnDisk {
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    fn list(&self, directory: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return Vec::new();
        };

        entries
            .flatten()
            .filter(|entry| entry.path().is_file())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect()
    }
}

/// A planned save standing where it will stand, over whatever is already on disk.
///
/// Reads what the plan holds and falls through to the filesystem for everything else, so a
/// save written as a CHANGE to one already committed can be resolved before it is written -
/// its own files come from the plan and the base it names comes from disk.
///
/// What this is for: holding a writer to what a reader will make of its output, without
/// writing anything anywhere first.
pub struct Pending<'a>(pub &'a [Written]);

impl Files for Pending<'_> {
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        let wanted = flatten(path);
        self.0
            .iter()
            .find(|file| flatten(&file.path) == wanted)
            .map(|file| file.bytes.clone())
            .or_else(|| OnDisk.read(path))
    }

    fn list(&self, directory: &Path) -> Vec<String> {
        let wanted = flatten(directory);
        let mut names: Vec<String> = self
            .0
            .iter()
            .filter(|file| flatten(&file.path).parent() == Some(wanted.as_path()))
            .filter_map(|file| file.path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect();

        for name in OnDisk.list(directory) {
            if !names.contains(&name) {
                names.push(name);
            }
        }

        names
    }
}

/// One save of a chain: where it is, and what it says it changes where it changes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Where the save is, with its `..` steps taken so one directory has one spelling.
    pub directory: PathBuf,
    /// What it is a change to, or nothing where it holds everything itself.
    pub manifest: Option<Manifest>,
}

impl Link {
    /// The save's own name, which every one of its files is prefixed with.
    ///
    /// From the manifest where there is one, because that is the name the members were
    /// written for and a directory can be renamed without its contents. Otherwise from the
    /// directory, which is the only thing a whole save has to go on.
    #[must_use]
    pub fn stem(&self) -> String {
        self.manifest
            .as_ref()
            .and_then(Manifest::stem)
            .map_or_else(|| stem_of(&self.directory), str::to_string)
    }
}

/// The saves a save is built on, oldest first, ending with the save itself.
///
/// The first link is always a whole save and every later one is a change to the one before
/// it, which is the order both halves of a save are resolved in - the pass-through members
/// here, and the Lua tables in [`super::lua_parts`].
///
/// # Errors
///
/// Where a manifest will not read or is not one, or where the bases run in a circle.
pub fn chain(files: &impl Files, directory: &Path) -> Result<Vec<Link>, SaveFault> {
    // FLATTENED, because a chain joins one relative path onto another and the same
    // directory arrives spelled differently each time round: "one", then "one/../two",
    // then "one/../two/../one". A guard keyed on the spelling would never see a repeat, and
    // a circle would run until the walk did.
    let mut at = flatten(directory);
    let mut walked = HashSet::new();
    let mut links = Vec::new();

    loop {
        let manifest_path = at.join(MANIFEST_NAME);
        let Some(raw) = files.read(&manifest_path) else {
            // No manifest: the directory holds everything itself, and the chain starts here.
            links.push(Link {
                directory: at,
                manifest: None,
            });
            break;
        };

        // BEFORE THE NEXT STEP IS TAKEN, so a circle is reported at the step that closes it
        // rather than by running forever. A chain is otherwise unbounded on purpose: a long
        // one is unusual and not wrong, and a depth cap would only turn a working save into
        // a refused one.
        if !walked.insert(at.clone()) {
            return Err(SaveFault::Circular(shown(&at)));
        }

        let context = shown(&manifest_path);
        let text = String::from_utf8(raw)
            .map_err(|why| SaveFault::Unreadable(context.clone(), why.to_string()))?;
        let manifest = read_manifest(&text, &context)?;
        let next = flatten(&at.join(&manifest.base));
        links.push(Link {
            directory: at,
            manifest: Some(manifest),
        });
        at = next;
    }

    links.reverse();
    Ok(links)
}

/// Where the file that answers for a member actually is, along a chain of saves.
///
/// NOT ALWAYS IN THE SAVE THE MANIFEST NAMES, which is the whole reason this is a walk.
/// A save's base may inherit the member rather than change it, and its base in turn, so the
/// file a diff is a diff of can be several links up. What comes back is the nearest link
/// that holds one.
///
/// # Errors
///
/// Where a manifest along the way will not read, or where the bases run in a circle.
pub fn member_beneath(
    files: &impl Files,
    save: &Path,
    suffix: &str,
) -> Result<Option<PathBuf>, SaveFault> {
    for link in chain(files, save)?.iter().rev() {
        let Some(manifest) = &link.manifest else {
            // A WHOLE SAVE, whose members are its files. It holds this one or nothing does.
            let path = link.directory.join(format!("{}{suffix}", link.stem()));
            return Ok(files.read(&path).map(|_| path));
        };

        let Some(member) = manifest.members.iter().find(|held| held.suffix == suffix) else {
            continue;
        };
        if let Some(diff) = &member.diff {
            return Ok(Some(link.directory.join(diff)));
        }
    }

    Ok(None)
}

/// Where the file that answers for one of a save's tables actually is.
///
/// The same walk as [`member_beneath`] and for the same reason: a save that changes no table
/// has no split directory at all, so the tree a diff is a diff of can be several links up.
///
/// `named` is what the file is called inside a split directory, and `directory_of` is how a
/// save's split directory is found - passed in because that is [`super::lua_parts`]'s to
/// say, and this walk is the chain's.
///
/// # Errors
///
/// As [`member_beneath`].
pub fn table_beneath(
    files: &impl Files,
    save: &Path,
    named: &str,
    directory_of: impl Fn(&Path) -> PathBuf,
) -> Result<Option<PathBuf>, SaveFault> {
    for link in chain(files, save)?.iter().rev() {
        let path = directory_of(&link.directory).join(named);
        if files.read(&path).is_some() {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

/// The pass-through members of an expanded save, with every diff between it and a whole
/// save applied.
///
/// `directory` holds either a manifest - in which case it is a change to something else -
/// or the members themselves.
///
/// # Errors
///
/// Where a file is missing or will not read, where a manifest is not one, where a diff will
/// not apply, or where the bases run in a circle.
pub fn members_of(files: &impl Files, directory: &Path) -> Result<Members, SaveFault> {
    let links = chain(files, directory)?;
    let here = links.last().expect("a chain ends at the save itself");
    let Some(manifest) = &here.manifest else {
        return whole_members(files, &here.directory);
    };

    let context = shown(&here.directory.join(MANIFEST_NAME));
    let beneath = here.directory.join(&manifest.base);
    let mut found = Vec::with_capacity(manifest.members.len());
    for member in &manifest.members {
        // AN INHERITED MEMBER IS THE ONE THE MANIFEST STILL ANSWERS FOR, because it has no
        // file of its own to say anything in. Everything else names its own base and is
        // followed from the file rather than from the walk.
        let path = if member.kind == MemberKind::Inherit {
            member_beneath(files, &beneath, &member.suffix)?
                .ok_or_else(|| SaveFault::NothingToInherit(context.clone(), member.name.clone()))?
        } else {
            here.directory
                .join(member.diff.as_deref().unwrap_or_default())
        };

        found.push((member.suffix.clone(), resolved(files, &path)?));
    }

    Ok(found)
}

/// One member's file, with every diff between it and a whole member applied.
///
/// WHAT IT IS A DIFF OF IS IN THE FILE. A `json-diff` says so in `_base`, a unified diff
/// says so on its first line, and anything else is whole. So a member can be resolved from
/// its own path, with no manifest anywhere in sight - which is the point of writing the
/// base down in each of them.
///
/// # Errors
///
/// Where a file along the way is missing or will not read, where a diff names no base or
/// will not apply, or where the bases run in a circle.
pub fn resolved(files: &impl Files, path: &Path) -> Result<Vec<u8>, SaveFault> {
    resolve_member(files, path, &mut HashSet::new(), &mut Vec::new())
}

fn resolve_member(
    files: &impl Files,
    path: &Path,
    walked: &mut HashSet<PathBuf>,
    chain: &mut Vec<String>,
) -> Result<Vec<u8>, SaveFault> {
    let at = flatten(path);
    let context = shown(&at);
    chain.push(context.clone());
    if !walked.insert(at.clone()) {
        return Err(SaveFault::Circular(chain.join(" -> ")));
    }

    let raw = files
        .read(&at)
        .ok_or_else(|| SaveFault::Missing(context.clone()))?;
    let named = at.file_name().unwrap_or_default().to_string_lossy();
    let beside = at.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();

    if named.ends_with(TEXT_DIFF_SUFFIX) {
        let patch = String::from_utf8(raw)
            .map_err(|why| SaveFault::Unreadable(context.clone(), why.to_string()))?;
        let base = text_diff::base_of(&patch, &context)
            .map_err(|why| SaveFault::Unapplicable(context.clone(), why.to_string()))?;
        let was = resolve_member(files, &beside.join(base), walked, chain)?;
        let before = String::from_utf8(was)
            .map_err(|why| SaveFault::Unreadable(context.clone(), why.to_string()))?;

        return text_diff::apply(&before, &patch, &context)
            .map(String::into_bytes)
            .map_err(|why| SaveFault::Unapplicable(context, why.to_string()));
    }

    if !named.ends_with(JSON_SUFFIX) {
        return Ok(raw);
    }

    let document: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|why| SaveFault::Unreadable(context.clone(), why.to_string()))?;
    if document
        .get(header::FORMAT_KEY)
        .and_then(|named| named.as_str())
        != Some(json_diff::FORMAT.format)
    {
        return Ok(raw);
    }

    let base = json_diff::base_of(&document).ok_or_else(|| SaveFault::Baseless(context.clone()))?;
    let was = resolve_member(files, &beside.join(base), walked, chain)?;
    let was: serde_json::Value = serde_json::from_slice(&was)
        .map_err(|why| SaveFault::Unreadable(context.clone(), why.to_string()))?;

    let merged = json_diff::apply(&was, &document)
        .map_err(|why| SaveFault::Unapplicable(context.clone(), why.to_string()))?;
    serde_json::to_vec_pretty(&merged)
        .map_err(|why| SaveFault::Unreadable(context, why.to_string()))
}

/// The members of a save that is written whole, which are its files but the manifest.
///
/// A FILE THAT IS NOT PREFIXED WITH THE SAVE'S NAME IS REFUSED rather than passed over.
/// Every member of a save carries that prefix, so one that does not is either not a member
/// or a member of another save, and skipping it would leave it out of the archive built
/// from this directory without anything saying so.
fn whole_members(files: &impl Files, directory: &Path) -> Result<Members, SaveFault> {
    let stem = stem_of(directory);
    let mut found = Vec::new();

    for name in files.list(directory) {
        if name == MANIFEST_NAME {
            continue;
        }

        let suffix = name
            .strip_prefix(stem.as_str())
            .ok_or_else(|| SaveFault::Unprefixed(shown(directory), name.clone(), stem.clone()))?;
        let path = directory.join(&name);
        let bytes = files
            .read(&path)
            .ok_or_else(|| SaveFault::Missing(shown(&path)))?;
        found.push((suffix.to_string(), bytes));
    }

    found.sort_by(|(left, _), (right, _)| left.cmp(right));
    Ok(found)
}

/// A path with its `..` steps taken, so one directory has one spelling.
///
/// Lexical rather than [`std::fs::canonicalize`], which touches the filesystem and would
/// refuse a path that is not there - and the walk has to be able to report a base that is
/// missing rather than fail to name it.
pub(crate) fn flatten(path: &Path) -> PathBuf {
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::CurDir => {}
            other => parts.push(other.as_os_str().to_os_string()),
        }
    }

    parts.iter().collect()
}

/// One path as it looks from a directory, spelled the way a manifest spells it.
///
/// The inverse of what [`chain`] does with a `base`: this writes the name, that one follows
/// it. Both sides are rooted and flattened first, because a caller naming one of them
/// relatively is the ordinary case and two spellings of one directory share no components.
///
/// FORWARD SLASHES whatever the platform, since a manifest is committed and read on both.
/// A path sharing no root with the directory - another drive, on Windows - is left as it
/// stands, because no number of `..` steps would reach it.
#[must_use]
pub fn relative(from: &Path, to: &Path) -> String {
    let from = settled(from);
    let to = settled(to);
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();

    let shared = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    let spelled = |parts: &[std::path::Component<'_>]| {
        parts
            .iter()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };

    if shared == 0 {
        return spelled(&to).join("/");
    }

    let mut steps = vec!["..".to_string(); from.len() - shared];
    steps.extend(spelled(&to[shared..]));
    if steps.is_empty() {
        ".".to_string()
    } else {
        steps.join("/")
    }
}

/// A path with one spelling: rooted where the process stands, its `..` steps taken.
fn settled(path: &Path) -> PathBuf {
    flatten(&std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()))
}

/// A whole save's own name, which is its directory's without the extension.
pub(crate) fn stem_of(directory: &Path) -> String {
    directory
        .file_name()
        .map(|name| {
            let name = name.to_string_lossy();
            name.strip_suffix(EXPANDED_SUFFIX)
                .unwrap_or(&name)
                .to_string()
        })
        .unwrap_or_default()
}

/// A path as a message says it, which is its last two parts.
pub(super) fn shown(path: &Path) -> String {
    let file = path.file_name().unwrap_or_default().to_string_lossy();
    match path.parent().and_then(Path::file_name) {
        Some(parent) => format!("{}/{file}", parent.to_string_lossy()),
        None => file.into_owned(),
    }
}

/// Files held in memory, so a chain is testable without a filesystem.
///
/// Beside the code it stands in for rather than in either module's tests, because both
/// halves of a save are resolved along the same chain and both are tested against it.
#[cfg(test)]
#[derive(Default)]
pub(super) struct Held(std::collections::HashMap<PathBuf, Vec<u8>>);

#[cfg(test)]
impl Held {
    pub(super) fn with(self, path: &str, text: &str) -> Self {
        self.holding(path, text.as_bytes().to_vec())
    }

    pub(super) fn holding(mut self, path: &str, bytes: Vec<u8>) -> Self {
        self.0.insert(PathBuf::from(path), bytes);
        self
    }
}

#[cfg(test)]
impl Files for Held {
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        self.0.get(&flatten(path)).cloned()
    }

    fn list(&self, directory: &Path) -> Vec<String> {
        let directory = flatten(directory);
        self.0
            .keys()
            .filter(|path| path.parent() == Some(directory.as_path()))
            .map(|path| {
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = r#""_format": "expanded-save-diff", "_formatVersion": 1"#;

    fn manifest(base: &str, members: &str) -> String {
        format!(r#"{{{HEADER}, "base": "{base}", "members": [{members}]}}"#)
    }

    #[test]
    fn a_manifest_says_its_base_and_its_members() {
        let read = read_manifest(
            &manifest(
                "../at-the-fan.ntwtf",
                r#"{"diff": null, "kind": "inherit", "name": "s.1st.ntwtf.json",
                    "suffix": ".1st.ntwtf.json"}"#,
            ),
            "_archive.json",
        )
        .expect("it reads");

        assert_eq!(read.base, "../at-the-fan.ntwtf");
        let member = &read.members[0];
        assert_eq!(member.kind, MemberKind::Inherit);
        assert_eq!(member.suffix, ".1st.ntwtf.json");
        assert_eq!(member.diff, None);
        assert_eq!(read.stem(), Some("s"), "the save's name, off its members");
    }

    #[test]
    fn a_manifest_that_is_not_one_is_refused() {
        assert!(matches!(
            read_manifest(r#"{"base": "x", "members": []}"#, "_archive.json"),
            Err(SaveFault::Header(_, _)),
        ));
        assert!(matches!(
            read_manifest(&manifest("x", ""), "_archive.json"),
            Err(SaveFault::Incomplete(_, _)),
        ));
        assert!(matches!(
            read_manifest(
                &format!(
                    r#"{{{HEADER}, "members": [{{"name": "a", "suffix": "a", "kind": "inherit"}}]}}"#
                ),
                "_archive.json",
            ),
            Err(SaveFault::Incomplete(_, _)),
        ));
    }

    #[test]
    fn a_member_kind_this_build_has_no_case_for_is_refused() {
        assert!(matches!(
            read_manifest(
                &manifest(
                    "x",
                    r#"{"kind": "sideways", "name": "s.x", "suffix": ".x", "diff": "d"}"#,
                ),
                "_archive.json",
            ),
            Err(SaveFault::UnknownKind(_, _, _)),
        ));
    }

    #[test]
    fn a_changed_member_that_names_no_diff_is_refused() {
        assert!(matches!(
            read_manifest(
                &manifest("x", r#"{"kind": "json", "name": "s.x", "suffix": ".x"}"#),
                "_archive.json",
            ),
            Err(SaveFault::Incomplete(_, _)),
        ));
    }

    /// An inherited member is the base's, byte for byte.
    #[test]
    fn an_inherited_member_comes_through_whole() {
        let files = Held::default()
            .with("base.ntwtf/base.states.lua", "kept\n")
            .with(
                "save.ntwtf/_archive.json",
                &manifest(
                    "../base.ntwtf",
                    r#"{"diff": null, "kind": "inherit", "name": "save.states.lua",
                        "suffix": ".states.lua"}"#,
                ),
            );

        let members = members_of(&files, Path::new("save.ntwtf")).expect("it resolves");

        assert_eq!(
            members,
            vec![(".states.lua".to_string(), b"kept\n".to_vec())]
        );
    }

    /// And a text member is the base's with the diff beside the manifest applied.
    #[test]
    fn a_text_member_is_the_base_with_its_diff_applied() {
        let files = Held::default()
            .with("base.ntwtf/base.states.lua", "one\ntwo\n")
            .with(
                "save.ntwtf/save.states.lua.diff",
                "--- ../base.ntwtf/base.states.lua\n+++ save.states.lua\n\
                 @@ -1,2 +1,2 @@\n one\n-two\n+TWO\n",
            )
            .with(
                "save.ntwtf/_archive.json",
                &manifest(
                    "../base.ntwtf",
                    r#"{"diff": "save.states.lua.diff", "kind": "text",
                        "name": "save.states.lua", "suffix": ".states.lua"}"#,
                ),
            );

        let members = members_of(&files, Path::new("save.ntwtf")).expect("it resolves");

        assert_eq!(members[0].1, b"one\nTWO\n".to_vec());
    }

    /// A base that is itself a diff is followed, which is what lets a setup be stated once.
    #[test]
    fn a_chain_of_bases_is_followed_to_the_whole_save() {
        let files = Held::default()
            .with("whole.ntwtf/whole.states.lua", "one\ntwo\n")
            .with(
                "middle.ntwtf/middle.states.lua.diff",
                "--- ../whole.ntwtf/whole.states.lua\n+++ middle.states.lua\n\
                 @@ -1,2 +1,2 @@\n one\n-two\n+TWO\n",
            )
            .with(
                "middle.ntwtf/_archive.json",
                &manifest(
                    "../whole.ntwtf",
                    r#"{"diff": "middle.states.lua.diff", "kind": "text",
                        "name": "middle.states.lua", "suffix": ".states.lua"}"#,
                ),
            )
            .with(
                "save.ntwtf/save.states.lua.diff",
                "--- ../middle.ntwtf/middle.states.lua.diff\n+++ save.states.lua\n\
                 @@ -1,2 +1,2 @@\n-one\n+ONE\n TWO\n",
            )
            .with(
                "save.ntwtf/_archive.json",
                &manifest(
                    "../middle.ntwtf",
                    r#"{"diff": "save.states.lua.diff", "kind": "text",
                        "name": "save.states.lua", "suffix": ".states.lua"}"#,
                ),
            );

        let members = members_of(&files, Path::new("save.ntwtf")).expect("it resolves");

        assert_eq!(members[0].1, b"ONE\nTWO\n".to_vec());
    }

    /// A chain that returns to itself is refused rather than followed forever.
    #[test]
    fn bases_that_run_in_a_circle_are_refused() {
        let one = manifest(
            "../two.ntwtf",
            r#"{"diff": null, "kind": "inherit", "name": "one.x", "suffix": ".x"}"#,
        );
        let two = manifest(
            "../one.ntwtf",
            r#"{"diff": null, "kind": "inherit", "name": "two.x", "suffix": ".x"}"#,
        );
        let files = Held::default()
            .with("one.ntwtf/_archive.json", &one)
            .with("two.ntwtf/_archive.json", &two);

        assert!(matches!(
            members_of(&files, Path::new("one.ntwtf")),
            Err(SaveFault::Circular(_)),
        ));
    }

    /// Inheriting something the base does not have is a manifest that disagrees with itself.
    #[test]
    fn inheriting_a_member_the_base_does_not_have_is_refused() {
        let files = Held::default().with("base.ntwtf/base.other", "x").with(
            "save.ntwtf/_archive.json",
            &manifest(
                "../base.ntwtf",
                r#"{"diff": null, "kind": "inherit", "name": "save.states.lua",
                        "suffix": ".states.lua"}"#,
            ),
        );

        assert!(matches!(
            members_of(&files, Path::new("save.ntwtf")),
            Err(SaveFault::NothingToInherit(_, _)),
        ));
    }

    /// A diff that is a diff of something else does not half-apply.
    #[test]
    fn a_diff_of_a_different_base_is_refused() {
        let files = Held::default()
            .with("base.ntwtf/base.states.lua", "something else\n")
            .with(
                "save.ntwtf/save.states.lua.diff",
                "--- ../base.ntwtf/base.states.lua\n+++ save.states.lua\n\
                 @@ -1,1 +1,1 @@\n-two\n+TWO\n",
            )
            .with(
                "save.ntwtf/_archive.json",
                &manifest(
                    "../base.ntwtf",
                    r#"{"diff": "save.states.lua.diff", "kind": "text",
                        "name": "save.states.lua", "suffix": ".states.lua"}"#,
                ),
            );

        assert!(matches!(
            members_of(&files, Path::new("save.ntwtf")),
            Err(SaveFault::Unapplicable(_, _)),
        ));
    }

    /// A diff file the manifest names and the directory does not hold.
    #[test]
    fn a_missing_diff_is_reported_by_name() {
        let files = Held::default()
            .with("base.ntwtf/base.states.lua", "x")
            .with(
                "save.ntwtf/_archive.json",
                &manifest(
                    "../base.ntwtf",
                    r#"{"diff": "gone.diff", "kind": "text",
                    "name": "save.states.lua", "suffix": ".states.lua"}"#,
                ),
            );

        let refused = members_of(&files, Path::new("save.ntwtf")).expect_err("refused");

        assert!(refused.to_string().contains("gone.diff"), "{refused}");
    }

    /// The shape every committed manifest's base has: up out of the scenarios and across.
    #[test]
    fn a_base_beside_the_directory_above_is_named_by_stepping_up_to_it() {
        assert_eq!(
            relative(
                Path::new("testing/scenarios/at-trashcan.ntwtf"),
                Path::new("testing/save_template.ntwtf"),
            ),
            "../../save_template.ntwtf",
        );
    }

    #[test]
    fn a_base_in_the_same_folder_is_named_by_itself() {
        assert_eq!(
            relative(Path::new("saves/one.ntwtf"), Path::new("saves/two.ntwtf")),
            "../two.ntwtf",
        );
    }

    /// The two spellings of one directory are the same directory, which is what `settled`
    /// is for: without it these share no components and the answer would be an absolute
    /// path.
    #[test]
    fn a_path_spelled_with_a_step_back_is_still_the_place_it_names() {
        assert_eq!(
            relative(
                Path::new("testing/scenarios/../scenarios/one.ntwtf"),
                Path::new("testing/base.ntwtf"),
            ),
            "../../base.ntwtf",
        );
    }

    /// What the manifest says and what the chain walk does with it have to be inverses.
    #[test]
    fn what_it_names_is_what_the_chain_walk_follows_back() {
        let save = Path::new("testing/scenarios/at-trashcan.ntwtf");
        let base = Path::new("testing/save_template.ntwtf");

        let named = relative(save, base);

        assert_eq!(flatten(&save.join(named)), flatten(base));
    }
}
