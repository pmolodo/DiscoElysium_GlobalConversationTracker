// SPDX-License-Identifier: MIT
//! The engine's work, with no transport attached to it.
//!
//! ## Why this is a layer of its own
//!
//! The work used to live inside the C ABI, which meant it was reachable only by something
//! that could load a DLL - and loading a DLL into the game's own process is exactly the
//! arrangement de-bnjy.1 is undoing, because a library that dies takes the game with it.
//! There are now two front ends: [`crate::ffi`], which is the C ABI as it always was, and
//! [`crate::host`], which reads framed requests from a pipe. Neither owns the work.
//!
//! So nothing here knows about pointers, C strings, frames or processes. It takes plain
//! Rust arguments, hands back plain Rust values, and reports a failure as a [`Status`] -
//! which the C ABI returns as its integer code and the pipe puts in a field.
//!
//! ## The status numbers ARE the C ABI's
//!
//! [`Status`] is `repr(i32)` and its discriminants are the `GCT_` constants, which
//! `ffi.rs` now derives from it rather than restating. Two front ends and one set of
//! numbers: a code that means one thing over the ABI cannot come to mean another over the
//! pipe, and the .NET `Status` enum stays in step with both by staying in step with one.

use std::path::Path;
use std::sync::Arc;

use crate::index::{read_index_with_header, Index, IndexHeader, VariableTable};

/// What a call reports. Zero is success; everything else is a reason.
///
/// `repr(i32)` and explicitly numbered because these values cross a language boundary in
/// two different ways and are matched by number on the far side of both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(i32)]
pub enum Status {
    /// The call succeeded.
    Ok = 0,
    /// A pointer argument was null, or a handle was not one this library handed out.
    BadHandle = -1,
    /// A string argument was not valid UTF-8, or a path could not be read.
    BadArgument = -2,
    /// The index could not be read.
    IndexUnreadable = -3,
    /// Something panicked. The engine is still standing; the call did nothing.
    Panic = -4,
    /// No such conversation in the index.
    NoSuchConversation = -5,
    /// An answer could not be turned into JSON. Should not happen; reported anyway.
    SerialiseFailed = -6,
}

/// The engine, opened over an index.
///
/// Holds the index, which is tens of megabytes and takes a moment to parse. Read once and
/// kept for the session; building it per response menu would put that parse inside the
/// frame that draws the menu.
pub struct Service {
    index: Index,
    /// The database's variable table, where the caller deployed one.
    ///
    /// Optional, and the mod works without it - see [`crate::bridge::SnapshotWorld`]. Its
    /// own file rather than something read out of the index because it describes VARIABLES
    /// and the index describes conversations; the extractor writes them separately and the
    /// measurements already read it from there.
    declared: Option<Arc<VariableTable>>,
    /// What the index said it was, or `None` where it had no header.
    header: Option<IndexHeader>,
}

impl Service {
    /// Opens the engine over a conversation index.
    ///
    /// `variables` is the database's variable table, or `None`. A table that will not read
    /// is not an error - the mod works without it, one variable in seventy-five answering
    /// less precisely - and [`Self::variable_count`] says which happened.
    pub fn open(index: &Path, variables: Option<&Path>) -> Result<Self, Status> {
        // Read before the index, which is the expensive one: a table that will not read
        // costs nothing here and would otherwise be discovered after a 15 MB parse.
        let declared = variables
            .and_then(|path| VariableTable::read(path).ok())
            .map(Arc::new);

        // An index whose header names a version this build does not read is refused
        // outright rather than half-understood: it would pass a content check while
        // missing fields the engine has since started reading.
        let (index, header) =
            read_index_with_header(index).map_err(|_| Status::IndexUnreadable)?;

        Ok(Self { index, declared, header })
    }

    /// An engine over nothing, for the tests that are about the ARGUMENT rather than the
    /// index.
    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self { index: Index::new(), declared: None, header: None }
    }

    /// How many conversations the index holds.
    pub fn conversation_count(&self) -> i32 {
        self.index.len() as i32
    }

    /// How many variables the deployed table declares; zero if none was read.
    ///
    /// The plugin logs this at load for the same reason it logs the conversation count: a
    /// table that was not deployed, or that would not read, is a mod that still works and
    /// answers less precisely - which is exactly the kind of thing that is never noticed
    /// unless a line says it.
    pub fn variable_count(&self) -> i32 {
        self.declared.as_ref().map_or(0, |table| table.len()) as i32
    }

    /// How many entries one conversation holds.
    ///
    /// The plugin's guard against a stale index. It builds its graph from the LIVE dialogue
    /// database while this reads a file shipped with the mod, and if a game update or
    /// another mod moves the content apart, the look-ahead would answer about a
    /// conversation the player is not in. Comparing the entry count is the cheap half of
    /// noticing.
    pub fn entry_count(&self, conversation: i32) -> Result<i32, Status> {
        match self.index.get(&conversation) {
            Some(found) => Ok(found.entries.len() as i32),
            None => Err(Status::NoSuchConversation),
        }
    }

    /// What one conversation's content reduced to when the index was written, or an EMPTY
    /// string where the index carries none.
    ///
    /// Empty means "this cannot be validated" rather than "this is wrong": that is what the
    /// full index looks like.
    ///
    /// THIS ENGINE NEVER HASHES. The extractor computes it from 170 MB of YAML and the
    /// plugin computes it from the live dialogue database, through one shared routine; a
    /// third writer here would be a third thing to keep in step, over a third
    /// representation, for no gain. All this does is hand back what it was given.
    pub fn conversation_hash(&self, conversation: i32) -> Result<&str, Status> {
        match self.index.get(&conversation) {
            Some(found) => Ok(&found.hash),
            None => Err(Status::NoSuchConversation),
        }
    }

    /// What version the opened index says it is; 0 where it has no header.
    ///
    /// Zero is not a failure. The full index has no header, carries no hashes, and is a
    /// build intermediate rather than a cache - a mod shipping one works exactly as it did
    /// before there was such a thing as validation, and simply cannot check itself.
    pub fn index_format(&self) -> i32 {
        self.header.map_or(0, |header| header.format)
    }

    /// Every question a crawl over one conversation's group can ask the world.
    ///
    /// The caller answers these keys and hands them back in a look-ahead request; see
    /// [`crate::bridge::Questions`] for why the engine names its own keys rather than
    /// letting the caller build them.
    pub fn questions(&self, conversation: i32) -> Result<crate::bridge::Questions, Status> {
        crate::bridge::questions_for(&self.index, conversation)
            .map_err(|_| Status::NoSuchConversation)
    }

    /// Answers a look-ahead request, given as JSON.
    ///
    /// A request that cannot be SERVED comes back as a response carrying `error`, not as a
    /// status: the caller then has one thing to parse and one place to look. A status is
    /// for what happens before there is a response at all - here, a request that is not
    /// JSON.
    pub fn look_ahead(
        &self,
        request: &str,
    ) -> Result<crate::bridge::LookAheadResponse, Status> {
        let parsed: crate::bridge::LookAheadRequest =
            serde_json::from_str(request).map_err(|_| Status::BadArgument)?;

        Ok(crate::bridge::answer(&self.index, self.declared.clone(), &parsed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers are the C ABI's, and this is what says so.
    ///
    /// Written out one at a time rather than derived, because a test that computes the
    /// same thing the code computes proves nothing. If one of these ever changes, the .NET
    /// `Status` enum changes with it and that is a deliberate act, not a refactor.
    #[test]
    fn the_status_numbers_are_the_ones_that_cross() {
        assert_eq!(Status::Ok as i32, 0);
        assert_eq!(Status::BadHandle as i32, -1);
        assert_eq!(Status::BadArgument as i32, -2);
        assert_eq!(Status::IndexUnreadable as i32, -3);
        assert_eq!(Status::Panic as i32, -4);
        assert_eq!(Status::NoSuchConversation as i32, -5);
        assert_eq!(Status::SerialiseFailed as i32, -6);
    }

    #[test]
    fn opening_a_path_that_is_not_an_index_reports_it() {
        let opened = Service::open(Path::new("no-such-file.jsonl"), None);
        assert!(matches!(opened, Err(Status::IndexUnreadable)));
    }

    #[test]
    fn an_absent_conversation_is_refused_by_every_call_that_names_one() {
        let service = Service::empty();
        assert!(matches!(service.entry_count(631), Err(Status::NoSuchConversation)));
        assert!(matches!(service.conversation_hash(631), Err(Status::NoSuchConversation)));
        assert!(matches!(service.questions(631), Err(Status::NoSuchConversation)));
    }

    #[test]
    fn a_request_that_is_not_json_is_a_bad_argument() {
        let service = Service::empty();
        assert!(matches!(service.look_ahead("not json at all"), Err(Status::BadArgument)));
    }

    /// An empty index answers nothing, and says so as a RESPONSE rather than as a status.
    #[test]
    fn a_well_formed_request_over_an_empty_index_still_answers() {
        let service = Service::empty();
        let response = service
            .look_ahead(
                r#"{"conversation":1,"starts":[],"world":{"money":0,"day_minutes":0,
                   "day_counter":1,"clock_locked":false}}"#,
            )
            .expect("a well-formed request is answered");

        assert!(response.answers.is_empty());
    }
}
