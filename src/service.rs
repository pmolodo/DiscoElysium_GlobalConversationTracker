// SPDX-License-Identifier: MIT
//! The engine's work, with no transport attached to it.
//!
//! ## Why this is a layer of its own
//!
//! The work used to live inside a C ABI, which meant it was reachable only by something
//! that could load a DLL - and loading a DLL into the game's own process is exactly the
//! arrangement de-bnjy.1 undid, because a library that dies takes the game with it. The
//! work came out here so that [`crate::host`] could reach it without a loader, and the ABI
//! was retired once nothing needed it.
//!
//! So nothing here knows about pointers, C strings, frames or processes. It takes plain
//! Rust arguments, hands back plain Rust values, and reports a failure as a [`Status`],
//! which the pipe puts in a field.
//!
//! ## The status numbers are the ones that used to be a C ABI's
//!
//! [`Status`] is `repr(i32)` and its discriminants were the `GCT_` constants. They kept
//! their numbers when the transport changed, deliberately: the .NET `Status` enum is
//! matched by number, the in-game harness has logs full of them, and renumbering a set of
//! codes buys nothing but a chance to get one wrong.

use std::path::Path;
use std::sync::Arc;

use crate::index::{read_index_with_header, Index, IndexHeader, VariableTable};

/// What a call reports. Zero is success; everything else is a reason.
///
/// `repr(i32)` and explicitly numbered because these values cross a language boundary in
/// two different ways and are matched by number on the far side of both.
///
/// ON THE WIRE IT IS THE NUMBER, not the name - hence the `into`/`try_from`. Serde's
/// default for a fieldless enum is its variant name, which would mean the pipe carried
/// `"NoSuchConversation"` where the ABI carries `-5`, and the .NET `Status` enum would
/// have to know both spellings of the same thing. One representation, and it is the one
/// that was already crossing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(into = "i32", try_from = "i32")]
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

impl From<Status> for i32 {
    fn from(status: Status) -> Self {
        status as i32
    }
}

impl TryFrom<i32> for Status {
    type Error = String;

    /// A number that is not one of these is refused rather than guessed at.
    ///
    /// It can only arrive from a peer built from different sources, and a status invented
    /// by a newer build would be read as whatever this one happened to map it to - which
    /// is a wrong answer wearing the clothes of a right one.
    fn try_from(number: i32) -> Result<Self, Self::Error> {
        match number {
            0 => Ok(Status::Ok),
            -1 => Ok(Status::BadHandle),
            -2 => Ok(Status::BadArgument),
            -3 => Ok(Status::IndexUnreadable),
            -4 => Ok(Status::Panic),
            -5 => Ok(Status::NoSuchConversation),
            -6 => Ok(Status::SerialiseFailed),
            other => Err(format!("{other} is not a status this build knows")),
        }
    }
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
    /// The live manager for the group most recently asked about, if there is one.
    ///
    /// ONE, not a map. A player stands in one conversation at a time, and a second
    /// workspace is a second preallocated node store - at the shipped 256 MB that is fine
    /// and at a measurement's six gigabytes it is not. Keeping one and replacing it makes
    /// the memory a fact about the budget rather than about how many groups a session has
    /// walked through.
    ///
    /// A `Mutex` because [`Self::look_ahead`] takes `&self` - the host holds one service
    /// and the FFI shape it grew out of had no `&mut` to offer - and because replacing a
    /// workspace has to be exclusive: two requests racing to build one would put two
    /// managers on two threads for the same group.
    workspace: std::sync::Mutex<Option<crate::workspace::Workspace>>,
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

        Ok(Self { index, declared, header, workspace: Default::default() })
    }

    /// An engine over nothing, for the tests that are about the ARGUMENT rather than the
    /// index.
    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self {
            index: Index::new(),
            declared: None,
            header: None,
            workspace: Default::default(),
        }
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

    /// Every question a search over one conversation's group can ask the world.
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

        Ok(self.answer_through_workspace(parsed))
    }

    /// How many diagram nodes the live workspace's manager holds, or `None` where there is
    /// no workspace - the per-request path builds and drops a manager per call, so there is
    /// nothing to accumulate and nothing to ask about.
    ///
    /// For measuring what a SESSION accumulates, which is a question the per-request path
    /// could not raise: one manager now serves every menu of a conversation, so the store
    /// grows across menus rather than starting empty each time. See
    /// `measurements/workspace_menus.rs` and de-dt75.2.
    pub fn workspace_held(&self) -> Option<usize> {
        self.workspace.lock().ok()?.as_ref()?.held()
    }

    /// Answers through the live workspace where one serves, and otherwise builds a new one.
    ///
    /// ## What a hit skips
    ///
    /// The group graph, the layout and the diagram manager - and the manager's WARM-UP,
    /// which `measurements/manager_reuse.rs` found is the larger half: the first request
    /// against a fresh manager takes ninety-eight milliseconds where later ones against the
    /// same manager take fifty-three. What it still pays is the compiled guards and the
    /// seed, one to five milliseconds, because the world is baked into those and moves every
    /// line the player reads.
    ///
    /// ## And what a miss does
    ///
    /// Replaces the workspace, which means DROPPING THE OLD ONE FIRST. Its thread is joined
    /// in `Drop`, so the old manager is released before the new one is asked for - two
    /// preallocated node stores at once is what a naive replacement would do, and at a
    /// measurement's six gigabytes that fails.
    ///
    /// A workspace that cannot be opened is not an error: the per-request path still works
    /// and is what shipped before this, so a machine that cannot hold one answers exactly
    /// as it used to.
    fn answer_through_workspace(
        &self,
        request: crate::bridge::LookAheadRequest,
    ) -> crate::bridge::LookAheadResponse {
        let budget = request.diagram_budget();
        let group = crate::index::discover_group(&self.index, request.conversation);
        // WHERE THE STARTS LIVE, which is what the layout is narrowed to and therefore part
        // of what the workspace is valid for. Almost always the one conversation the
        // request names - the plugin groups its starts by conversation before sending - but
        // taken from the starts rather than assumed, because nothing enforces that.
        let entered_at = crate::bridge::entered_at_of(&request);
        if group.is_empty() {
            return crate::bridge::answer(&self.index, self.declared.clone(), &request);
        }

        let mut held = match self.workspace.lock() {
            Ok(held) => held,
            // A poisoned lock means a previous request panicked inside this. The per-request
            // path is stateless and cannot be poisoned, so it is the honest fallback.
            Err(_) => return crate::bridge::answer(&self.index, self.declared.clone(), &request),
        };

        let serves = held.as_ref().is_some_and(|workspace| {
            workspace.serves(
                &group,
                &entered_at,
                &request.world,
                self.declared.clone(),
                budget,
            )
        });

        if !serves {
            // DROPPED BEFORE THE NEXT IS BUILT. `take` releases the old thread and its
            // manager here rather than at the end of the statement that replaces it.
            *held = None;

            let Ok((graph, group)) =
                crate::index::build_group_graph(&self.index, request.conversation)
            else {
                return crate::bridge::answer(&self.index, self.declared.clone(), &request);
            };
            *held = crate::workspace::Workspace::open(
                graph,
                group,
                entered_at.clone(),
                request.world.clone(),
                self.declared.clone(),
                budget,
            );
        }

        match held.as_ref().and_then(|workspace| workspace.answer(request.clone())) {
            Some(Ok(answers)) => crate::bridge::LookAheadResponse { answers, error: None },
            // REFUSED, and the reason has to reach the caller. This is the one failure a
            // positional answer list makes possible - a world answering a different set of
            // questions than the group asks - and it is refused precisely so that it cannot
            // pass for an answer. Falling back to the per-request path here would only
            // reach the same refusal by a longer road (de-r4e0).
            Some(Err(reason)) => crate::bridge::LookAheadResponse::failed(reason),
            // The owner thread is gone, or could never be started. Answer the request the
            // way this always did rather than failing it.
            None => crate::bridge::answer(&self.index, self.declared.clone(), &request),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers, written down so they cannot drift.
    ///
    /// One at a time rather than derived, because a test that computes the same thing the
    /// code computes proves nothing. If one of these ever changes, the .NET `Status` enum
    /// changes with it and that is a deliberate act, not a refactor.
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

    /// And the wire carries that number, rather than the variant's name.
    #[test]
    fn a_status_crosses_as_its_number() {
        assert_eq!(serde_json::to_string(&Status::Ok).unwrap(), "0");
        assert_eq!(serde_json::to_string(&Status::NoSuchConversation).unwrap(), "-5");
        assert_eq!(
            serde_json::from_str::<Status>("-3").unwrap(),
            Status::IndexUnreadable,
        );
        assert!(
            serde_json::from_str::<Status>("-99").is_err(),
            "a status this build does not know must be refused rather than guessed at",
        );
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
