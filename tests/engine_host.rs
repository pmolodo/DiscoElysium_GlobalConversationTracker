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

use lookahead_engine::host::{Request, Response, read_frame, write_frame};
use lookahead_engine::service::{Service, Status};

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
    fn ask(&mut self, request: Request) -> Response {
        let body = serde_json::to_vec(&request).expect("a request serialises");
        write_frame(&mut self.input, &body).expect("the child is still reading");

        let frame = read_frame(&mut self.output)
            .expect("the child is still writing")
            .expect("the child answered rather than closing the pipe");
        serde_json::from_slice(&frame).expect("a response parses")
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

/// Every accessor, over the real index, answered both ways and compared.
#[test]
fn the_host_answers_what_the_service_answers() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let here = Service::open(&path, None).expect("the index reads in this process");
    let mut host = Host::spawn();

    let version = host.ask(Request::Version);
    assert_eq!(version.status, Status::Ok);
    assert_eq!(
        version.text.as_deref(),
        Some(env!("CARGO_PKG_VERSION")),
        "the child is a different build from this test's",
    );

    let opened = host.ask(Request::Open {
        index: path.to_string_lossy().into_owned(),
        variables: None,
    });
    assert_eq!(
        opened.status,
        Status::Ok,
        "the host would not open the index"
    );

    assert_eq!(
        host.ask(Request::ConversationCount).value,
        Some(here.conversation_count()),
    );
    assert_eq!(
        host.ask(Request::VariableCount).value,
        Some(here.variable_count())
    );
    assert_eq!(
        host.ask(Request::IndexFormat).value,
        Some(here.index_format())
    );

    for conversation in CHECKABLE {
        assert_eq!(
            host.ask(Request::EntryCount { conversation }).value,
            Some(here.entry_count(conversation).expect("the index holds it")),
            "entry count for {conversation}",
        );
        assert_eq!(
            host.ask(Request::ConversationHash { conversation })
                .text
                .as_deref(),
            Some(
                here.conversation_hash(conversation)
                    .expect("the index holds it")
            ),
            "hash for {conversation}",
        );

        // Compared as the JSON that actually crossed, not as parsed documents: what this
        // file is about is the crossing, and two documents that parse the same from
        // different bytes would hide a difference in what was sent.
        let questions = host.ask(Request::Questions { conversation });
        assert_eq!(questions.status, Status::Ok);
        assert_eq!(
            questions.text,
            Some(
                serde_json::to_string(&here.questions(conversation).expect("the group builds"))
                    .expect("questions serialise")
            ),
            "questions for {conversation}",
        );
    }

    host.finish();
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

    let opened = host.ask(Request::Open {
        index: path.to_string_lossy().into_owned(),
        variables: None,
    });
    assert_eq!(opened.status, Status::Ok);

    for conversation in CHECKABLE {
        // The first entry of the group, asked about from a world with nothing in it. The
        // answer is not the point - the two answers being identical is.
        let request = format!(
            r#"{{"conversation":{conversation},"starts":[{{"conversation":{conversation},
               "entry":0}}],"world":{{"money":0,"day_minutes":720,"day_counter":1,
               "clock_locked":false}}}}"#,
        );

        let crossed = host.ask(Request::LookAhead {
            request: request.clone(),
        });
        assert_eq!(crossed.status, Status::Ok, "look-ahead for {conversation}");
        assert_eq!(
            crossed.text,
            Some(
                serde_json::to_string(&here.look_ahead(&request).expect("a valid request"))
                    .expect("a response serialises")
            ),
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
        host.ask(Request::ConversationCount).status,
        Status::BadHandle
    );

    let missing = host.ask(Request::Open {
        index: "no-such-file.jsonl".into(),
        variables: None,
    });
    assert_eq!(missing.status, Status::IndexUnreadable);

    let Some(path) = common::conversation_index() else {
        return;
    };
    let opened = host.ask(Request::Open {
        index: path.to_string_lossy().into_owned(),
        variables: None,
    });
    assert_eq!(
        opened.status,
        Status::Ok,
        "a failed open must not poison the next one"
    );

    // And after it, when the engine is there and the question is not answerable.
    assert_eq!(
        host.ask(Request::EntryCount {
            conversation: ABSENT
        })
        .status,
        Status::NoSuchConversation,
    );
    assert_eq!(
        host.ask(Request::ConversationHash {
            conversation: ABSENT
        })
        .status,
        Status::NoSuchConversation,
    );
    assert_eq!(
        host.ask(Request::Questions {
            conversation: ABSENT
        })
        .status,
        Status::NoSuchConversation,
    );
    assert_eq!(
        host.ask(Request::LookAhead {
            request: "not json at all".into()
        })
        .status,
        Status::BadArgument,
    );

    // Still serving after all of that, which is the point.
    assert_eq!(host.ask(Request::ConversationCount).status, Status::Ok);

    host.finish();
}
