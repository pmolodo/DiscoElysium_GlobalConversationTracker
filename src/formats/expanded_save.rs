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

/// What a manifest is called, inside the directory it describes.
pub const MANIFEST_NAME: &str = "_archive.json";

/// Where a manifest names what it is a change to.
pub const BASE_KEY: &str = "base";

/// Where a manifest lists what the save holds.
pub const MEMBERS_KEY: &str = "members";

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
    /// A base chain that returns to itself.
    #[error("the bases run in a circle, through {0}")]
    Circular(String),
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
    let mut members = Members::new();
    for link in chain(files, directory)? {
        members = match &link.manifest {
            None => whole_members(files, &link.directory),
            Some(manifest) => applied_members(files, &link.directory, manifest, &members)?,
        };
    }

    Ok(members)
}

/// What one link of a chain leaves the members as, given what the link before it held.
fn applied_members(
    files: &impl Files,
    directory: &Path,
    manifest: &Manifest,
    base: &Members,
) -> Result<Members, SaveFault> {
    let context = shown(&directory.join(MANIFEST_NAME));
    let mut applied = Vec::with_capacity(manifest.members.len());
    for member in &manifest.members {
        let was = base
            .iter()
            .find(|(suffix, _)| *suffix == member.suffix)
            .map(|(_, bytes)| bytes.as_slice());

        applied.push((
            member.suffix.clone(),
            apply_member(files, directory, member, was, &context)?,
        ));
    }

    Ok(applied)
}

fn apply_member(
    files: &impl Files,
    directory: &Path,
    member: &Member,
    was: Option<&[u8]>,
    context: &str,
) -> Result<Vec<u8>, SaveFault> {
    if member.kind == MemberKind::Inherit {
        return was
            .map(<[u8]>::to_vec)
            .ok_or_else(|| SaveFault::NothingToInherit(context.to_string(), member.name.clone()));
    }

    let named = member.diff.as_deref().unwrap_or_default();
    let path = directory.join(named);
    let raw = files
        .read(&path)
        .ok_or_else(|| SaveFault::Missing(shown(&path)))?;
    let patch = String::from_utf8(raw)
        .map_err(|why| SaveFault::Unreadable(shown(&path), why.to_string()))?;

    // A member the base does not have is an ADDITION, so it is a diff against nothing.
    let baseline = was.unwrap_or_default();

    match member.kind {
        MemberKind::Json => apply_json(&patch, baseline, &shown(&path)),
        MemberKind::Text => {
            let before = String::from_utf8(baseline.to_vec())
                .map_err(|why| SaveFault::Unreadable(shown(&path), why.to_string()))?;
            text_diff::apply(&before, &patch, &shown(&path))
                .map(String::into_bytes)
                .map_err(|why| SaveFault::Unapplicable(shown(&path), why.to_string()))
        }
        MemberKind::Inherit => unreachable!("answered above"),
    }
}

fn apply_json(patch: &str, baseline: &[u8], context: &str) -> Result<Vec<u8>, SaveFault> {
    let patch: serde_json::Value = serde_json::from_str(patch)
        .map_err(|why| SaveFault::Unreadable(context.to_string(), why.to_string()))?;

    let was: serde_json::Value = if baseline.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(baseline)
            .map_err(|why| SaveFault::Unreadable(context.to_string(), why.to_string()))?
    };

    let merged = json_diff::apply(&was, &patch)
        .map_err(|why| SaveFault::Unapplicable(context.to_string(), why.to_string()))?;

    serde_json::to_vec_pretty(&merged)
        .map_err(|why| SaveFault::Unreadable(context.to_string(), why.to_string()))
}

/// The members of a save that is written whole, which are its files but the manifest.
fn whole_members(files: &impl Files, directory: &Path) -> Members {
    let stem = stem_of(directory);
    let mut found = Vec::new();

    for name in files.list(directory) {
        if name == MANIFEST_NAME {
            continue;
        }

        if let Some(suffix) = name.strip_prefix(stem.as_str())
            && let Some(bytes) = files.read(&directory.join(&name))
        {
            found.push((suffix.to_string(), bytes));
        }
    }

    found.sort_by(|(left, _), (right, _)| left.cmp(right));
    found
}

/// A path with its `..` steps taken, so one directory has one spelling.
///
/// Lexical rather than [`std::fs::canonicalize`], which touches the filesystem and would
/// refuse a path that is not there - and the walk has to be able to report a base that is
/// missing rather than fail to name it.
fn flatten(path: &Path) -> PathBuf {
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

/// A whole save's own name, which is its directory's without the extension.
fn stem_of(directory: &Path) -> String {
    directory
        .file_name()
        .map(|name| {
            let name = name.to_string_lossy();
            name.strip_suffix(".ntwtf").unwrap_or(&name).to_string()
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
                "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n",
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
                "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n",
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
                "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n-one\n+ONE\n TWO\n",
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
                "--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-two\n+TWO\n",
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
}
