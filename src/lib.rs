// SPDX-License-Identifier: MIT

pub mod core;
pub mod parser;
pub mod graph;
pub mod world;
pub mod index;
pub mod symbolic;

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

/// A manager that outlives one query, on the thread that owns it - de-2wtl.
pub mod workspace;
