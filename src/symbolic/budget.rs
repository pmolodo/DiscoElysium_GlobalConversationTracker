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
    ///
    /// ASK [`Self::can_be_supplied`] FIRST if the allowance is a large one. This cannot
    /// fail; it aborts. See that method for why.
    pub fn manager(&self) -> BDDManagerRef {
        new_manager(self.nodes(), self.cache_entries(), 1)
    }

    /// Whether this machine can supply the allowance at all, asked BEFORE spending it.
    ///
    /// ## Why the question has to be asked separately
    ///
    /// Because the spending itself has no failure path. The manager preallocates its node
    /// store up front and cannot grow past it - node ids are four-byte indices into a
    /// fixed slice - and oxidd builds that slice with `Vec::with_capacity` followed by
    /// `set_len`. `with_capacity` does not return an error when the allocator says no: it
    /// ABORTS THE PROCESS. So by the time anything here could react, there is no process
    /// left to react in, and from outside it is indistinguishable from a crash.
    ///
    /// This reserves the same number of bytes fallibly and drops them again, which turns
    /// "the machine cannot do this" from a dead process into a value a caller can act on.
    ///
    /// ## What a false answer means, and what it does not
    ///
    /// It means the RUN is invalid, not that the row is a result. The machine failing to
    /// supply a budget says nothing whatever about the algorithm being measured, so there
    /// is nothing to record: the row is NOT MEASURED and wants running again when the
    /// machine has the memory free. That is a different thing from `no-room`, which is a
    /// real finding - the search had every byte it was allowed and still had no answer.
    ///
    /// ## What it is not
    ///
    /// NOT A GUARANTEE. Another process can take the memory between this answer and the
    /// allocation, and on Windows a reservation that succeeds may still be paid for in
    /// paging rather than in RAM - a row that swaps for ten minutes is honestly neither
    /// verdict. It converts the common case from an abort into a value, which is all it
    /// claims to do.
    ///
    /// It asks for the WHOLE allowance in one piece, which is more than the manager takes
    /// up front - about two thirds of it, the store plus the apply cache, with the unique
    /// table growing into the rest later. Deliberately conservative: the budget is what
    /// the diagram may spend, and a machine that cannot supply it will fail during the
    /// run instead of before it.
    pub fn can_be_supplied(&self) -> bool {
        let mut probe: Vec<u8> = Vec::new();
        probe.try_reserve_exact(self.memory).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_modest_budget_is_one_any_machine_running_this_suite_can_supply() {
        assert!(DiagramBudget::modest().can_be_supplied());
    }

    #[test]
    fn a_budget_no_machine_could_supply_is_refused_rather_than_aborting() {
        // The whole point of the fallible probe: this REPORTS. The allocation the manager
        // would make with the same number kills the process instead, which is why the
        // question has to be asked before rather than handled after.
        let more_than_exists = DiagramBudget::new(usize::MAX / 2);
        assert!(!more_than_exists.can_be_supplied());
    }

    #[test]
    fn the_capacities_divide_the_allowance_the_way_the_constants_say() {
        let budget = DiagramBudget::new(DiagramBudget::BYTES_PER_NODE * 1024);
        assert_eq!(budget.nodes(), 1024);
        assert_eq!(budget.cache_entries(), 1024 / DiagramBudget::NODES_PER_CACHE_ENTRY);
    }

    #[test]
    fn a_budget_too_small_for_a_single_cache_entry_still_gets_one() {
        // Zero would be a manager that can remember nothing, which is a pathology rather
        // than a small budget.
        assert_eq!(DiagramBudget::new(0).cache_entries(), 1);
    }
}
