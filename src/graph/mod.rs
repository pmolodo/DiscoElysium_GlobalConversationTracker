// SPDX-License-Identifier: MIT

// `graph::graph` is the path every user of the type already spells, across the engine, the
// tests and the measurements; renaming the module to satisfy the lint would change all of
// them and say nothing clearer.
#[allow(clippy::module_inception)]
pub mod graph;
pub mod node;
