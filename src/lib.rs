// SPDX-License-Identifier: MIT

pub mod core;
pub mod graph;
pub mod index;
pub mod parser;
pub mod symbolic;
pub mod world;

/// The file formats this repository defines, read and written in one place.
///
/// TWO CRATES BEHIND ONE NAME, re-exported here under the name they had as a module so that
/// every path naming them still names them. `gct_formats` is what a running game reads and
/// `gct_save_files` is how this repository stores a save; the second re-exports the first,
/// so this reaches both. The split is for the library the mod links, which takes the first
/// alone - see either crate's own note.
pub use gct_save_files as formats;

/// The types that cross the pipe, generated from `proto/engine.proto`.
pub mod wire;

/// Between those generated types and the engine's own.
pub mod wire_convert;

/// What crosses between the plugin and this engine, and what it means.
pub mod bridge;

/// The engine's work, with no transport attached to it.
pub mod service;

/// That work, spoken to over a pipe by a process that is not the game.
pub mod host;

/// Builds small graphs for tests, the way the database writes them.
///
/// PUBLIC, like `world::test_world` beside it: the integration tests are separate crates
/// and can only reach what the library exports, and a generated fixture built by hand there
/// would intern its flag and seen slots by its own rules rather than by the builder's.
pub mod test_graph;

/// A state-at-a-time walk, for the tests to check the searches against.
///
/// NOT SOMETHING THE PRODUCT RUNS, and here beside `world::test_world` for the same reason
/// that is: the integration tests are separate crates and can only reach what the library
/// exports. Guards, costs, once slots and cycles are covered in `symbolic::reachability`
/// and `symbolic::backward` over their own fixtures; this is what those fixtures cannot be,
/// an oracle independent of the searches they check (de-eonm).
pub mod oracle;

/// The walk a scenario's inputs make through a conversation, for the offline runner.
///
/// NOT SOMETHING THE PRODUCT RUNS: in game the game itself walks the conversation, and the
/// harness presses the same inputs through the probe. Public for the reason `oracle` is.
pub mod walkthrough;

/// A manager that outlives one query, on the thread that owns it - de-2wtl.
pub mod workspace;
