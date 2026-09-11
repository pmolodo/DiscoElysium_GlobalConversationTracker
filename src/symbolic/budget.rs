// SPDX-License-Identifier: MIT
//! How much memory a decision diagram may spend, and what that buys.
//!
//! ONE NUMBER, IN BYTES, IS WHAT A CALLER SETS. It is the same quantity the player's
//! `LookAheadMemoryBudgetMb` sets, which is
//! the only way two searches can be read against each other (de-e23q).
//!
//! Callers used to pass a node capacity and a cache capacity instead, picked per file:
//! `1 << 24` in one measurement and `1 << 20` in another, which at sixteen bytes a node is
//! a ceiling of 268 MB against 17 MB. Numbers taken under two different ceilings cannot be
//! compared, and a verdict of "no room" then said the harness rationed the diagram rather
//! than the diagram failing to fit.

use oxidd::bdd::{BDDManagerRef, new_manager};

/// A memory allowance for one diagram, and the capacities it works out to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagramBudget {
    memory: usize,
    /// Nodes per apply-cache entry - see [`DiagramBudget::NODES_PER_CACHE_ENTRY`].
    ///
    /// A FIELD RATHER THAN ONLY A CONSTANT so that de-1e8l has one place to vary it. Every
    /// constructor here sets it to the constant, so nothing changes for a caller that does
    /// not ask; [`DiagramBudget::with_cache_split`] is the only thing that moves it.
    cache_split: usize,
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
    /// ## NOW MEASURED, and 32 was an undercount (de-mnrb)
    ///
    /// `measurements/manager_memory.rs` fills one manager and reads the allocator as it
    /// grows, so the MARGINAL cost of a node - the table's share, which construction cannot
    /// show - is measured rather than reasoned:
    ///
    /// ```text
    ///        nodes     held MB      grown MB  marginal B/node
    ///      5968824       761.0          89.0            15.6
    ///     11781351       848.1         176.0            15.7
    ///     17536674       934.7         262.7            15.8
    ///     23254780      1020.1         348.1            15.7
    /// ```
    ///
    /// Flat from six million nodes to twenty-three, so there is no late doubling waiting to
    /// surprise a full manager. 21.0 preallocated plus 15.7 grown is 36.7 BYTES A NODE, and
    /// the eight this used to leave for the table was half what the table takes.
    ///
    /// What that cost: a manager built for 1 GB spent 1020 of its 1024 MB while holding
    /// 23.3 million of the 33.5 million nodes the budget said it had bought - it ran out of
    /// real memory at 69 per cent of its stated capacity. Every search that spends its
    /// allowance was overshooting, and `DataVars::memory_used` could not report it because
    /// it prices nodes at this same constant.
    ///
    /// ## Why FORTY rather than 37
    ///
    /// 37 would be the measurement and nothing more. The marginal figure comes from one
    /// synthetic diagram shape, and how densely a real diagram packs the unique table is
    /// not guaranteed to match it - so the number that matters is a CEILING, and a ceiling
    /// wants margin on the side that cannot hurt. Being too generous costs unused budget,
    /// which shows up as a search giving up early; being too tight costs a process that
    /// spends past what it was allowed, which on a player's machine is the game.
    ///
    /// Forty leaves about nine per cent over the measurement and still keeps construction
    /// above half the budget (21.0/40), which is what the test below asserts.
    pub const BYTES_PER_NODE: usize = 40;

    /// How many nodes there are per apply-cache entry.
    ///
    /// A quarter, and it was convention until de-1e8l measured it -
    /// `measurements/cache_split.rs`, which sweeps a sixty-fourth to a half over the heavy
    /// groups at a held total. Every group with a signal peaks HERE and is flat or worse
    /// either side: 368 settles fastest at a quarter, and 14 and 631 get furthest in a
    /// fixed minute at a quarter. Going wider to a half buys nothing anywhere and doubles
    /// what construction costs.
    ///
    /// AND THE FAILURE AT THE OTHER END IS THE ONE NOBODY EXPECTED. de-1e8l predicted that
    /// too GENEROUS a cache would starve the node store into a no-room; the only no-room in
    /// the sweep is at a sixty-fourth, the stingiest cache and the row that bought the most
    /// nodes. A cache is what stops a subproblem being recomputed, and recomputing allocates
    /// nodes - so starving it costs room as well as time. The safe direction to be wrong in
    /// is WIDE, which degrades smoothly, rather than narrow, which can end a search.
    ///
    /// AND IT HOLDS AT THE SHIPPED BUDGET AND OVER A WHOLE MENU, which de-1e8l could not
    /// say - its sweep was single searches at 512 MB. `measurements/cache_split_menu.rs`
    /// runs a 24-start menu against ONE manager at 256 MB, where the cache is warm from the
    /// second option onwards, and the optimum does not move. The expected result was that a
    /// warm cache would want to be wider; instead the curve is flat enough that the fastest
    /// split is not in the same place twice - 28's moved from a sixteenth to an eighth
    /// between two runs, 1030's from a quarter to a half.
    ///
    /// What DOES reproduce is the asymmetry above, from both ends: 362 prefers the narrowest
    /// cache monotonically, and 1030 is punished by it (3.5 and 4.7 seconds at a
    /// sixty-fourth against about 3.2 from an eighth on). No split is best everywhere, so
    /// the default is the one that is never bad - and that is this one.
    pub const NODES_PER_CACHE_ENTRY: usize = 4;

    /// What one apply-cache entry costs, in bytes.
    ///
    /// About twenty, from `tests/manager_memory.rs` counting what construction asks the
    /// allocator for. It is separated out from [`Self::BYTES_PER_NODE`] because the two
    /// are not independent: at one entry per four nodes the cache is five of those forty
    /// bytes, and a caller that changes the split changes what a node costs. See
    /// [`Self::bytes_per_node`].
    pub const BYTES_PER_CACHE_ENTRY: usize = 20;

    /// An allowance of this many bytes, split the way every caller has always split it.
    pub const fn new(memory: usize) -> Self {
        Self {
            memory,
            cache_split: Self::NODES_PER_CACHE_ENTRY,
        }
    }

    /// The same allowance, divided differently between the node store and the apply cache.
    ///
    /// ONE NODE PER `nodes_per_entry`, so a SMALLER number is a BIGGER cache: two is a
    /// cache half the size of the store, sixteen is a sixteenth of it.
    ///
    /// THE TOTAL DOES NOT MOVE, which is the whole reason this exists rather than callers
    /// passing two capacities again. A bigger cache buys fewer nodes out of the same bytes
    /// - see [`Self::bytes_per_node`] - so a sweep over the split is a sweep over one
    /// trade rather than over two budgets that happen to differ.
    ///
    /// FOR MEASURING, not for shipping. de-1e8l swept it and de-bnjy.8 swept it again over
    /// a whole menu at the shipped budget; the quarter survived both, so
    /// [`Self::NODES_PER_CACHE_ENTRY`] is a conclusion rather than a convention and every
    /// shipped caller should be taking it by going through [`Self::new`]. The callers here
    /// are `measurements/cache_split.rs` and `measurements/cache_split_menu.rs`, and this
    /// exists for them.
    ///
    /// # Panics
    ///
    /// If `nodes_per_entry` is zero, which would ask for one cache entry per node and
    /// divide by zero pricing it.
    pub fn with_cache_split(self, nodes_per_entry: usize) -> Self {
        assert!(
            nodes_per_entry > 0,
            "a cache split of zero is one entry per node"
        );
        Self {
            cache_split: nodes_per_entry,
            ..self
        }
    }

    /// What a node costs under THIS split, in bytes.
    ///
    /// [`Self::BYTES_PER_NODE`] is this at the default split and is where the reasoning
    /// lives; what this adds is that the cache's share of it moves when the split does. At
    /// one entry per four nodes the cache is twenty bytes over four nodes - five - and the
    /// answer is the forty that constant states. At one per two it is ten, and a node costs
    /// forty-five, so the same allowance buys about eleven per cent fewer of them.
    ///
    /// THAT IS THE TRADE BEING MEASURED, and it only exists if it is priced: leaving the
    /// cost at forty however the split moved would let a bigger cache be bought with memory
    /// the budget had already promised to the node store, and the manager would spend past
    /// its allowance rather than trading inside it.
    pub fn bytes_per_node(&self) -> usize {
        Self::BYTES_PER_NODE - (Self::BYTES_PER_CACHE_ENTRY / Self::NODES_PER_CACHE_ENTRY)
            + (Self::BYTES_PER_CACHE_ENTRY / self.cache_split)
    }

    /// What a player's search gets when they have not said otherwise, in bytes.
    ///
    /// 300 MB. It buys diagram nodes, at [`Self::BYTES_PER_NODE`] apiece.
    ///
    /// ## Why 300 and not 256
    ///
    /// 256 was never measured as a threshold, only as a round number that was enough for
    /// everything the engine then did. It stopped being enough for one thing: conversation
    /// 761 cannot be marked EXACTLY below about 288 MB at any clock, and answers above it
    /// (de-0jsf.16). A ceiling that sits eleven per cent under the one group that needs it is
    /// an arbitrary number doing harm, so this clears it with headroom rather than sitting on
    /// the edge of it.
    ///
    /// THE SHIPPED MARKING DOES NOT NEED IT, which is what makes the bump cheap rather than a
    /// concession. `menu::mark_menu_hybrid` answers every menu in the game at 256, 761
    /// included, in 1.3 seconds. The headroom is for the exact search behind it - the thing a
    /// fallback reaches for - and for the groups nobody has profiled.
    ///
    /// It is 44 MB of a preallocation made in-process with a Unity game, against a worst case
    /// that already leaves most of the store idle.
    ///
    /// ## What 256 bought, which still holds
    ///
    /// IT WAS ENOUGH FOR THE MARKING, AND THAT IS MEASURED. de-dt75.2 asked the question two
    /// ways.
    ///
    /// NOTHING IN THE GAME RUNS OUT. A whole-game matrix, 2,334 measured rows across both
    /// engine arms, records no manager that filled - `no-room` and `no-ram` appear on no row.
    ///
    /// AND A SESSION DOES NOT ACCUMULATE INTO IT, which is the question one manager per
    /// conversation raises and a per-request manager could not. `measurements/workspace_menus.rs`
    /// walks forty menus through the shipped call: conversation 761, the heaviest group in
    /// the game, leaves the store holding 17.9% of this, and the next highest of the forty
    /// heaviest is 1.5%. The store is filled by the FIRST request and the other thirty-nine
    /// add two tenths of a per cent, so what a group holds is a constant of the group rather
    /// than a curve heading for the ceiling.
    ///
    /// So there is over six times the headroom on the worst group, and every megabyte here is
    /// memory a player gives up whether their conversations need it or not - the node store
    /// is one preallocation made in-process with a Unity game. That is the argument against
    /// raising it further, and it is why this went to 300 rather than to a gigabyte.
    ///
    /// A CHANGE HERE WANTS A MATRIX RUN EITHER SIDE OF IT, since a regression traced to a
    /// budget that moved in the same commit is not traced at all.
    pub const DEFAULT_MEMORY_BUDGET: usize = 300 * 1024 * 1024;

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
    /// writing it out twice - a budget is stated in bytes, and two
    /// searches are only comparable when they were rationed alike (de-e23q).
    pub const fn memory(&self) -> usize {
        self.memory
    }

    /// How many diagram nodes it buys.
    ///
    /// Under this budget's own split, so a bigger cache buys fewer nodes out of the same
    /// bytes rather than being added on top of them - see [`Self::bytes_per_node`]. At the
    /// default split this is the allowance over [`Self::BYTES_PER_NODE`], which is what it
    /// has always been.
    pub fn nodes(&self) -> usize {
        self.memory / self.bytes_per_node()
    }

    /// How many apply-cache entries it buys.
    ///
    /// At least one: oxidd rounds a cache capacity up to a power of two and a zero would
    /// be a manager that can remember nothing, which is a pathology rather than a small
    /// budget.
    pub fn cache_entries(&self) -> usize {
        (self.nodes() / self.cache_split).max(1)
    }

    /// Nodes per apply-cache entry, as this budget divides them.
    ///
    /// [`Self::NODES_PER_CACHE_ENTRY`] unless [`Self::with_cache_split`] moved it.
    pub fn cache_split(&self) -> usize {
        self.cache_split
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

    /// A manager sized to this allowance, or None where the machine cannot supply it.
    ///
    /// ASKING AND SPENDING IN ONE PLACE, which is the point. [`Self::can_be_supplied`] and
    /// [`Self::manager`] are two calls that have to happen in that order, and an order a
    /// caller has to remember is one a caller can forget - at which point the failure is
    /// not a wrong answer but a dead process, because the allocation the manager makes
    /// ABORTS rather than returning an error. Everything that builds a manager for an
    /// allowance it did not choose itself should come through here.
    ///
    /// Everything [`Self::can_be_supplied`] does not promise, this does not promise
    /// either: another process can take the memory in between, and a reservation Windows
    /// accepts may still be paid for in paging. It converts the common case from an abort
    /// into a value.
    pub fn try_manager(&self) -> Option<BDDManagerRef> {
        self.can_be_supplied().then(|| self.manager())
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
        assert_eq!(
            budget.cache_entries(),
            1024 / DiagramBudget::NODES_PER_CACHE_ENTRY
        );
    }

    #[test]
    fn a_budget_too_small_for_a_single_cache_entry_still_gets_one() {
        // Zero would be a manager that can remember nothing, which is a pathology rather
        // than a small budget.
        assert_eq!(DiagramBudget::new(0).cache_entries(), 1);
    }

    #[test]
    fn the_default_split_prices_a_node_at_the_constant_that_states_it() {
        // The two have to agree or every number in BYTES_PER_NODE's own note is about a
        // manager nobody builds.
        assert_eq!(
            DiagramBudget::new(1).bytes_per_node(),
            DiagramBudget::BYTES_PER_NODE,
        );
    }

    #[test]
    fn a_bigger_cache_is_paid_for_in_nodes_rather_than_added_to_the_budget() {
        // THE WHOLE POINT OF THE SWEEP de-1e8l is to run. If a wider cache did not cost
        // nodes, the split would not be a trade and a manager asked for a half-sized cache
        // would spend half again as much as its allowance said.
        let allowance = 64 * 1024 * 1024;
        let quarter = DiagramBudget::new(allowance);
        let half = quarter.with_cache_split(2);
        let sixteenth = quarter.with_cache_split(16);

        assert!(half.nodes() < quarter.nodes());
        assert!(sixteenth.nodes() > quarter.nodes());

        // And what it buys moves the other way, by more than the node count lost.
        assert!(half.cache_entries() > quarter.cache_entries());
        assert!(sixteenth.cache_entries() < quarter.cache_entries());
    }

    #[test]
    fn no_split_lets_a_manager_ask_for_more_than_the_allowance() {
        // Nodes at sixteen bytes plus their cache entries at twenty, against the bytes the
        // budget was given. The unique table grows into the rest, which is why this is a
        // headroom check rather than an equality.
        for split in [1, 2, 4, 8, 16, 64] {
            let budget = DiagramBudget::new(64 * 1024 * 1024).with_cache_split(split);
            let up_front =
                budget.nodes() * 16 + budget.cache_entries() * DiagramBudget::BYTES_PER_CACHE_ENTRY;
            assert!(
                up_front <= budget.memory(),
                "a split of {split} asks for {up_front} bytes up front out of {}",
                budget.memory(),
            );
        }
    }

    #[test]
    #[should_panic(expected = "one entry per node")]
    fn a_split_of_zero_is_refused_rather_than_dividing_by_it() {
        let _ = DiagramBudget::new(1).with_cache_split(0);
    }
}
