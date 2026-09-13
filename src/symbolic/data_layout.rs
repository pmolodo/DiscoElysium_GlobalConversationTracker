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

use crate::core::guard::{Guard, GuardExpression, GuardRef};
use crate::core::state::{
    ITEM_PREFIX, ONCE_PREFIX, SEEN_PREFIX, StateSymbols, TASK_PREFIX, THOUGHT_PREFIX,
};
use crate::core::types::DialogueNodeId;
use crate::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::world::MONEY_QUERY;

/// Minutes in a day; the clock is wrapped into `0..MINUTES_IN_DAY`.
const MINUTES_IN_DAY: u32 = 1440;

/// What a slot held as a DELTA needs from the encoding it replaced.
///
/// Both numbers describe the ABSOLUTE reading - the value a guard is written about - which
/// the slot no longer holds. See [`DataLayout::narrow_to_deltas`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeltaSlot {
    /// What the search's starting value was clamped into, which is the width the slot would
    /// have had without rebasing.
    pub ceiling: u32,
    /// What an increment saturates at, which is the counter cap held down to the ceiling.
    pub cap: u32,
}

/// How many bits it takes to represent `0..=max`.
fn bits_for(max: u32) -> u8 {
    if max == 0 {
        1
    } else {
        (u32::BITS - max.leading_zeros()) as u8
    }
}

/// Where each part of a data state lives, in variable numbers.
#[derive(Debug, Clone)]
pub struct DataLayout {
    /// Per slot: the first variable of its little-endian run, and how many bits it has.
    slots: Vec<(u32, u8)>,
    money: Option<(u32, u8)>,
    clock: Option<(u32, u8)>,
    total: u32,
    /// The slots held as a DELTA from the value the search started with, and the value the
    /// absolute reading of each saturates at. See [`Self::narrow_to_deltas`].
    ///
    /// A map rather than a per-slot field because it is nearly always empty: 26 slots of
    /// 104 over the whole game, in 24 of its 521 groups.
    deltas: HashMap<usize, DeltaSlot>,
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
        let deltas = Self::narrow_to_deltas(graph, &mut widths, counter_cap);

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

        Self {
            slots,
            money,
            clock,
            total: next,
            deltas,
        }
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
            let Some(&high) = highest.get(&slot) else {
                continue;
            };
            let wanted = bits_for(high.saturating_add(1)).max(assigned[slot]);
            *width = (*width).min(wanted);
        }
    }

    /// Squeezes each slot to the range its own increments can COVER, by holding the
    /// distance from the value the search started at rather than the value itself.
    ///
    /// ## The argument
    ///
    /// The threshold narrowing above bounds a slot by what the guards can distinguish. It
    /// cannot bound it by what the group can REACH, because the search starts at whatever
    /// the save holds and the save can hold anything. Rebasing removes that: the slot holds
    /// `d`, the search's own contribution, and the true value is `v0 + d` where `v0` is what
    /// the save held. `v0` never enters the slot, so the slot only has to be wide enough for
    /// what the group's own increments add up to.
    ///
    /// Where the group adds at most two and the guards compare against seven, that is two
    /// bits instead of three.
    ///
    /// ## What it costs, and where the cost is paid
    ///
    /// The value the guards want is the absolute one, so every comparison against a rebased
    /// slot has to be rewritten - `v >= c` becomes `d >= c - v0` - and that needs `v0`, which
    /// is world-dependent. THE LAYOUT STAYS WORLD-INDEPENDENT ANYWAY, which is what keeps
    /// [`crate::workspace::Key`] intact: the WIDTH is the sum of the group's increments and
    /// the sum does not depend on the save. Only the rewriting does, and guards are compiled
    /// per request already. [`super::guard_formula::GuardCompiler`] does it.
    ///
    /// `saturates_at` is the other half of the bargain, and the reason the map holds a value
    /// rather than a flag. The absolute reading saturates - a counter stops at its cap - and
    /// a rebased slot has lost the ceiling that used to do it, so the ceiling has to be
    /// remembered for the rewriting to saturate in its place. Without it a slot whose group
    /// adds thirty would answer a guard the shipped encoding saturates at sixteen.
    ///
    /// ## The three things that disqualify a slot
    ///
    /// AN ASSIGN, which writes a number rather than adding one: `d := c - v0` is negative
    /// wherever the save holds more than the assignment.
    ///
    /// A DECREMENT, for the same reason from the other side.
    ///
    /// AN INCREMENT THAT CAN FIRE TWICE, which is where the strongly connected components
    /// come in: the bound is "every site fired once, summed", and a site inside a dialogue
    /// loop fires as often as the loop goes round. An action marked `once` is exempt - it
    /// fires at most once whatever the link structure does.
    ///
    /// ## Only where it actually wins
    ///
    /// A slot keeps the absolute encoding unless rebasing makes it strictly narrower, so
    /// the rewriting above applies to the few slots that paid for it rather than to every
    /// counter in the game.
    fn narrow_to_deltas(
        graph: &LookAheadGraph,
        widths: &mut [u8],
        counter_cap: i32,
    ) -> HashMap<usize, DeltaSlot> {
        /// What one slot's increments add up to, and whether anything disqualifies it.
        #[derive(Default)]
        struct Reach {
            sum: u32,
            barred: bool,
        }

        let order = super::order::IterationOrder::of(graph);
        let cyclic = Self::entries_on_a_cycle(graph, &order);

        let mut reach: HashMap<usize, Reach> = HashMap::new();
        for node in graph.nodes() {
            let repeatable = cyclic.contains(&node.id);
            for action in &node.actions {
                let slot = action.slot();
                if slot < 0 || slot as usize >= widths.len() {
                    continue;
                }
                let slot = slot as usize;
                match action.kind() {
                    DialogueActionKind::Increment => {
                        let entry = reach.entry(slot).or_default();
                        let amount = action.value();
                        if amount < 0 || (repeatable && !action.once()) {
                            entry.barred = true;
                        }
                        entry.sum = entry.sum.saturating_add(amount.max(0) as u32);
                    }
                    DialogueActionKind::Assign => reach.entry(slot).or_default().barred = true,
                    _ => {}
                }
            }
        }

        // THE SEED MUST BE RECOVERABLE FROM THE VARIABLE'S OWN VALUE, because the rewriting
        // has to reproduce exactly what the slot would have started at. `seed_state` fills
        // these five kinds from somewhere else - the inventory, the journal, the thought
        // cabinet, and the entries the save records as seen - so a rebasing of one of them
        // would be rebasing by the wrong number. None of them is a counter in this content;
        // barring them costs nothing and removes the whole question.
        let symbols = graph.symbols();
        let elsewhere = |slot: usize| {
            symbols.name_of(slot).is_some_and(|name| {
                [
                    ITEM_PREFIX,
                    TASK_PREFIX,
                    THOUGHT_PREFIX,
                    ONCE_PREFIX,
                    SEEN_PREFIX,
                ]
                .iter()
                .any(|prefix| name.starts_with(prefix))
            })
        };

        let mut deltas = HashMap::new();
        for (slot, found) in reach {
            if found.barred || found.sum == 0 || elsewhere(slot) {
                continue;
            }
            let absolute = widths[slot];
            let rebased = bits_for(found.sum);
            if rebased >= absolute {
                continue;
            }
            // WHAT THE ABSOLUTE ENCODING DID, which the rewriting now has to do by hand:
            // the seed clamped into the slot's own ceiling, and an increment saturated at
            // the counter cap. The two are different numbers whenever the thresholds left
            // the slot wider than the cap needs, and a rewriting that used one for both
            // would answer a guard the shipped encoding does not.
            let ceiling = (1u32 << absolute) - 1;
            deltas.insert(
                slot,
                DeltaSlot {
                    ceiling,
                    cap: ceiling.min(counter_cap.max(0) as u32),
                },
            );
            widths[slot] = rebased;
        }

        deltas
    }

    /// Every entry the group can arrive at twice.
    ///
    /// A strongly connected component of more than one entry is a cycle by definition, and
    /// an entry that links to itself is a cycle of one - which Tarjan puts in a component by
    /// itself, so it has to be caught separately rather than by the size.
    fn entries_on_a_cycle(
        graph: &LookAheadGraph,
        order: &super::order::IterationOrder,
    ) -> HashSet<DialogueNodeId> {
        let mut size: HashMap<u32, usize> = HashMap::new();
        for node in graph.nodes() {
            if let Some(component) = order.component_of(node.id) {
                *size.entry(component).or_default() += 1;
            }
        }

        graph
            .nodes()
            .filter(|node| {
                node.links.contains(&node.id)
                    || order
                        .component_of(node.id)
                        .and_then(|component| size.get(&component))
                        .is_some_and(|members| *members > 1)
            })
            .map(|node| node.id)
            .collect()
    }

    /// Collects, per slot, the largest constant compared against it - and which slots are
    /// compared in a shape this cannot read.
    ///
    /// See [`Self::narrow_to_thresholds`] for why the second is not merely an omission.
    /// A SWEEP RATHER THAN A WALK, because every node is looked at wherever it sits. The
    /// structure is wanted one node at a time - which side of a comparison names a slot,
    /// and what a call was handed - and never between nodes, so nothing here descends.
    fn read_comparisons(
        guard: &Guard,
        symbols: &StateSymbols,
        highest: &mut HashMap<usize, u32>,
        unreadable: &mut HashSet<usize>,
    ) {
        for node in guard.nodes() {
            match node.expression() {
                GuardExpression::Comparison(_, a, b) => {
                    for (side, other) in [(a, b), (b, a)] {
                        let Some(slot) = Self::slot_named(side, symbols) else {
                            continue;
                        };
                        let GuardExpression::Literal(value) = other.expression() else {
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
                }
                // A SLOT HANDED TO A QUERY is not something this can reason about at all.
                GuardExpression::Call(_, arguments) => {
                    for argument in arguments.iter() {
                        if let Some(slot) = Self::slot_named(argument, symbols) {
                            unreadable.insert(slot);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// The slot a guard expression names, if it simply names one.
    fn slot_named(guard: GuardRef<'_>, symbols: &StateSymbols) -> Option<usize> {
        let GuardExpression::Variable(name) = guard.expression() else {
            return None;
        };
        (0..symbols.count()).find(|slot| symbols.name_of(*slot) == Some(name))
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
        world: &dyn crate::world::ILookAheadWorld,
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
        world: &dyn crate::world::ILookAheadWorld,
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

        Self::for_graph(
            graph,
            counter_cap,
            Self::money_ceiling(graph, world.money()),
            false,
        )
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
            let Some(name) = symbols.name_of(slot) else {
                continue;
            };
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

    /// Whether this slot holds a DELTA from the value the search started at, and if so the
    /// value its absolute reading saturates at.
    ///
    /// Everything that reads or writes such a slot has to know: [`super::action_image`] so an
    /// increment saturates at the slot's own top rather than at the counter cap, the guard
    /// compiler so a comparison is rebased, and [`super::reachability::seed_of`] so the
    /// search starts at a distance of nothing rather than at the save's value. See
    /// [`Self::narrow_to_deltas`].
    pub fn delta_slot(&self, slot: usize) -> Option<DeltaSlot> {
        // A DROPPED SLOT IS NOT A DELTA SLOT, because it is not a slot - `keeping_only_read`
        // zeroes the width and leaves the index, and this map is keyed by index.
        self.slot(slot)?;
        self.deltas.get(&slot).copied()
    }

    /// Whether a slot is a single bit, which is the common case.
    pub fn is_boolean(&self, slot: usize) -> bool {
        self.slots
            .get(slot)
            .map(|(_, bits)| *bits == 1)
            .unwrap_or(false)
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
    fn guard_reads_money(guard: &Guard) -> bool {
        guard
            .nodes()
            .any(|node| matches!(node.expression(), GuardExpression::Call(name, _) if name == MONEY_QUERY))
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
                if let Ok(slot) = usize::try_from(slot)
                    && let Some(name) = symbols.name_of(slot)
                {
                    names.insert(name.to_string());
                }
            }
        }

        names
    }

    /// The names one guard reads, including the subjects of the queries answered from
    /// search state.
    fn read_by_guard(guard: &Guard, names: &mut HashSet<String>) {
        for node in guard.nodes() {
            match node.expression() {
                GuardExpression::Variable(name) => {
                    names.insert(name.to_string());
                }
                GuardExpression::Call(function, arguments) => {
                    // The two queries `BoundContext::query` answers from a slot. Their
                    // subject is a literal string, and the slot it corresponds to carries a
                    // prefix.
                    let prefix = match function {
                        "CheckItem" => Some(ITEM_PREFIX),
                        "IsTaskActive" => Some(TASK_PREFIX),
                        "IsTHCPresent" => Some(THOUGHT_PREFIX),
                        // FlagSet(name) is Variable[name] written another way.
                        "FlagSet" => Some(""),
                        _ => None,
                    };

                    let Some(prefix) = prefix else { continue };
                    let Some(only) = arguments.only() else {
                        continue;
                    };
                    if let GuardExpression::Literal(value) = only.expression() {
                        names.insert(format!("{prefix}{}", value.text()));
                    }
                }
                _ => {}
            }
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
            node.actions
                .iter()
                .any(|a| a.kind() == DialogueActionKind::PassTime)
        })
    }

    /// How many slots carry a name with the given prefix, such as `item:` or `task:`.
    ///
    /// A slot exists only for something an ACTION in the group touches, so this counts
    /// what the group actually manipulates rather than what the game contains.
    pub fn slots_named(symbols: &crate::core::state::StateSymbols, prefix: &str) -> usize {
        (0..symbols.count())
            .filter(|slot| {
                symbols
                    .name_of(*slot)
                    .is_some_and(|name| name.starts_with(prefix))
            })
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
    use crate::core::guard::Guard;
    use crate::core::state::StateSymbols;
    use crate::core::types::DialogueNodeId;
    use crate::graph::node::LookAheadNode;

    fn graph_with(actions: Vec<DialogueAction>, symbols: StateSymbols) -> LookAheadGraph {
        graph_linking(actions, symbols, vec![])
    }

    /// The same one-entry graph, linking where it is told to.
    ///
    /// A link back to itself is what makes an increment repeatable, which is what keeps a
    /// slot on the absolute encoding - see [`DataLayout::narrow_to_deltas`].
    fn graph_linking(
        actions: Vec<DialogueAction>,
        symbols: StateSymbols,
        links: Vec<DialogueNodeId>,
    ) -> LookAheadGraph {
        let node = LookAheadNode {
            actions,
            links,
            ..LookAheadNode::new(DialogueNodeId::new(1, 0))
        };
        LookAheadGraph::new(vec![node], symbols).unwrap()
    }

    /// A one-entry graph whose increment can fire again, because the entry links to itself.
    fn looping_graph_with(actions: Vec<DialogueAction>, symbols: StateSymbols) -> LookAheadGraph {
        graph_linking(actions, symbols, vec![DialogueNodeId::new(1, 0)])
    }

    #[test]
    fn a_slot_only_ever_set_to_one_stays_a_single_bit() {
        let mut symbols = StateSymbols::new();
        let slot = symbols.variable("met_kim");
        let graph = graph_with(
            vec![DialogueAction::assign(
                slot,
                1,
                "SetVariableValue".to_string(),
            )],
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
        // ON A LOOP, so the increment can fire as often as the loop goes round and the cap
        // is what bounds it. An increment that fires once is bounded by its own amount.
        let graph = looping_graph_with(
            vec![DialogueAction::increment(
                slot,
                1,
                false,
                "SetVariableValue".to_string(),
            )],
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
            vec![DialogueAction::assign(
                slot,
                5,
                "SetVariableValue".to_string(),
            )],
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
        let graph = looping_graph_with(
            vec![DialogueAction::increment(
                counter,
                1,
                false,
                "SetVariableValue".to_string(),
            )],
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

    /// The saving the whole rebasing exists for: a counter nothing can fire twice needs room
    /// for its own increments and nothing more.
    #[test]
    fn an_increment_that_cannot_repeat_is_held_as_a_distance() {
        let mut symbols = StateSymbols::new();
        let slot = symbols.variable("bribes_taken");
        let graph = graph_with(
            vec![DialogueAction::increment(
                slot,
                1,
                false,
                "SetVariableValue".to_string(),
            )],
            symbols,
        );

        let layout = DataLayout::for_graph(&graph, 16, None, false);
        assert_eq!(layout.slot(slot), Some((0, 1)));
        let delta = layout
            .delta_slot(slot)
            .expect("rebased, since it narrowed the slot");
        // The absolute reading is what the cap and the old five-bit ceiling described.
        assert_eq!(delta.ceiling, 31);
        assert_eq!(delta.cap, 16);
    }

    /// The bound is the SUM of the sites, not the largest of them.
    #[test]
    fn a_distance_has_room_for_every_site_firing() {
        let mut symbols = StateSymbols::new();
        let slot = symbols.variable("tally");
        let graph = graph_with(
            vec![
                DialogueAction::increment(slot, 2, false, "SetVariableValue".to_string()),
                DialogueAction::increment(slot, 3, false, "SetVariableValue".to_string()),
            ],
            symbols,
        );

        // Five needs three bits, which is still narrower than the cap's five.
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        assert_eq!(layout.slot(slot), Some((0, 3)));
        assert!(layout.delta_slot(slot).is_some());
    }

    /// An assign writes a number rather than adding one, so a distance cannot express it.
    #[test]
    fn a_slot_something_assigns_is_not_rebased() {
        let mut symbols = StateSymbols::new();
        let slot = symbols.variable("stage");
        let graph = graph_with(
            vec![
                DialogueAction::increment(slot, 1, false, "SetVariableValue".to_string()),
                DialogueAction::assign(slot, 4, "SetVariableValue".to_string()),
            ],
            symbols,
        );

        let layout = DataLayout::for_graph(&graph, 16, None, false);
        assert_eq!(layout.delta_slot(slot), None);
        assert_eq!(layout.slot(slot), Some((0, 5)));
    }

    /// The soundness condition: a site on a loop fires as often as the loop goes round, so
    /// the sum of the sites is not a bound on anything.
    #[test]
    fn an_increment_on_a_loop_is_not_rebased() {
        let mut symbols = StateSymbols::new();
        let slot = symbols.variable("loop_counter");
        let graph = looping_graph_with(
            vec![DialogueAction::increment(
                slot,
                1,
                false,
                "SetVariableValue".to_string(),
            )],
            symbols,
        );

        let layout = DataLayout::for_graph(&graph, 16, None, false);
        assert_eq!(layout.delta_slot(slot), None);
    }

    /// Unless the action fires at most once by construction, which a loop cannot undo.
    #[test]
    fn a_once_increment_on_a_loop_is_still_rebased() {
        let mut symbols = StateSymbols::new();
        let slot = symbols.variable("first_time_only");
        let graph = looping_graph_with(
            vec![DialogueAction::increment(
                slot,
                1,
                true,
                "SetVariableValue".to_string(),
            )],
            symbols,
        );

        let layout = DataLayout::for_graph(&graph, 16, None, false);
        assert!(layout.delta_slot(slot).is_some());
        assert_eq!(layout.slot(slot), Some((0, 1)));
    }

    /// Rebasing may only ever narrow. A slot the guards have already squeezed below what its
    /// increments add up to keeps what it has, and stays on the absolute encoding.
    #[test]
    fn rebasing_never_widens_a_slot_the_thresholds_already_squeezed() {
        let mut symbols = StateSymbols::new();
        let slot = symbols.variable("tally");
        let mut node = LookAheadNode {
            guard: Guard::comparison(
                ">=".to_string(),
                Guard::variable("tally".to_string()),
                Guard::literal(crate::core::guard_value::GuardValue::from_number(1.0)),
            ),
            actions: vec![DialogueAction::increment(
                slot,
                9,
                false,
                "SetVariableValue".to_string(),
            )],
            ..LookAheadNode::new(DialogueNodeId::new(1, 0))
        };
        node.actions
            .push(DialogueAction::increment(slot, 9, false, "s".to_string()));
        let graph = LookAheadGraph::new(vec![node], symbols).unwrap();

        // The only threshold is 1, so the slot saturates at 2 and needs two bits; the
        // eighteen its increments add up to would ask for five.
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        assert_eq!(layout.slot(slot), Some((0, 2)));
        assert_eq!(layout.delta_slot(slot), None);
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
