// SPDX-License-Identifier: MIT
//! Does the engine answer the same over a pipe as it does in this process?
//!
//! `src/host.rs` has unit tests for the framing and for the serve loop, but they call the
//! loop as a function with a `Vec` on each end of it. That is not the thing the mod will
//! do. THIS SPAWNS THE ACTUAL BINARY, writes frames into its real stdin and reads frames
//! out of its real stdout - so the parts that only exist in a process are covered too: that
//! the binary was built, that it writes nothing to stdout but frames, that it flushes each
//! answer rather than holding it until it has more, and that it exits when its input ends.
//!
//! The comparison is against the SAME engine, run in-process through
//! [`lookahead_engine::service::Service`]. Anything that differs is the crossing, which is
//! the only thing this file is about - the answers themselves are the subject of
//! tests/bridge_contract.rs.

use std::io::{BufReader, BufWriter};
use std::process::{Child, Command, Stdio};

use prost::Message;

use lookahead_engine::host::{Kind, Request, Response, read_frame, write_frame};
use lookahead_engine::service::{Service, Status};
use lookahead_engine::{wire, wire_convert};

mod common;

/// The groups asked about. Small, because none of this depends on the search.
const CHECKABLE: [i32; 3] = [1123, 484, 1066];

/// A conversation no index holds, for the refusals.
const ABSENT: i32 = -1;

/// The engine, spawned and spoken to over its own pipes.
struct Host {
    child: Child,
    input: BufWriter<std::process::ChildStdin>,
    output: BufReader<std::process::ChildStdout>,
}

impl Host {
    /// Spawns the binary Cargo built for this test run.
    ///
    /// `CARGO_BIN_EXE_` is the path to the binary of that name, filled in by Cargo, so this
    /// cannot pick up a stale copy from a previous build or a deployed one from the game
    /// directory. Stderr is INHERITED rather than piped: the process says nothing there
    /// unless something has gone wrong, and inheriting means whatever it does say lands in
    /// the test output instead of in a buffer nobody drains.
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_gct-engine-host"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the engine host binary runs");

        let input = BufWriter::new(child.stdin.take().expect("its stdin is a pipe"));
        let output = BufReader::new(child.stdout.take().expect("its stdout is a pipe"));
        Self {
            child,
            input,
            output,
        }
    }

    /// Sends one request and reads the answer back.
    ///
    /// One frame in, one frame out, in that order and with nothing in between: the protocol
    /// has no way to say which answer belongs to which question, so it relies on this.
    fn ask(&mut self, kind: Kind) -> Response {
        let request = Request { kind: Some(kind) };
        write_frame(&mut self.input, &request.encode_to_vec()).expect("the child is still reading");

        let frame = read_frame(&mut self.output)
            .expect("the child is still writing")
            .expect("the child answered rather than closing the pipe");
        Response::decode(frame.as_slice()).expect("a response decodes")
    }

    /// Closes the pipe and waits, which is how the child is told there is no more.
    ///
    /// The exit status is part of what is being checked: a server that ends its loop
    /// because the input ran out has finished its work, and one that ends because a frame
    /// was malformed has not - so a clean shutdown has to be distinguishable, and the exit
    /// code is where that shows.
    fn finish(mut self) {
        drop(self.input);
        let ended = self
            .child
            .wait()
            .expect("the child ends when its input does");
        assert!(
            ended.success(),
            "the host should exit cleanly on a closed pipe: {ended}"
        );
    }
}

/// The status a response carries, as the engine's own enum.
///
/// A number the schema does not name is a build mismatch rather than an answer, and
/// unwrapping here says so at the point it arrives.
fn status_of(response: &Response) -> Status {
    Status::try_from(response.status).expect("a status this build knows")
}

/// Every accessor, over the real index, answered both ways and compared.
#[test]
fn the_host_answers_what_the_service_answers() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let here = Service::open(&path, None).expect("the index reads in this process");
    let mut host = Host::spawn();

    let version = host.ask(Kind::Version(wire::VersionRequest {}));
    assert_eq!(status_of(&version), Status::Ok);
    assert_eq!(
        version.text.as_deref(),
        Some(env!("CARGO_PKG_VERSION")),
        "the child is a different build from this test's",
    );

    let opened = host.ask(Kind::Open(wire::OpenRequest {
        index: path.to_string_lossy().into_owned(),
        variables: None,
    }));
    assert_eq!(
        status_of(&opened),
        Status::Ok,
        "the host would not open the index"
    );

    assert_eq!(
        host.ask(Kind::ConversationCount(wire::ConversationCountRequest {}))
            .value,
        Some(here.conversation_count()),
    );
    assert_eq!(
        host.ask(Kind::VariableCount(wire::VariableCountRequest {}))
            .value,
        Some(here.variable_count()),
    );
    assert_eq!(
        host.ask(Kind::IndexFormat(wire::IndexFormatRequest {}))
            .value,
        Some(here.index_format()),
    );

    for conversation in CHECKABLE {
        assert_eq!(
            host.ask(Kind::EntryCount(wire::EntryCountRequest { conversation }))
                .value,
            Some(here.entry_count(conversation).expect("the index holds it")),
            "entry count for {conversation}",
        );
        assert_eq!(
            host.ask(Kind::ConversationHash(wire::ConversationHashRequest {
                conversation
            }))
            .text
            .as_deref(),
            Some(
                here.conversation_hash(conversation)
                    .expect("the index holds it")
            ),
            "hash for {conversation}",
        );

        // Compared as the ENCODED BYTES rather than as decoded messages: what this file is
        // about is the crossing, and two messages that decode alike from different bytes
        // would hide a difference in what was sent.
        let questions = host.ask(Kind::Questions(wire::QuestionsRequest { conversation }));
        assert_eq!(status_of(&questions), Status::Ok);
        assert_eq!(
            questions.questions.map(|answered| answered.encode_to_vec()),
            Some(
                wire_convert::write_questions(
                    here.questions(conversation).expect("the group builds")
                )
                .encode_to_vec()
            ),
            "questions for {conversation}",
        );
    }

    host.finish();
}

/// One look-ahead's bytes, with how long it took taken out.
///
/// COMPARED AS BYTES ON PURPOSE, and that is worth keeping: encoding both sides and comparing
/// the result catches a field that crosses under the wrong NUMBER, which a comparison written
/// field by field cannot - it would read each one by the name this side gives it and agree
/// with itself.
///
/// EVERYTHING IN AN ANSWER IS DETERMINISTIC EXCEPT THE TIME. The two sides run the same search
/// twice, so one can take a millisecond where the other takes none, and `elapsed_ms` is the
/// one field that then differs - a zero is not encoded at all, so the bytes differ by its
/// whole tag. Zeroing it keeps the comparison and drops the only thing in it that is about the
/// machine rather than the answer. See de-ujm9, where this failed a full suite twice.
fn untimed(mut answer: wire::LookAheadResponse) -> Vec<u8> {
    for one in &mut answer.answers {
        one.elapsed_ms = 0;
    }
    answer.encode_to_vec()
}

/// A whole look-ahead, over the pipe, against the same one run here.
///
/// The only request whose body is big and whose answer is bigger, so it is the one that
/// exercises the framing rather than merely using it.
#[test]
fn a_look_ahead_crosses_and_comes_back_the_same() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let here = Service::open(&path, None).expect("the index reads in this process");
    let mut host = Host::spawn();

    let opened = host.ask(Kind::Open(wire::OpenRequest {
        index: path.to_string_lossy().into_owned(),
        variables: None,
    }));
    assert_eq!(status_of(&opened), Status::Ok);

    for conversation in CHECKABLE {
        // The first entry of the group, asked about from a world with nothing in it. The
        // answer is not the point - the two answers being identical is.
        let request = wire::LookAheadRequest {
            conversation,
            starts: vec![wire::NodeRef {
                conversation,
                entry: 0,
            }],
            world: Some(wire::WorldSnapshot {
                day_minutes: 720,
                day_counter: 1,
                ..Default::default()
            }),
            ..Default::default()
        };

        let crossed = host.ask(Kind::LookAhead(request.clone()));
        assert_eq!(
            status_of(&crossed),
            Status::Ok,
            "look-ahead for {conversation}"
        );

        let answered_here =
            here.answer_request(wire_convert::read_look_ahead(request).expect("the request reads"));
        assert_eq!(
            crossed.look_ahead.map(untimed),
            Some(untimed(wire_convert::write_look_ahead(answered_here))),
            "look-ahead for {conversation}",
        );
    }

    host.finish();
}

/// The refusals, which are the half that has to survive a process boundary intact.
///
/// A status is the only thing the caller has to go on when there is no answer, so a code
/// that arrived as something else - or a child that died instead of answering - would be
/// exactly the failure this whole change exists to prevent.
#[test]
fn a_refusal_crosses_as_a_status_and_leaves_the_host_serving() {
    let mut host = Host::spawn();

    // Before the open, when there is no engine to ask.
    assert_eq!(
        status_of(&host.ask(Kind::ConversationCount(wire::ConversationCountRequest {}))),
        Status::BadHandle,
    );

    let missing = host.ask(Kind::Open(wire::OpenRequest {
        index: "no-such-file.jsonl".into(),
        variables: None,
    }));
    assert_eq!(status_of(&missing), Status::IndexUnreadable);

    let Some(path) = common::conversation_index() else {
        return;
    };
    let opened = host.ask(Kind::Open(wire::OpenRequest {
        index: path.to_string_lossy().into_owned(),
        variables: None,
    }));
    assert_eq!(
        status_of(&opened),
        Status::Ok,
        "a failed open must not poison the next one"
    );

    // And after it, when the engine is there and the question is not answerable.
    assert_eq!(
        status_of(&host.ask(Kind::EntryCount(wire::EntryCountRequest {
            conversation: ABSENT
        }))),
        Status::NoSuchConversation,
    );
    assert_eq!(
        status_of(
            &host.ask(Kind::ConversationHash(wire::ConversationHashRequest {
                conversation: ABSENT
            }))
        ),
        Status::NoSuchConversation,
    );
    assert_eq!(
        status_of(&host.ask(Kind::Questions(wire::QuestionsRequest {
            conversation: ABSENT
        }))),
        Status::NoSuchConversation,
    );

    // A look-ahead whose SHAPE will not read. The bytes decode - it is a well-formed
    // message - and the entry set inside it holds a run that ends before it starts, which
    // is nonsense rather than absence and so is refused rather than read as empty.
    let backwards = wire::NodeSet {
        conversations: vec![wire::ConversationRuns {
            conversation: 1123,
            runs: vec![wire::NodeRun { first: 50, last: 1 }],
        }],
    };
    assert_eq!(
        status_of(&host.ask(Kind::LookAhead(wire::LookAheadRequest {
            conversation: 1123,
            seen_any_game: Some(backwards),
            ..Default::default()
        }))),
        Status::BadArgument,
    );

    // Still serving after all of that, which is the point.
    assert_eq!(
        status_of(&host.ask(Kind::ConversationCount(wire::ConversationCountRequest {}))),
        Status::Ok,
    );

    host.finish();
}
