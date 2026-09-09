// SPDX-License-Identifier: MIT
//! Settled backward passes kept past the request that paid for them.
//!
//! ## What is kept, and why it is the sets rather than the verdict
//!
//! A backward pass answers "from which states does this target become reachable", and the
//! VERDICT a search reads off it - reachable or not - is that answer met with one seed. The
//! seed carries what the player has read and moves on every line, so a verdict is stale
//! almost at once. The SETS are not: they are a function of the graph, the compiled guards
//! and the target, and none of those knows what has been seen.
//!
//! So the sets are what outlives the request, and the verdict is recomputed against
//! whatever seed the next one brings - one diagram operation, in
//! [`Kept::reachable_from`]. Keeping the wrong one of the two is what made de-znov look
//! hard for as long as it did.
//!
//! ## Why it is worth having
//!
//! `measurements/cacheable_asks.rs`, 2026-09-09, over nine groups and 360 menus: of the
//! 3361 asks that reached the backward driver at all, every one settled without meeting,
//! and 2305 of them - 68.6 per cent - were repeats of an ask an earlier menu of the same
//! walk had already paid for. At 22.4 ms a pass that is 51.5 of the 75.2 seconds those
//! walks spent on fixed points.
//!
//! ## What must NOT be kept, which is the whole of the soundness argument
//!
//! [`Memo::remember`] takes a pass only where all of these hold, and each of them is a way
//! the sets could be a subset of the real answer rather than the answer:
//!
//! - THE PASS SETTLED. An unsettled pass ran out of budget holding what it had got to. It
//!   proves what it found and nothing about what it did not, which is the same rule
//!   `novelty_search` already applies to its within-request refused set.
//! - IT DID NOT MEET. `BackwardStats::met_at` means the pass stopped early against what one
//!   search holds where it begins - a proof for that seed, and a partial fixed point for any
//!   other. A memo is asked about seeds it has never seen.
//! - THE MANAGER DID NOT RUN OUT. `out_of_memory` is the third way to hold a subset.
//!
//! Each of these is a property of the pass rather than of how it was asked for, which is
//! what lets a caller offer every pass it runs and leave the choosing here.
//!
//! ## What it is keyed on, and what the caller owns
//!
//! ONE MANAGER. Every set here is a formula in the diagram manager that built it, and two
//! formulas over different managers cannot be combined at all - so a memo belongs to a
//! manager and dies with it. `crate::workspace` is where that is true of something that
//! outlives a request; `bridge::answer_within` builds a manager per request and passes
//! `None`.
//!
//! ONE WORLD, MINUS WHAT IS SEEN. The sets depend on the compiled guards, which fold in the
//! clock, the variables, the items, the tasks, the thoughts and the world queries. They do
//! not depend on `WorldSnapshot::seen`, which is consumed by the seed alone. [`MemoKey`] is
//! that distinction, and it is why this is worth having at all: between two menus of one
//! conversation, usually only the seed has changed.
//!
//! The key is checked by the OWNER, not here - [`Memo::keyed_on`] - because forgetting a
//! memo and respawning a workspace are different sizes of event and must not be confused. A
//! changed variable throws away remembered passes; it must not throw away the manager.

use std::cell::RefCell;
use std::collections::HashMap;

use oxidd::BooleanFunction;
use oxidd::bdd::BDDFunction;

use crate::core::types::DialogueNodeId;
use crate::symbolic::backward::{Backward, SettledPass};

/// What a set of remembered passes is valid FOR.
///
/// A HASH RATHER THAN THE WORLD, because the alternative is keeping a whole `WorldSnapshot`
/// per memo and comparing several hash maps and four sets on every request. What a collision
/// would cost is the one thing worth being careful about, and it is bounded: two worlds that
/// hashed alike would have to differ in something the guards read AND agree everywhere else,
/// and the result would be a marker computed against the wrong variable. At sixty-four bits
/// over a snapshot that changes a few fields a menu, that is not a risk anybody will meet;
/// at thirty-two it would have been.
pub type MemoKey = u64;

/// A key no world produces, for a memo that has not been given one yet.
///
/// A CONSTANT RATHER THAN AN `Option`, so that the first request re-keys the memo through
/// exactly the same line every later one does and nothing has to special-case being empty.
/// It is not reachable from [`key_of`] in practice for the same reason a collision is not.
pub const NO_WORLD: MemoKey = 0;

/// The key a world produces: everything the compiled guards read, and nothing else.
///
/// ## What is in it, and why each field is
///
/// `GuardCompiler::with_world` folds in the clock, the variables, the items, the tasks, the
/// thoughts and the world queries, and `world.check_passes` reads the two check sets. Money
/// reaches the guards through what a price can be paid out of. All of those change what a
/// backward pass computes, so all of them are here.
///
/// ## What is deliberately LEFT OUT, which is the whole point
///
/// `WorldSnapshot::seen`. It is consumed by the seed alone - `core::state` seeds a node's
/// seen-slot from `world.is_seen` - and the compiler never asks `is_seen` at all. A seen-slot
/// a guard reads is a tracked VARIABLE, and its starting value is the seed's business.
///
/// That is what makes a memo worth having. Between two menus of one conversation usually
/// only the seed has changed, so keying on the whole snapshot would empty the memo on every
/// line the player reads - which is the same correction de-2wtl needed for the workspace
/// itself.
///
/// ## And the positional fields are not here
///
/// `variable_values` and `query_values` are the same answers as `variables` and `queries`,
/// sent positionally to keep 14 KB off the wire. `WorldSnapshot::resolve` moves them onto
/// their names and clears them, so a caller must key a RESOLVED snapshot; hashing the
/// positional lists as well would make the two spellings of one world key differently.
pub fn key_of(world: &crate::bridge::WorldSnapshot) -> MemoKey {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    world.money.hash(&mut hasher);
    world.day_minutes.hash(&mut hasher);
    world.day_counter.hash(&mut hasher);
    world.clock_locked.hash(&mut hasher);
    // ORDERED, because a hash map's iteration order is not, and a key that depended on it
    // would forget the memo at random. Sorting a few hundred names is nothing beside the
    // fixed points this exists to avoid.
    hash_named(&world.variables, &mut hasher);
    hash_named(&world.queries, &mut hasher);
    hash_set(&world.items, &mut hasher);
    hash_set(&world.tasks, &mut hasher);
    hash_set(&world.thoughts, &mut hasher);
    hash_nodes(&world.checks_pass, &mut hasher);
    hash_nodes(&world.checks_fail, &mut hasher);
    hasher.finish()
}

/// A map from names to wire values, in name order.
fn hash_named(
    named: &HashMap<String, crate::bridge::WireValue>,
    hasher: &mut impl std::hash::Hasher,
) {
    let mut names: Vec<&String> = named.keys().collect();
    names.sort_unstable();
    for name in names {
        std::hash::Hash::hash(name, hasher);
        hash_value(&named[name], hasher);
    }
}

/// One answer from the world.
///
/// BY HAND, because a wire number is an `f64` and floats are not hashable - two values that
/// compare equal can have different bits, and one value is equal to nothing including
/// itself. Hashing the BITS makes minus zero a different key from zero and every NaN a
/// consistent one, which is the safe direction for a memo: a spurious difference forgets
/// passes that were still good, where a spurious sameness would answer against the wrong
/// world.
fn hash_value(value: &crate::bridge::WireValue, hasher: &mut impl std::hash::Hasher) {
    use crate::bridge::WireValue;
    use std::hash::Hash;

    // The discriminant, so that a text "1" and a number 1 are different answers.
    std::mem::discriminant(value).hash(hasher);
    match value {
        WireValue::Bool { value } => value.hash(hasher),
        WireValue::Number { value } => value.to_bits().hash(hasher),
        WireValue::Text { value } => value.hash(hasher),
        WireValue::Unknown => {}
    }
}

/// A set of names, in order.
fn hash_set(set: &std::collections::HashSet<String>, hasher: &mut impl std::hash::Hasher) {
    let mut names: Vec<&String> = set.iter().collect();
    names.sort_unstable();
    for name in names {
        std::hash::Hash::hash(name, hasher);
    }
}

/// A set of entries, in order.
fn hash_nodes(set: &crate::bridge::NodeSet, hasher: &mut impl std::hash::Hasher) {
    let mut entries: Vec<(i32, i32)> = set
        .iter()
        .map(|node| (node.conversation, node.entry))
        .collect();
    entries.sort_unstable();
    for entry in entries {
        std::hash::Hash::hash(&entry, hasher);
    }
}

/// One settled backward pass, kept.
pub struct Kept {
    /// Per entry, the states from which entering it goes on to reach the target. The same
    /// sets [`Backward`] holds, and read the same way.
    sets: HashMap<DialogueNodeId, BDDFunction>,
    /// Diagram nodes across those sets, which is what this entry costs the manager.
    ///
    /// Counted with sharing inside the pass and not between passes, exactly as
    /// `BackwardStats::diagram_nodes` is - so it is what an entry costs in isolation rather
    /// than what dropping it would give back. That is the honest direction for an eviction
    /// rule: it can only over-state what is reclaimed.
    nodes: usize,
    /// What the pass took to compute, which is what a hit saves.
    took: std::time::Duration,
}

impl Kept {
    /// Diagram nodes across this pass's sets.
    pub fn nodes(&self) -> usize {
        self.nodes
    }

    /// What computing it cost, which is what reading it back saves.
    pub fn took(&self) -> std::time::Duration {
        self.took
    }
}

impl SettledPass for Kept {
    fn reachable_from(&self, node: DialogueNodeId, states: &BDDFunction) -> bool {
        match self.sets.get(&node) {
            // An `Err` is the manager out of room, and answering "not reachable" on it
            // would be the one wrong direction - the same reading `Backward` gives it.
            Some(set) => set
                .and(states)
                .map(|both| both.satisfiable())
                .unwrap_or(true),
            None => false,
        }
    }
}

/// What one memo did for the requests it served.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MemoStats {
    /// Passes asked for and found.
    pub hits: usize,
    /// Passes asked for and not held.
    pub misses: usize,
    /// Passes offered and taken.
    pub kept: usize,
    /// Passes offered and refused, split by which condition refused them.
    ///
    /// SEPARATELY, because they say different things about a run and a single count says
    /// none of them. A memo that keeps nothing because every pass MET is looking at a world
    /// with plenty left unread, and is working; one that keeps nothing because nothing
    /// SETTLED is being asked questions too hard for its budget, which is the one of these
    /// that is a reason to change something here.
    pub unsettled: usize,
    pub met: usize,
    pub out_of_room: usize,
    /// Entries dropped to stay under the cap.
    pub evicted: usize,
}

/// Settled backward passes over one graph, in one manager, for one world minus its seen.
///
/// INTERIOR MUTABILITY, so that a memo can be handed down a call chain that is already
/// carrying a `&mut GuardCompiler`. The alternative is a second mutable borrow threaded
/// through five signatures beside the first, which is the plumbing de-a88z declined to
/// write and no more palatable now.
pub struct Memo {
    key: MemoKey,
    passes: RefCell<HashMap<DialogueNodeId, Kept>>,
    /// Diagram nodes across every kept pass, summed, against which [`Self::cap`] is read.
    held: RefCell<usize>,
    /// The most diagram nodes this may hold before it starts evicting.
    cap: usize,
    stats: RefCell<MemoStats>,
}

impl Memo {
    /// An empty memo for one key, allowed `cap` diagram nodes.
    pub fn new(key: MemoKey, cap: usize) -> Self {
        Self {
            key,
            passes: RefCell::new(HashMap::new()),
            held: RefCell::new(0),
            cap,
            stats: RefCell::new(MemoStats::default()),
        }
    }

    /// Whether what is held was computed for this world.
    ///
    /// ASKED BY THE OWNER BEFORE EVERY REQUEST. A memo cannot check its own key, because
    /// the key is a fact about the world a request carries and this holds no world.
    pub fn keyed_on(&self, key: MemoKey) -> bool {
        self.key == key
    }

    /// Drops everything and starts again on a new key.
    ///
    /// The whole memo, because nothing here knows which passes a given world change could
    /// have touched. A cone-of-influence rule that kept the untouched ones is the obvious
    /// refinement and is deliberately not attempted first: it would have to be right about
    /// every guard, and being wrong about one is a marker lost rather than a slower search.
    pub fn re_key(&mut self, key: MemoKey) {
        self.key = key;
        self.passes.borrow_mut().clear();
        *self.held.borrow_mut() = 0;
    }

    /// The pass for `target`, if one is held.
    pub fn recall(&self, target: DialogueNodeId) -> Option<std::cell::Ref<'_, Kept>> {
        let passes = self.passes.borrow();
        if !passes.contains_key(&target) {
            self.stats.borrow_mut().misses += 1;
            return None;
        }
        self.stats.borrow_mut().hits += 1;
        Some(std::cell::Ref::map(passes, |held| &held[&target]))
    }

    /// Offers a finished pass, which is kept only if every condition in the module note
    /// holds.
    ///
    pub fn remember(&self, target: DialogueNodeId, backward: &Backward<'_>) -> bool {
        let stats = backward.stats();
        if !stats.reached_fixed_point || stats.met_at.is_some() || stats.out_of_memory {
            let mut counted = self.stats.borrow_mut();
            // ONE REASON EACH, and `reached_fixed_point` is asked LAST rather than first.
            // It is false for a pass that met and for one that ran out of nodes as well as
            // for one that ran out of clock - `Backward::reaching_knowing` sets it from all
            // three - so testing it first would file every refusal under "unsettled" and the
            // split would say nothing.
            if stats.met_at.is_some() {
                counted.met += 1;
            } else if stats.out_of_memory {
                counted.out_of_room += 1;
            } else {
                counted.unsettled += 1;
            }
            return false;
        }

        let sets: HashMap<DialogueNodeId, BDDFunction> = backward
            .entries()
            .filter_map(|entry| backward.states_at(entry).map(|set| (entry, set.clone())))
            .collect();
        let kept = Kept {
            sets,
            nodes: stats.diagram_nodes,
            took: stats.elapsed,
        };

        self.make_room_for(kept.nodes);
        let mut passes = self.passes.borrow_mut();
        if let Some(replaced) = passes.insert(target, kept) {
            *self.held.borrow_mut() -= replaced.nodes;
        }
        *self.held.borrow_mut() += stats.diagram_nodes;
        self.stats.borrow_mut().kept += 1;
        true
    }

    /// Diagram nodes across every pass held.
    pub fn held(&self) -> usize {
        *self.held.borrow()
    }

    /// How many passes are held.
    pub fn len(&self) -> usize {
        self.passes.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn stats(&self) -> MemoStats {
        *self.stats.borrow()
    }

    /// Evicts until `wanted` more nodes fit under the cap.
    ///
    /// BY WHAT AN ENTRY IS WORTH PER NODE, which is the ratio the two recorded numbers make:
    /// a pass that took ten milliseconds and holds a hundred nodes earns its place over one
    /// that took one and holds ten thousand. Insertion order would have thrown away the
    /// cheapest thing to rebuild as readily as the dearest.
    ///
    /// A SINGLE PASS LARGER THAN THE WHOLE CAP still goes in, after emptying what is there.
    /// Refusing it would leave the memo permanently unable to hold the one target that costs
    /// most to answer, which is the opposite of what a cap is for; the next insert evicts it
    /// again, and the manager's own allowance is what actually bounds this.
    fn make_room_for(&self, wanted: usize) {
        if *self.held.borrow() + wanted <= self.cap {
            return;
        }

        let mut passes = self.passes.borrow_mut();
        let mut order: Vec<(DialogueNodeId, f64)> = passes
            .iter()
            .map(|(target, kept)| (*target, worth(kept)))
            .collect();
        // Cheapest first, and the identifier breaks the tie so eviction is not decided by
        // whatever order the hash map happened to yield.
        order.sort_by(|a, b| {
            a.1.total_cmp(&b.1).then_with(|| {
                (a.0.conversation_id, a.0.entry_id).cmp(&(b.0.conversation_id, b.0.entry_id))
            })
        });

        let mut held = self.held.borrow_mut();
        for (target, _) in order {
            if *held + wanted <= self.cap {
                break;
            }
            if let Some(dropped) = passes.remove(&target) {
                *held -= dropped.nodes;
                self.stats.borrow_mut().evicted += 1;
            }
        }
    }
}

/// What one kept pass is worth per diagram node it costs.
fn worth(kept: &Kept) -> f64 {
    if kept.nodes == 0 {
        // Nothing at all is held for it, so it is free to keep and there is no reason to be
        // the one evicted.
        return f64::INFINITY;
    }
    kept.took.as_secs_f64() / kept.nodes as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(entry: i32) -> DialogueNodeId {
        DialogueNodeId::new(1, entry)
    }

    fn kept_of(nodes: usize, millis: u64) -> Kept {
        Kept {
            sets: HashMap::new(),
            nodes,
            took: std::time::Duration::from_millis(millis),
        }
    }

    /// Puts a pass in without going through `remember`, which needs a real `Backward`.
    fn put(memo: &Memo, at: DialogueNodeId, kept: Kept) {
        memo.make_room_for(kept.nodes);
        *memo.held.borrow_mut() += kept.nodes;
        memo.passes.borrow_mut().insert(at, kept);
    }

    #[test]
    fn a_memo_holds_what_it_is_given_until_the_cap() {
        let memo = Memo::new(7, 100);
        put(&memo, target(1), kept_of(40, 10));
        put(&memo, target(2), kept_of(40, 10));
        assert_eq!(memo.len(), 2);
        assert_eq!(memo.held(), 80);
    }

    #[test]
    fn the_cheapest_per_node_is_evicted_first() {
        let memo = Memo::new(7, 100);
        // Dear per node: a millisecond saved for every node held.
        put(&memo, target(1), kept_of(10, 10));
        // Cheap per node: the same millisecond for eight times the room.
        put(&memo, target(2), kept_of(80, 10));
        assert_eq!(memo.len(), 2);

        // No room for this without dropping something, and the cheap one goes.
        put(&memo, target(3), kept_of(20, 5));
        assert!(
            memo.recall(target(2)).is_none(),
            "the cheap-per-node pass survived"
        );
        assert!(
            memo.recall(target(1)).is_some(),
            "the dear-per-node pass was evicted"
        );
        assert!(
            memo.recall(target(3)).is_some(),
            "the new pass did not go in"
        );
        assert!(memo.held() <= 100, "the cap was exceeded: {}", memo.held());
    }

    #[test]
    fn a_pass_larger_than_the_cap_still_goes_in() {
        let memo = Memo::new(7, 100);
        put(&memo, target(1), kept_of(40, 10));
        put(&memo, target(2), kept_of(500, 10));
        assert!(
            memo.recall(target(2)).is_some(),
            "the outsized pass was refused"
        );
        assert!(
            memo.recall(target(1)).is_none(),
            "the room for it was not made"
        );
    }

    #[test]
    fn re_keying_forgets_everything() {
        let mut memo = Memo::new(7, 100);
        put(&memo, target(1), kept_of(40, 10));
        assert!(memo.keyed_on(7));

        memo.re_key(8);
        assert!(!memo.keyed_on(7));
        assert!(memo.is_empty());
        assert_eq!(memo.held(), 0);
    }

    #[test]
    fn hits_and_misses_are_counted() {
        let memo = Memo::new(7, 100);
        put(&memo, target(1), kept_of(40, 10));
        assert!(memo.recall(target(1)).is_some());
        assert!(memo.recall(target(2)).is_none());

        let stats = memo.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    /// A world differing only in what has been SEEN keys the same, which is the whole
    /// reason this is worth having.
    #[test]
    fn what_has_been_seen_is_not_part_of_the_key() {
        use crate::bridge::{NodeRef, WorldSnapshot};

        let mut early = WorldSnapshot {
            money: 40,
            day_minutes: 720,
            ..Default::default()
        };
        early.seen.insert(NodeRef {
            conversation: 1,
            entry: 2,
        });
        let mut later = early.clone();
        later.seen.insert(NodeRef {
            conversation: 1,
            entry: 3,
        });

        assert_eq!(
            key_of(&early),
            key_of(&later),
            "reading a line emptied the memo"
        );
    }

    /// And anything a guard reads does not.
    #[test]
    fn what_a_guard_reads_is_part_of_the_key() {
        use crate::bridge::{WireValue, WorldSnapshot};

        let before = WorldSnapshot {
            money: 40,
            day_minutes: 720,
            ..Default::default()
        };

        let mut spent = before.clone();
        spent.money = 10;
        assert_ne!(key_of(&before), key_of(&spent), "money is not in the key");

        let mut later = before.clone();
        later.day_minutes = 1200;
        assert_ne!(
            key_of(&before),
            key_of(&later),
            "the clock is not in the key"
        );

        let mut told = before.clone();
        told.variables
            .insert("x".into(), WireValue::Bool { value: true });
        assert_ne!(
            key_of(&before),
            key_of(&told),
            "a variable is not in the key"
        );

        let mut carrying = before.clone();
        carrying.items.insert("a-key".into());
        assert_ne!(
            key_of(&before),
            key_of(&carrying),
            "an item is not in the key"
        );
    }

    /// The two spellings of one world key alike, once the positional form is resolved.
    #[test]
    fn the_key_does_not_depend_on_map_order() {
        use crate::bridge::{WireValue, WorldSnapshot};

        let mut one = WorldSnapshot::default();
        let mut other = WorldSnapshot::default();
        for name in ["a", "b", "c", "d", "e", "f", "g", "h"] {
            one.variables
                .insert(name.into(), WireValue::Text { value: name.into() });
        }
        for name in ["h", "g", "f", "e", "d", "c", "b", "a"] {
            other
                .variables
                .insert(name.into(), WireValue::Text { value: name.into() });
        }

        assert_eq!(
            key_of(&one),
            key_of(&other),
            "the key followed the insertion order"
        );
    }
}
