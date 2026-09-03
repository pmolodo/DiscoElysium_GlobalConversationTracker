// SPDX-License-Identifier: MIT
//! Reachability asked backwards: from which states does ONE entry become reachable?
//!
//! [`super::reachability`] computes what a start can reach and reads the answer off it.
//! The question the look-ahead actually asks is smaller - `LookAheadResult::best` is a
//! maximum over the novelty of entries a start can reach, and the plugin displays that
//! and nothing else - so the reachable set is a means, and an expensive one.
//!
//! Run the same fixed point in reverse and the question becomes: for each entry, from
//! which data states does the target become reachable onward? Compute that once and every
//! option in the group is answered by testing its seed against the result.
//!
//! ## Why backwards is cheap here in particular
//!
//! Two properties of this database, neither of which holds of transition systems in
//! general.
//!
//! IT PRUNES ITSELF. A variable enters a backward formula only if a guard on some path to
//! the target mentions it. Better than that, a WRITE ERASES its slot:
//! [`ActionImage::pre_assign`] selects and then quantifies away, so a slot assigned on the
//! way to the target and not read again leaves nothing behind. Forward, the same
//! assignment constrains that slot in every set downstream. So the cone-of-influence
//! reduction that a forward search needs a separate analysis for is just what the
//! pre-image does.
//!
//! THE PRE-IMAGE IS A COFACTOR. Every action assigns or increments a CONSTANT rather than
//! a function of another slot, so going backwards needs no relation over primed and
//! unprimed variables, exactly as going forwards needs none. This is the twin of the
//! property the forward module is built on.
//!
//! ## Not everything flips: transformations do, conditions do not
//!
//! The rule to hold on to when reading `pre_enter` against `Reachability::enter`, because
//! getting it wrong is a silent bug rather than a compile error.
//!
//! A TRANSFORMATION inverts and moves. `slot := k` forgets the slot and then asserts the
//! value; its pre-image asserts the value and then forgets the slot, and it is undone in
//! the mirror position - last written is first undone.
//!
//! A CONDITION does not invert and does not move. The guard, "this entry has not been
//! seen", "this check has not already passed", "this one-time effect has not fired" are
//! all predicates about the state at one point in the sequence, and they are still
//! predicates about the state at that same point when read backwards. What changes is
//! only WHEN the backward pass can conjoin them: a condition on the incoming state can
//! only be expressed once everything after it has been undone, so it comes last.
//!
//! The order is load-bearing where a condition and a write touch the same slot. `Fake`
//! tests that its seen flag is clear and then sets it. Backwards, the assignment must be
//! undone FIRST - which selects the states where the flag is set and then frees the slot -
//! and only then can "the flag was clear" be conjoined. Conjoining it earlier would put a
//! constraint on a slot that the very next step erases, and the condition would silently
//! vanish. `unseen` is called after `pre_charge` for exactly that reason, and the same
//! goes for a rolled check's pass and fail flags.
//!
//! One assumption holds this together and is worth stating: nothing in a node's action
//! list writes that node's once slot. It is written by `charge`, outside and before the
//! actions, so "has this one-time effect fired" has the same answer at every position in
//! the sequence and can be read at any of them.
//!
//! ## What it is allowed to get wrong
//!
//! The same one-directional approximation as everything else symbolic here: guards come
//! from `may_be_true`, so a state may be included that the real crawl could not reach the
//! target from. What must never happen is the reverse - a state excluded that can. A
//! backward NO against a forward YES is a bug in the pre-image, not an approximation, and
//! `tests/backward_oracle.rs` exists to catch it.
//!
//! ## What it does not do yet
//!
//! Ordering candidates and stopping at the first witness - de-sze.14.3 - and the money
//! approximation is inherited from the forward pass unchanged: a cost option is treated as
//! affordable, because money is not in the layout.

use std::collections::{HashMap, HashSet, VecDeque};

use oxidd::bdd::BDDFunction;
use oxidd::{BooleanFunction, Function};

use crate::core::types::{DialogueCheckKind, DialogueNodeId, Ternary};
use crate::graph::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::symbolic::action_image::ActionImage;
use crate::symbolic::guard_formula::GuardCompiler;
use crate::symbolic::vars::DataVars;
use crate::world::world::ILookAheadWorld;

/// What one backward fixed point cost.
#[derive(Debug, Clone, Default)]
pub struct BackwardStats {
    /// Whether the sets stopped moving, rather than the budget running out.
    pub reached_fixed_point: bool,
    /// How many times an entry was taken off the queue.
    pub steps: usize,
    /// How many times an entry's set actually grew.
    pub widenings: usize,
    /// Entries from which the target can be reached in at least one state.
    pub entries_reaching: usize,
    /// Diagram nodes across every entry's set, counted with sharing inside each set only.
    pub diagram_nodes: usize,
    /// The largest single entry's set.
    pub largest_set: usize,
    /// Whether the diagram manager ran out of nodes.
    pub out_of_memory: bool,
    /// Actions skipped because the layout does not carry what they touch.
    pub actions_ignored: usize,
    pub elapsed: std::time::Duration,
}

/// When to give up on a backward pass.
pub struct Budget {
    pub steps: usize,
    pub time: std::time::Duration,
}

impl Default for Budget {
    fn default() -> Self {
        Self { steps: 2_000_000, time: std::time::Duration::from_secs(120) }
    }
}

/// For each entry, the data states from which the target is reachable.
pub struct Backward<'a> {
    vars: &'a DataVars<'a>,
    /// Per entry: the states at that entry, AFTER entering it, from which the target can
    /// still be reached.
    sets: HashMap<DialogueNodeId, BDDFunction>,
    stats: BackwardStats,
}

impl<'a> Backward<'a> {
    /// Runs the fixed point backwards from `target`.
    pub fn reaching(
        graph: &LookAheadGraph,
        target: DialogueNodeId,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
    ) -> Self {
        Self::reaching_within(graph, target, compiler, world, counter_cap, &Budget::default())
    }

    /// The same, under a budget.
    pub fn reaching_within(
        graph: &LookAheadGraph,
        target: DialogueNodeId,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
        budget: &Budget,
    ) -> Self {
        let vars = compiler.vars();
        let mut image = ActionImage::new(vars, counter_cap);
        let mut this = Self {
            vars,
            sets: HashMap::new(),
            stats: BackwardStats::default(),
        };

        // Only the entries that can reach the target through links at all are worth
        // visiting. Guards can refuse an edge but never create one, so this is an upper
        // bound on what the fixed point will touch - and on the hub-shaped groups it is
        // most of them, which is a measured fact rather than a disappointment: see
        // de-sze.14. Where it IS small, it saves visiting the rest entirely.
        let parents = Self::parents_of(graph);
        let relevant = Self::can_reach(&parents, target);

        let began = std::time::Instant::now();
        let mut queue: VecDeque<DialogueNodeId> = VecDeque::new();
        let mut ran_out = false;

        // The target's own set: enter it in any state at all and the target has been
        // reached, so there is nothing to ask about what happens afterwards. Everything
        // else is derived from this one set by walking links backwards.
        let mut frontier: HashMap<DialogueNodeId, BDDFunction> = HashMap::new();
        if let Some(node) = graph.get(target) {
            let arriving = this.pre_enter(node, &vars.top(), compiler, world, &mut image);
            if let Some(fresh) = this.widen(target, &arriving) {
                frontier.insert(target, fresh);
                queue.push_back(target);
            }
        }

        // What travels is the DELTA, the way the forward pass propagates only the delta
        // and for the same reason: `pre_enter` distributes over union - the guard is a
        // conjunction, every pre-image is a select-and-forget, and each branch unions its
        // cases - so the pre-image of the whole set is the pre-image of what has already
        // been sent plus the pre-image of what is new. Re-sending the whole set at every
        // visit recomputes the first half each time, which on a group of four thousand
        // entries is the difference between finishing and not.
        'search: while let Some(id) = queue.pop_front() {
            // Take what is pending and leave nothing behind: an entry can be queued more
            // than once, and the second visit has nothing left to do.
            let delta = match frontier.insert(id, vars.bottom()) {
                Some(pending) if pending.satisfiable() => pending,
                _ => continue,
            };

            this.stats.steps += 1;
            if this.stats.steps >= budget.steps || began.elapsed() >= budget.time {
                ran_out = true;
                break;
            }

            for &parent in parents.get(&id).into_iter().flatten() {
                if !relevant.contains(&parent) {
                    continue;
                }

                let Some(node) = graph.get(parent) else { continue };
                // Entering the parent has to leave the crawl somewhere that can go on to
                // reach the target through this child.
                let before = this.pre_enter(node, &delta, compiler, world, &mut image);
                if image.out_of_memory() {
                    this.stats.out_of_memory = true;
                    break 'search;
                }
                if !before.satisfiable() {
                    continue;
                }

                let Some(fresh) = this.widen(parent, &before) else {
                    if this.stats.out_of_memory {
                        break 'search;
                    }
                    continue;
                };

                let pending = frontier
                    .get(&parent)
                    .cloned()
                    .unwrap_or_else(|| vars.bottom());
                let Ok(waiting) = pending.or(&fresh) else {
                    this.stats.out_of_memory = true;
                    break 'search;
                };
                frontier.insert(parent, waiting);
                queue.push_back(parent);
            }
        }

        this.stats.actions_ignored = image.ignored();
        this.stats.reached_fixed_point = !ran_out && !this.stats.out_of_memory;
        this.stats.elapsed = began.elapsed();
        this.finish();
        this
    }

    /// Adds `arriving` to what is known at `node`, and says what part of it was new.
    ///
    /// `None` means nothing changed - which is also what is reported when the manager
    /// runs out of room, with [`BackwardStats::out_of_memory`] set to say so, because
    /// once that happens every set since is the pre-image of nothing in particular and
    /// the caller must stop rather than carry on with a smaller answer.
    fn widen(&mut self, node: DialogueNodeId, arriving: &BDDFunction) -> Option<BDDFunction> {
        if !arriving.satisfiable() {
            return None;
        }

        let known = self.sets.get(&node).cloned().unwrap_or_else(|| self.vars.bottom());
        // Diagrams are canonical for a fixed variable order, so an empty difference is
        // exactly "nothing changed" - no membership test and no approximation in it.
        let (Ok(complement), Ok(())) = (known.not(), Ok::<(), ()>(())) else {
            self.stats.out_of_memory = true;
            return None;
        };
        let Ok(fresh) = arriving.and(&complement) else {
            self.stats.out_of_memory = true;
            return None;
        };
        if !fresh.satisfiable() {
            return None;
        }

        let Ok(widened) = known.or(arriving) else {
            self.stats.out_of_memory = true;
            return None;
        };
        self.sets.insert(node, widened);
        self.stats.widenings += 1;
        Some(fresh)
    }

    /// The states from which entering `node` lands in `onward`.
    ///
    /// The mirror of `Reachability::enter`, case for case. The requirement is not
    /// elegance but agreement: a backward pass that reads a node differently from the
    /// forward one is answering a different question, and the oracle test would catch it
    /// as an unreachable entry the crawl walks to.
    fn pre_enter(
        &mut self,
        node: &LookAheadNode,
        onward: &BDDFunction,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        let inner = match node.kind {
            // A hidden test is entered and goes nowhere, so nothing reaches anything
            // through one.
            DialogueCheckKind::Test => return self.vars.bottom(),

            DialogueCheckKind::Fake => {
                let charged = self.pre_charge(node, onward, image);
                self.unseen(node, &charged)
            }

            DialogueCheckKind::KimSwitch => {
                let charged = self.pre_charge(node, onward, image);
                if node.boolean_only {
                    charged
                } else {
                    self.unseen(node, &charged)
                }
            }

            DialogueCheckKind::Red | DialogueCheckKind::White => {
                self.pre_rolled(node, onward, image)
            }

            DialogueCheckKind::Passive => {
                let passes = world.check_passes(node.id);
                let mut result = self.vars.bottom();
                if passes != Ternary::False {
                    result = self.pre_charge(node, onward, image);
                }
                // The failing branch passes the state through UNCHARGED, so backwards it
                // passes `onward` through untouched - the one branch that does not go
                // through `charge`, forward or back.
                if passes != Ternary::True {
                    result = result.or(onward).expect("or");
                }
                result
            }

            _ => self.pre_charge(node, onward, image),
        };

        if !inner.satisfiable() {
            return self.vars.bottom();
        }

        // The guard is tested on the way IN, so it is the last thing undone. Affordability
        // is where the forward pass gives up on money and lets everything through; doing
        // anything else here would disagree with it.
        let (may_be_true, _) = self.guard_of(node, compiler);
        inner.and(&may_be_true).expect("and")
    }

    /// A rolled check, backwards: the states that reach `onward` down either branch.
    fn pre_rolled(
        &mut self,
        node: &LookAheadNode,
        onward: &BDDFunction,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        // Success raised the pass flag, so undoing it selects the states where it is
        // raised and then forgets it.
        let mut landed = match node.flag_slot {
            slot if slot >= 0 => image.pre_assign(onward, slot as usize, 1),
            _ => onward.clone(),
        };

        // Failure: red recorded it in its own flag, white left the state alone, and
        // anything else has no failing branch at all.
        let failing = if node.kind == DialogueCheckKind::Red && node.failed_flag_slot >= 0 {
            image.pre_assign(onward, node.failed_flag_slot as usize, 1)
        } else if node.kind == DialogueCheckKind::White {
            onward.clone()
        } else {
            self.vars.bottom()
        };
        landed = landed.or(&failing).expect("or");

        let entered = self.pre_charge(node, &landed, image);

        // A check already passed is closed, and a red check already failed is closed too.
        let mut open = entered;
        if let Some(passed) = self.flag(node.flag_slot) {
            open = open.and(&passed.not().expect("not")).expect("and");
        }
        if node.kind == DialogueCheckKind::Red {
            if let Some(failed) = self.flag(node.failed_flag_slot) {
                open = open.and(&failed.not().expect("not")).expect("and");
            }
        }

        open
    }

    /// Paying, marking seen and acting, undone - in the reverse of the order they happen.
    fn pre_charge(
        &mut self,
        node: &LookAheadNode,
        onward: &BDDFunction,
        image: &mut ActionImage<'a>,
    ) -> BDDFunction {
        if !onward.satisfiable() {
            return self.vars.bottom();
        }

        // The once slot says whether a one-time effect has already fired, and it is read
        // at the moment the actions apply - which is AFTER the two assignments below, so
        // it is undone first and read against the same state the forward pass reads it
        // against.
        let already = match self.flag(node.once_slot) {
            Some(flag) => flag,
            None => self.vars.bottom(),
        };

        // The mirror of `Reachability::charge` raising the once flag. Forward, an entry
        // with a one-time action splits: the states that had not fired apply it and come
        // out with the flag up, and the states that had skip it. So backwards there are
        // two ways to have arrived, and they are told apart by the flag - which is why
        // undoing the assignment on the fresh branch comes BEFORE conjoining "it was
        // clear", exactly as it does for `Fake` and its seen marker.
        let fires_once = node.actions.iter().any(|action| action.is_once());
        let mut current = if fires_once && node.once_slot >= 0 {
            let raised = image.pre_assign(onward, node.once_slot as usize, 1);
            let fresh = image.pre_apply(&raised, &node.actions, &self.vars.bottom());
            let fresh = fresh.and(&already.not().expect("not")).expect("and");

            let spent = image.pre_apply(onward, &node.actions, &self.vars.top());
            let spent = spent.and(&already).expect("and");

            fresh.or(&spent).expect("or")
        } else {
            image.pre_apply(onward, &node.actions, &already)
        };

        if node.seen_slot >= 0 {
            current = image.pre_assign(&current, node.seen_slot as usize, 1);
        }

        if node.is_cost_option() && node.cost_once && node.once_slot >= 0 {
            current = image.pre_assign(&current, node.once_slot as usize, 1);
        }

        current
    }

    /// The states in which this node has not been seen.
    fn unseen(&self, node: &LookAheadNode, states: &BDDFunction) -> BDDFunction {
        match self.flag(node.seen_slot) {
            Some(seen) => states.and(&seen.not().expect("not")).expect("and"),
            None => states.clone(),
        }
    }

    /// A slot's "is set" formula, for a slot number that may be -1 for "no slot".
    fn flag(&self, slot: i32) -> Option<BDDFunction> {
        let slot = usize::try_from(slot).ok()?;
        self.vars.slot_is_set(slot)
    }

    /// This node's compiled guard.
    ///
    /// Not cached the way the forward pass caches it, and for a reason worth stating: a
    /// backward pass visits an entry once per widening of its own set, which is far fewer
    /// times than a forward pass visits one in a cycle. If a measurement shows otherwise
    /// this should grow the same map.
    fn guard_of(
        &mut self,
        node: &LookAheadNode,
        compiler: &mut GuardCompiler<'a>,
    ) -> (BDDFunction, BDDFunction) {
        let compiled = compiler.compile(&node.guard);
        (compiled.may_be_true, compiled.may_be_false)
    }

    /// Every entry's incoming links, which is the edge direction this search walks.
    fn parents_of(
        graph: &LookAheadGraph,
    ) -> HashMap<DialogueNodeId, Vec<DialogueNodeId>> {
        let mut parents: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
        for node in graph.nodes() {
            for &child in &node.links {
                parents.entry(child).or_default().push(node.id);
            }
        }
        parents
    }

    /// The entries that can reach `target` through links, guards ignored.
    fn can_reach(
        parents: &HashMap<DialogueNodeId, Vec<DialogueNodeId>>,
        target: DialogueNodeId,
    ) -> HashSet<DialogueNodeId> {
        let mut seen = HashSet::from([target]);
        let mut queue = VecDeque::from([target]);
        while let Some(id) = queue.pop_front() {
            for &parent in parents.get(&id).into_iter().flatten() {
                if seen.insert(parent) {
                    queue.push_back(parent);
                }
            }
        }
        seen
    }

    /// Totals that can only be taken once the sets have stopped moving.
    fn finish(&mut self) {
        self.stats.entries_reaching = self.sets.len();
        self.stats.diagram_nodes = self.sets.values().map(|s| s.node_count()).sum();
        self.stats.largest_set =
            self.sets.values().map(|s| s.node_count()).max().unwrap_or(0);
    }

    /// The states from which ENTERING `node` goes on to reach the target.
    ///
    /// Before rather than after, which is what lets a query be answered without running
    /// anything forwards: a crawl that begins by entering `node` holding the seed reaches
    /// the target exactly when the seed meets this set. An "after entering" set would
    /// need the seed pushed through that first entry by some other means, and the only
    /// thing that could do it is the forward search this exists to avoid.
    pub fn states_at(&self, node: DialogueNodeId) -> Option<&BDDFunction> {
        self.sets.get(&node)
    }

    /// Whether a crawl that starts by entering `node` in any state in `states` reaches
    /// the target.
    ///
    /// `states` is the SEED - what the crawl holds on arrival at `node`, before that
    /// node's own guard, cost or actions have been considered. `seed_of` produces one.
    pub fn reachable_from(&self, node: DialogueNodeId, states: &BDDFunction) -> bool {
        match self.sets.get(&node) {
            // An `Err` here is the manager out of room, and answering "not reachable" on
            // it would be the one wrong direction. Say reachable and let the caller read
            // `out_of_memory`.
            Some(set) => set.and(states).map(|both| both.satisfiable()).unwrap_or(true),
            None => false,
        }
    }

    /// The entries from which the target can be reached at all.
    pub fn entries(&self) -> impl Iterator<Item = DialogueNodeId> + '_ {
        self.sets.keys().copied()
    }

    pub fn stats(&self) -> &BackwardStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::core::guard_value::GuardValue;
    use crate::symbolic::data_layout::DataLayout;
    use crate::symbolic::reachability::{seed_of, Reachability};
    use crate::test_graph::{node, Entry, GraphBuilder};
    use crate::world::test_world::TestWorld;

    const CAP: i32 = 16;
    const NODES: usize = 1 << 18;
    const CACHE: usize = 1 << 16;

    /// Both searches over one graph: does the forward one reach `target`, and does the
    /// backward one say it can be reached from the seed?
    ///
    /// Asked together on purpose. The backward answer alone proves nothing - a pre-image
    /// that dropped every state would answer "no" to everything and look tidy doing it -
    /// so every case here checks it against the search that is already trusted.
    fn both(
        entries: Vec<Entry>,
        world: &TestWorld,
        target: i32,
    ) -> (bool, bool) {
        let mut builder = GraphBuilder::new();
        for entry in entries {
            builder = builder.add(entry);
        }
        // From the GRAPH, not from what the builder handed back: `LookAheadGraph::new`
        // interns the once slots itself, after the builder's copy was taken, so the
        // builder's copy cannot name them.
        let graph = builder.build();
        let symbols = graph.symbols().clone();

        let layout = DataLayout::for_graph(&graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, NODES, CACHE);
        let mut compiler = GuardCompiler::new(&vars).with_world(world);
        let seed = seed_of(&graph, world, &vars);
        let start = node(0);
        let target = node(target);

        let forward = Reachability::explore(
            &graph, start, &seed, &mut compiler, world, CAP as u32,
        );
        let reached = forward
            .states_at(target)
            .is_some_and(|states| states.satisfiable());

        let backward = Backward::reaching(&graph, target, &mut compiler, world, CAP as u32);
        assert!(
            backward.stats().reached_fixed_point,
            "the backward pass did not settle",
        );

        (reached, backward.reachable_from(start, &seed))
    }

    /// The forward and backward answers must agree, and the assertion says which way any
    /// disagreement ran - because the two directions are not equally bad. A backward NO
    /// against a forward YES means the pre-image lost states, which would make the engine
    /// miss a marker.
    fn agree(entries: Vec<Entry>, world: &TestWorld, target: i32, expected: bool) {
        let (forward, backward) = both(entries, world, target);
        assert_eq!(
            forward, expected,
            "the forward search disagrees with the test's own expectation",
        );
        assert_eq!(
            backward, forward,
            "backward said {backward} where forward said {forward} about entry {target}",
        );
    }

    #[test]
    fn an_unguarded_chain_is_reachable_end_to_end() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).links(&[2]),
                Entry::new(2),
            ],
            &TestWorld::new(),
            2,
            true,
        );
    }

    #[test]
    fn a_guard_nothing_can_satisfy_shuts_the_way() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).guard(r#"Variable["shut"]"#).links(&[2]),
                Entry::new(2),
            ],
            &TestWorld::new().set_variable("shut", GuardValue::from_boolean(false)),
            2,
            false,
        );
    }

    /// The case the whole approach is for: a door the path opens for itself.
    ///
    /// The backward pass has to carry "opened is set" back through the assignment that
    /// sets it and come out with a constraint the seed satisfies - which it does by the
    /// assignment ERASING the constraint rather than by proving anything about it.
    #[test]
    fn an_action_on_the_way_opens_a_guard_further_along() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).script(r#"SetVariableValue("opened", true)"#).links(&[2]),
                Entry::new(2).guard(r#"Variable["opened"]"#).links(&[3]),
                Entry::new(3),
            ],
            &TestWorld::new(),
            3,
            true,
        );
    }

    /// And the same shape with the action missing, which must close it.
    #[test]
    fn without_that_action_the_guard_stays_shut() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).links(&[2]),
                Entry::new(2).guard(r#"Variable["opened"]"#).links(&[3]),
                Entry::new(3),
            ],
            &TestWorld::new().set_variable("opened", GuardValue::from_boolean(false)),
            3,
            false,
        );
    }

    /// A counter climbing to a threshold, which needs the increment's pre-image to be a
    /// range rather than a point.
    #[test]
    fn a_counter_reaches_its_threshold_round_a_cycle() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(r#"SetVariableValue("count", Variable["count"] + 1)"#)
                    .links(&[1, 2]),
                Entry::new(2).guard(r#"Variable["count"] >= 3"#).links(&[3]),
                Entry::new(3),
            ],
            &TestWorld::new(),
            3,
            true,
        );
    }

    /// A once increment inside a cycle does NOT climb past its single step.
    ///
    /// It used to, in both directions, because the symbolic passes never raised a once
    /// slot for a once action - `charge` assigned it only for a cost charged once - so
    /// `once_already_fired` was empty at every visit and the action fired every time round
    /// the loop. That was de-sze.15, and this test was written asserting the wrong
    /// behaviour on purpose, so that fixing it would fail here and force both directions
    /// to change together. It did.
    #[test]
    fn a_once_increment_does_not_climb_past_its_single_step() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(r#"SetVariableValue("count", Variable["count"] +once(1))"#)
                    .links(&[1, 2]),
                Entry::new(2).guard(r#"Variable["count"] >= 3"#).links(&[3]),
                Entry::new(3),
            ],
            // Declared numeric, so the explicit crawl can decide the comparison too - see
            // the note in `an_awkward_shape_reaches_everything_the_explicit_crawl_does`.
            &TestWorld::new().set_variable("count", GuardValue::from_number(0.0)),
            3,
            false,
        );
    }

    /// The pre-image DOES split on the once slot, even though nothing currently sets it.
    ///
    /// Asked directly, because the test above can no longer ask it: with the slot never
    /// raised, both branches of the split behave alike and a pre-image that dropped the
    /// split entirely would pass. Here the slot is set in the world, so the states that
    /// have already fired are real, and a once action must leave them alone.
    #[test]
    fn a_once_action_leaves_a_state_that_has_already_fired_alone() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(
                Entry::new(1)
                    .script(r#"SetVariableValue("count", Variable["count"] +once(1))"#)
                    .links(&[2]),
            )
            .add(Entry::new(2).guard(r#"Variable["count"] >= 1"#).links(&[3]))
            .add(Entry::new(3))
            .build();
        let symbols = graph.symbols().clone();

        let world = TestWorld::new();
        let layout = DataLayout::for_graph(&graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, NODES, CACHE);
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        let backward =
            Backward::reaching(&graph, node(3), &mut compiler, &world, CAP as u32);

        let entering = backward.states_at(node(1)).expect("entry 1 can reach the target");
        let once = symbols
            .find(&format!("once:{}:{}", node(1).conversation_id, node(1).entry_id))
            .expect("a once action interns a once slot");
        let count = symbols.find("count").expect("the counter is interned");

        // Having already fired, the increment does not happen, so only a state that
        // already holds the count can get through.
        let fired = vars.slot_is_set(once).unwrap();
        let empty = vars.slot_equals(count, 0).unwrap();
        let spent_and_empty = fired.and(&empty).unwrap().and(entering).unwrap();
        assert!(
            !spent_and_empty.satisfiable(),
            "a spent once action was allowed to fire again",
        );

        // Not yet fired, and the increment carries the same state through.
        let fresh = fired.not().unwrap();
        let fresh_and_empty = fresh.and(&empty).unwrap().and(entering).unwrap();
        assert!(
            fresh_and_empty.satisfiable(),
            "a once action that has not fired should still be able to",
        );
    }

    /// A hidden test entry is entered and goes nowhere, in both directions.
    #[test]
    fn nothing_is_reachable_through_a_test_entry() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).kind(DialogueCheckKind::Test).links(&[2]),
                Entry::new(2),
            ],
            &TestWorld::new(),
            2,
            false,
        );
    }

    /// A white check can be retried, so both of its branches lead onward.
    #[test]
    fn a_white_check_leads_onward_down_both_branches() {
        agree(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .kind(DialogueCheckKind::White)
                    .flag("check.jump")
                    .links(&[2]),
                Entry::new(2),
            ],
            &TestWorld::new(),
            2,
            true,
        );
    }

    /// The target itself is the answer: entering it is the whole event, so a start that
    /// IS the target reaches it with no steps at all.
    #[test]
    fn a_start_that_is_the_target_reaches_it() {
        agree(vec![Entry::new(0).links(&[1]), Entry::new(1)], &TestWorld::new(), 0, true);
    }

    /// An entry no link leads to is unreachable, and the backward pass never visits it.
    #[test]
    fn an_orphan_entry_is_unreachable() {
        agree(
            vec![Entry::new(0).links(&[1]), Entry::new(1), Entry::new(2)],
            &TestWorld::new(),
            2,
            false,
        );
    }

    /// The awkward shape, against the EXPLICIT crawl rather than against the forward
    /// symbolic search.
    ///
    /// Everything else here compares the two symbolic searches, which share their
    /// approximations and so cannot catch one. This asks the engine the plugin actually
    /// runs, over the shape that has caught the most bugs in this file: a cycle, a once
    /// action inside it, a counter, and a threshold on the counter.
    ///
    /// Containment rather than equality, and in one direction only. The symbolic side may
    /// reach more - it does here, because de-sze.15 lets the once action fire every time
    /// round - and a surplus costs precision. Reaching LESS would cost a marker.
    #[test]
    fn an_awkward_shape_reaches_everything_the_explicit_crawl_does() {
        use crate::engine::engine::{LookAheadEngine, LookAheadOptions};
        use crate::core::types::Novelty;
        use std::collections::HashSet;
        use std::sync::{Arc, Mutex};

        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            // A cycle that counts, with the increment marked once.
            .add(
                Entry::new(1)
                    .script(r#"SetVariableValue("count", Variable["count"] +once(1))"#)
                    .links(&[1, 2, 4]),
            )
            // A threshold only a repeated increment could pass.
            .add(Entry::new(2).guard(r#"Variable["count"] >= 2"#).links(&[3]))
            .add(Entry::new(3))
            // And a branch behind a flag the cycle never sets, which nothing can open.
            .add(Entry::new(4).guard(r#"Variable["locked"]"#).links(&[5]))
            .add(Entry::new(5))
            .build();
        let symbols = graph.symbols().clone();
        // `count` is declared a NUMBER, and it has to be. The explicit engine reads a
        // tracked slot back through the world's idea of its type - see
        // `BoundContext::get_variable` - so a counter the world has never heard of comes
        // back as a boolean, `try_as_number` refuses it, and `count >= 2` is undecidable
        // and therefore permissive. The symbolic side compiles the same comparison against
        // the slot's bits and decides it. Without this line the two disagree about the
        // fixture rather than about the once slot, which is what this test is for.
        // de-sze.5.4 is the real fix: the index does not carry declared types yet.
        let world = TestWorld::new()
            .set_variable("locked", GuardValue::from_boolean(false))
            .set_variable("count", GuardValue::from_number(0.0));

        let walked: Arc<Mutex<HashSet<DialogueNodeId>>> = Arc::default();
        let sink = Arc::clone(&walked);
        let engine = LookAheadEngine::new(LookAheadOptions {
            state_sample_interval: 1,
            counter_cap: CAP,
            on_state_reached: Some(Box::new(move |id, _state, _count| {
                sink.lock().expect("the sink").insert(id);
            })),
            ..Default::default()
        });
        let result = engine.evaluate(&graph, node(0), &world, |_| Novelty::SeenThisGame);
        assert!(!result.budget_exhausted(), "the fixture should be exhaustible");
        let walked = walked.lock().expect("the sink").clone();

        let layout = DataLayout::for_graph(&graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, NODES, CACHE);
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        let seed = seed_of(&graph, &world, &vars);

        for id in graph.nodes().map(|n| n.id).collect::<Vec<_>>() {
            let backward = Backward::reaching(&graph, id, &mut compiler, &world, CAP as u32);
            let says = backward.reachable_from(node(0), &seed);
            if walked.contains(&id) {
                assert!(says, "the crawl walked to {id} and the backward pass refused it");
            }
        }

        // The locked branch is out of reach both ways, which is what stops this test
        // passing vacuously by calling everything reachable.
        assert!(!walked.contains(&node(5)));
        let backward = Backward::reaching(&graph, node(5), &mut compiler, &world, CAP as u32);
        assert!(
            !backward.reachable_from(node(0), &seed),
            "a branch behind a guard nothing sets should be out of reach",
        );
    }

    /// What the module claims about pruning, asserted rather than described.
    ///
    /// A slot written on the way to the target and never read by any guard must leave no
    /// trace in the backward sets: `pre_assign` selects and then forgets, so the
    /// constraint on it is erased rather than carried. Forward, the same assignment pins
    /// that slot in every set downstream.
    #[test]
    fn a_slot_nothing_reads_leaves_no_trace_in_the_backward_sets() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).script(r#"SetVariableValue("noise", true)"#).links(&[2]))
            .add(Entry::new(2))
            .build();
        let symbols = graph.symbols().clone();

        let world = TestWorld::new();
        let layout = DataLayout::for_graph(&graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, NODES, CACHE);
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);

        let backward =
            Backward::reaching(&graph, node(2), &mut compiler, &world, CAP as u32);

        // Nothing reads `noise`, so every backward set is everywhere-true: the target is
        // reachable whatever the state holds.
        for id in [node(0), node(1), node(2)] {
            let set = backward.states_at(id).expect("every entry can reach the target");
            assert!(set.valid(), "entry {id} carries a constraint it should not");
        }

        // And the whole thing is one diagram node - the constant - which is the concrete
        // form of "the slot left no trace".
        assert_eq!(backward.stats().largest_set, 1);
        let _ = symbols;
    }
}
