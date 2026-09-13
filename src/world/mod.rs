// SPDX-License-Identifier: MIT

pub mod test_world;
// `world::world` is the path every user of the trait already spells; see `graph::graph` for
// why the lint is answered here rather than by renaming.
#[allow(clippy::module_inception)]
pub mod world;
