// SPDX-License-Identifier: MIT
//! How much memory a decision diagram may spend, and what that buys.
//!
//! ONE NUMBER, IN BYTES, IS WHAT A CALLER SETS. It is the same quantity the player's
//! `LookAheadMemoryBudgetMb` sets and the same one the forward crawl is held to, which is
//! the only way two searches can be read against each other (de-e23q).
//!
//! Callers used to pass a node capacity and a cache capacity instead, picked per file:
//! `1 << 24` in one measurement and `1 << 20` in another, which at sixteen bytes a node is
//! a ceiling of 268 MB against 17 MB. Numbers taken under two different ceilings cannot be
//! compared, and a verdict of "no room" then said the harness rationed the diagram rather
//! than the diagram failing to fit.

use oxidd::bdd::{new_manager, BDDManagerRef};

/// A memory allowance for one diagram, and the capacities it works out to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagramBudget {
    memory: usize,
}

impl DiagramBudget {
    /// What one node costs, all in, in bytes.
    ///
    /// THE PARTS, and where the figure comes from. tests/manager_memory.rs counts what the
    /// allocator is asked for when a manager is built, which gives the two that are paid up
    /// front:
    ///
    /// - the node itself, sixteen bytes in oxidd's index manager: a reference count, a
    ///   level, and two four-byte edges;
    /// - the apply cache, at one entry per [`Self::NODES_PER_CACHE_ENTRY`] nodes and about
    ///   twenty bytes an entry, so five bytes a node - and up to five more, because oxidd
    ///   rounds a cache capacity up to a power of two and the ask can land just above one.
    ///
    /// Construction therefore measures 21 to 24 bytes a node, the spread being that
    /// rounding. IT IS A FLOOR RATHER THAN THE TRUE COST: the unique table that finds a
    /// node again starts empty - `unique_table: Vec::new()` - and grows as nodes are
    /// actually inserted, so an empty manager has not yet paid for it.
    ///
    /// 32 leaves that growth about eight bytes a node, which is the shape of a table of
    /// four-byte indices at a sensible load factor. It is deliberately not tuned to the
    /// construction measurement alone: sizing to 24 would spend the budget exactly on the
    /// parts that can be counted today and let the table push a full manager past it.
    /// de-mnrb is to measure a FILLED manager and replace this with the real number.
    pub const BYTES_PER_NODE: usize = 32;

    /// How many nodes there are per apply-cache entry.
    ///
    /// A quarter, which is convention rather than measurement: the cache changes no
    /// answer, only how much is recomputed, so the split between it and the node store is
    /// a speed-for-room trade with no obviously right setting. de-1e8l is to measure what
    /// it should be; until then this is what every caller has always used.
    pub const NODES_PER_CACHE_ENTRY: usize = 4;

    /// An allowance of this many bytes.
    pub const fn new(memory: usize) -> Self {
        Self { memory }
    }

    /// What a measurement gets: six gigabytes, and the same six for every one of them.
    ///
    /// GENEROUS ON PURPOSE, and shared on purpose. A measurement wants to find where an
    /// algorithm actually stops, not where a hand-picked ceiling stopped it, and two
    /// measurements can only be read against each other if they were rationed alike. Six
    /// gigabytes is far past anything shipped; a machine that cannot supply it makes the
    /// RUN invalid rather than the row a result (de-e33h).
    ///
    /// It is spent up front, deliberately: a manager that grew or was rebuilt mid-run
    /// would make one row's timing unreadable against another's.
    pub const fn measurement() -> Self {
        Self::new(6 * 1024 * 1024 * 1024)
    }

    /// What a test over a real conversation group gets.
    ///
    /// Sized to buy the 1 << 24 nodes that the largest of them used to ask for directly,
    /// so nothing lost room in the move to budgets - and so the suite allocates what it
    /// always did.
    pub const fn over_a_group() -> Self {
        Self::new(512 * 1024 * 1024)
    }

    /// What a test that only needs a working manager gets.
    ///
    /// The 1 << 20 nodes the small tests used to ask for. They are checking that a formula
    /// is built correctly, not how much fits, and a suite where every test allocated a
    /// group-sized manager would spend gigabytes proving arithmetic.
    pub const fn modest() -> Self {
        Self::new(32 * 1024 * 1024)
    }

    /// The allowance, in bytes.
    ///
    /// `const` so that a measurement can hold the OTHER engine to the same number without
    /// writing it out twice - the forward crawl takes a budget in bytes directly, and two
    /// searches are only comparable when they were rationed alike (de-e23q).
    pub const fn memory(&self) -> usize {
        self.memory
    }

    /// How many diagram nodes it buys.
    pub fn nodes(&self) -> usize {
        self.memory / Self::BYTES_PER_NODE
    }

    /// How many apply-cache entries it buys.
    ///
    /// At least one: oxidd rounds a cache capacity up to a power of two and a zero would
    /// be a manager that can remember nothing, which is a pathology rather than a small
    /// budget.
    pub fn cache_entries(&self) -> usize {
        (self.nodes() / Self::NODES_PER_CACHE_ENTRY).max(1)
    }

    /// A manager sized to this allowance.
    ///
    /// SINGLE THREADED, as every caller here has always built it. The searches are one
    /// diagram at a time and a worker pool would be a second thing to hold to a budget.
    pub fn manager(&self) -> BDDManagerRef {
        new_manager(self.nodes(), self.cache_entries(), 1)
    }
}
