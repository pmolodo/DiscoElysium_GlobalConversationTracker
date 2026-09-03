// SPDX-License-Identifier: MIT
//! How a crawl's DATA state maps onto decision-diagram variables.
//!
//! ## Explicit control, symbolic data
//!
//! The entry a crawl is sitting on stays an ordinary value - there are a few thousand of
//! them and they are enumerated anyway - while everything the crawl carries WITH it
//! becomes decision-diagram variables. So a reachability pass keeps one set of data
//! states per entry, rather than one set of (entry, data) pairs, and the entry never
//! costs a variable.
//!
//! That is the opposite of what [`super::StateEncoding`] does, and deliberately. That one
//! encodes the entry too, because it is measuring how a set of whole crawl states
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
use std::collections::HashSet;

use crate::core::guard::GuardExpression;
use crate::core::state::{
    StateSymbols, ITEM_PREFIX, ONCE_PREFIX, SEEN_PREFIX, TASK_PREFIX, THOUGHT_PREFIX,
};
use crate::graph::graph::LookAheadGraph;

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
    /// Lays out variables for the data a crawl over `graph` can carry.
    ///
    /// `counter_cap` is the ceiling an incremented slot saturates at, which is what makes
    /// it finite and therefore what bounds its width.
    ///
    /// `money_max` and `track_clock` say whether those two are worth variables at all. A
    /// crawl over a graph with no cost options and no `PassTime` never moves either, and
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
                    DialogueActionKind::Assign => bits_for(action.value().max(0) as u32),
                    // Money, clock and unmodelled actions do not write a slot.
                    _ => continue,
                };
                widths[slot] = widths[slot].max(needed);
            }
        }

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
    /// A conversation group is closed under links, so a crawl over it only ever evaluates
    /// guards belonging to it. A slot that no guard in the group reads therefore cannot
    /// change which entries are reachable, whatever any action writes to it - it is
    /// write-only for the length of the crawl. Dropping it removes a variable and changes
    /// no answer at all, which is a different and better thing than
    /// [`Self::without_visit_flags`], where the saving is paid for in precision.
    ///
    /// ## How much it removes
    ///
    /// A great deal, because the content is full of bookkeeping the dialogue never reads
    /// back. Conversation 631's group writes 45 `XP.` accomplishment latches and reads
    /// two of them; the other 43 are variables the rest of the game cares about and this
    /// crawl cannot.
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
            // The engine's own bookkeeping, which no guard mentions and every crawl needs.
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
    /// `LookAheadEngine::enter_rolled` reads them to decide whether a check can be
    /// attempted, so a layout without them would let a check be retried for ever.
    pub fn read_by(graph: &LookAheadGraph) -> HashSet<String> {
        let mut names = HashSet::new();
        let symbols = graph.symbols();

        for node in graph.nodes() {
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
    /// crawl state.
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
