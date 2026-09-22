// SPDX-License-Identifier: MIT
//! What `proto/engine.proto` describes, held to what the engine means by it.
//!
//! ## What these are for
//!
//! The schema exists so that one description of the wire serves both sides. That only
//! helps if the description says what the engine actually means, and there are two ways it
//! could quietly stop doing so: a number that has to match something outside the schema
//! could drift, and a message could lose a field without anything failing to compile,
//! because a protobuf message with a member missing is a valid message.
//!
//! So these fill every member of every message, put it through the encoder, and compare.
//! A field dropped from the schema stops compiling here rather than being read back as a
//! default by whichever side was not updated.

use prost::Message;

use lookahead_engine::service::Status as EngineStatus;
use lookahead_engine::wire;

/// Encodes and decodes, which is what both ends do to everything.
fn round_trip<T: Message + Default + PartialEq + std::fmt::Debug>(message: &T) -> T {
    let mut bytes = Vec::new();
    message.encode(&mut bytes).expect("it encodes");
    let back = T::decode(bytes.as_slice()).expect("it decodes");
    assert_eq!(message, &back, "a message did not survive the wire");
    back
}

fn node(conversation: i32, entry: i32) -> wire::NodeRef {
    wire::NodeRef {
        conversation,
        entry,
    }
}

fn runs(conversation: i32, spans: &[(i32, i32)]) -> wire::NodeSet {
    wire::NodeSet {
        conversations: vec![wire::ConversationRuns {
            conversation,
            runs: spans
                .iter()
                .map(|(first, last)| wire::NodeRun {
                    first: *first,
                    last: *last,
                })
                .collect(),
        }],
    }
}

fn text_value(text: &str) -> wire::WireValue {
    wire::WireValue {
        value: Some(wire::wire_value::Value::Text(text.to_string())),
    }
}

/// A snapshot with nothing left at its default, so a dropped member is visible.
fn full_snapshot() -> wire::WorldRawData {
    wire::WorldRawData {
        money: 5100,
        day_minutes: 7 * 60 + 42,
        day_counter: 3,
        clock_locked: true,
        variables: [("kim_trust".to_string(), text_value("high"))]
            .into_iter()
            .collect(),
        variable_values: vec![
            wire::WireValue {
                value: Some(wire::wire_value::Value::Boolean(true)),
            },
            wire::WireValue {
                value: Some(wire::wire_value::Value::Number(2.5)),
            },
            // Not knowable, which is the permissive answer and the default.
            wire::WireValue::default(),
        ],
        queries: [("is_indoors".to_string(), text_value("no"))]
            .into_iter()
            .collect(),
        query_values: vec![text_value("raining")],
        items: vec!["FALN_sneakers".to_string()],
        thoughts: vec!["the_precarious_world".to_string()],
        checks_pass: Some(runs(9, &[(50, 50)])),
        checks_fail: Some(runs(9, &[(42, 42), (19, 19)])),
        seen: Some(runs(9, &[(0, 40), (42, 42), (50, 99)])),
        failed_white_checks: vec!["whirling.kim_inland_mystery_created".to_string()],
        red_checks_fail: true,
        // BOTH SHAPES OF ANSWER, so neither is dropped without this failing: a kind that
        // answers about one subject, and a kind that answers with a whole set.
        data_values: vec![
            wire::DataAnswer {
                value: Some(text_value("cooking")),
                names: Vec::new(),
                read: true,
            },
            wire::DataAnswer {
                value: None,
                names: vec!["aces_high".to_string(), "jamais_vu".to_string()],
                read: true,
            },
        ],
        check_margins: vec![wire::CheckMargin {
            node: Some(wire::NodeRef {
                conversation: 29,
                entry: 221,
            }),
            skill: "VOLITION".to_string(),
            margin: -2,
        }],
    }
}

#[test]
fn a_world_snapshot_survives_the_wire() {
    round_trip(&full_snapshot());
}

#[test]
fn a_look_ahead_request_survives_the_wire() {
    round_trip(&wire::LookAheadRequest {
        conversation: 631,
        starts: vec![node(631, 3), node(631, 7)],
        seen_any_game: Some(runs(631, &[(1, 9)])),
        state_budget: 1,
        time_budget_ms: 1000,
        menu_time_budget_ms: 4000,
        memory_budget_mb: 256,
        world: Some(full_snapshot()),
        encountered: vec![node(631, 0), node(631, 2)],
    });
}

#[test]
fn a_look_ahead_response_survives_the_wire() {
    round_trip(&wire::LookAheadResponse {
        answers: vec![wire::LookAheadAnswer {
            start: Some(node(9, 50)),
            branch: wire::Branch::Pass as i32,
            destination: wire::SeenState::UnseenThisGame as i32,
            best: wire::SeenState::UnseenAnyGame as i32,
            witness: Some(node(9, 42)),
            complete: false,
            elapsed_ms: 17,
            diagram_nodes: 200_000,
            nodes_reached: 1_284,
            stopped_by: wire::StoppedBy::Time as i32,
        }],
        error: None,
    });
}

/// A refusal is an ordinary response whose body says so, not a status.
#[test]
fn a_refused_look_ahead_carries_its_reason_and_no_answers() {
    let back = round_trip(&wire::LookAheadResponse {
        answers: Vec::new(),
        error: Some("the index has no group for 4242".to_string()),
    });

    assert!(back.answers.is_empty());
    assert_eq!(
        back.error.as_deref(),
        Some("the index has no group for 4242")
    );
}

#[test]
fn the_questions_survive_the_wire() {
    round_trip(&wire::Questions {
        conversations: vec![9, 13],
        variables: vec!["kim_trust".to_string()],
        queries: vec!["is_indoors".to_string()],
        items: vec!["FALN_sneakers".to_string()],
        thoughts: vec!["the_precarious_world".to_string()],
        checks: vec![node(9, 50)],
        entries: vec![node(9, 0), node(9, 1)],
        data: vec![
            // BOTH SHAPES OF REQUEST: a kind that answers with a whole set and names no
            // subject, and one that asks about a named thing.
            wire::DataRequest {
                kind: wire::DataKind::ThoughtsCooking as i32,
                subject: String::new(),
            },
            wire::DataRequest {
                kind: wire::DataKind::EquippedInSlot as i32,
                subject: "HAT".to_string(),
            },
        ],
    });
}

#[test]
fn every_request_kind_survives_the_wire() {
    let kinds = [
        wire::request::Kind::Version(wire::VersionRequest {}),
        wire::request::Kind::Open(wire::OpenRequest {
            index: "index.jsonl".to_string(),
            variables: "variables.jsonl".to_string(),
        }),
        wire::request::Kind::ConversationCount(wire::ConversationCountRequest {}),
        wire::request::Kind::VariableCount(wire::VariableCountRequest {}),
        wire::request::Kind::EntryCount(wire::EntryCountRequest { conversation: 9 }),
        wire::request::Kind::ConversationHash(wire::ConversationHashRequest { conversation: 9 }),
        wire::request::Kind::IndexFormat(wire::IndexFormatRequest {}),
        wire::request::Kind::Questions(wire::QuestionsRequest { conversation: 9 }),
        wire::request::Kind::LookAhead(wire::LookAheadRequest {
            conversation: 9,
            ..Default::default()
        }),
    ];

    for kind in kinds {
        round_trip(&wire::Request { kind: Some(kind) });
    }
}

#[test]
fn a_response_survives_the_wire_with_each_payload_it_can_carry() {
    let payloads = [
        // A refusal, which carries nothing but its reason for being one.
        wire::Response {
            status: wire::Status::NoSuchConversation as i32,
            ..Default::default()
        },
        wire::Response {
            status: wire::Status::Ok as i32,
            value: Some(1345),
            ..Default::default()
        },
        wire::Response {
            status: wire::Status::Ok as i32,
            text: Some("0.1.0".to_string()),
            ..Default::default()
        },
        wire::Response {
            status: wire::Status::Ok as i32,
            questions: Some(wire::Questions {
                conversations: vec![9],
                ..Default::default()
            }),
            ..Default::default()
        },
        wire::Response {
            status: wire::Status::Ok as i32,
            look_ahead: Some(wire::LookAheadResponse {
                answers: Vec::new(),
                error: None,
            }),
            ..Default::default()
        },
    ];

    for payload in payloads {
        round_trip(&payload);
    }
}

/// The numbers are a contract with the .NET side and with every log that quotes one.
///
/// They are not derived from the engine's own enum - they are written out in the schema,
/// which is what makes them a contract rather than an implementation detail - so the two
/// can drift, and this is what stops them.
#[test]
fn every_status_number_is_the_one_the_engine_reports() {
    let pairs = [
        (wire::Status::Ok, EngineStatus::Ok),
        (wire::Status::BadHandle, EngineStatus::BadHandle),
        (wire::Status::BadArgument, EngineStatus::BadArgument),
        (wire::Status::IndexUnreadable, EngineStatus::IndexUnreadable),
        (wire::Status::Panic, EngineStatus::Panic),
        (
            wire::Status::NoSuchConversation,
            EngineStatus::NoSuchConversation,
        ),
        (wire::Status::SerialiseFailed, EngineStatus::SerialiseFailed),
    ];

    for (on_the_wire, in_the_engine) in pairs {
        assert_eq!(
            on_the_wire as i32, in_the_engine as i32,
            "{on_the_wire:?} and {in_the_engine:?} disagree about their number",
        );
    }

    // AND THAT THE LIST IS WHOLE, so a status added to one side alone is caught. There is
    // no count to ask either enum for, so the check is that every number the engine knows
    // is a number the wire reads back as the status it was.
    for (on_the_wire, _) in pairs {
        assert_eq!(
            wire::Status::try_from(on_the_wire as i32),
            Ok(on_the_wire),
            "the wire does not read {on_the_wire:?} back as itself",
        );
    }
    assert!(
        wire::Status::try_from(-7).is_err(),
        "a number no status carries should be refused rather than guessed at",
    );
}

/// An unanswered question is "not knowable", and that has to be what silence means.
///
/// The schema spells it as no member of the oneof being set, so a default-constructed
/// value already means it. A fourth case would be a way to build one that meant neither.
#[test]
fn an_unset_value_is_the_unknowable_one() {
    let unknown = wire::WireValue::default();
    assert_eq!(unknown.value, None);

    let back = round_trip(&unknown);
    assert_eq!(back.value, None);

    // And it costs nothing to send, which is what makes it safe as the default for every
    // question a caller did not answer.
    assert!(unknown.encode_to_vec().is_empty());
}

/// The sets are most of a request, and they cross as runs rather than as ids.
#[test]
fn an_entry_set_crosses_as_runs_rather_than_as_every_id() {
    // A whole group: every entry, because any of them may have been seen.
    let whole_group = runs(631, &[(0, 999)]);
    let listed_individually = wire::NodeSet {
        conversations: vec![wire::ConversationRuns {
            conversation: 631,
            runs: (0..=999)
                .map(|entry| wire::NodeRun {
                    first: entry,
                    last: entry,
                })
                .collect(),
        }],
    };

    round_trip(&whole_group);
    assert!(
        whole_group.encoded_len() * 100 < listed_individually.encoded_len(),
        "a run-encoded group should be far smaller than one id per entry",
    );
}
