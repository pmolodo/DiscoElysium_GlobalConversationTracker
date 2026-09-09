// SPDX-License-Identifier: MIT
//! The engine spoken to over a pipe, one framed request at a time.
//!
//! ## Why a pipe rather than a DllImport
//!
//! de-bnjy.1: a library loaded into the game's address space cannot fail alone. An
//! allocation the game could not satisfy, a stack overflow inside a recursive diagram
//! operation (de-fpax), an abort - each of them ends the player's session, and none of
//! them leaves anything to read. A CHILD PROCESS can die on its own, and the parent finds
//! out by the pipe going quiet rather than by ceasing to exist.
//!
//! So the transport is the child's own stdin and stdout: no port, no name, nothing to
//! configure, nothing another program on the machine can connect to, and a lifetime the
//! operating system already ties to the parent's.
//!
//! ## The frame
//!
//! A four-byte LITTLE-ENDIAN length, then that many bytes of UTF-8 JSON. Both directions,
//! and that is the whole of it.
//!
//! Length-prefixed rather than line-delimited because the bodies carry conversation text
//! and a newline inside a JSON string is legal; a reader that split on newlines would work
//! until the first line of dialogue that had one. Little-endian because both ends are
//! x86-64 and `BitConverter` on the .NET side is little-endian on every platform the game
//! ships on - stated here because it is the kind of thing that is silently assumed and
//! then silently wrong.
//!
//! [`MAX_FRAME`] bounds what a reader will allocate on being told a length. Without it a
//! corrupt or hostile four bytes is a four-gigabyte allocation, which is a crash rather
//! than an error - and the point of this module is that failures are legible.
//!
//! ## The requests are the old C ABI's calls, minus the two with no meaning here
//!
//! Ten of the eleven entry points the plugin used to `DllImport` come across as request
//! kinds. `gct_string_free` does not: there is no shared heap between two processes, so a
//! string that crosses is a copy the receiver owns, and a whole class of mistake goes with
//! it. Nor does `gct_engine_close`: the process is the handle, so closing it is closing the
//! process.
//!
//! ## What a failure looks like
//!
//! A response always carries a [`Status`]. A request that names a conversation the index
//! does not hold is `NoSuchConversation` with no payload; a panic inside the work is
//! `Panic`, caught here rather than allowed to end the loop, because a serving process
//! that dies of a bad request is worse than one that answers with a code. What is NOT a
//! status is a look-ahead the engine could not serve: that comes back as a successful
//! response whose body carries `error`, so the caller has one thing to parse.

use std::io::{BufReader, BufWriter, Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use crate::service::{Service, Status};

/// The largest frame either side will read.
///
/// Sixteen megabytes, which is far more than anything the protocol actually carries - the
/// biggest body is a look-ahead response for one response menu - and far less than an
/// allocation that would hurt. It exists so that a length nobody meant to send is an error
/// rather than an out-of-memory abort.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// What the caller asked for.
///
/// Tagged externally, so a frame reads as `{"kind":{...}}` and an unknown kind is a parse
/// failure the server answers rather than something it acts on by accident.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Request {
    /// This build's version, which the caller checks against the one it was built with.
    Version,
    /// Open the engine over an index, and optionally a variable table.
    ///
    /// A serving process holds ONE engine, so there is no handle in the protocol: the
    /// process IS the handle, and closing it is closing the process. That is the whole
    /// reason `gct_engine_close` has no counterpart here either.
    Open {
        index: String,
        variables: Option<String>,
    },
    /// How many conversations the open index holds.
    ConversationCount,
    /// How many variables the deployed table declares; zero if none was read.
    VariableCount,
    /// How many entries one conversation holds.
    EntryCount { conversation: i32 },
    /// What the index says one conversation's content reduced to.
    ConversationHash { conversation: i32 },
    /// What version the open index says it is; 0 where it has no header.
    IndexFormat,
    /// Every question a search over one conversation's group can ask.
    Questions { conversation: i32 },
    /// Answer a look-ahead request, whose body is the JSON the wire already uses.
    LookAhead { request: String },
}

/// What came back.
///
/// `status` is always present and is the number [`Status`] has always carried. The payload
/// fields are populated by the calls that have one and absent otherwise, rather than being
/// a tagged union, because the .NET side reads one field per call and a union would make
/// it read a discriminant first to learn what it already knew from what it asked.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Response {
    pub status: Status,
    /// The answer to a call that returns a count or a format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<i32>,
    /// The answer to a call that returns text or a JSON document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl Response {
    /// A status and nothing else - a refusal, or a call whose whole answer is that it
    /// worked.
    fn bare(status: Status) -> Self {
        Self {
            status,
            value: None,
            text: None,
        }
    }

    /// A number.
    fn value(value: i32) -> Self {
        Self {
            status: Status::Ok,
            value: Some(value),
            text: None,
        }
    }

    /// A string, which for most calls is a JSON document.
    fn text(text: impl Into<String>) -> Self {
        Self {
            status: Status::Ok,
            value: None,
            text: Some(text.into()),
        }
    }

    /// A value serialised to JSON, or [`Status::SerialiseFailed`] if it would not.
    ///
    /// Reported rather than unwrapped: these are plain data types and it should not
    /// happen, but "should not happen" is not a reason to end a process the game is
    /// waiting on.
    fn json<T: serde::Serialize>(value: &T) -> Self {
        match serde_json::to_string(value) {
            Ok(text) => Self::text(text),
            Err(_) => Self::bare(Status::SerialiseFailed),
        }
    }
}

/// Writes one frame: the length, then the body.
///
/// FLUSHED BEFORE RETURNING, because the caller is blocked on a read that this is the
/// answer to. A buffered writer that held the last frame would be a deadlock that looks
/// exactly like a slow search.
pub fn write_frame(out: &mut impl Write, body: &[u8]) -> std::io::Result<()> {
    if body.len() > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "a frame of {} bytes is past the {MAX_FRAME} limit",
                body.len()
            ),
        ));
    }

    out.write_all(&(body.len() as u32).to_le_bytes())?;
    out.write_all(body)?;
    out.flush()
}

/// Reads one frame, or `None` where the far end has closed the pipe cleanly.
///
/// A clean close is not an error and is how a served process learns it is finished: the
/// parent exits, its end of the pipe goes away, and the next read sees end of file with
/// nothing buffered. A close PART WAY through a frame is a different thing and is reported,
/// because it means a message was lost rather than that none was sent.
pub fn read_frame(input: &mut impl Read) -> std::io::Result<Option<Vec<u8>>> {
    let mut length = [0u8; 4];
    match input.read_exact(&mut length) {
        Ok(()) => {}
        Err(closed) if closed.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(other) => return Err(other),
    }

    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("a frame claiming {length} bytes is past the {MAX_FRAME} limit"),
        ));
    }

    let mut body = vec![0u8; length];
    input.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Answers one request against the engine, opening it where the request says to.
///
/// `engine` is the process's single engine, which starts empty and is filled by
/// [`Request::Open`]. Every other call needs one, and a call that arrives before the open
/// is [`Status::BadHandle`] - what that code meant when there were handles, and what it
/// means now: the caller asked the engine something before there was an engine.
pub fn answer(engine: &mut Option<Service>, request: Request) -> Response {
    // A panic must not end the process, because the process is what the game is waiting
    // on. It would unwind out of the serve loop, which is quieter than unwinding into
    // managed frames used to be and just as final.
    let work = AssertUnwindSafe(|| answer_unguarded(engine, request));
    catch_unwind(work).unwrap_or_else(|_| Response::bare(Status::Panic))
}

fn answer_unguarded(engine: &mut Option<Service>, request: Request) -> Response {
    // Answered before the engine is looked at, because it is a fact about the BUILD rather
    // than about the index - and the caller asks it precisely when it is not yet sure the
    // two sides match.
    if let Request::Version = request {
        return Response::text(env!("CARGO_PKG_VERSION"));
    }

    if let Request::Open { index, variables } = request {
        let variables = variables.map(PathBuf::from);
        return match Service::open(&PathBuf::from(index), variables.as_deref()) {
            Ok(opened) => {
                *engine = Some(opened);
                Response::bare(Status::Ok)
            }
            Err(status) => Response::bare(status),
        };
    }

    let Some(engine) = engine.as_ref() else {
        return Response::bare(Status::BadHandle);
    };

    match request {
        // Both answered above, and unreachable here.
        Request::Version | Request::Open { .. } => Response::bare(Status::BadArgument),
        Request::ConversationCount => Response::value(engine.conversation_count()),
        Request::VariableCount => Response::value(engine.variable_count()),
        Request::IndexFormat => Response::value(engine.index_format()),
        Request::EntryCount { conversation } => match engine.entry_count(conversation) {
            Ok(count) => Response::value(count),
            Err(status) => Response::bare(status),
        },
        Request::ConversationHash { conversation } => {
            match engine.conversation_hash(conversation) {
                Ok(hash) => Response::text(hash),
                Err(status) => Response::bare(status),
            }
        }
        Request::Questions { conversation } => match engine.questions(conversation) {
            Ok(questions) => Response::json(&questions),
            Err(status) => Response::bare(status),
        },
        Request::LookAhead { request } => match engine.look_ahead(&request) {
            Ok(response) => Response::json(&response),
            Err(status) => Response::bare(status),
        },
    }
}

/// Reads requests until the pipe closes, answering each one.
///
/// The whole of the server. It holds one engine for its lifetime, which is why the
/// protocol has no handle in it.
///
/// A frame that will not parse is answered with [`Status::BadArgument`] and the loop goes
/// on, because one unreadable request is not a reason to stop serving the ones after it. A
/// frame that cannot be READ is different - the stream is out of step and every byte after
/// it is suspect - so that ends the loop and is returned.
pub fn serve(input: impl Read, output: impl Write) -> std::io::Result<()> {
    let mut input = BufReader::new(input);
    let mut output = BufWriter::new(output);
    let mut engine: Option<Service> = None;

    while let Some(frame) = read_frame(&mut input)? {
        let response = match serde_json::from_slice::<Request>(&frame) {
            Ok(request) => answer(&mut engine, request),
            Err(_) => Response::bare(Status::BadArgument),
        };

        // A response that will not serialise is a bug here rather than in the caller, and
        // there is nowhere to report it but the status - so it is answered with the code
        // that means exactly that, and the loop continues.
        let body = serde_json::to_vec(&response).unwrap_or_else(|_| {
            serde_json::to_vec(&Response::bare(Status::SerialiseFailed))
                .expect("a status-only response always serialises")
        });
        write_frame(&mut output, &body)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trips one request through the serve loop and reads the answer back.
    ///
    /// The loop itself rather than [`answer`] alone, so the framing is under test too.
    fn served(requests: &[Request]) -> Vec<Response> {
        let mut input: Vec<u8> = Vec::new();
        for request in requests {
            let body = serde_json::to_vec(request).expect("a request serialises");
            write_frame(&mut input, &body).expect("writing to a vec");
        }

        let mut output: Vec<u8> = Vec::new();
        serve(input.as_slice(), &mut output).expect("the loop runs to the end of the input");

        let mut answers = Vec::new();
        let mut reader = output.as_slice();
        while let Some(frame) = read_frame(&mut reader).expect("reading back") {
            answers.push(serde_json::from_slice(&frame).expect("a response parses"));
        }
        answers
    }

    /// THE WIRE SHAPE, written out rather than round-tripped.
    ///
    /// A round trip through serde proves the two halves of serde agree, which they always
    /// will. What the .NET client is written against is these exact bytes, and it does not
    /// use serde - so this is the contract, and a derive attribute changed without meaning
    /// to should fail here rather than in the game.
    #[test]
    fn a_request_looks_on_the_wire_the_way_the_client_writes_it() {
        let wrote = |request: &Request| serde_json::to_string(request).expect("serialises");

        assert_eq!(wrote(&Request::Version), r#""version""#);
        assert_eq!(
            wrote(&Request::ConversationCount),
            r#""conversation_count""#
        );
        assert_eq!(
            wrote(&Request::EntryCount { conversation: 631 }),
            r#"{"entry_count":{"conversation":631}}"#,
        );
        assert_eq!(
            wrote(&Request::Open {
                index: "i.jsonl".into(),
                variables: None
            }),
            r#"{"open":{"index":"i.jsonl","variables":null}}"#,
        );
        assert_eq!(
            wrote(&Request::LookAhead {
                request: "{}".into()
            }),
            r#"{"look_ahead":{"request":"{}"}}"#,
        );
    }

    /// And the answer, whose absent fields are absent rather than null.
    #[test]
    fn a_response_looks_on_the_wire_the_way_the_client_reads_it() {
        let wrote = |response: &Response| serde_json::to_string(response).expect("serialises");

        assert_eq!(wrote(&Response::bare(Status::Ok)), r#"{"status":0}"#);
        assert_eq!(
            wrote(&Response::bare(Status::NoSuchConversation)),
            r#"{"status":-5}"#,
        );
        assert_eq!(wrote(&Response::value(7)), r#"{"status":0,"value":7}"#);
        assert_eq!(wrote(&Response::text("hi")), r#"{"status":0,"text":"hi"}"#);
    }

    #[test]
    fn a_frame_round_trips_through_its_own_length_prefix() {
        let mut buffer: Vec<u8> = Vec::new();
        write_frame(&mut buffer, b"hello").expect("writing");
        write_frame(&mut buffer, b"").expect("an empty body is a legal frame");

        let mut reader = buffer.as_slice();
        assert_eq!(
            read_frame(&mut reader).unwrap().as_deref(),
            Some(&b"hello"[..])
        );
        assert_eq!(read_frame(&mut reader).unwrap().as_deref(), Some(&b""[..]));
        assert_eq!(
            read_frame(&mut reader).unwrap(),
            None,
            "and then the end of the pipe"
        );
    }

    /// A length nobody meant to send is an error, not an allocation.
    #[test]
    fn a_frame_longer_than_the_limit_is_refused_rather_than_allocated() {
        let mut claimed = (MAX_FRAME as u32 + 1).to_le_bytes().to_vec();
        claimed.extend_from_slice(b"not actually that long");

        let refused = read_frame(&mut claimed.as_slice()).expect_err("past the limit");
        assert_eq!(refused.kind(), std::io::ErrorKind::InvalidData);
    }

    /// A close part way through a frame is a lost message, not a clean end.
    #[test]
    fn a_truncated_frame_is_an_error_and_not_an_end_of_pipe() {
        let mut truncated = 64u32.to_le_bytes().to_vec();
        truncated.extend_from_slice(b"only a few bytes of it");

        assert!(read_frame(&mut truncated.as_slice()).is_err());
    }

    /// The version is answerable before anything is open, which is when it is asked.
    #[test]
    fn the_version_needs_no_engine() {
        let answers = served(&[Request::Version]);
        assert_eq!(answers[0].status, Status::Ok);
        assert_eq!(answers[0].text.as_deref(), Some(env!("CARGO_PKG_VERSION")));
    }

    /// Everything else does, and says so with the code that has always meant it.
    #[test]
    fn a_call_before_the_open_is_a_bad_handle() {
        let answers = served(&[
            Request::ConversationCount,
            Request::EntryCount { conversation: 631 },
            Request::LookAhead {
                request: "{}".into(),
            },
        ]);

        assert_eq!(answers.len(), 3);
        for answer in &answers {
            assert_eq!(answer.status, Status::BadHandle);
            assert!(
                answer.value.is_none(),
                "a refused call must not answer a number"
            );
            assert!(
                answer.text.is_none(),
                "a refused call must not answer a string"
            );
        }
    }

    #[test]
    fn opening_a_path_that_is_not_an_index_reports_it_and_keeps_serving() {
        let answers = served(&[
            Request::Open {
                index: "no-such-file.jsonl".into(),
                variables: None,
            },
            Request::Version,
        ]);

        assert_eq!(answers[0].status, Status::IndexUnreadable);
        assert_eq!(
            answers[1].status,
            Status::Ok,
            "one bad request does not end the server"
        );
    }

    /// A frame that is not a request at all is answered rather than acted on.
    #[test]
    fn a_frame_that_is_not_a_request_is_a_bad_argument() {
        let mut input: Vec<u8> = Vec::new();
        write_frame(&mut input, b"not json at all").expect("writing");
        write_frame(&mut input, br#"{"no_such_kind":{}}"#).expect("writing");

        let mut output: Vec<u8> = Vec::new();
        serve(input.as_slice(), &mut output).expect("the loop survives both");

        let mut reader = output.as_slice();
        while let Some(frame) = read_frame(&mut reader).unwrap() {
            let response: Response = serde_json::from_slice(&frame).unwrap();
            assert_eq!(response.status, Status::BadArgument);
        }
    }
}
