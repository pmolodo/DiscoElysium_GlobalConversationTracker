// SPDX-License-Identifier: MIT
//! How a search's DATA state maps onto decision-diagram variables.
//!
//! ## Explicit control, symbolic data
//!
//! The entry a search is sitting on stays an ordinary value - there are a few thousand of
//! them and they are enumerated anyway - while everything the search carries WITH it
//! becomes decision-diagram variables. So a reachability pass keeps one set of data
//! states per entry, rather than one set of (entry, data) pairs, and the entry never
//! costs a variable.
//!
//! That is the opposite of what [`super::StateEncoding`] does, and deliberately. That one
//! encodes the entry too, because it is measuring how a set of whole search states
//! compresses. This one is for computing with, where keeping the control explicit means
//! the image of one edge is a formula about data alone.
//!
//! ## Widths come from the graph, not from a guess
//!
//! Most slots only ever hold 0 or 1 - items, tasks, check flags, the once and seen
//! markers - and only a slot something INCREMENTS needs room to count. Reading the widths
//! off the graph's actions is what keeps the variable count near the number of slots
//! rather than at five times it: for conversation 631's group that is the difference
//! between about 350 variables and about 1,700.

use crate::core::action::DialogueActionKind;
use std::collections::{HashMap, HashSet};

use crate::core::guard::GuardExpression;
use crate::core::state::{
    StateSymbols, ITEM_PREFIX, ONCE_PREFIX, SEEN_PREFIX, TASK_PREFIX, THOUGHT_PREFIX,
};
use crate::graph::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::world::world::MONEY_QUERY;

/// Minutes in a day; the clock is wrapped into `0..MINUTES_IN_DAY`.
const MINUTES_IN_DAY: u32 = 1440;

/// How many bits it takes to represent `0..=max`.
fn bits_for(max: u32) -> u8 {
    if max == 0 { 1 } else { (u32::BITS - max.leading_zeros()) as u8 }
}

/// Where each part of a data state lives, in variable numbers.
#[derive(Debug, Clone)]
pub struct DataLayout {
    /// Per slot: the first variable of its little-endian run, and how many bits it has.
    slots: Vec<(u32, u8)>,
    money: Option<(u32, u8)>,
    clock: Option<(u32, u8)>,
    total: u32,
}

impl DataLayout {
    /// Lays out variables for the data a search over `graph` can carry.
    ///
    /// `counter_cap` is the ceiling an incremented slot saturates at, which is what makes
    /// it finite and therefore what bounds its width.
    ///
    /// `money_max` and `track_clock` say whether those two are worth variables at all. A
    /// search over a graph with no cost options and no `PassTime` never moves either, and
    /// spending bits on a constant is pure cost - see the measurements on
    /// `super::StateEncoding`, where money and the clock never moved once.
    pub fn for_graph(
        graph: &LookAheadGraph,
        counter_cap: i32,
        money_max: Option<u32>,
        track_clock: bool,
    ) -> Self {
        let slot_count = graph.symbols().count();
        // Every slot is one bit until something proves it needs more.
        let mut widths = vec![1u8; slot_count];
        // THE FLOOR AN ASSIGN PUTS UNDER A SLOT, kept apart from the total because the
        // threshold narrowing below may squeeze what an INCREMENT asked for and may never
        // squeeze this. See `narrow_to_thresholds`.
        let mut assigned = vec![1u8; slot_count];

        for node in graph.nodes() {
            // A cost charged once records that in its own slot, which is boolean.
            for action in &node.actions {
                let slot = action.slot();
                if slot < 0 || slot as usize >= slot_count {
                    continue;
                }

                let slot = slot as usize;
                let needed = match action.kind() {
                    DialogueActionKind::Increment => bits_for(counter_cap.max(0) as u32),
                    DialogueActionKind::Assign => {
                        let bits = bits_for(action.value().max(0) as u32);
                        assigned[slot] = assigned[slot].max(bits);
                        bits
                    }
                    // Money, clock and unmodelled actions do not write a slot.
                    _ => continue,
                };
                widths[slot] = widths[slot].max(needed);
            }
        }

        Self::narrow_to_thresholds(graph, &mut widths, &assigned);

        let mut next = 0u32;
        let mut slots = Vec::with_capacity(slot_count);
        for width in widths {
            slots.push((next, width));
            next += width as u32;
        }

        let money = money_max.map(|max| {
            let at = (next, bits_for(max));
            next += at.1 as u32;
            at
        });

        let clock = if track_clock {
            let at = (next, bits_for(MINUTES_IN_DAY - 1));
            next += at.1 as u32;
            Some(at)
        } else {
            None
        };

        Self { slots, money, clock, total: next }
    }

    /// Squeezes each slot to the largest value its guards can still tell apart.
    ///
    /// ## The argument, which is what makes this sound
    ///
    /// The guard language has NO ARITHMETIC: a slot is only ever compared against a
    /// constant, assigned, or incremented. So if every comparison in the group tests a slot
    /// against constants no larger than `T`, then EVERY VALUE ABOVE `T` ANSWERS ALL OF THEM
    /// IDENTICALLY - `>= c` is true, `< c` is false and `== c` is false for any `c <= T` -
    /// and the slot may saturate at `T + 1` instead of at the counter cap.
    ///
    /// A cap of 16 is five bits; a slot compared only against 0 and 1 is one.
    ///
    /// ## Why it needs no graph analysis at all
    ///
    /// de-3x76.2 proposed bounding a counter by the number of sites that write it, which
    /// needs to know whether those sites sit inside a cycle, which needs an SCC pass. IT
    /// ALSO NEEDED THE WORLD, because a save can arrive holding more than the group's own
    /// sites could produce, and `seed_of` clamps it - so that bound could silently corrupt
    /// a save, and avoiding it would have made the layout world-dependent and cost the kept
    /// workspace its key.
    ///
    /// The threshold bound has neither problem. It holds however many times the slot is
    /// incremented, whether or not it sits in a cycle, and whatever the save arrives
    /// holding, because it is an argument about what the GUARDS can distinguish rather than
    /// about what the slot can reach. Measured against the site-count version it is also
    /// strictly better: 7.6% against 4.2% on 362, 6.6% against 5.4% on 368, and 1.3%
    /// against nothing on 14 (`measurements/layout_shape.rs`).
    ///
    /// ## The two things it must not do
    ///
    /// AN ASSIGN IS NOT SATURATING. `ActionImage::assign` encodes the assigned number
    /// directly where an increment clamps into the slot's ceiling, so a slot assigned 100
    /// must keep room for 100 however small its thresholds are. That is what `assigned` is.
    ///
    /// A COMPARISON THIS CANNOT READ MUST BLOCK THE NARROWING, not be ignored. The argument
    /// is that EVERY comparison agrees, so one whose shape is unrecognised - a non-literal
    /// other side, or the slot handed to a world query - leaves the slot at full width.
    fn narrow_to_thresholds(graph: &LookAheadGraph, widths: &mut [u8], assigned: &[u8]) {
        let symbols = graph.symbols();
        let mut highest: HashMap<usize, u32> = HashMap::new();
        let mut unreadable: HashSet<usize> = HashSet::new();
        for node in graph.nodes() {
            Self::read_comparisons(&node.guard, symbols, &mut highest, &mut unreadable);
        }

        for (slot, width) in widths.iter_mut().enumerate() {
            if unreadable.contains(&slot) {
                continue;
            }
            // No comparison at all means the slot is read as a condition, or not read - and
            // one bit already tells zero from non-zero, so there is nothing to squeeze.
            let Some(&high) = highest.get(&slot) else { continue };
            let wanted = bits_for(high.saturating_add(1)).max(assigned[slot]);
            *width = (*width).min(wanted);
        }
    }

    /// Collects, per slot, the largest constant compared against it - and which slots are
    /// compared in a shape this cannot read.
    ///
    /// See [`Self::narrow_to_thresholds`] for why the second is not merely an omission.
    fn read_comparisons(
        guard: &GuardExpression,
        symbols: &StateSymbols,
        highest: &mut HashMap<usize, u32>,
        unreadable: &mut HashSet<usize>,
    ) {
        match guard {
            GuardExpression::Comparison(_, a, b) => {
                for (side, other) in [(a.as_ref(), b.as_ref()), (b.as_ref(), a.as_ref())] {
                    let Some(slot) = Self::slot_named(side, symbols) else { continue };
                    let GuardExpression::Literal(value) = other else {
                        unreadable.insert(slot);
                        continue;
                    };
                    let number = value.number();
                    if !number.is_finite() || number < 0.0 {
                        unreadable.insert(slot);
                        continue;
                    }
                    let seen = highest.entry(slot).or_insert(0);
                    *seen = (*seen).max(number as u32);
                }
                Self::read_comparisons(a, symbols, highest, unreadable);
                Self::read_comparisons(b, symbols, highest, unreadable);
            }
            GuardExpression::Not(inner) => {
                Self::read_comparisons(inner, symbols, highest, unreadable)
            }
            GuardExpression::And(a, b) | GuardExpression::Or(a, b) => {
                Self::read_comparisons(a, symbols, highest, unreadable);
                Self::read_comparisons(b, symbols, highest, unreadable);
            }
            GuardExpression::Call(_, args) => {
                // A SLOT HANDED TO A QUERY is not something this can reason about at all.
                for arg in args {
                    if let Some(slot) = Self::slot_named(arg, symbols) {
                        unreadable.insert(slot);
                    }
                    Self::read_comparisons(arg, symbols, highest, unreadable);
                }
            }
            GuardExpression::Variable(_) | GuardExpression::Literal(_) => {}
        }
    }

    /// The slot a guard expression names, if it simply names one.
    fn slot_named(guard: &GuardExpression, symbols: &StateSymbols) -> Option<usize> {
        let GuardExpression::Variable(name) = guard else { return None };
        (0..symbols.count()).find(|slot| symbols.name_of(*slot) == Some(name.as_str()))
    }

    /// The layout a search over a whole group gets, which is what the product runs.
    ///
    /// Widths from the graph, a balance where something in the group reads one, the clock
    /// left to [`GuardCompiler::with_constant_clock`], and every slot nothing reads
    /// dropped. Four callers had these three lines copied out - the bridge and the three
    /// halves of the performance matrix - which is how the matrix came to measure a
    /// different layout from the one that ships. Stated once instead.
    ///
    /// [`GuardCompiler::with_constant_clock`]: crate::symbolic::guard_formula::GuardCompiler::with_constant_clock
    pub fn for_group(
        graph: &LookAheadGraph,
        world: &dyn crate::world::world::ILookAheadWorld,
        counter_cap: i32,
    ) -> Self {
        Self::for_group_entered_at(graph, world, counter_cap, None)
    }

    /// The same, narrowed to what a query ENTERING AT one conversation can reach.
    ///
    /// `entered_at` names the conversation the request's starts live in, or `None` for the
    /// whole group - which is what [`Self::for_group`] passes and what every measurement
    /// that is about a group rather than a query wants.
    ///
    /// ## Why the unit is a conversation and not a start (de-3x76.8)
    ///
    /// `read_by` collects every name any guard ANYWHERE in the group reads, and a group is
    /// much bigger than the conversation a player is standing in - 368 is 4,724 entries
    /// over five conversations. A slot read only by guards on entries the query cannot
    /// structurally reach is carried for nothing.
    ///
    /// Narrowing per START would save slightly more, and cannot be had.
    /// `measurements/start_relative_layout.rs` measured all three granularities:
    ///
    /// ```text
    ///  conv   whole   per start   per menu   per conversation
    ///   368     241   118 (51%)  119 (51%)         124 (49%)
    ///    14     236   195 (17%)  201 (15%)         193 (18%)
    ///    28     144    71 (51%)   70 (51%)         104 (28%)
    /// ```
    ///
    /// Per conversation keeps almost all of the saving on the largest group AND IS THE ONLY
    /// ONE COMPATIBLE WITH THE KEPT MANAGER. `workspace::Workspace` holds a diagram manager
    /// across requests and `measurements/manager_reuse.rs` prices that at 45 ms a request; a
    /// layout that moved per menu would rebuild it on every menu and hand back more than
    /// this saves. A layout that is a property of the CONVERSATION does not move between the
    /// menus inside it, so the manager survives exactly where it earns its keep - and the
    /// plugin already sends one request per conversation, so this is the granularity the
    /// wire has anyway.
    ///
    /// ## Why it cannot drop a slot a real path needs
    ///
    /// The reachable set is STRUCTURAL - links followed, guards ignored - so it
    /// over-approximates what any search can walk. A search starts at one of the request's
    /// starts, which are entries of `entered_at`, so everything it can visit is in the set.
    pub fn for_group_entered_at(
        graph: &LookAheadGraph,
        world: &dyn crate::world::world::ILookAheadWorld,
        counter_cap: i32,
        entered_at: Option<&[i32]>,
    ) -> Self {
        let reads = match entered_at {
            Some(conversations) => Self::read_by_some(
                graph,
                Self::reachable_from_conversations(graph, conversations),
            ),
            None => Self::read_by(graph),
        };

        Self::for_graph(graph, counter_cap, Self::money_ceiling(graph, world.money()), false)
            .keeping_only_read(graph.symbols(), &reads)
    }

    /// Every entry reachable by links from any entry of `conversations`, those included.
    ///
    /// Structural only, for the reason given on [`Self::for_group_entered_at`]. A
    /// conversation the graph does not hold reaches nothing, and the caller then gets a
    /// layout with no slots - which is why callers pass the conversations the request's
    /// STARTS live in rather than any taken from somewhere else.
    fn reachable_from_conversations(
        graph: &LookAheadGraph,
        conversations: &[i32],
    ) -> Vec<crate::core::types::DialogueNodeId> {
        let mut seen: HashSet<crate::core::types::DialogueNodeId> = HashSet::new();
        let mut pending = std::collections::VecDeque::new();

        for node in graph.nodes() {
            if conversations.contains(&node.id.conversation_id) && seen.insert(node.id) {
                pending.push_back(node.id);
            }
        }

        while let Some(id) = pending.pop_front() {
            let Some(node) = graph.get(id) else { continue };
            for &next in &node.links {
                if graph.contains(next) && seen.insert(next) {
                    pending.push_back(next);
                }
            }
        }

        seen.into_iter().collect()
    }

    /// How many decision-diagram variables the layout uses.
    pub fn total_vars(&self) -> u32 {
        self.total
    }

    /// How many slots it covers.
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// The variable run for one slot: its first variable and its width.
    ///
    /// `None` for a slot the layout does not carry, which is how a dropped one is
    /// reported - see [`Self::without_visit_flags`]. Every caller already had to handle
    /// `None`, because a slot number can come from a symbol table wider than the layout.
    pub fn slot(&self, slot: usize) -> Option<(u32, u8)> {
        match self.slots.get(slot).copied() {
            Some((_, 0)) => None,
            other => other,
        }
    }

    /// The same layout with every slot nothing in the group READS dropped.
    ///
    /// ## This one is exact, not an approximation
    ///
    /// A conversation group is closed under links, so a search over it only ever evaluates
    /// guards belonging to it. A slot that no guard in the group reads therefore cannot
    /// change which entries are reachable, whatever any action writes to it - it is
    /// write-only for the length of the search. Dropping it removes a variable and changes
    /// no answer at all, which is a different and better thing than
    /// [`Self::without_visit_flags`], where the saving is paid for in precision.
    ///
    /// ## How much it removes
    ///
    /// A great deal, because the content is full of bookkeeping the dialogue never reads
    /// back. Conversation 631's group writes 45 `XP.` accomplishment latches and reads
    /// two of them; the other 43 are variables the rest of the game cares about and this
    /// search cannot.
    ///
    /// ## What must be kept even though no guard names it
    ///
    /// Three kinds of slot are read by the ENGINE rather than by a guard, and dropping
    /// them would change behaviour:
    ///
    /// - `seen:`, which closes a `Fake` or non-boolean `KimSwitch` entry;
    /// - `once:`, which stops a one-time effect firing twice and a once-cost being paid
    ///   twice;
    /// - a rolled check's pass and fail flags, which decide whether it can be retried.
    ///
    /// Items and tasks are kept when a guard asks about them through `CheckItem` or
    /// `IsTaskActive`, which name their subject as a string rather than as a variable -
    /// so `reads` must be given those names too, spelled the way the symbol table spells
    /// them.
    pub fn keeping_only_read(mut self, symbols: &StateSymbols, reads: &HashSet<String>) -> Self {
        for slot in 0..self.slots.len() {
            let Some(name) = symbols.name_of(slot) else { continue };
            // The engine's own bookkeeping, which no guard mentions and every search needs.
            if name.starts_with(SEEN_PREFIX) || name.starts_with(ONCE_PREFIX) {
                continue;
            }
            if !reads.contains(name) {
                self.slots[slot].1 = 0;
            }
        }

        self.renumber();
        self
    }

    /// The same layout with the per-entry visit flags dropped.
    ///
    /// Unlike [`Self::keeping_only_read`] this one is an APPROXIMATION, and it was
    /// measured and found not to pay: dropping the flags moved conversation 368's diagram
    /// from 13,634,773 nodes to 13,583,773, about five per cent. Kept because the
    /// question is a reasonable one to ask again and the answer should stay reproducible.
    ///
    /// What it costs, when it is used. A `seen:` slot closes a `Fake` or non-boolean
    /// `KimSwitch` entry and a `once:` slot stops a one-time effect firing twice; without
    /// them such an entry never closes and a one-time action fires every time round a
    /// loop, driving its counter to the cap. Both make the reachable set BIGGER, never
    /// smaller, so no reachable entry is lost - which is the only error that matters.
    pub fn without_visit_flags(mut self, symbols: &StateSymbols) -> Self {
        for slot in 0..self.slots.len() {
            let is_flag = symbols.name_of(slot).is_some_and(|name: &str| {
                name.starts_with(SEEN_PREFIX) || name.starts_with(ONCE_PREFIX)
            });
            if is_flag {
                self.slots[slot].1 = 0;
            }
        }

        self.renumber();
        self
    }

    /// Closes the gaps a dropped slot leaves.
    ///
    /// Without this a dropped slot costs no reads but still costs its variable numbers,
    /// so the diagram keeps exactly the depth the dropping was meant to remove.
    fn renumber(&mut self) {
        let mut next = 0u32;
        for (base, bits) in &mut self.slots {
            *base = next;
            next += *bits as u32;
        }

        if let Some((base, bits)) = &mut self.money {
            *base = next;
            next += *bits as u32;
        }

        if let Some((base, bits)) = &mut self.clock {
            *base = next;
            next += *bits as u32;
        }

        self.total = next;
    }

    /// Whether a slot is a single bit, which is the common case.
    pub fn is_boolean(&self, slot: usize) -> bool {
        self.slots.get(slot).map(|(_, bits)| *bits == 1).unwrap_or(false)
    }

    /// The variable run for money, if it is tracked.
    pub fn money(&self) -> Option<(u32, u8)> {
        self.money
    }

    /// The widest balance a search over this group can hold, or `None` where nothing in it
    /// can read one - which is what [`Self::for_graph`] wants for `money_max`.
    ///
    /// ## Only where something asks
    ///
    /// Money is a REGISTER rather than a bit, thirteen variables wide for a save carrying
    /// fifty-one real, and every one of them is depth every diagram in the group pays for.
    /// Two things read it: an option with a price, and a guard calling `MoneyAmount`. A
    /// group with neither can gain and lose all it likes and no answer depends on the
    /// balance, so it is not worth a variable - the same rule the slots are laid out by.
    ///
    /// ## Why the ceiling is the start plus every gain
    ///
    /// A register SATURATES, and the two directions are not equally bad. A balance clipped
    /// DOWN refuses an option the player can afford, which loses a marker; one that is
    /// merely wide costs diagram nodes. So the ceiling has to be at least every balance the
    /// group can produce, and the sum of what its entries add is that - with one exception,
    /// a `GainMoney` inside a cycle, which can be collected repeatedly and climb past it.
    /// The sum is over the entries and not over the paths, so that case saturates and can
    /// lose a marker; it is noted rather than fixed, because bounding it properly means
    /// knowing how often a cycle can turn, which is the same question the counter cap
    /// answers by decree.
    pub fn money_ceiling(graph: &LookAheadGraph, starting: i32) -> Option<u32> {
        let read = graph
            .nodes()
            .any(|node| node.is_cost_option() || Self::guard_reads_money(&node.guard));
        if !read {
            return None;
        }

        let gained: i64 = graph
            .nodes()
            .flat_map(|node| &node.actions)
            .filter(|action| action.kind() == DialogueActionKind::GainMoney)
            .map(|action| i64::from(action.value().max(0)))
            .sum();

        let ceiling = i64::from(starting.max(0)) + gained;
        Some(ceiling.min(i64::from(u32::MAX)) as u32)
    }

    /// Whether a guard asks what the player is carrying.
    fn guard_reads_money(guard: &GuardExpression) -> bool {
        match guard {
            GuardExpression::Call(name, args) => {
                name == MONEY_QUERY || args.iter().any(Self::guard_reads_money)
            }
            GuardExpression::Not(inner) => Self::guard_reads_money(inner),
            GuardExpression::And(a, b)
            | GuardExpression::Or(a, b)
            | GuardExpression::Comparison(_, a, b) => {
                Self::guard_reads_money(a) || Self::guard_reads_money(b)
            }
            GuardExpression::Variable(_) | GuardExpression::Literal(_) => false,
        }
    }

    /// The variable run for the clock, if it is tracked.
    pub fn clock(&self) -> Option<(u32, u8)> {
        self.clock
    }

    /// Every slot name something in `graph` reads, for [`Self::keeping_only_read`].
    ///
    /// Guards read a variable by name, and an item or a task through `CheckItem` or
    /// `IsTaskActive`, whose subject is a string argument rather than a variable - so
    /// those are collected under the prefixes the symbol table stores them with.
    ///
    /// The rolled checks' pass and fail flags are added too. Nothing NAMES them, but
    /// `Reachability::rolled_cases` reads them to decide whether a check can be
    /// attempted, so a layout without them would let a check be retried for ever.
    pub fn read_by(graph: &LookAheadGraph) -> HashSet<String> {
        Self::read_by_nodes(graph.nodes(), graph.symbols())
    }

    /// The same, for SOME of the entries rather than all of them.
    ///
    /// What a per-target analysis needs: the names read on the paths that can reach one
    /// entry, rather than the names read anywhere in the group.
    pub fn read_by_some(
        graph: &LookAheadGraph,
        nodes: impl IntoIterator<Item = crate::core::types::DialogueNodeId>,
    ) -> HashSet<String> {
        Self::read_by_nodes(
            nodes.into_iter().filter_map(|id| graph.get(id)),
            graph.symbols(),
        )
    }

    /// The reading rules themselves, over entries that need not be a graph yet.
    ///
    /// Nodes and a symbol table are all the rules ever needed, and taking those rather
    /// than a `LookAheadGraph` is what lets `build_group_graph` ask the question DURING
    /// construction - the point at which the slots nothing reads can still be dropped.
    ///
    /// The one implementation, shared by both callers above rather than written twice: a
    /// second copy of the reading rules is a copy that drifts, and the modelling-gaps
    /// report carries the scar of exactly that.
    pub fn read_by_nodes<'a>(
        nodes: impl IntoIterator<Item = &'a LookAheadNode>,
        symbols: &StateSymbols,
    ) -> HashSet<String> {
        let mut names = HashSet::new();

        for node in nodes {
            Self::read_by_guard(&node.guard, &mut names);

            for slot in [node.flag_slot, node.failed_flag_slot] {
                if let Ok(slot) = usize::try_from(slot) {
                    if let Some(name) = symbols.name_of(slot) {
                        names.insert(name.to_string());
                    }
                }
            }
        }

        names
    }

    /// The names one guard reads, including the subjects of the queries answered from
    /// search state.
    fn read_by_guard(guard: &GuardExpression, names: &mut HashSet<String>) {
        match guard {
            GuardExpression::Variable(name) => {
                names.insert(name.clone());
            }
            GuardExpression::Not(inner) => Self::read_by_guard(inner, names),
            GuardExpression::And(a, b)
            | GuardExpression::Or(a, b)
            | GuardExpression::Comparison(_, a, b) => {
                Self::read_by_guard(a, names);
                Self::read_by_guard(b, names);
            }
            GuardExpression::Call(function, args) => {
                // The two queries `BoundContext::query` answers from a slot. Their subject
                // is a literal string, and the slot it corresponds to carries a prefix.
                let prefix = match function.as_str() {
                    "CheckItem" => Some(ITEM_PREFIX),
                    "IsTaskActive" => Some(TASK_PREFIX),
                    "IsTHCPresent" => Some(THOUGHT_PREFIX),
                    // FlagSet(name) is Variable[name] written another way.
                    "FlagSet" => Some(""),
                    _ => None,
                };

                if let Some(prefix) = prefix {
                    if let [GuardExpression::Literal(value)] = &args[..] {
                        names.insert(format!("{prefix}{}", value.text()));
                    }
                }

                for arg in args {
                    Self::read_by_guard(arg, names);
                }
            }
            GuardExpression::Literal(_) => {}
        }
    }

    /// Whether any action in the group advances the clock.
    ///
    /// What decides whether treating the clock as constant is exact or an approximation.
    /// Rare in the shipped content: only 19 of the 8,339 distinct scripts in the database
    /// contain a `PassTime` at all.
    ///
    /// This is about what the ENGINE models. Whether the GAME advances the clock by other
    /// means - it is thought to tick roughly a minute per unseen entry, which the engine
    /// does not model at all - is de-sze.10 and is a different question.
    pub fn group_passes_time(graph: &LookAheadGraph) -> bool {
        graph.nodes().any(|node| {
            node.actions.iter().any(|a| a.kind() == DialogueActionKind::PassTime)
        })
    }

    /// How many slots carry a name with the given prefix, such as `item:` or `task:`.
    ///
    /// A slot exists only for something an ACTION in the group touches, so this counts
    /// what the group actually manipulates rather than what the game contains.
    pub fn slots_named(symbols: &crate::core::state::StateSymbols, prefix: &str) -> usize {
        (0..symbols.count())
            .filter(|slot| symbols.name_of(*slot).is_some_and(|name| name.starts_with(prefix)))
            .count()
    }

    /// How many slots need more than one bit - the counters.
    ///
    /// The figure that says whether reading widths off the graph was worth it.
    pub fn wide_slots(&self) -> usize {
        self.slots.iter().filter(|(_, bits)| *bits > 1).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::action::DialogueAction;
    use crate::core::guard::GuardExpression;
    use crate::core::state::StateSymbols;
    use crate::core::types::{DialogueCheckKind, DialogueNodeId};
    use crate::graph::node::LookAheadNode;

    fn graph_with(actions: Vec<DialogueAction>, symbols: StateSymbols) -> LookAheadGraph {
        let node = LookAheadNode::new(
            DialogueNodeId::new(1, 0),
            false,
            DialogueCheckKind::None,
            GuardExpression::always_true(),
            actions,
            vec![],
            0,
            false,
            false,
            -1,
            -1,
            false,
            -1,
        );
        LookAheadGraph::new(vec![node], symbols).unwrap()
    }

    #[test]
    fn a_slot_only_ever_set_to_one_stays_a_single_bit() {
        let mut symbols = StateSymbols::new();
        let slot = symbols.variable("met_kim");
        let graph = graph_with(
            vec![DialogueAction::assign(slot, 1, "SetVariableValue".to_string())],
            symbols,
        );

        let layout = DataLayout::for_graph(&graph, 16, None, false);
        assert!(layout.is_boolean(slot));
        assert_eq!(layout.slot(slot), Some((0, 1)));
        assert_eq!(layout.wide_slots(), 0);
    }

    #[test]
    fn an_incremented_slot_gets_room_for_the_counter_cap() {
        let mut symbols = StateSymbols::new();
        let slot = symbols.variable("loop_counter");
        let graph = graph_with(
            vec![DialogueAction::increment(slot, 1, false, "SetVariableValue".to_string())],
            symbols,
        );

        // A cap of 16 needs five bits, because 16 itself must be representable.
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        assert_eq!(layout.slot(slot), Some((0, 5)));
        assert!(!layout.is_boolean(slot));
        assert_eq!(layout.wide_slots(), 1);
    }

    #[test]
    fn a_slot_assigned_a_large_value_gets_room_for_it() {
        let mut symbols = StateSymbols::new();
        let slot = symbols.variable("stage");
        let graph = graph_with(
            vec![DialogueAction::assign(slot, 5, "SetVariableValue".to_string())],
            symbols,
        );

        let layout = DataLayout::for_graph(&graph, 16, None, false);
        assert_eq!(layout.slot(slot), Some((0, 3)));
    }

    #[test]
    fn slots_are_laid_out_end_to_end_in_slot_order() {
        let mut symbols = StateSymbols::new();
        let first = symbols.variable("a");
        let counter = symbols.variable("b");
        let last = symbols.variable("c");
        let graph = graph_with(
            vec![DialogueAction::increment(counter, 1, false, "SetVariableValue".to_string())],
            symbols,
        );

        let layout = DataLayout::for_graph(&graph, 16, None, false);
        assert_eq!(layout.slot(first), Some((0, 1)));
        assert_eq!(layout.slot(counter), Some((1, 5)));
        assert_eq!(layout.slot(last), Some((6, 1)));
        assert_eq!(layout.total_vars(), 7);
    }

    #[test]
    fn money_and_the_clock_cost_nothing_when_they_cannot_move() {
        let mut symbols = StateSymbols::new();
        symbols.variable("a");
        let graph = graph_with(vec![], symbols);

        let still = DataLayout::for_graph(&graph, 16, None, false);
        assert_eq!(still.total_vars(), 1);
        assert_eq!(still.money(), None);
        assert_eq!(still.clock(), None);

        let moving = DataLayout::for_graph(&graph, 16, Some(1000), true);
        // Ten bits for money up to 1000, eleven for a day of minutes.
        assert_eq!(moving.money(), Some((1, 10)));
        assert_eq!(moving.clock(), Some((11, 11)));
        assert_eq!(moving.total_vars(), 22);
    }

    #[test]
    fn bit_widths_are_inclusive_of_the_maximum() {
        assert_eq!(bits_for(0), 1);
        assert_eq!(bits_for(1), 1);
        assert_eq!(bits_for(2), 2);
        assert_eq!(bits_for(16), 5);
        assert_eq!(bits_for(1439), 11);
    }
}
