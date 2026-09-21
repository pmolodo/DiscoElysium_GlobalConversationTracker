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
//! A four-byte LITTLE-ENDIAN length, then that many bytes of an encoded protobuf message.
//! Both directions, and that is the whole of it.
//!
//! Length-prefixed rather than delimited because an encoded message is binary and contains
//! every byte value, so there is no delimiter to choose. Little-endian because both ends
//! are x86-64 and `BitConverter` on the .NET side is little-endian on every platform the
//! game ships on - stated here because it is the kind of thing that is silently assumed and
//! then silently wrong.
//!
//! ## The bodies are generated from one schema
//!
//! `proto/engine.proto` describes everything that crosses, and both sides generate their
//! types from it - see [`crate::wire`] for this one. The shape used to be written twice,
//! here as serde types and on the .NET side as classes composing the same members by hand,
//! and the two agreed only because someone remembered. A look-ahead request also had to be
//! serialised to JSON and then embedded in a JSON envelope as a string, which is a wire
//! admitting it could not carry what it was carrying.
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

use prost::Message;

use crate::service::{Service, Status};
use crate::wire_convert;

/// The largest frame either side will read.
///
/// Sixteen megabytes, which is far more than anything the protocol actually carries - the
/// biggest body is a look-ahead response for one response menu - and far less than an
/// allocation that would hurt. It exists so that a length nobody meant to send is an error
/// rather than an out-of-memory abort.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// What the caller asked for, and what goes back.
///
/// Both are the generated types, re-exported so a reader of this module finds them where
/// the protocol is described rather than having to know which schema package they came
/// from. `proto/engine.proto` is where their members are documented.
///
/// A request's `kind` is a oneof, so a kind this build does not know arrives as a field
/// number it does not recognise - which decodes to nothing rather than to something it
/// acts on by accident.
pub use crate::wire::{Request, Response};

/// The kinds a request can be, as the generated oneof spells them.
pub use crate::wire::request::Kind;

/// Builders for the answers this module gives.
///
/// Free functions rather than an `impl` block, because [`Response`] is generated and an
/// inherent impl on it would live here while its members live in the schema - two places
/// to look for one type. These are the host's own vocabulary for filling it in.
mod answers {
    use super::{Response, Status};
    use crate::wire;

    /// A status and nothing else - a refusal, or a call whose whole answer is that it
    /// worked.
    pub fn bare(status: Status) -> Response {
        Response {
            status: status as i32,
            ..Default::default()
        }
    }

    /// A number.
    pub fn value(value: i32) -> Response {
        Response {
            status: Status::Ok as i32,
            value: Some(value),
            ..Default::default()
        }
    }

    /// A string.
    pub fn text(text: impl Into<String>) -> Response {
        Response {
            status: Status::Ok as i32,
            text: Some(text.into()),
            ..Default::default()
        }
    }

    /// The questions a group can ask.
    pub fn questions(questions: wire::Questions) -> Response {
        Response {
            status: Status::Ok as i32,
            questions: Some(questions),
            ..Default::default()
        }
    }

    /// A whole menu's worth of answers.
    pub fn look_ahead(response: wire::LookAheadResponse) -> Response {
        Response {
            status: Status::Ok as i32,
            look_ahead: Some(response),
            ..Default::default()
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
    catch_unwind(work).unwrap_or_else(|_| answers::bare(Status::Panic))
}

fn answer_unguarded(engine: &mut Option<Service>, request: Request) -> Response {
    // A request naming no kind at all. Either a caller sent an empty message or one built
    // from a newer schema sent a kind this build has no field for, and the two are
    // indistinguishable here - which is the right answer to both: this cannot act on it.
    let Some(kind) = request.kind else {
        return answers::bare(Status::BadArgument);
    };

    // Answered before the engine is looked at, because it is a fact about the BUILD rather
    // than about the index - and the caller asks it precisely when it is not yet sure the
    // two sides match.
    if let Kind::Version(_) = kind {
        return answers::text(env!("CARGO_PKG_VERSION"));
    }

    if let Kind::Open(open) = kind {
        // AN EMPTY PATH IS WHAT AN OMITTED TABLE ARRIVES AS, since proto3 has no way to say
        // required and the field is a plain string - see the proto. Refused here rather than
        // handed on, so the reason is "no table named" rather than an io error about "".
        if open.variables.is_empty() {
            return answers::bare(Status::BadArgument);
        }
        return match Service::open(&PathBuf::from(open.index), &PathBuf::from(open.variables)) {
            Ok(opened) => {
                *engine = Some(opened);
                answers::bare(Status::Ok)
            }
            Err(status) => answers::bare(status),
        };
    }

    let Some(engine) = engine.as_ref() else {
        return answers::bare(Status::BadHandle);
    };

    match kind {
        // Both answered above, and unreachable here.
        Kind::Version(_) | Kind::Open(_) => answers::bare(Status::BadArgument),
        Kind::ConversationCount(_) => answers::value(engine.conversation_count()),
        Kind::VariableCount(_) => answers::value(engine.variable_count()),
        Kind::IndexFormat(_) => answers::value(engine.index_format()),
        Kind::EntryCount(asked) => match engine.entry_count(asked.conversation) {
            Ok(count) => answers::value(count),
            Err(status) => answers::bare(status),
        },
        Kind::ConversationHash(asked) => match engine.conversation_hash(asked.conversation) {
            Ok(hash) => answers::text(hash),
            Err(status) => answers::bare(status),
        },
        Kind::Questions(asked) => match engine.questions(asked.conversation) {
            Ok(questions) => answers::questions(wire_convert::write_questions(questions)),
            Err(status) => answers::bare(status),
        },
        Kind::LookAhead(asked) => match wire_convert::read_look_ahead(asked) {
            // A request whose SHAPE will not read is BadArgument, the same as one whose
            // bytes would not decode: nothing was asked. A request that reads and cannot
            // be SERVED is a successful response carrying `error`, which is the engine's
            // answer rather than the wire's - see the module note.
            Err(_) => answers::bare(Status::BadArgument),
            Ok(request) => answers::look_ahead(wire_convert::write_look_ahead(
                engine.answer_request(request),
            )),
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
        let response = match Request::decode(frame.as_slice()) {
            Ok(request) => answer(&mut engine, request),
            Err(_) => answers::bare(Status::BadArgument),
        };

        // ENCODING CANNOT FAIL for a generated message - prost writes into a Vec that
        // grows - so there is no fallback here, unlike the JSON writer this replaced,
        // which could refuse a value and needed a status for it.
        write_frame(&mut output, &response.encode_to_vec())?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire;

    /// A request of one kind, which is how every caller builds one.
    fn asking(kind: Kind) -> Request {
        Request { kind: Some(kind) }
    }

    /// Round-trips requests through the serve loop and reads the answers back.
    ///
    /// The loop itself rather than [`answer`] alone, so the framing is under test too.
    fn served(requests: &[Request]) -> Vec<Response> {
        let mut input: Vec<u8> = Vec::new();
        for request in requests {
            write_frame(&mut input, &request.encode_to_vec()).expect("writing to a vec");
        }

        let mut output: Vec<u8> = Vec::new();
        serve(input.as_slice(), &mut output).expect("the loop runs to the end of the input");

        let mut answers = Vec::new();
        let mut reader = output.as_slice();
        while let Some(frame) = read_frame(&mut reader).expect("reading back") {
            answers.push(Response::decode(frame.as_slice()).expect("a response decodes"));
        }
        answers
    }

    /// The status a response carries, as the engine's own enum.
    fn status_of(response: &Response) -> Status {
        Status::try_from(response.status).expect("a status this build knows")
    }

    /// THE WIRE SHAPE, written out as bytes rather than round-tripped.
    ///
    /// A round trip through the generated encoder proves the encoder agrees with the
    /// decoder, which it always will. What the .NET client is written against is these
    /// exact bytes, and it decodes them with its own generated code from the same schema -
    /// so this is the contract, and a FIELD NUMBER changed without meaning to should fail
    /// here rather than in the game.
    ///
    /// Field numbers are what protobuf actually carries; the names are not on the wire at
    /// all. So a renamed field is invisible and harmless, and a renumbered one is silent
    /// and not - it decodes as whatever the other side has under that number, or as
    /// nothing. These bytes are spelled out so that a renumber cannot be silent.
    #[test]
    fn a_request_looks_on_the_wire_the_way_the_client_writes_it() {
        // Tag byte = (field number << 3) | wire type. Kind::Version is field 1 and a
        // message, so 0x0a, and an empty VersionRequest is zero bytes long.
        assert_eq!(
            asking(Kind::Version(wire::VersionRequest {})).encode_to_vec(),
            vec![0x0a, 0x00],
        );

        // Field 3, same shape.
        assert_eq!(
            asking(Kind::ConversationCount(wire::ConversationCountRequest {})).encode_to_vec(),
            vec![0x1a, 0x00],
        );

        // Field 5, two bytes long, holding its own field 1 varint 631.
        assert_eq!(
            asking(Kind::EntryCount(wire::EntryCountRequest {
                conversation: 631
            }))
            .encode_to_vec(),
            vec![0x2a, 0x03, 0x08, 0xf7, 0x04],
        );

        // Field 2, holding both paths as strings in ITS fields 1 and 2 - seven bytes of
        // name apiece behind a tag and a length.
        assert_eq!(
            asking(Kind::Open(wire::OpenRequest {
                index: "i.jsonl".into(),
                variables: "v.jsonl".into(),
            }))
            .encode_to_vec(),
            vec![
                0x12, 0x12, //
                0x0a, 0x07, b'i', b'.', b'j', b's', b'o', b'n', b'l', //
                0x12, 0x07, b'v', b'.', b'j', b's', b'o', b'n', b'l',
            ],
        );
    }

    /// And the answer, whose absent members take no bytes rather than a null.
    #[test]
    fn a_response_looks_on_the_wire_the_way_the_client_reads_it() {
        // A success carries nothing at all: status is field 1 and zero is the default, and
        // protobuf does not write a default. An empty frame IS "it worked".
        assert_eq!(answers::bare(Status::Ok).encode_to_vec(), Vec::<u8>::new());

        // A refusal is the status and nothing else. Negative enums are varint-encoded as
        // ten bytes, which is what proto3 does with a negative and is why the numbers
        // being negative is a decision the schema states rather than hides.
        let refused = answers::bare(Status::NoSuchConversation).encode_to_vec();
        assert_eq!(refused[0], 0x08, "field 1, a varint");
        assert_eq!(
            Response::decode(refused.as_slice())
                .expect("it decodes")
                .status,
            Status::NoSuchConversation as i32,
        );

        // Field 2, a varint.
        assert_eq!(answers::value(7).encode_to_vec(), vec![0x10, 0x07]);
        // Field 3, a string.
        assert_eq!(
            answers::text("hi").encode_to_vec(),
            vec![0x1a, 0x02, b'h', b'i'],
        );
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
        let answers = served(&[asking(Kind::Version(wire::VersionRequest {}))]);
        assert_eq!(status_of(&answers[0]), Status::Ok);
        assert_eq!(answers[0].text.as_deref(), Some(env!("CARGO_PKG_VERSION")));
    }

    /// Everything else does, and says so with the code that has always meant it.
    #[test]
    fn a_call_before_the_open_is_a_bad_handle() {
        let answers = served(&[
            asking(Kind::ConversationCount(wire::ConversationCountRequest {})),
            asking(Kind::EntryCount(wire::EntryCountRequest {
                conversation: 631,
            })),
            asking(Kind::LookAhead(wire::LookAheadRequest::default())),
        ]);

        assert_eq!(answers.len(), 3);
        for answer in &answers {
            assert_eq!(status_of(answer), Status::BadHandle);
            assert!(
                answer.value.is_none(),
                "a refused call must not answer a number"
            );
            assert!(
                answer.text.is_none(),
                "a refused call must not answer a string"
            );
            assert!(
                answer.questions.is_none() && answer.look_ahead.is_none(),
                "nor a message"
            );
        }
    }

    #[test]
    fn opening_a_path_that_is_not_an_index_reports_it_and_keeps_serving() {
        // A table that reads, so the refusal is about the index and nothing else.
        let table = std::env::temp_dir().join("degct-host-open-table.jsonl");
        std::fs::write(&table, "").expect("an empty table writes");

        let answers = served(&[
            asking(Kind::Open(wire::OpenRequest {
                index: "no-such-file.jsonl".into(),
                variables: table.to_string_lossy().into_owned(),
            })),
            asking(Kind::Version(wire::VersionRequest {})),
        ]);

        assert_eq!(status_of(&answers[0]), Status::IndexUnreadable);
        assert_eq!(
            status_of(&answers[1]),
            Status::Ok,
            "one bad request does not end the server"
        );
    }

    /// An open naming no table is refused before the index is even looked at.
    ///
    /// The engine answers a variable the plugin could not read with what the database
    /// declares it starts as, and without a table it would have to answer Unknown - which a
    /// symbolic search cannot prune on, so both branches of every guard reading such a
    /// variable stay in the crawl. See `Service::open`.
    #[test]
    fn opening_without_naming_a_variable_table_is_refused() {
        let answers = served(&[
            asking(Kind::Open(wire::OpenRequest {
                index: "no-such-file.jsonl".into(),
                variables: String::new(),
            })),
            asking(Kind::Version(wire::VersionRequest {})),
        ]);

        assert_eq!(
            status_of(&answers[0]),
            Status::BadArgument,
            "an omitted table is the argument being wrong, not the index being unreadable"
        );
        assert_eq!(status_of(&answers[1]), Status::Ok);
    }

    /// A frame that is not a request at all is answered rather than acted on.
    ///
    /// A REQUEST NAMING NO KIND IS ONE OF THESE, and it is what a kind from a newer schema
    /// arrives as: the field number carrying it is not one this build has, so it is skipped
    /// and what is left names nothing. Answering rather than guessing is the whole reason
    /// the kinds are a oneof.
    #[test]
    fn a_frame_that_is_not_a_request_is_a_bad_argument() {
        let mut input: Vec<u8> = Vec::new();
        // Field 15, a varint, which no Request has - an unknown kind, skipped on decode.
        write_frame(&mut input, &[0x78, 0x01]).expect("writing");
        // A request that decodes and names nothing.
        write_frame(&mut input, &Request::default().encode_to_vec()).expect("writing");
        // Bytes that are not a message at all: field 0 is illegal in every protobuf.
        write_frame(&mut input, &[0x00, 0x01, 0x02]).expect("writing");

        let mut output: Vec<u8> = Vec::new();
        serve(input.as_slice(), &mut output).expect("the loop survives all three");

        let mut seen = 0;
        let mut reader = output.as_slice();
        while let Some(frame) = read_frame(&mut reader).unwrap() {
            let response = Response::decode(frame.as_slice()).unwrap();
            assert_eq!(status_of(&response), Status::BadArgument);
            seen += 1;
        }
        assert_eq!(seen, 3, "every one of them is answered");
    }
}
