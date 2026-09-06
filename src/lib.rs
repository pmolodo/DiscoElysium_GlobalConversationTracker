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
#[cfg(test)]
pub(crate) mod test_graph;

// Guards, costs, once slots and cycles are covered in `symbolic::reachability` and
// `symbolic::backward`, over their own fixtures. de-eonm carries what those fixtures
// cannot be: an oracle independent of the searches they check.
