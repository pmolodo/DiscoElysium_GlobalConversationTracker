// SPDX-License-Identifier: MIT

pub mod core;
pub mod parser;
pub mod graph;
pub mod world;
pub mod engine;
pub mod index;
pub mod symbolic;

/// What crosses between the plugin and this engine, and what it means.
pub mod bridge;

/// The C ABI the game plugin reaches this engine through.
pub mod ffi;

/// Builds small graphs for tests, the way the database writes them.
#[cfg(test)]
pub(crate) mod test_graph;

/// Tests that cross module boundaries, and so belong to no single module.
#[cfg(test)]
mod integration_tests;
