// SPDX-License-Identifier: MIT
//! A file this repository wrote, brought up to the current version of its own format.
//!
//! ## What it is, worked out from the file
//!
//! Never from the caller. Both facts are already in every file here - that is what the
//! two-field header is for - so asking a caller to repeat them would be asking them to be
//! able to get it wrong. [`detect`] reads the header and looks the name up in
//! [`super::registry`], which is the one list of what this build writes.
//!
//! TWO PLACES A VERSION CAN LIVE, and the difference is not untidiness. Everything here
//! carries `_format` and `_formatVersion`. The mod's own state file ALSO carried a plain
//! `version` at its root through four versions of real player history, before it named
//! itself like everything else; a file from those versions is recognised by that `version`
//! beside a `conversations`, which is the pair no other document has.
//!
//! ## What it refuses, and the direction that matters
//!
//! A FILE FROM A NEWER BUILD IS REFUSED, not converted. It is full of real history this
//! build cannot read all of, so writing anything would be inventing the parts it could not.
//!
//! A FILE ALREADY CURRENT IS A SUCCESS that writes nothing. A tool pointed at whatever is
//! to hand will be pointed at current files constantly, and failing there would make
//! "convert everything" something nobody can write.
//!
//! ## And what it converts, which today is one thing
//!
//! Only the state file has ever had an older shape. Every other format is at version 1, so
//! there is nothing to bring forward - which is not a reason to leave them out of the
//! detection: every reader here refuses anything but the current version, and a refusal
//! from a converter that does not recognise the file is a wall.

use super::global_state::{self, GlobalState, Status};
use super::header;
use super::registry;
use super::runs;

/// What a file that records no version at all is taken to be.
///
/// THE ONE PLACE THIS MAY BE ASSUMED, and the reason is what this module is for: every
/// format was stamped without changing its shape, so a file written before the stamp
/// existed IS version 1, and bringing one forward is precisely the job. Every reader
/// outside this refuses an unstamped file instead.
const UNSTAMPED: u32 = 1;

/// What the state file called its version before it named its format.
const OLD_VERSION_KEY: &str = "version";

/// The last version of the state file that wrote a status per entry.
const PER_ENTRY_VERSION: u32 = 2;

/// The first version that ran a conversation's entry ids together into one string.
const RUN_ENCODED_VERSION: u32 = 4;

/// What a file is, and what this build would write instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    /// What the file says it is.
    pub name: String,
    /// Which version of that it is.
    pub version: u32,
    /// Which version of it this build writes.
    pub current: u32,
}

impl Detected {
    /// Whether the file is already what this build writes.
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.version == self.current
    }

    /// Whether it was written by a build newer than this one.
    #[must_use]
    pub fn is_from_the_future(&self) -> bool {
        self.version > self.current
    }
}

/// Why a file could not be recognised or converted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConvertFault {
    /// It is not JSON at all.
    #[error("{0} is not valid JSON: {1}")]
    Malformed(String, String),
    /// It is JSON and not an object, which every format here is.
    #[error("{0} is not a JSON object, and every format this converts is one")]
    NotAnObject(String),
    /// It names no format, and is not the state file in its old shape either.
    #[error(
        "{0} names no format this converts. One it can read carries either a '{}' naming \
         its format, or a '{OLD_VERSION_KEY}' beside a '{}'. The formats it knows are: {1}",
        header::FORMAT_KEY,
        global_state::CONVERSATIONS_KEY
    )]
    Unnamed(String, String),
    /// It names a format this build has never heard of.
    #[error("{0} says it is '{1}', which this build does not know. It knows: {2}")]
    Unknown(String, String, String),
    /// It says it is something whose version is not a whole number.
    #[error("{0} carries a version that is not a whole number")]
    Unstamped(String),
    /// It is a version this build has no conversion from.
    #[error("{0} is {1} version {2}, and this build knows no conversion from it")]
    NoRoute(String, String, u32),
    /// Its content is not what its version says it is.
    ///
    /// REFUSED RATHER THAN SKIPPED, which is the one way this deliberately differs from the
    /// reader the game runs. That reader drops a row it cannot read and warns, because a
    /// player's session has to go on; a conversion that dropped one would write a file
    /// missing history nobody asked it to lose.
    #[error("{0} will not convert: {1}")]
    Refused(String, String),
}

/// Works out what a file is, from the file.
///
/// # Errors
///
/// Where it is not a JSON object, names no format, or names one this build does not know.
pub fn detect(utf8_json: &[u8], context: &str) -> Result<Detected, ConvertFault> {
    let document: serde_json::Value = serde_json::from_slice(utf8_json)
        .map_err(|why| ConvertFault::Malformed(context.to_string(), why.to_string()))?;
    let Some(root) = document.as_object() else {
        return Err(ConvertFault::NotAnObject(context.to_string()));
    };

    // THE STATE FILE AS IT WAS BEFORE IT NAMED ITSELF, and on TWO properties rather than
    // one: a bare version is a plausible thing for some other file to carry, so requiring
    // the conversations beside it is what stops this claiming a file it cannot read.
    if let Some(version) = root.get(OLD_VERSION_KEY)
        && root.contains_key(global_state::CONVERSATIONS_KEY)
    {
        let version = whole(version).ok_or_else(|| ConvertFault::Unstamped(context.to_string()))?;
        return Ok(Detected {
            name: global_state::FORMAT.format.to_string(),
            version,
            current: global_state::FORMAT.version,
        });
    }

    let named = root
        .get(header::FORMAT_KEY)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            ConvertFault::Unnamed(context.to_string(), registry::every_name().join(", "))
        })?;

    let current = registry::current_version(named).ok_or_else(|| {
        ConvertFault::Unknown(
            context.to_string(),
            named.to_string(),
            registry::every_name().join(", "),
        )
    })?;

    Ok(Detected {
        name: named.to_string(),
        version: stamp_of(root),
        current,
    })
}

/// The current version of what `what` says the file is.
///
/// # Errors
///
/// Where this build has no conversion from that version, or the file's content is not what
/// its version says.
pub fn to_current(
    what: &Detected,
    utf8_json: &[u8],
    context: &str,
) -> Result<Vec<u8>, ConvertFault> {
    if what.name == global_state::FORMAT.format {
        return from_older_state(what.version, utf8_json, context);
    }

    // NO OTHER FORMAT HAS EVER HAD AN OLDER SHAPE - every one of them is at version 1 - so
    // reaching here means a file claims a version below 1, which nothing ever wrote.
    Err(ConvertFault::NoRoute(
        context.to_string(),
        what.name.clone(),
        what.version,
    ))
}

/// One state file of an older version, as the bytes the current one would hold.
fn from_older_state(
    version: u32,
    utf8_json: &[u8],
    context: &str,
) -> Result<Vec<u8>, ConvertFault> {
    let refused = |why: String| ConvertFault::Refused(context.to_string(), why);

    // EVERY VERSION BELOW THE CURRENT ONE, derived rather than listed. A bound that has to
    // be remembered when a version is added is a bound that will be forgotten.
    if version == 0 || version >= global_state::FORMAT.version {
        return Err(refused(format!(
            "version {version} is not an older version this build converts (expected 1 to {})",
            global_state::FORMAT.version - 1,
        )));
    }

    let document: serde_json::Value = serde_json::from_slice(utf8_json)
        .map_err(|why| ConvertFault::Malformed(context.to_string(), why.to_string()))?;
    let root = document
        .as_object()
        .ok_or_else(|| ConvertFault::NotAnObject(context.to_string()))?;

    let conversations = root
        .get(global_state::CONVERSATIONS_KEY)
        .ok_or_else(|| refused(format!("it has no '{}'", global_state::CONVERSATIONS_KEY)))?;

    let mut state = GlobalState::new();
    if version > PER_ENTRY_VERSION {
        read_grouped(conversations, version, &mut state, context)?;
    } else {
        read_per_entry(conversations, &mut state, context)?;
    }

    read_orbs(root, &mut state, context)?;
    Ok(global_state::write(&state).into_bytes())
}

/// The shape that groups ids under a status, as versions 3 and 4 wrote it.
fn read_grouped(
    conversations: &serde_json::Value,
    version: u32,
    state: &mut GlobalState,
    context: &str,
) -> Result<(), ConvertFault> {
    let refused = |why: String| ConvertFault::Refused(context.to_string(), why);
    let run_encoded = version >= RUN_ENCODED_VERSION;

    for (named, held) in members(conversations, global_state::CONVERSATIONS_KEY, context)? {
        let status = Status::parse(named)
            .ok_or_else(|| refused(format!("'{named}' is not a status a state records")))?;

        for (key, ids) in members(held, named, context)? {
            let conversation = id_of(key, context)?;

            // WHICH SHAPE, BY THE VERSION AT THE TOP rather than by looking at the value. A
            // version 4 file carrying a list is a DAMAGED version 4 file rather than a
            // version 3 one, and sniffing the value would accept what the version denies.
            for entry in entry_ids(ids, run_encoded, conversation, named, context)? {
                merge(state, conversation, entry, status, context)?;
            }
        }
    }

    Ok(())
}

/// One conversation's entry ids, in whichever way its version wrote them.
fn entry_ids(
    ids: &serde_json::Value,
    run_encoded: bool,
    conversation: i32,
    status: &str,
    context: &str,
) -> Result<Vec<i64>, ConvertFault> {
    let refused = |why: String| ConvertFault::Refused(context.to_string(), why);
    let wrong = || {
        refused(format!(
            "conversation {conversation} in '{status}' is not {}",
            if run_encoded {
                "a run-encoded string"
            } else {
                "a list of ids"
            },
        ))
    };

    if run_encoded {
        let text = ids.as_str().ok_or_else(wrong)?;
        return runs::unpack(text, &format!("conversation {conversation} in '{status}'"))
            .map_err(|why| refused(why.to_string()));
    }

    let listed = ids.as_array().ok_or_else(wrong)?;
    listed
        .iter()
        .map(|entry| {
            entry.as_i64().ok_or_else(|| {
                refused(format!(
                    "conversation {conversation} in '{status}' holds {entry}, which is not \
                     an entry id",
                ))
            })
        })
        .collect()
}

/// The shape that names a status per entry, as versions 1 and 2 wrote it.
fn read_per_entry(
    conversations: &serde_json::Value,
    state: &mut GlobalState,
    context: &str,
) -> Result<(), ConvertFault> {
    let refused = |why: String| ConvertFault::Refused(context.to_string(), why);

    for (key, entries) in members(conversations, global_state::CONVERSATIONS_KEY, context)? {
        let conversation = id_of(key, context)?;

        for (named, held) in members(entries, key, context)? {
            let entry = id_of(named, context)?;
            let status = held.as_str().and_then(Status::parse).ok_or_else(|| {
                refused(format!("the status of {conversation}/{entry} is {held}"))
            })?;

            merge(state, conversation, i64::from(entry), status, context)?;
        }
    }

    Ok(())
}

/// The opened orbs, which a version 1 file simply does not have.
fn read_orbs(
    root: &serde_json::Map<String, serde_json::Value>,
    state: &mut GlobalState,
    context: &str,
) -> Result<(), ConvertFault> {
    let refused = |why: String| ConvertFault::Refused(context.to_string(), why);
    let Some(orbs) = root.get(global_state::ORBS_KEY) else {
        return Ok(());
    };

    let listed = orbs
        .as_array()
        .ok_or_else(|| refused(format!("'{}' is not a list", global_state::ORBS_KEY)))?;

    for orb in listed {
        let title = orb
            .as_str()
            .filter(|title| !title.is_empty())
            .ok_or_else(|| refused(format!("an orb is {orb} rather than a conversation title")))?;
        state.merge_orb(title);
    }

    Ok(())
}

/// Records one entry, refusing an id no entry could have.
fn merge(
    state: &mut GlobalState,
    conversation: i32,
    entry: i64,
    status: Status,
    context: &str,
) -> Result<(), ConvertFault> {
    let entry = i32::try_from(entry).map_err(|_| {
        ConvertFault::Refused(
            context.to_string(),
            format!("conversation {conversation} holds entry {entry}, which is not an id"),
        )
    })?;

    state.merge(conversation, entry, status);
    Ok(())
}

/// The members of an object, refusing anything that is not one.
fn members<'a>(
    value: &'a serde_json::Value,
    what: &str,
    context: &str,
) -> Result<Vec<(&'a str, &'a serde_json::Value)>, ConvertFault> {
    value
        .as_object()
        .map(|held| {
            held.iter()
                .map(|(key, value)| (key.as_str(), value))
                .collect()
        })
        .ok_or_else(|| {
            ConvertFault::Refused(context.to_string(), format!("'{what}' is not an object"))
        })
}

/// One key as the id it has to be.
fn id_of(key: &str, context: &str) -> Result<i32, ConvertFault> {
    key.parse()
        .map_err(|_| ConvertFault::Refused(context.to_string(), format!("'{key}' is not an id")))
}

/// The version a document records, or what a document without one is.
fn stamp_of(root: &serde_json::Map<String, serde_json::Value>) -> u32 {
    root.get(header::VERSION_KEY)
        .and_then(whole)
        .unwrap_or(UNSTAMPED)
}

fn whole(value: &serde_json::Value) -> Option<u32> {
    u32::try_from(value.as_u64()?).ok()
}

/// A file's name with a version in it, beside the file: `state.json` at version 4 is
/// `state.v4.json`.
///
/// Where the original is kept when a file is converted in place, named for the version it
/// was. The converted file takes the original name, because that is the name its reader
/// looks for; the original keeps its history under a name that says which version it is.
#[must_use]
pub fn beside(input: &std::path::Path, version: u32) -> std::path::PathBuf {
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    let named = match input.extension() {
        Some(extension) => format!("{stem}.v{version}.{}", extension.to_string_lossy()),
        None => format!("{stem}.v{version}"),
    };

    input.with_file_name(named)
}

/// What the header of a document of this format looks like, for a test that writes one.
#[cfg(test)]
fn stamped(format: header::Expected) -> String {
    format!(
        r#""{}": "{}", "{}": {}"#,
        header::FORMAT_KEY,
        format.format,
        header::VERSION_KEY,
        format.version,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn detected(text: &str) -> Detected {
        detect(text.as_bytes(), "a test file").expect("it is recognised")
    }

    fn refused(text: &str) -> ConvertFault {
        detect(text.as_bytes(), "a test file").expect_err("it is refused")
    }

    #[test]
    fn a_document_that_names_its_format_is_recognised_by_it() {
        let what = detected(&format!("{{{}}}", stamped(crate::json_diff::FORMAT)));

        assert_eq!(what.name, "json-diff");
        assert_eq!(what.version, 1);
        assert!(what.is_current());
    }

    /// The stamp was added without changing any shape, so a file from before it IS v1.
    #[test]
    fn a_document_that_names_a_format_and_no_version_is_version_one() {
        let what = detected(r#"{"_format": "json-diff"}"#);

        assert_eq!(what.version, 1);
        assert!(what.is_current());
    }

    /// And one at a version this build has moved past is neither current nor from ahead.
    #[test]
    fn a_document_at_a_version_this_build_has_moved_past_is_recognised_as_older() {
        let what = detected(r#"{"_format": "sparse-diff", "_formatVersion": 1}"#);

        assert_eq!(what.version, 1);
        assert_eq!(what.current, crate::sparse_diff::FORMAT.version);
        assert!(!what.is_current() && !what.is_from_the_future());
    }

    #[test]
    fn a_format_this_build_never_wrote_is_named_in_the_refusal() {
        let why = refused(r#"{"_format": "lua-tables-in-yaml"}"#).to_string();

        assert!(why.contains("lua-tables-in-yaml"), "{why}");
        assert!(
            why.contains("sparse-diff"),
            "the refusal lists what it knows: {why}"
        );
    }

    #[test]
    fn something_that_is_not_json_says_so() {
        assert!(matches!(
            refused("not json at all"),
            ConvertFault::Malformed(_, _),
        ));
    }

    #[test]
    fn a_json_document_that_is_not_an_object_says_so() {
        assert!(matches!(refused("[1, 2, 3]"), ConvertFault::NotAnObject(_)));
    }

    /// A bare version is not enough, or this would claim files it cannot read.
    #[test]
    fn a_version_without_conversations_beside_it_is_not_the_state_file() {
        assert!(matches!(
            refused(r#"{"version": 3}"#),
            ConvertFault::Unnamed(_, _)
        ));
    }

    #[test]
    fn a_state_file_from_the_future_is_recognised_and_not_converted() {
        let what = detected(&format!(
            r#"{{"version": {}, "conversations": {{}}}}"#,
            global_state::FORMAT.version + 1,
        ));

        assert!(what.is_from_the_future());
        assert!(!what.is_current());
    }

    /// Versions 1 and 2 wrote a status per entry.
    #[test]
    fn a_state_that_names_a_status_per_entry_converts() {
        let older = r#"{"version": 2, "conversations": {"10": {"5": "WasDisplayed"}}}"#;
        let what = detected(older);

        let converted = to_current(&what, older.as_bytes(), "a test file").expect("it converts");
        let read = global_state::read(
            std::str::from_utf8(&converted).expect("it is text"),
            "the converted file",
        )
        .expect("what it wrote reads");

        assert_eq!(read.state.status_of(10, 5), Status::WasDisplayed);
    }

    /// Version 3 grouped ids under their status and listed them.
    #[test]
    fn a_state_that_lists_ids_under_a_status_converts() {
        let older = r#"{"version": 3, "conversations": {"WasDisplayed": {"10": [5, 6, 7]}}}"#;
        let what = detected(older);

        let converted = to_current(&what, older.as_bytes(), "a test file").expect("it converts");
        let read = global_state::read(
            std::str::from_utf8(&converted).expect("it is text"),
            "the converted file",
        )
        .expect("what it wrote reads");

        assert_eq!(
            read.state.entries_at(10, Status::WasDisplayed),
            vec![5, 6, 7]
        );
    }

    /// Version 4 ran them together, and 5 only added the header.
    #[test]
    fn a_state_that_runs_ids_together_converts_and_keeps_its_orbs() {
        let older = concat!(
            r#"{"version": 4, "conversations": {"WasDisplayed": {"10": "5-7,9"}},"#,
            r#" "orbs": ["a orb"]}"#,
        );
        let what = detected(older);

        let converted = to_current(&what, older.as_bytes(), "a test file").expect("it converts");
        let read = global_state::read(
            std::str::from_utf8(&converted).expect("it is text"),
            "the converted file",
        )
        .expect("what it wrote reads");

        assert_eq!(
            read.state.entries_at(10, Status::WasDisplayed),
            vec![5, 6, 7, 9],
        );
        assert_eq!(read.state.orbs().collect::<Vec<_>>(), vec!["a orb"]);
    }

    /// A version 4 file carrying a list is damaged, not a version 3 one.
    #[test]
    fn a_run_encoded_version_carrying_a_list_is_refused() {
        let damaged = r#"{"version": 4, "conversations": {"WasDisplayed": {"10": [5, 6]}}}"#;
        let what = detected(damaged);

        let why = to_current(&what, damaged.as_bytes(), "a test file").expect_err("refused");

        assert!(why.to_string().contains("run-encoded"), "{why}");
    }

    /// A row it cannot read stops the whole file rather than being dropped.
    #[test]
    fn a_status_a_state_does_not_record_stops_the_conversion() {
        let older = r#"{"version": 3, "conversations": {"WasEaten": {"10": [5]}}}"#;
        let what = detected(older);

        let why = to_current(&what, older.as_bytes(), "a test file").expect_err("refused");

        assert!(why.to_string().contains("WasEaten"), "{why}");
    }

    #[test]
    fn a_state_already_current_has_no_conversion_to_make() {
        let current = format!(
            r#"{{{}, "conversations": {{}}}}"#,
            stamped(global_state::FORMAT),
        );

        assert!(detected(&current).is_current());
    }

    #[test]
    fn a_versioned_name_sits_beside_the_file_and_names_the_version() {
        assert_eq!(
            beside(Path::new("saves/state.json"), 5),
            Path::new("saves/state.v5.json"),
        );
        assert_eq!(beside(Path::new("state"), 5), Path::new("state.v5"));
    }
}
