// SPDX-License-Identifier: MIT
//! A reference walk over a graph and a world: the oracle the searches are checked against.
//!
//! ## Why this exists at all
//!
//! The two symbolic searches share their layout, their guard compiler, their action images
//! and their three deliberate approximations. Comparing them against each other therefore
//! cannot catch a fault they share - and until 2026-09-06 nothing had to, because the
//! explicit crawl was there to contradict them. It went with the engine it belonged to, and
//! this is what replaces it (de-eonm).
//!
//! ## What it is, and what it deliberately is not
//!
//! It enumerates `(entry, state)` pairs one at a time, the obvious way, and that is the
//! whole of it. NO BUDGETS beyond one hard ceiling that exists so a test cannot hang, no
//! memory accounting, no progress reporting, no early exit when the answer is already
//! settled, no trace. The crawl was fast and complicated for reasons a test-only oracle
//! does not have, and every one of those reasons was a place a bug could hide.
//!
//! It is not a search anything ships. It belongs to the tests the way
//! [`crate::world::test_world`] does, and a group it cannot exhaust is a group it cannot
//! be an oracle for - which the tests check by asking [`Walk::exhausted`] and skipping.
//!
//! ## What it IS authoritative about
//!
//! The model: which entries can be entered, what entering one does, and when a check, a
//! cost or a once action closes a door. Those rules used to live in the crawl and now live
//! here and in [`crate::symbolic::reachability`], so this file is one of the two places
//! they are written down. It is checked against the other one, not against itself, and the
//! unit tests below pin the rules it applies so the comparison is between two understood
//! things rather than two guesses.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::core::action::{CounterCaps, DialogueAction};
use crate::core::state::{LookAheadState, seed_state};
use crate::core::types::{DialogueCheckKind, DialogueNodeId, Novelty, StartBranch, Ternary};
use crate::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::world::{CrawlContext, ILookAheadWorld};

/// The cap every search in this repository counts to, and so the one a comparison uses.
///
/// A counter the walk lets climb further than the layout can encode is a disagreement about
/// the fixture rather than about the searches.
pub const COUNTER_CAP: i32 = 16;

/// How many distinct `(entry, state)` pairs the walk will hold before giving up.
///
/// A CEILING AND NOT A BUDGET: nothing about it is meant to be tuned, and a walk that hits
/// it has failed to be an oracle rather than produced a smaller answer. It is here so a
/// fixture that turns out to be unbounded fails a test instead of hanging one.
pub const CEILING: usize = 400_000;

/// What a reference walk found.
#[derive(Debug, Clone, Default)]
pub struct Walk {
    entries: HashSet<DialogueNodeId>,
    arrived: HashSet<DialogueNodeId>,
    states: usize,
    exhausted: bool,
}

impl Walk {
    /// Every entry some reachable state sat on, the start included.
    ///
    /// This is the set a reachability claim is checked against: an entry in here is
    /// reachable, full stop, and a search that calls one of them unreachable is wrong in
    /// the direction that costs a marker.
    pub fn entries(&self) -> &HashSet<DialogueNodeId> {
        &self.entries
    }

    /// Whether the walk got to `id`.
    pub fn reached(&self, id: DialogueNodeId) -> bool {
        self.entries.contains(&id)
    }

    /// How many distinct `(entry, state)` pairs it held.
    pub fn states(&self) -> usize {
        self.states
    }

    /// Whether it hit [`CEILING`] and so proved nothing.
    ///
    /// A caller comparing against this must ask, and skip: a walk that stopped early
    /// reaches fewer entries than the graph allows, and a search reaching more of them is
    /// then correct rather than surplus.
    pub fn exhausted(&self) -> bool {
        self.exhausted
    }

    /// The best novelty anything the walk ARRIVED AT carries.
    ///
    /// Arrived at, not merely stood on: the start is where the seed states were built, and
    /// for one outcome of a rolled check it is the CHECK rather than anything that outcome
    /// opens. Scoring it would report the option the player is standing on as somewhere
    /// this branch leads. It is scored only where a link leads back to it.
    ///
    /// Groups are traversed and never scored, because the player never sees a group.
    pub fn best_novelty<F>(&self, novelty: F) -> Novelty
    where
        F: Fn(DialogueNodeId) -> Novelty,
    {
        self.arrived
            .iter()
            .map(|id| novelty(*id))
            .max()
            .unwrap_or(Novelty::SeenThisGame)
    }
}

/// Walks everything reachable from `start`, taking both outcomes where the start rolls.
pub fn walk(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    counter_cap: i32,
) -> Walk {
    walk_branch(
        graph,
        start,
        StartBranch::Either,
        world,
        counter_cap,
        CEILING,
    )
}

/// The same walk, told which outcome of a rolled start to explore and what it may hold.
///
/// A white or red check is two options wearing one line of text, and the interesting thing
/// about it is often which of the two leads somewhere.
pub fn walk_branch(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    branch: StartBranch,
    world: &dyn ILookAheadWorld,
    counter_cap: i32,
    ceiling: usize,
) -> Walk {
    let start_node = graph.get(start).expect("the start is in the graph");
    let context = CrawlContext::new(graph.symbols(), world);
    let caps = CounterCaps::flat(counter_cap);
    let clock_locked = world.is_clock_locked();

    let seed = seed_state(graph, world);
    let entered = keep(
        branch,
        enter(start_node, &seed, &context, &caps, clock_locked),
    );

    let mut walk = Walk::default();
    if entered.is_empty() {
        return walk;
    }

    let mut seen: HashSet<(DialogueNodeId, LookAheadState)> = HashSet::new();
    let mut queue: VecDeque<(DialogueNodeId, LookAheadState)> = VecDeque::new();
    for state in entered {
        if seen.insert((start, state.clone())) {
            queue.push_back((start, state));
        }
    }
    walk.entries.insert(start);

    while let Some((id, state)) = queue.pop_front() {
        // AFTER THE POP, so a walk that exactly fills the ceiling is not called exhausted
        // for having finished. What is left in the queue is what it did not get to.
        if seen.len() >= ceiling {
            walk.exhausted = true;
            break;
        }

        let node = graph.get(id).expect("a queued entry is in the graph");
        for &child_id in &node.links {
            let Some(child) = graph.get(child_id) else {
                continue;
            };

            for next in enter(child, &state, &context, &caps, clock_locked) {
                // SCORED ON ARRIVAL AND EXPANDED ONCE ARE DIFFERENT QUESTIONS. An entry a
                // loop leads back to is arrived at again even though its states are all
                // familiar, and an improvement reachable only round a loop is still an
                // improvement.
                if !child.is_group {
                    walk.arrived.insert(child_id);
                }

                if seen.insert((child_id, next.clone())) {
                    walk.entries.insert(child_id);
                    queue.push_back((child_id, next));
                }
            }
        }
    }

    walk.states = seen.len();
    walk
}

/// Exact choice distances from one menu outcome, enumerating concrete states.
/// A missing result means the walk hit its ceiling and cannot serve as an oracle.
pub fn choice_distances(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    branch: StartBranch,
    cut: &HashSet<DialogueNodeId>,
    world: &dyn ILookAheadWorld,
    counter_cap: i32,
) -> Option<HashMap<DialogueNodeId, usize>> {
    let context = CrawlContext::new(graph.symbols(), world);
    let caps = CounterCaps::flat(counter_cap);
    let clock_locked = world.is_clock_locked();
    let start_node = graph.get(start)?;
    let entered = keep(
        branch,
        enter(
            start_node,
            &seed_state(graph, world),
            &context,
            &caps,
            clock_locked,
        ),
    );
    let mut pending = VecDeque::new();
    let mut seen = HashMap::new();
    let mut distances = HashMap::new();
    for state in entered {
        seen.insert((start, state.clone(), true), 0usize);
        pending.push_back((start, state, 0usize, true));
    }
    while let Some((id, state, distance, first)) = pending.pop_front() {
        if seen.len() >= CEILING {
            return None;
        }
        if seen.get(&(id, state.clone(), first)) != Some(&distance) {
            continue;
        }
        let node = graph.get(id)?;
        if !node.is_group
            && !(node.kind == DialogueCheckKind::Passive
                && world.check_passes(id) == Ternary::False)
        {
            distances
                .entry(id)
                .and_modify(|d: &mut usize| *d = (*d).min(distance))
                .or_insert(distance);
        }
        let cost = usize::from(node.choice && !first);
        for &child_id in &node.links {
            if cut.contains(&child_id) {
                continue;
            }
            let Some(child) = graph.get(child_id) else {
                continue;
            };
            let candidate = distance + cost;
            for next in enter(child, &state, &context, &caps, clock_locked) {
                let key = (child_id, next.clone(), false);
                if seen.get(&key).is_none_or(|d| candidate < *d) {
                    seen.insert(key, candidate);
                    let item = (child_id, next, candidate, false);
                    if cost == 0 {
                        pending.push_front(item);
                    } else {
                        pending.push_back(item);
                    }
                }
            }
        }
    }
    Some(distances)
}

/// The states one branch of a rolled start keeps, out of everything entering it produced.
///
/// WHY THE ORDER IS LOAD BEARING: `Pass` and `Fail` are positions in the vector
/// [`enter_rolled`] builds, not a re-derivation of the roll, so anything that reorders that
/// has to reorder this with it.
fn keep(branch: StartBranch, entered: Vec<LookAheadState>) -> Vec<LookAheadState> {
    match branch {
        StartBranch::Either => entered,
        // A single state means the start does not roll, and one branch is all there is - so
        // naming a branch of it names that state rather than nothing.
        StartBranch::Pass => entered.into_iter().take(1).collect(),
        StartBranch::Fail => {
            if entered.len() < 2 {
                Vec::new()
            } else {
                entered.into_iter().skip(1).collect()
            }
        }
    }
}

/// The states entering this node can leave the walk in: none if it is closed, one for an
/// ordinary entry, two where a rolled check can go either way.
fn enter(
    node: &LookAheadNode,
    state: &LookAheadState,
    context: &CrawlContext<'_>,
    caps: &CounterCaps<'_>,
    clock_locked: bool,
) -> Vec<LookAheadState> {
    let mut results = Vec::new();

    // UNDECIDED IS NOT CLOSED. A guard the search cannot decide lets the state through,
    // which is the first of the deliberate over-approximations - and the walk shares it,
    // because an oracle that answered a different question would report every one of them
    // as a disagreement.
    if node.guard.test(&context.bound(state)) == Ternary::False {
        return results;
    }
    if !can_afford(node, state) {
        return results;
    }

    match node.kind {
        // A hidden test is entered and goes nowhere.
        DialogueCheckKind::Test => {}

        DialogueCheckKind::Fake => {
            if !has_been_seen(node, state) {
                results.push(charge(node, state, caps, clock_locked));
            }
        }

        DialogueCheckKind::KimSwitch => {
            if node.boolean_only || !has_been_seen(node, state) {
                results.push(charge(node, state, caps, clock_locked));
            }
        }

        DialogueCheckKind::Red | DialogueCheckKind::White => {
            results.extend(enter_rolled(
                node,
                state,
                caps,
                clock_locked,
                crate::world::roll_may_succeed(node, context.world),
            ));
        }

        DialogueCheckKind::Passive => {
            let passes = context.world.check_passes(node.id);
            if passes != Ternary::False {
                results.push(charge(node, state, caps, clock_locked));
            }
            // The failing branch passes the incoming state through UNCHARGED - the one
            // branch that does not go through `charge`.
            if passes != Ternary::True {
                results.push(state.clone());
            }
        }

        _ => results.push(charge(node, state, caps, clock_locked)),
    }

    results
}

/// The two ways a roll can go, kept in the order [`keep`] indexes them by.
fn enter_rolled(
    node: &LookAheadNode,
    state: &LookAheadState,
    caps: &CounterCaps<'_>,
    clock_locked: bool,
    may_succeed: bool,
) -> Vec<LookAheadState> {
    let mut results = Vec::new();

    let passed = node.flag_slot >= 0 && state.is_set(node.flag_slot as usize);
    let failed = node.failed_flag_slot >= 0 && state.is_set(node.failed_flag_slot as usize);

    // A check already resolved is closed, whichever way it went and whichever kind it is.
    // The game keeps failed white checks in FailedWhiteChecks and only reopens one when the
    // skill rank rises or a modifier lowers the target; neither is modelled here, so a
    // failure closes it for the rest of the walk. See de-1uy8.
    if passed || failed {
        return results;
    }

    let entered = charge(node, state, caps, clock_locked);

    // Both branches start from the same charged state, so the success branch takes a copy
    // and leaves the original for the failure branch - where the roll may succeed at all;
    // see `world::roll_may_succeed`.
    if may_succeed {
        let success = if node.flag_slot >= 0 {
            entered.with(node.flag_slot as usize, 1)
        } else {
            entered.clone()
        };
        results.push(success);
    }

    if node.failed_flag_slot >= 0 {
        results.push(entered.with(node.failed_flag_slot as usize, 1));
    } else if node.kind == DialogueCheckKind::White {
        // No flag to record the failure with, so it stays retryable and the ceiling is what
        // bounds the loop.
        results.push(entered);
    }

    results
}

/// Whether the player could pay for this entry out of the state's own purse.
pub(crate) fn can_afford(node: &LookAheadNode, state: &LookAheadState) -> bool {
    if !node.is_cost_option() {
        return true;
    }
    if node.cost_once && node.once_slot >= 0 && state.is_set(node.once_slot as usize) {
        return true;
    }
    node.cost <= state.money()
}

fn has_been_seen(node: &LookAheadNode, state: &LookAheadState) -> bool {
    node.seen_slot >= 0 && state.is_set(node.seen_slot as usize)
}

/// Pays for the entry, records having seen it, and applies its actions.
fn charge(
    node: &LookAheadNode,
    state: &LookAheadState,
    caps: &CounterCaps<'_>,
    clock_locked: bool,
) -> LookAheadState {
    let mut paid = state.clone();
    if node.is_cost_option() {
        let already_paid =
            node.cost_once && node.once_slot >= 0 && state.is_set(node.once_slot as usize);
        if !already_paid {
            paid = paid.with_money(paid.money() - node.cost);
            if node.cost_once && node.once_slot >= 0 {
                paid = paid.with(node.once_slot as usize, 1);
            }
        }
    }

    if node.seen_slot >= 0 {
        paid = paid.with(node.seen_slot as usize, 1);
    }

    DialogueAction::apply(&node.actions, &paid, node.once_slot, caps, clock_locked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::guard_value::GuardValue;
    use crate::test_graph::{Entry, GraphBuilder, node};
    use crate::world::test_world::TestWorld;

    /// A white check, which is the rolled kind these fixtures use.
    fn white(entry: Entry) -> Entry {
        entry.kind(DialogueCheckKind::White)
    }

    /// The rules this file is authoritative about, one fixture each.
    ///
    /// They are not a second opinion on the searches - they are what makes the walk worth
    /// comparing against, since an oracle nobody checks is just another implementation.
    fn walked(entries: Vec<Entry>, world: &TestWorld) -> Walk {
        let mut builder = GraphBuilder::new();
        for entry in entries {
            builder = builder.add(entry);
        }
        let graph = builder.build();
        let walk = walk(&graph, node(0), world, COUNTER_CAP);
        assert!(
            !walk.exhausted(),
            "a fixture this small should be exhaustible"
        );
        walk
    }

    #[test]
    fn a_false_guard_closes_an_entry() {
        let world = TestWorld::new().set_variable("shut", GuardValue::from_boolean(false));
        let walk = walked(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).guard(r#"Variable["shut"]"#).links(&[2]),
                Entry::new(2),
            ],
            &world,
        );
        assert!(!walk.reached(node(1)));
        assert!(!walk.reached(node(2)));
    }

    #[test]
    fn an_undecided_guard_does_not() {
        let walk = walked(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).guard("IsKimHere()").links(&[2]),
                Entry::new(2),
            ],
            &TestWorld::new(),
        );
        assert!(walk.reached(node(2)));
    }

    #[test]
    fn an_action_opens_the_guard_below_it() {
        let walk = walked(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(r#"SetVariableValue("opened", true)"#)
                    .links(&[2]),
                Entry::new(2).guard(r#"Variable["opened"]"#),
            ],
            &TestWorld::new(),
        );
        assert!(walk.reached(node(2)));
    }

    #[test]
    fn a_price_the_purse_cannot_meet_closes_an_option() {
        let world = TestWorld::new().with_money(5);
        let walk = walked(
            vec![
                Entry::new(0).links(&[1, 2]),
                Entry::new(1).cost(10).links(&[3]),
                Entry::new(2).cost(5).links(&[4]),
                Entry::new(3),
                Entry::new(4),
            ],
            &world,
        );
        assert!(
            !walk.reached(node(1)),
            "ten centimes out of a purse of five"
        );
        assert!(walk.reached(node(4)), "five out of five is affordable");
    }

    #[test]
    fn paying_once_leaves_less_for_the_next_price() {
        let world = TestWorld::new().with_money(10);
        let walk = walked(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).cost(6).links(&[2]),
                Entry::new(2).cost(6).links(&[3]),
                Entry::new(3),
            ],
            &world,
        );
        assert!(walk.reached(node(1)), "six out of ten is affordable once");
        assert!(
            !walk.reached(node(2)),
            "and leaves four, which the second price of six cannot come out of",
        );
    }

    #[test]
    fn a_once_increment_does_not_climb_past_its_single_step() {
        // Declared numeric, so the comparison in the guard can be decided at all: a slot
        // the world has never heard of reads back as a boolean and the guard is then
        // undecidable, which would open entry 2 for the wrong reason.
        let world = TestWorld::new().set_variable("count", GuardValue::from_number(0.0));
        let walk = walked(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(r#"SetVariableValue("count", Variable["count"] +once(1))"#)
                    .links(&[1, 2]),
                Entry::new(2).guard(r#"Variable["count"] >= 3"#).links(&[3]),
                Entry::new(3),
            ],
            &world,
        );
        assert!(
            !walk.reached(node(3)),
            "one step cannot reach a threshold of three"
        );
    }

    #[test]
    fn a_counter_in_a_cycle_does_climb() {
        let world = TestWorld::new().set_variable("count", GuardValue::from_number(0.0));
        let walk = walked(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(r#"SetVariableValue("count", Variable["count"] + 1)"#)
                    .links(&[1, 2]),
                Entry::new(2).guard(r#"Variable["count"] >= 3"#).links(&[3]),
                Entry::new(3),
            ],
            &world,
        );
        assert!(
            walk.reached(node(3)),
            "an unconditioned increment round a loop reaches it"
        );
    }

    #[test]
    fn a_cycle_terminates() {
        let walk = walked(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).links(&[2]),
                Entry::new(2).links(&[1]),
            ],
            &TestWorld::new(),
        );
        assert!(walk.reached(node(2)));
    }

    #[test]
    fn a_group_is_walked_through_and_never_scored() {
        let walk = walked(
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).group().links(&[2]),
                Entry::new(2),
            ],
            &TestWorld::new(),
        );
        assert!(walk.reached(node(1)), "a group is walked through");
        let unseen = |id: DialogueNodeId| {
            if id == node(1) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        };
        assert_eq!(
            walk.best_novelty(unseen),
            Novelty::SeenThisGame,
            "and never scored",
        );
    }

    #[test]
    fn the_start_is_scored_only_where_a_link_leads_back_to_it() {
        let unseen = |id: DialogueNodeId| {
            if id == node(0) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        };

        let onward = walked(
            vec![Entry::new(0).links(&[1]), Entry::new(1)],
            &TestWorld::new(),
        );
        assert_eq!(onward.best_novelty(unseen), Novelty::SeenThisGame);

        let loops_back = walked(
            vec![Entry::new(0).links(&[1]), Entry::new(1).links(&[0])],
            &TestWorld::new(),
        );
        assert_eq!(loops_back.best_novelty(unseen), Novelty::UnseenAnyGame);
    }

    #[test]
    fn a_rolled_start_can_be_asked_about_one_outcome() {
        let graph = GraphBuilder::new()
            .add(white(Entry::new(0)).flag("roll").links(&[1, 2]))
            .add(Entry::new(1).guard(r#"Variable["roll"]"#))
            .add(Entry::new(2).guard(r#"Variable["roll_failed"]"#))
            .build();
        let world = TestWorld::new();

        let passing = walk_branch(
            &graph,
            node(0),
            StartBranch::Pass,
            &world,
            COUNTER_CAP,
            CEILING,
        );
        assert!(passing.reached(node(1)));
        assert!(!passing.reached(node(2)));

        let failing = walk_branch(
            &graph,
            node(0),
            StartBranch::Fail,
            &world,
            COUNTER_CAP,
            CEILING,
        );
        assert!(!failing.reached(node(1)));
        assert!(failing.reached(node(2)));

        let either = walk_branch(
            &graph,
            node(0),
            StartBranch::Either,
            &world,
            COUNTER_CAP,
            CEILING,
        );
        assert!(either.reached(node(1)) && either.reached(node(2)));
    }

    #[test]
    fn a_check_already_resolved_is_closed() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(white(Entry::new(1)).flag("roll").links(&[2]))
            .add(Entry::new(2))
            .build();
        // The roll has already been made and lost, so the check cannot be entered at all.
        let world = TestWorld::new().set_variable("roll_failed", GuardValue::from_boolean(true));
        let walk = walk(&graph, node(0), &world, COUNTER_CAP);
        assert!(!walk.reached(node(2)));
    }

    /// A thought that forces red checks to fail closes a red check's success to the walk.
    ///
    /// 1 is the check: passing opens 2 and failing opens 3. With the effect on only the
    /// failure is left - and a white check of the same shape is untouched by it.
    #[test]
    fn a_red_check_forced_to_fail_opens_only_its_failure() {
        let check_of = |kind| {
            GraphBuilder::new()
                .add(Entry::new(0).links(&[1]))
                .add(Entry::new(1).kind(kind).flag("roll").links(&[2, 3]))
                .add(Entry::new(2).guard(r#"Variable["roll"]"#))
                .add(Entry::new(3).guard(r#"Variable["roll_failed"]"#))
                .build()
        };
        let world = TestWorld::new().with_red_checks_failing(true);

        let red = walk(
            &check_of(DialogueCheckKind::Red),
            node(0),
            &world,
            COUNTER_CAP,
        );
        assert!(!red.reached(node(2)), "a red success is closed");
        assert!(red.reached(node(3)), "a red failure is not");

        let white = walk(
            &check_of(DialogueCheckKind::White),
            node(0),
            &world,
            COUNTER_CAP,
        );
        assert!(
            white.reached(node(2)),
            "nothing forces a white check to fail"
        );
    }

    #[test]
    fn the_ceiling_is_reported_rather_than_hit_quietly() {
        // A white check with no flag to record its failure with is retryable for ever, and
        // the increment beneath it gives every retry a state of its own.
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(
                Entry::new(1)
                    .script(r#"SetVariableValue("count", Variable["count"] + 1)"#)
                    .links(&[1]),
            )
            .build();
        let world = TestWorld::new().set_variable("count", GuardValue::from_number(0.0));
        let walk = walk_branch(&graph, node(0), StartBranch::Either, &world, 1 << 20, 64);
        assert!(walk.exhausted(), "a walk that ran out of room must say so");
    }
}
