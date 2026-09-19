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
use crate::core::clock::ClockReading;
use std::collections::{HashMap, HashSet};

use crate::core::guard::{Guard, GuardExpression, GuardRef};
use crate::core::state::{
    DAMAGE_PREFIX, ITEM_PREFIX, ONCE_PREFIX, SEEN_PREFIX, StateSymbols, THOUGHT_PREFIX,
    UNEQUIPPED_PREFIX,
};
use crate::core::types::DialogueNodeId;
use crate::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::symbolic::var_order::Ordering;
use crate::world::MONEY_QUERY;

/// Minutes in a day; the clock is wrapped into `0..MINUTES_IN_DAY`.
const MINUTES_IN_DAY: u32 = 1440;

/// How many bits it takes to represent `0..=max`.
fn bits_for(max: u32) -> u8 {
    if max == 0 {
        1
    } else {
        (u32::BITS - max.leading_zeros()) as u8
    }
}

/// Counter slots whose value is a second copy of state the search already carries, and the
/// `once` slots that determine each one.
///
/// ## What makes a counter redundant
///
/// Every write to it an INCREMENT BY ONE, every one of them `once`, and nothing assigning or
/// decrementing it. A `once` action fires only while its entry's own `once_slot` is clear and
/// raises that slot as it fires - so the guard and the increment are on ONE node by
/// construction, and the counter moves by exactly as many as those slots gain, on every route
/// that could reach anything reading it. There is no dominance question to ask, because there
/// is no route where one happens without the other.
///
/// The world's starting value is no bar. The slots are seeded from the save, so what fired
/// before it was written is in the count already; what a conversation OUTSIDE this group
/// contributed is not, and is a constant for the request. `DataLayout::counter_onces` carries
/// that constant and the guards rebase their thresholds by it.
///
/// ## Why this is worth more than the bits
///
/// A slot three bits wide costs three decision-diagram variables, which is the small part. The
/// large part is that the diagram must carry the RELATION between them and the once slots -
/// `counter == how many of these five are set` - and no variable order makes that cheap: the
/// counter's bits have to interleave with the once bits, and whatever order is chosen is wrong
/// for some of them. Dropping the counter does not narrow the state by three bits, it deletes a
/// constraint over six variables and leaves five independent ones with nothing to relate.
///
/// Conversation 1168, in 761's group, holds two of these: `seafort.deserter_charge_counter`,
/// five sites and three bits, and `seafort.deserter_scope_hub_counter`, six sites and two. The
/// first is the deserter confession - press him on at least three of five charges.
///
/// ## What a caller still has to do
///
/// DROPPING THE SLOT IS NOT ENOUGH ON ITS OWN, and is unsound by itself: a guard still reading
/// it would compile against a slot the layout does not carry, which reads as unknown and lets
/// the gate through. The comparison has to be rewritten as a threshold over the returned slots
/// first. See de-bfs0.
/// How many `once` slots a counter may be determined by and still be worth dropping.
///
/// ## Why there is a limit at all
///
/// Dropping replaces a comparison against a few bits with `at_least_set` over every contributing
/// slot, whose width is the NUMBER of contributors rather than the log of it. Somewhere that
/// stops being a trade worth making, and conversation 347 is past it: one of its counters is
/// determined by THIRTY once slots, and dropping it took the group from 4,429 diagram nodes to
/// 9,427.
///
/// ## Where the line is, measured
///
/// Whole game, the 31 groups holding a redundant counter in a layout of at least twenty
/// variables - below that a counter IS most of the state and a percentage says nothing - by how
/// many once slots the widest counter has:
///
/// ```text
///   1 to 9 once slots     29 groups, every one of them cheaper
///  11 once slots           1 group,  +2.7 per cent      (conversation 1105)
///  30 once slots           1 group,  +112.8 per cent    (conversation 347)
/// ```
///
/// So the cutoff is free: the two groups it stops dropping for are the two that lost, and they
/// had no saving to give up. Eight would not be - conversation 362's counter has nine and saves
/// 2,254 nodes - which is why this is ten rather than a rounder-looking number.
///
/// WHAT IT IS NOT. Not a bound on how far the relation REACHES across the variable order, which
/// was the other candidate and does not separate: conversation 379 reaches 97 per cent of its
/// layout and saves 12 per cent, where 347 reaches 97 per cent and costs 113. See de-0q6b.
const MOST_ONCES: usize = 10;

pub fn counters_from_onces(graph: &LookAheadGraph) -> HashMap<usize, Vec<(DialogueNodeId, usize)>> {
    let mut contributors: HashMap<usize, Vec<(DialogueNodeId, usize)>> = HashMap::new();
    let mut disqualified: HashSet<usize> = HashSet::new();

    for node in graph.nodes() {
        for action in node.all_actions() {
            let slot = action.slot();
            if slot < 0 {
                continue;
            }
            let slot = slot as usize;
            match action.kind() {
                DialogueActionKind::Increment => {
                    // BY ONE AND `once`, or the count of fired slots is not the value.
                    if action.value() != 1 || !action.once() || node.once_slot < 0 {
                        disqualified.insert(slot);
                        continue;
                    }
                    contributors
                        .entry(slot)
                        .or_default()
                        .push((node.id, node.once_slot as usize));
                }
                // AN ASSIGNMENT PUTS A VALUE IN THE SLOT THAT NO COUNT EXPLAINS.
                DialogueActionKind::Assign => {
                    disqualified.insert(slot);
                }
                _ => {}
            }
        }
    }

    contributors.retain(|slot, onces| {
        onces.sort_unstable_by_key(|(node, once)| (node.conversation_id, node.entry_id, *once));
        onces.dedup();
        !disqualified.contains(slot) && !onces.is_empty()
    });
    // ASKED ONLY WHERE THERE IS SOMETHING TO ASK ABOUT. The sweep walks every guard in the
    // group, which the great majority of groups would pay for nothing: most hold no counter
    // whose every writer is `once`.
    if !contributors.is_empty() {
        let rewritable = read_only_as_thresholds(graph);
        contributors.retain(|slot, _| rewritable.contains(slot));
    }
    contributors
}

/// The slots whose every read is a comparison against a constant, which is the only shape the
/// count can be substituted into.
///
/// ## Why the writers are not the whole story
///
/// `counters_from_onces` establishes what a slot's value IS. This establishes that nothing
/// needs the slot to hold it. Dropping the bits is only sound where every reader has been
/// taught to ask the once slots instead, and exactly one has -
/// `GuardCompiler::comparison` on a whole-number literal, which rewrites to a threshold.
///
/// EVERY OTHER READER STILL WANTS THE BITS. A reputation question compares a range's amounts
/// with each other rather than with a constant and reads them through
/// `GuardCompiler::amounts_of`; a slot handed to a query is read by whatever the query does; a
/// variable used as a bare condition asks whether its run of bits is non-zero. A slot dropped
/// out from under any of those does not fail loudly - it reads as undecided, which is the
/// permissive answer, and opens a gate that should have stayed shut.
///
/// So this counts, per slot, how often it appears as a variable at all and how often it
/// appears as one side of a comparison the compiler can rewrite, and keeps only the slots
/// where those agree. [`DataLayout::read_comparisons`] supplies the shapes that are reads
/// without being variable nodes - a reputation range, a call's argument - which no count of
/// variable nodes would see.
fn read_only_as_thresholds(graph: &LookAheadGraph) -> HashSet<usize> {
    let GuardReads {
        unreadable,
        mentions,
        as_threshold,
        ..
    } = DataLayout::guard_reads(graph);

    mentions
        .into_iter()
        .filter(|(slot, seen)| !unreadable.contains(slot) && as_threshold.get(slot) == Some(seen))
        .map(|(slot, _)| slot)
        .collect()
}

/// What one sweep of a group's guards says about how each slot is read.
///
/// ONE SWEEP RATHER THAN THREE. Two questions want this - how wide a slot has to be, and
/// whether its every read is a shape the compiler can rewrite - and on the biggest groups the
/// guards are large enough that walking them again to ask the second costs more than the
/// answer saves.
#[derive(Debug, Default)]
struct GuardReads {
    /// The largest constant compared against each slot, which bounds how wide it must be.
    highest: HashMap<usize, u32>,
    /// Slots read in a shape no constant describes - a reputation range, a query's argument,
    /// a comparison against something other than a literal.
    unreadable: HashSet<usize>,
    /// How many times each slot is named as a variable at all.
    mentions: HashMap<usize, usize>,
    /// How many of those are one side of a comparison against a constant the compiler can
    /// rewrite. Equal to `mentions` exactly when nothing reads the slot any other way.
    as_threshold: HashMap<usize, usize>,
}

/// Where each part of a data state lives, in variable numbers.
#[derive(Debug, Clone)]
pub struct DataLayout {
    /// Per slot: the first variable of its little-endian run, and how many bits it has.
    slots: Vec<(u32, u8)>,
    money: Option<(u32, u8)>,
    clock: Option<(u32, u8)>,
    total: u32,
    /// The slots held as a DELTA from the value the search started with. See
    /// [`Self::lay_out_counters`].
    ///
    /// A set rather than a per-slot field because most slots are not counters.
    deltas: HashSet<usize>,
    /// Counter slots dropped because their value is how many of some `once` slots are set, and
    /// which slots those are. Empty unless [`Self::dropping_redundant_counters`] ran.
    ///
    /// A GUARD READING ONE OF THESE MUST BE REWRITTEN, not compiled against the slot: the slot
    /// is gone, and a comparison against a slot the layout does not carry reads as unknown,
    /// which lets the gate through. [`crate::symbolic::guard_formula::GuardCompiler`] asks this
    /// and builds a threshold over the listed slots instead.
    from_onces: HashMap<usize, (Vec<usize>, i32)>,
    /// The counters held as their value that saturate at their own ceiling rather than at the
    /// counter cap, because they cannot loop. See [`Self::lay_out_counters`].
    unsaturated: HashSet<usize>,
    /// The sequence [`Self::renumber`] hands out variables in, which IS the variable order.
    ///
    /// A permutation of the slot indices, `0..slots.len()` unless
    /// [`Self::in_variable_order`] put the slots in another one. See
    /// [`crate::symbolic::var_order`] for what else it could be and why it matters.
    order: Vec<usize>,
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
            for action in node.all_actions() {
                let slot = action.slot();
                if slot < 0 || slot as usize >= slot_count {
                    continue;
                }

                let slot = slot as usize;
                let needed = match action.kind() {
                    DialogueActionKind::Increment => bits_for(counter_cap.max(0) as u32),
                    DialogueActionKind::Assign | DialogueActionKind::AssignUnless => {
                        let bits = bits_for(action.value().max(0) as u32);
                        assigned[slot] = assigned[slot].max(bits);
                        bits
                    }
                    // WIDE ENOUGH FOR ANY DAY'S READING, since the layout outlives the world
                    // and the value is read off the clock when the action runs.
                    DialogueActionKind::AssignClock => {
                        let bits = bits_for(ClockReading::VALUE_CEILING);
                        assigned[slot] = assigned[slot].max(bits);
                        bits
                    }
                    // Money, clock and unmodelled actions do not write a slot.
                    _ => continue,
                };
                widths[slot] = widths[slot].max(needed);
            }
        }

        let unsqueezed = widths.clone();
        Self::narrow_to_thresholds(graph, &mut widths, &assigned);
        let (deltas, unsaturated) = Self::lay_out_counters(graph, &mut widths, &unsqueezed);

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
            order: (0..slots.len()).collect(),
            slots,
            money,
            clock,
            total: next,
            deltas,
            unsaturated,
            from_onces: HashMap::new(),
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
    /// against nothing on 14 (`performance/layout_shape.rs`).
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
        let GuardReads {
            highest,
            unreadable,
            ..
        } = Self::guard_reads(graph);

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

    /// Lays out every counter that cannot loop without the counter cap, holding it as the
    /// DISTANCE from the value the search started at wherever that is what keeps it exact.
    ///
    /// ## Why these counters are not capped
    ///
    /// The cap keeps a counter finite when a dialogue loop can raise it without end. A counter
    /// that cannot loop - see [`LookAheadGraph::counters_that_cannot_loop`] - climbs at most by
    /// the sum of its raises, so it is finite already, and a cap would only clip a value the
    /// game keeps. The explicit engine leaves the same counters uncapped
    /// ([`crate::core::action::CounterCaps::for_graph`]), so both read every value alike.
    ///
    /// ## The distance
    ///
    /// The threshold narrowing above bounds a slot by what the guards can distinguish. It
    /// cannot bound it by what the group can REACH, because the search starts at whatever the
    /// save holds and the save can hold anything. Rebasing removes that: the slot holds `d`,
    /// the search's own contribution, and the true value is `v0 + d` where `v0` is what the
    /// save held. `v0` never enters the slot, so the slot only has to be wide enough for what
    /// the group's own increments add up to, and nothing is clamped or saturated.
    ///
    /// The value the guards want is the absolute one, so every comparison against a rebased
    /// slot is rewritten - `v >= c` becomes `d >= c - v0` - and that needs `v0`, which is
    /// world-dependent. THE LAYOUT STAYS WORLD-INDEPENDENT ANYWAY, which is what keeps
    /// [`crate::workspace::Key`] intact: the WIDTH is the sum of the group's increments and the
    /// sum does not depend on the save. Only the rewriting does, and guards are compiled per
    /// request already. [`super::guard_formula::GuardCompiler`] does it.
    ///
    /// ## Where a counter keeps its value instead
    ///
    /// Where the thresholds already squeezed the slot to no wider than its distance would
    /// need. Every guard then compares it against constants below its ceiling, so every value
    /// above the ceiling answers every guard alike - which makes saturating at the CEILING,
    /// and clamping the save's value into it, exact. It is the counter cap that is not: a
    /// ceiling of 31 for guards comparing against 20 would be cut to 16. So such a slot is
    /// recorded as unsaturated, and saturates at its own ceiling.
    ///
    /// A slot the thresholds did not squeeze has only the cap's width to hold its value in, and
    /// a save can arrive with more, so it is always held as a distance.
    fn lay_out_counters(
        graph: &LookAheadGraph,
        widths: &mut [u8],
        unsqueezed: &[u8],
    ) -> (HashSet<usize>, HashSet<usize>) {
        let mut deltas = HashSet::new();
        let mut unsaturated = HashSet::new();
        for (slot, sum) in graph.counters_that_cannot_loop() {
            if slot >= widths.len() {
                continue;
            }
            let distance = bits_for(sum);
            if widths[slot] < unsqueezed[slot] && widths[slot] <= distance {
                unsaturated.insert(slot);
            } else {
                deltas.insert(slot);
                widths[slot] = distance;
            }
        }
        (deltas, unsaturated)
    }

    /// Every entry the group can arrive at twice.
    ///
    /// A strongly connected component of more than one entry is a cycle by definition, and
    /// an entry that links to itself is a cycle of one - which Tarjan puts in a component by
    /// itself, so it has to be caught separately rather than by the size.
    pub(crate) fn entries_on_a_cycle(
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

    /// Collects, per slot, everything one sweep of the group's guards can say about how it is
    /// read - see [`GuardReads`], and [`Self::read_comparisons`] for the sweep.
    fn guard_reads(graph: &LookAheadGraph) -> GuardReads {
        let symbols = graph.symbols();
        let mut reads = GuardReads::default();
        for node in graph.nodes() {
            Self::read_comparisons(&node.guard, symbols, &mut reads);
        }
        reads
    }

    /// Collects, per slot, the largest constant compared against it - and which slots are
    /// compared in a shape this cannot read.
    ///
    /// See [`Self::narrow_to_thresholds`] for why the second is not merely an omission.
    /// A SWEEP RATHER THAN A WALK, because every node is looked at wherever it sits. The
    /// structure is wanted one node at a time - which side of a comparison names a slot,
    /// and what a call was handed - and never between nodes, so nothing here descends.
    fn read_comparisons(guard: &Guard, symbols: &StateSymbols, reads: &mut GuardReads) {
        let GuardReads {
            highest,
            unreadable,
            mentions,
            as_threshold,
        } = reads;
        for node in guard.nodes() {
            match node.expression() {
                GuardExpression::Variable(name) => {
                    if let Some(slot) = symbols.find(name) {
                        *mentions.entry(slot).or_default() += 1;
                    }
                }
                GuardExpression::Comparison(_, a, b) => {
                    for (side, other) in [(a, b), (b, a)] {
                        let Some(slot) = Self::slot_named(side, symbols) else {
                            continue;
                        };
                        let GuardExpression::Literal(value) = other.expression() else {
                            unreadable.insert(slot);
                            continue;
                        };
                        // WHAT THE GUARD COMPILER CAN REWRITE, which is a narrower question than
                        // what a width can be read off - see [`read_only_as_thresholds`].
                        if crate::symbolic::guard_formula::whole_number(value).is_some() {
                            *as_threshold.entry(slot).or_default() += 1;
                        }
                        let number = value.number();
                        if !number.is_finite() || number < 0.0 {
                            unreadable.insert(slot);
                            continue;
                        }
                        let seen = highest.entry(slot).or_insert(0);
                        *seen = (*seen).max(number as u32);
                    }
                }
                // A REPUTATION QUESTION compares its range's amounts with EACH OTHER, not with
                // a constant, so no threshold says where the amounts stop mattering: raising
                // 20 to 21 overtakes a rival at 20.
                GuardExpression::Call(function, _)
                    if crate::core::reputation::range_of(function).is_some() =>
                {
                    for variable in crate::core::reputation::variables_read_by(function) {
                        if let Some(slot) = symbols.find(&variable) {
                            unreadable.insert(slot);
                        }
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
        symbols.find(name)
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
    /// `performance/start_relative_layout.rs` measured all three granularities:
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
    /// across requests and `performance/manager_reuse.rs` prices that at 45 ms a request; a
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
        .dropping_redundant_counters(graph, world)
        .in_variable_order(graph, Ordering::asked_for())
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

    /// The same layout with every counter dropped whose value is how many `once` slots are set.
    ///
    /// EXACT, NOT AN APPROXIMATION, unlike [`Self::without_visit_flags`]. The dropped slot's
    /// value is recoverable from slots the layout still carries and a constant this records -
    /// see [`counters_from_onces`] for why the count moves with the value, and
    /// [`Self::counter_onces`] for the constant between them.
    ///
    /// A CALLER MUST REWRITE THE GUARDS. This leaves the map behind in [`Self::counter_onces`]
    /// precisely because dropping alone is unsound: a comparison compiled against a slot the
    /// layout no longer carries reads as unknown and lets its gate through. The compiler asks
    /// for the map and builds a threshold over the contributing slots instead.
    ///
    /// NOT WHERE THE COUNTER HAS TOO MANY CONTRIBUTORS - see [`MOST_ONCES`].
    pub fn dropping_redundant_counters(
        mut self,
        graph: &LookAheadGraph,
        world: &dyn crate::world::ILookAheadWorld,
    ) -> Self {
        let symbols = graph.symbols();
        self.from_onces = counters_from_onces(graph)
            .into_iter()
            // WHAT THE WORLD CONTRIBUTES FROM OUTSIDE, which is what makes this sound rather
            // than group-local. `counters_from_onces` sees ONE group, and these are global
            // variables: another conversation can increment the same one, and the save's value
            // then counts a write no slot here reflects. That does not stop the substitution,
            // it shifts it. The value is
            //
            //     counter = outside + (how many of these once slots are set)
            //
            // because a site that fired before the save arrives with its slot seeded, so the
            // set counts the group's own history already. Take `outside` to be the save's value
            // less the sites it records as shown, and every guard rebases by it: `counter >= k`
            // is `at least k - outside of these are set`.
            .filter(|(_, contributors)| contributors.len() <= MOST_ONCES)
            .filter_map(|(slot, contributors)| {
                let name = symbols.name_of(slot)?;
                let declared = symbols.variable_ref(name)?;
                let held = crate::core::state::slot_value_of(&world.get_variable(declared));
                let fired = contributors
                    .iter()
                    .filter(|(node, _)| world.is_seen(*node))
                    .count() as i32;
                let onces = contributors.into_iter().map(|(_, once)| once).collect();
                Some((slot, (onces, held - fired)))
            })
            .collect();

        for slot in self.from_onces.keys() {
            if let Some(held) = self.slots.get_mut(*slot) {
                held.1 = 0;
            }
        }
        self.renumber();
        self
    }

    /// The same layout with the slots put in `ordering`'s sequence.
    ///
    /// THE LAST STEP, because it is the only one that cares where a slot ends up rather than
    /// how wide it is - and because an order chosen before the counters were dropped would be
    /// placing slots that are no longer there.
    ///
    /// An ordering cannot change an answer, only how much room reaching it takes. See
    /// [`crate::symbolic::var_order`].
    pub fn in_variable_order(mut self, graph: &LookAheadGraph, ordering: Ordering) -> Self {
        self.order = ordering.of(graph, self.slots.len());
        self.renumber();
        self
    }

    /// The order the slots take their variables in - see [`Self::in_variable_order`].
    pub fn variable_order(&self) -> &[usize] {
        &self.order
    }

    /// The `once` slots a dropped counter's value counts, and what the value holds on top of
    /// them - `None` for a slot the layout still carries.
    ///
    /// The second number is what writers outside this group contributed, so the counter is
    /// `offset + (how many of the slots are set)` and a guard rebases its constant by it.
    pub fn counter_onces(&self, slot: usize) -> Option<(&[usize], i32)> {
        self.from_onces
            .get(&slot)
            .map(|(onces, offset)| (onces.as_slice(), *offset))
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
        // IN THE CHOSEN ORDER, which is what makes this the variable order and not just a
        // numbering: `DataVars` adds the manager's variables in layout order, so a slot's
        // place in this sequence is its level in every diagram built over it.
        for slot in &self.order {
            let (base, bits) = &mut self.slots[*slot];
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

    /// Whether this slot holds a DELTA from the value the search started at.
    ///
    /// Everything that reads or writes such a slot has to know: the guard compiler so a
    /// comparison is rebased, and [`super::reachability::seed_of`] so the search starts at a
    /// distance of nothing rather than at the save's value. See [`Self::lay_out_counters`].
    pub fn is_delta(&self, slot: usize) -> bool {
        // A DROPPED SLOT IS NOT A DELTA SLOT, because it is not a slot - `keeping_only_read`
        // zeroes the width and leaves the index, and this set is keyed by index.
        self.slot(slot).is_some() && self.deltas.contains(&slot)
    }

    /// Whether an increment on this slot stops at the counter cap.
    ///
    /// Only a counter that can loop does. A distance never reaches its own top, and a counter
    /// held as its value that cannot loop stops at its own ceiling - see
    /// [`Self::lay_out_counters`].
    pub fn saturates_at_cap(&self, slot: usize) -> bool {
        !self.deltas.contains(&slot) && !self.unsaturated.contains(&slot)
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
    /// ## Why the ceiling is the start or the dearest price, plus every gain
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
    ///
    /// The dearest price stands in for the start where it is higher, because an option the
    /// purse cannot cover is answered from a purse that covers it - see
    /// `bridge::answer_starts` - and a register narrower than that price would clip the
    /// lifted purse back below it.
    pub fn money_ceiling(graph: &LookAheadGraph, starting: i32) -> Option<u32> {
        let read = graph
            .nodes()
            .any(|node| node.is_cost_option() || Self::guard_reads_money(&node.guard));
        if !read {
            return None;
        }

        let gained: i64 = graph
            .nodes()
            .flat_map(|node| node.all_actions())
            .filter(|action| action.kind() == DialogueActionKind::GainMoney)
            .map(|action| i64::from(action.value().max(0)))
            .sum();
        let dearest = graph
            .nodes()
            .filter(|node| node.is_cost_option())
            .map(|node| node.cost)
            .max()
            .unwrap_or(0);

        let ceiling = i64::from(starting.max(dearest).max(0)) + gained;
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
            if Self::reads_any_slot_contents(&node.guard) {
                // Which item a slot holds is the world's to say, so a question about whether
                // a slot is filled reads every item the group can take away.
                names.extend(
                    (0..symbols.count())
                        .filter_map(|slot| symbols.name_of(slot))
                        .filter(|name| name.starts_with(UNEQUIPPED_PREFIX))
                        .map(str::to_string),
                );
            }

            for slot in [node.flag_slot, node.failed_flag_slot] {
                if let Ok(slot) = usize::try_from(slot)
                    && let Some(name) = symbols.name_of(slot)
                {
                    names.insert(name.to_string());
                }
            }

            names.extend(Self::tested_by_actions(node, symbols));
        }

        names
    }

    /// The names a node's conditional writes TEST, which an action reads as surely as a guard.
    ///
    /// Kept apart from the guard reads because it needs keeping on a stronger rule: a slot a
    /// guard reads and nothing writes may be dropped, since the guard can then be answered from
    /// the world; a condition is decided inside an action, where there is no world to ask.
    pub fn tested_by_actions<'a>(
        node: &'a LookAheadNode,
        symbols: &'a StateSymbols,
    ) -> impl Iterator<Item = String> + 'a {
        node.actions
            .iter()
            .filter_map(|action| action.unless())
            .filter_map(|slot| symbols.name_of(slot))
            .map(str::to_string)
    }

    /// Whether a guard asks an equipment question that any lost item could change.
    fn reads_any_slot_contents(guard: &Guard) -> bool {
        guard.nodes().any(|node| match node.expression() {
            GuardExpression::Call(function, arguments) => {
                crate::core::equipment::reads_equipment(function)
                    && crate::core::equipment::only_loss_read_by(
                        function,
                        Self::literal_text(arguments).as_deref(),
                    )
                    .is_none()
            }
            _ => false,
        })
    }

    /// The names one guard reads, including the subjects of the queries answered from
    /// search state.
    fn read_by_guard(guard: &Guard, names: &mut HashSet<String>) {
        for node in guard.nodes() {
            match node.expression() {
                GuardExpression::Variable(name) => {
                    names.insert(name.to_string());
                }
                GuardExpression::Call(function, _)
                    if crate::core::damage::skill_read_by(function).is_some() =>
                {
                    let skill = crate::core::damage::skill_read_by(function).expect("just matched");
                    names.insert(format!("{DAMAGE_PREFIX}{skill}"));
                }
                GuardExpression::Call(function, _)
                    if crate::core::party::reads_kim_removal(function) =>
                {
                    names.insert(crate::core::party::KIM_REMOVED_SLOT.to_string());
                }
                // Every amount the question compares, none of which its text names.
                GuardExpression::Call(function, _)
                    if crate::core::reputation::range_of(function).is_some() =>
                {
                    names.extend(crate::core::reputation::variables_read_by(function));
                }
                GuardExpression::Call(function, arguments)
                    if crate::core::equipment::reads_equipment(function) =>
                {
                    let argument = Self::literal_text(arguments);
                    if let Some(item) =
                        crate::core::equipment::only_loss_read_by(function, argument.as_deref())
                    {
                        names.insert(format!("{UNEQUIPPED_PREFIX}{item}"));
                    }
                }
                GuardExpression::Call(function, arguments) => {
                    // The two queries `BoundContext::query` answers from a slot. Their
                    // subject is a literal string, and the slot it corresponds to carries a
                    // prefix.
                    let prefix = match function {
                        "CheckItem" => Some(ITEM_PREFIX),
                        "IsTHCPresent" => Some(THOUGHT_PREFIX),
                        // FlagSet(name) and FlagNotSet(name) are Variable[name] written
                        // another way, so both spend a slot on the same name.
                        _ if crate::world::flag_query(function).is_some() => Some(""),
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

    /// A call's single literal argument, as text.
    fn literal_text(arguments: crate::core::guard::Arguments<'_>) -> Option<String> {
        match arguments.only()?.expression() {
            GuardExpression::Literal(value) => Some(value.text().to_string()),
            _ => None,
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
    /// slot on the absolute encoding - see [`DataLayout::lay_out_counters`].
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

    /// The register holds the dearest price even when the purse starts below it.
    ///
    /// An option the purse cannot cover is answered from a purse lifted to its price, and the
    /// seed clamps to the ceiling - so a ceiling taken from the start alone would clip the
    /// lifted purse back under the price and leave the option as closed as before.
    #[test]
    fn the_money_ceiling_covers_the_dearest_price() {
        const PRICE: i32 = 50;
        const STARTING: i32 = 10;

        let node = LookAheadNode {
            cost: PRICE,
            ..LookAheadNode::new(DialogueNodeId::new(1, 0))
        };
        let graph = LookAheadGraph::new(vec![node], StateSymbols::new()).unwrap();

        assert_eq!(
            DataLayout::money_ceiling(&graph, STARTING),
            Some(PRICE as u32)
        );
        assert_eq!(
            DataLayout::money_ceiling(&graph, PRICE * 2),
            Some((PRICE * 2) as u32),
            "a purse above the price is its own ceiling"
        );
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
        assert!(layout.is_delta(slot));
        assert!(!layout.saturates_at_cap(slot));
    }

    /// A counter that cannot loop is held as a distance even where its raises add up past the
    /// cap, and so is wider than the cap needs: the cap would clip it, and the width it gets
    /// is what its own raises can cover.
    #[test]
    fn a_counter_that_cannot_loop_is_not_capped() {
        let mut symbols = StateSymbols::new();
        let slot = symbols.variable("donations");
        let graph = graph_with(
            vec![
                DialogueAction::increment(slot, 20, false, "SetVariableValue".to_string()),
                DialogueAction::increment(slot, 20, false, "SetVariableValue".to_string()),
            ],
            symbols,
        );

        // Forty needs six bits, one more than the cap's five.
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        assert_eq!(layout.slot(slot), Some((0, 6)));
        assert!(layout.is_delta(slot));
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
        assert!(layout.is_delta(slot));
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
        assert!(!layout.is_delta(slot));
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
        assert!(!layout.is_delta(slot));
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
        assert!(layout.is_delta(slot));
        assert_eq!(layout.slot(slot), Some((0, 1)));
    }

    /// A slot the guards have already squeezed below what its increments add up to keeps what
    /// it has, and stays on the absolute encoding.
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

        // The only threshold is 1, so the slot saturates at its ceiling of 3 and needs two
        // bits; the eighteen its increments add up to would ask for five.
        let layout = DataLayout::for_graph(&graph, 16, None, false);
        assert_eq!(layout.slot(slot), Some((0, 2)));
        assert!(!layout.is_delta(slot));
        assert!(
            !layout.saturates_at_cap(slot),
            "it cannot loop, so its ceiling stops it"
        );
    }

    /// A group whose counter is raised once from each of two entries, behind a gate reading it.
    ///
    /// The gate is what makes the variable DECLARED, which is what lets a world be asked for
    /// its value - only what a guard reads is.
    fn gated_counter(name: &str, sites: usize, raise: i32, once: bool) -> LookAheadGraph {
        let mut symbols = StateSymbols::new();
        let slot = symbols.variable(name);
        let gate = Guard::comparison(
            ">=".to_string(),
            Guard::variable(name.to_string()),
            Guard::literal(crate::core::guard_value::GuardValue::from_number(2.0)),
        );

        let nodes = (0..sites)
            .map(|site| LookAheadNode {
                guard: gate.clone(),
                actions: vec![DialogueAction::increment(
                    slot,
                    raise,
                    once,
                    "SetVariableValue".to_string(),
                )],
                ..LookAheadNode::new(DialogueNodeId::new(1, site as i32))
            })
            .collect();
        LookAheadGraph::new(nodes, symbols).unwrap()
    }

    /// The slot a name ended up on, which graph building is free to move.
    fn slot_named(graph: &LookAheadGraph, name: &str) -> usize {
        let symbols = graph.symbols();
        (0..symbols.count())
            .find(|slot| symbols.name_of(*slot) == Some(name))
            .expect("the name is on a slot")
    }

    /// Every raise `once` and by one, so the value is how many of the once slots are set.
    #[test]
    fn a_counter_only_once_actions_raise_is_a_count_of_them() {
        let graph = gated_counter("charges", 3, 1, true);
        let slot = slot_named(&graph, "charges");

        let found = counters_from_onces(&graph);
        let onces = found.get(&slot).expect("the counter is redundant");
        assert_eq!(onces.len(), 3, "one contributor per site");
    }

    /// A raise that is not `once` can fire again, so no set of slots counts it.
    #[test]
    fn a_counter_an_unguarded_raise_touches_is_not() {
        assert!(counters_from_onces(&gated_counter("charges", 3, 1, false)).is_empty());
    }

    /// A raise by more than one moves the value further than the count.
    #[test]
    fn a_counter_raised_by_more_than_one_is_not() {
        assert!(counters_from_onces(&gated_counter("charges", 3, 2, true)).is_empty());
    }

    /// The bits leave the layout, and the map the guards need is left behind.
    ///
    /// The layout to compare against is built by [`DataLayout::for_graph`], which lays every
    /// slot out and drops nothing - [`DataLayout::for_group`] drops these as it builds, so it
    /// cannot say what carrying one costs.
    #[test]
    fn dropping_a_redundant_counter_takes_its_bits_out() {
        let graph = gated_counter("charges", 3, 1, true);
        let slot = slot_named(&graph, "charges");
        let world = crate::world::test_world::TestWorld::new();

        let kept = DataLayout::for_graph(&graph, 16, None, false);
        let (_, width) = kept.slot(slot).expect("carried before");
        assert!(width > 0);

        let dropped = kept.clone().dropping_redundant_counters(&graph, &world);
        assert_eq!(dropped.slot(slot), None, "the slot is gone");
        assert_eq!(
            kept.total_vars() - dropped.total_vars(),
            width as u32,
            "and exactly its own width went with it"
        );
        assert_eq!(
            dropped.counter_onces(slot).map(|(onces, _)| onces.len()),
            Some(3)
        );
    }

    /// What the save holds beyond this group's own shown sites is the offset the guards rebase
    /// by: a conversation elsewhere raising the same variable does not stop the substitution,
    /// it lowers the threshold the once slots have to meet.
    #[test]
    fn a_counter_the_world_holds_above_its_shown_sites_keeps_the_difference() {
        use crate::core::guard_value::GuardValue;

        const HELD: i32 = 3;
        let graph = gated_counter("charges", 3, 1, true);
        let slot = slot_named(&graph, "charges");

        // One site shown, three raises in the value: two of them came from somewhere else.
        let world = crate::world::test_world::TestWorld::new()
            .set_variable("charges", GuardValue::from_number(HELD as f64))
            .set_seen(DialogueNodeId::new(1, 0), true);

        let dropped = DataLayout::for_group(&graph, &world, 16);
        assert_eq!(
            dropped.counter_onces(slot).map(|(_, offset)| offset),
            Some(HELD - 1)
        );
    }

    /// A save whose counter is exactly its shown sites rebases by nothing.
    #[test]
    fn a_counter_the_world_holds_at_its_shown_sites_rebases_by_nothing() {
        let graph = gated_counter("charges", 3, 1, true);
        let slot = slot_named(&graph, "charges");
        let world = crate::world::test_world::TestWorld::new();

        let dropped = DataLayout::for_group(&graph, &world, 16);
        assert_eq!(
            dropped.counter_onces(slot).map(|(_, offset)| offset),
            Some(0)
        );
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
