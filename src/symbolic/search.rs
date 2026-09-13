// SPDX-License-Identifier: MIT
//! What a symbolic walk over one group is asked against.

use super::guard_formula::GuardCompiler;
use crate::graph::LookAheadGraph;
use crate::world::ILookAheadWorld;

/// What every symbolic walk over one group is asked against: the group, the compiler its
/// guards go through, the world the search cannot change, and the cap its counters saturate
/// at.
///
/// ONE VALUE BECAUSE THEY TRAVEL TOGETHER. The backward crawls, the pooled meeting and every
/// menu marking take all four, and the same four for the whole of one menu. As four arguments
/// they would sit in every signature in the search, beside - and outnumbering - the arguments
/// that say what is actually being asked.
///
/// BY VALUE, HOLDING THE COMPILER'S MUTABLE BORROW, so it is handed on rather than shared: a
/// function that asks more than one question passes [`Search::reborrow`] to each.
pub struct Search<'s, 'a> {
    pub graph: &'s LookAheadGraph,
    pub compiler: &'s mut GuardCompiler<'a>,
    pub world: &'s dyn ILookAheadWorld,
    pub counter_cap: u32,
}

impl<'a> Search<'_, 'a> {
    /// The same search, lent for one call and handed back when it returns.
    pub fn reborrow(&mut self) -> Search<'_, 'a> {
        Search {
            graph: self.graph,
            compiler: &mut *self.compiler,
            world: self.world,
            counter_cap: self.counter_cap,
        }
    }
}
