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
//! [`crate::world::GameWorld`] does, and a group it cannot exhaust is a group it cannot
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
use crate::core::types::{DialogueCheckKind, DialogueNodeId, SeenState, StartBranch, Ternary};
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
///
/// Sized above the largest real group a test walks. Conversation 640's option 12 reaches
/// 434,666 states now that the journal is tracked as its variables - a task's progress in
/// the Hardie conversations moves independently of the others' - and a walk that size takes
/// under two seconds.
pub const CEILING: usize = 2_000_000;

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

    /// The best seen state anything the walk ARRIVED AT carries.
    ///
    /// Arrived at, not merely stood on: the start is where the seed states were built, and
    /// for one outcome of a rolled check it is the CHECK rather than anything that outcome
    /// opens. Scoring it would report the option the player is standing on as somewhere
    /// this branch leads. It is scored only where a link leads back to it.
    ///
    /// Groups are traversed and never scored, because the player never sees a group.
    pub fn best_novelty<F>(&self, seen_state: F) -> SeenState
    where
        F: Fn(DialogueNodeId) -> SeenState,
    {
        self.arrived
            .iter()
            .map(|id| seen_state(*id))
            .max()
            .unwrap_or(SeenState::SeenThisGame)
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
    let caps = CounterCaps::for_graph(counter_cap, graph);

    let seed = seed_state(graph, world);
    let entered = keep(branch, enter(start_node, &seed, &context, &caps));

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

            for next in enter(child, &state, &context, &caps) {
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
    let caps = CounterCaps::for_graph(counter_cap, graph);
    let start_node = graph.get(start)?;
    let entered = keep(
        branch,
        enter(start_node, &seed_state(graph, world), &context, &caps),
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
                && crate::world::passive_outcome(node, world) == Ternary::False)
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
            for next in enter(child, &state, &context, &caps) {
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
pub(crate) fn enter(
    node: &LookAheadNode,
    state: &LookAheadState,
    context: &CrawlContext<'_>,
    caps: &CounterCaps,
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
            // A fake check's roll is forced, so entering one is its one branch - and the graph
            // gives it failure actions only where that is a failure.
            if !has_been_seen(node, state) {
                let entered = charge(node, state, caps, context.world);
                results.push(fail(node, entered, caps, context.world));
            }
        }

        DialogueCheckKind::KimSwitch => {
            if node.boolean_only || !has_been_seen(node, state) {
                results.push(charge(node, state, caps, context.world));
            }
        }

        DialogueCheckKind::Red | DialogueCheckKind::White => {
            results.extend(enter_rolled(
                node,
                state,
                caps,
                context.world,
                crate::world::roll_may_succeed(node, context.world),
            ));
        }

        DialogueCheckKind::Passive => {
            let passes = crate::world::passive_outcome(node, context.world);
            if passes != Ternary::False {
                results.push(charge(node, state, caps, context.world));
            }
            // The failing branch passes the incoming state through UNCHARGED - the one
            // branch that does not go through `charge`.
            if passes != Ternary::True {
                results.push(state.clone());
            }
        }

        _ => results.push(charge(node, state, caps, context.world)),
    }

    results
}

/// The two ways a roll can go, kept in the order [`keep`] indexes them by.
fn enter_rolled(
    node: &LookAheadNode,
    state: &LookAheadState,
    caps: &CounterCaps,
    world: &dyn ILookAheadWorld,
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

    let entered = charge(node, state, caps, world);

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
        let failed = entered.with(node.failed_flag_slot as usize, 1);
        results.push(fail(node, failed, caps, world));
    } else if node.kind == DialogueCheckKind::White {
        // No flag to record the failure with, so it stays retryable and the ceiling is what
        // bounds the loop.
        results.push(fail(node, entered, caps, world));
    }

    results
}

/// Whether the player could pay for this entry out of the state's own purse.
///
/// A COST CHARGED ONCE IS STILL PRICED THE SECOND TIME. Entering it again takes nothing from
/// the purse (`CostOptionNode.HandleEntry` skips the charge for a seen once-cost entry), but
/// the option is disabled whenever the price is above the purse
/// (`CostOptionNode.HandleResponseText`, with no exception for having paid) - so a path back
/// through a shop door it has paid at still needs the price in hand. The same in the
/// pre-final-cut export and Final Cut's ISIL dump.
pub(crate) fn can_afford(node: &LookAheadNode, state: &LookAheadState) -> bool {
    !node.is_cost_option() || node.cost <= state.money()
}

pub(crate) fn has_been_seen(node: &LookAheadNode, state: &LookAheadState) -> bool {
    node.seen_slot >= 0 && state.is_set(node.seen_slot as usize)
}

/// Pays for the entry, records having seen it, and applies its actions.
pub(crate) fn charge(
    node: &LookAheadNode,
    state: &LookAheadState,
    caps: &CounterCaps,
    world: &dyn ILookAheadWorld,
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

    DialogueAction::apply(
        &node.actions,
        &paid,
        node.once_slot,
        caps,
        world.is_clock_locked(),
        world.day_counter(),
    )
}

/// What a check's failing branch does beyond recording the failure - see
/// [`LookAheadNode::failure_actions`]. Not once: a failure only happens once per check, which
/// the failure flag already records.
fn fail(
    node: &LookAheadNode,
    state: LookAheadState,
    caps: &CounterCaps,
    world: &dyn ILookAheadWorld,
) -> LookAheadState {
    DialogueAction::apply(
        &node.failure_actions,
        &state,
        -1,
        caps,
        world.is_clock_locked(),
        world.day_counter(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::guard_value::GuardValue;
    use crate::test_graph::{Entry, GraphBuilder, node};
    use crate::world::GameWorld;

    /// A white check, which is the rolled kind these fixtures use.
    fn white(entry: Entry) -> Entry {
        entry.kind(DialogueCheckKind::White)
    }

    /// The rules this file is authoritative about, one fixture each.
    ///
    /// They are not a second opinion on the searches - they are what makes the walk worth
    /// comparing against, since an oracle nobody checks is just another implementation.
    fn walked(entries: Vec<Entry>, world: &GameWorld) -> Walk {
        let mut builder = GraphBuilder::new();
        for entry in entries {
            builder = builder.add(entry);
        }
        let mut graph = builder.build();
        graph.fit(&crate::graph::Fitting::read(&graph, world));
        let walk = walk(&graph, node(0), world, COUNTER_CAP);
        assert!(
            !walk.exhausted(),
            "a fixture this small should be exhaustible"
        );
        walk
    }

    /// Whether entry 2's passive check comes out SETTLED once the graph is fitted.
    ///
    /// The subject of the two tests below. `world::passive_outcome` answers definitely
    /// whether or not a check is settled - see its note - so this asks the fitting directly
    /// rather than through a search that no longer tells the two apart.
    fn settled_check(entries: Vec<Entry>, world: &GameWorld) -> bool {
        let mut builder = GraphBuilder::new();
        for entry in entries {
            builder = builder.add(entry);
        }
        let mut graph = builder.build();
        graph.fit(&crate::graph::Fitting::read(&graph, world));
        graph
            .get(node(2))
            .expect("the fixture has entry 2")
            .check_settled
    }

    /// A garment unsettles the checks on the skill IT moves, and leaves the others settled.
    ///
    /// THE POINT OF de-sr1u.3. Before it, a group that could take off anything worn unsettled
    /// every passive check in it, because nothing knew which skill a garment moved. The hat
    /// here moves Encyclopedia - `core::garment` says so, from what the database states - so a
    /// Logic check beside it keeps the answer the world gives.
    ///
    /// EACH CHECK NEEDS A STATED SKILL for this to narrow anything: a check the world states
    /// none for is unsettled whatever the garment moves, because nothing can tell whether it is
    /// reached. That is why both worlds below set a margin.
    #[test]
    fn a_garment_unsettles_only_the_checks_on_the_skill_it_moves() {
        let shape = || {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(r#"LoseItem("hat_mullen")"#)
                    .links(&[2]),
                Entry::new(2).kind(DialogueCheckKind::Passive).links(&[3]),
                Entry::new(3),
            ]
        };
        let wearing_it = |skill: &str, margin: i32| {
            GameWorld::blank()
                .set_equipped("HAT", "hat_mullen")
                .set_check_result(node(2), Ternary::False)
                .set_check_margin(node(2), skill, margin)
        };

        // PASSING BY NOTHING, which one point of Encyclopedia is enough to take away. A failing
        // check is the other question: losing a garment that HELPED cannot rescue it.
        assert!(
            !settled_check(shape(), &wearing_it("ENCYCLOPEDIA", 0)),
            "the hat is worth one Encyclopedia, so a check passing by nothing flips without it"
        );
        assert!(
            settled_check(shape(), &wearing_it("LOGIC", 0)),
            "the hat does not move Logic, so a Logic check keeps the world's answer"
        );
        // AND ITS REACH IS ONE. A check passing by two survives the hat coming off.
        assert!(
            settled_check(shape(), &wearing_it("ENCYCLOPEDIA", 2)),
            "one point of Encyclopedia cannot carry a check passing by two past its threshold"
        );
    }

    /// A passive check the world says fails is carried both ways where the group can take off
    /// something worn - and not where what it takes is not worn. What the check's own script
    /// raises is what shows it was entered.
    #[test]
    fn a_passive_check_whose_skill_can_move_is_undecided() {
        let shape = || {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(r#"LoseItem("hat_mullen")"#)
                    .links(&[2]),
                Entry::new(2)
                    .kind(DialogueCheckKind::Passive)
                    .script(r#"SetVariableValue("fired", true)"#)
                    .links(&[3]),
                Entry::new(3).guard(r#"Variable["fired"]"#),
            ]
        };
        let failing = || {
            GameWorld::blank()
                .set_variable("fired", GuardValue::from_boolean(false))
                .set_check_result(node(2), Ternary::False)
        };

        let wearing = failing().set_equipped("HAT", "hat_mullen");
        assert!(
            !settled_check(shape(), &wearing),
            "the group takes off a hat this world is wearing, so the check can move"
        );
        assert!(
            settled_check(shape(), &failing()),
            "nothing it takes off is worn, so nothing can move the check"
        );
    }

    /// A failing Volition check one short is carried both ways after a heal of one, and a check
    /// two short is not.
    #[test]
    fn healing_unsettles_a_passive_check_only_within_its_margin() {
        let shape = || {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1).script("HealVolition(1)").links(&[2]),
                Entry::new(2)
                    .kind(DialogueCheckKind::Passive)
                    .script(r#"SetVariableValue("fired", true)"#)
                    .links(&[3]),
                Entry::new(3).guard(r#"Variable["fired"]"#),
            ]
        };
        let short_by = |shortfall: i32| {
            GameWorld::blank()
                .set_variable("fired", GuardValue::from_boolean(false))
                .set_damage("VOLITION", -3.0)
                .set_check_result(node(2), Ternary::False)
                .set_check_margin(node(2), "VOLITION", -shortfall)
        };

        assert!(
            !settled_check(shape(), &short_by(1)),
            "one point of healing crosses a check short by one"
        );
        assert!(
            settled_check(shape(), &short_by(2)),
            "one point does not reach a check short by two"
        );
    }

    /// A failed Logic check pays out with Return on Investment fixed, and an Encyclopedia passive
    /// pays out on success with Trant Heidelstam fixed - each only while its thought is fixed.
    #[test]
    fn a_fixed_thought_pays_out_on_a_check_result() {
        let fixed = |world: GameWorld| world.set_fixed(crate::core::thought_effects::EVERY_THOUGHT);
        let purse = || GameWorld::blank().with_money(100);

        let failed_logic = || {
            vec![
                Entry::new(0).links(&[1]),
                white(Entry::new(1))
                    .flag("logic_check")
                    .field("SkillType", "0x0100000400000767")
                    .links(&[2]),
                Entry::new(2).cost(150),
            ]
        };
        assert!(!walked(failed_logic(), &purse()).reached(node(2)));
        assert!(walked(failed_logic(), &fixed(purse())).reached(node(2)));

        let encyclopedia = || {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .kind(DialogueCheckKind::Passive)
                    .field("Actor", "399")
                    .links(&[2]),
                Entry::new(2).cost(250),
            ]
        };
        assert!(!walked(encyclopedia(), &purse()).reached(node(2)));
        assert!(walked(encyclopedia(), &fixed(purse())).reached(node(2)));
    }

    /// A reputation action pays out or hurts only while its thought is fixed.
    ///
    /// Ultraliberal's hundred is what makes the second price affordable, and Revacholian
    /// Nationhood's blow is what opens the damage question.
    #[test]
    fn a_fixed_copotype_thought_adds_to_a_reputation_action() {
        let shape = |reputation: &str, reader: Entry| {
            vec![
                Entry::new(0).links(&[1]),
                Entry::new(1)
                    .script(&format!(r#"ReputationGrows("{reputation}")"#))
                    .links(&[2]),
                reader,
            ]
        };
        let fixed = |world: GameWorld| world.set_fixed(crate::core::thought_effects::EVERY_THOUGHT);

        let priced = || Entry::new(2).cost(150);
        let purse = || GameWorld::blank().with_money(100);
        assert!(!walked(shape("ultraliberal", priced()), &purse()).reached(node(2)));
        assert!(walked(shape("ultraliberal", priced()), &fixed(purse())).reached(node(2)));

        let hurt = || Entry::new(2).guard("HasVolitionDamage()");
        let whole = || GameWorld::blank().set_damage("VOLITION", 0.0);
        assert!(!walked(shape("revacholian_nationhood", hurt()), &whole()).reached(node(2)));
        assert!(walked(shape("revacholian_nationhood", hurt()), &fixed(whole())).reached(node(2)));
    }

    #[test]
    fn a_false_guard_closes_an_entry() {
        let world = GameWorld::blank().set_variable("shut", GuardValue::from_boolean(false));
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
            &GameWorld::blank(),
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
            &GameWorld::blank(),
        );
        assert!(walk.reached(node(2)));
    }

    #[test]
    fn a_price_the_purse_cannot_meet_closes_an_option() {
        let world = GameWorld::blank().with_money(5);
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
        let world = GameWorld::blank().with_money(10);
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
        let world = GameWorld::blank().set_variable("count", GuardValue::from_number(0.0));
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
        let world = GameWorld::blank().set_variable("count", GuardValue::from_number(0.0));
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
            &GameWorld::blank(),
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
            &GameWorld::blank(),
        );
        assert!(walk.reached(node(1)), "a group is walked through");
        let unseen = |id: DialogueNodeId| {
            if id == node(1) {
                SeenState::UnseenAnyGame
            } else {
                SeenState::SeenThisGame
            }
        };
        assert_eq!(
            walk.best_novelty(unseen),
            SeenState::SeenThisGame,
            "and never scored",
        );
    }

    #[test]
    fn the_start_is_scored_only_where_a_link_leads_back_to_it() {
        let unseen = |id: DialogueNodeId| {
            if id == node(0) {
                SeenState::UnseenAnyGame
            } else {
                SeenState::SeenThisGame
            }
        };

        let onward = walked(
            vec![Entry::new(0).links(&[1]), Entry::new(1)],
            &GameWorld::blank(),
        );
        assert_eq!(onward.best_novelty(unseen), SeenState::SeenThisGame);

        let loops_back = walked(
            vec![Entry::new(0).links(&[1]), Entry::new(1).links(&[0])],
            &GameWorld::blank(),
        );
        assert_eq!(loops_back.best_novelty(unseen), SeenState::UnseenAnyGame);
    }

    #[test]
    fn a_rolled_start_can_be_asked_about_one_outcome() {
        let graph = GraphBuilder::new()
            .add(white(Entry::new(0)).flag("roll").links(&[1, 2]))
            .add(Entry::new(1).guard(r#"Variable["roll"]"#))
            .add(Entry::new(2).guard(r#"Variable["roll_failed"]"#))
            .build();
        let world = GameWorld::blank();

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

    /// A once-only effect on an entry the save has already shown does not fire again.
    ///
    /// 1 raises a counter once, and 2 opens only on it. Unseen, the raise happens and 2 is
    /// reachable; seen, the game's `Once` returns 0 and 2 stays shut.
    #[test]
    fn a_once_effect_on_a_shown_entry_does_not_fire() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(
                Entry::new(1)
                    .script(r#"SetVariableValue("raised", Variable["raised"] + once(1))"#)
                    .links(&[2]),
            )
            .add(Entry::new(2).guard(r#"Variable["raised"] >= 1"#))
            .build();

        // A NUMBER, as the database declares a counter: an unread variable reads as a flag,
        // and a flag compared against a number is undecided, which would open 2 either way.
        let counter = || GameWorld::blank().set_variable("raised", GuardValue::from_number(0.0));

        let unseen = walk(&graph, node(0), &counter(), COUNTER_CAP);
        assert!(
            unseen.reached(node(2)),
            "an unshown entry's once effect fires"
        );

        let shown = counter().set_seen(node(1), true);
        let seen = walk(&graph, node(0), &shown, COUNTER_CAP);
        assert!(
            !seen.reached(node(2)),
            "a shown entry's once effect has already fired"
        );
    }

    /// Damage taken on the way opens what asks whether morale is damaged; healing it closes
    /// that again; and damage the save already carries is where the count starts.
    #[test]
    fn damage_and_healing_move_the_damage_question() {
        let shape = |script: &str| {
            GraphBuilder::new()
                .add(Entry::new(0).links(&[1]))
                .add(Entry::new(1).script(script).links(&[2]))
                .add(Entry::new(2).guard("HasVolitionDamage()"))
                .build()
        };
        let whole = GameWorld::blank().set_damage("VOLITION", 0.0);

        let hurt = walk(&shape("DamageVolition(1)"), node(0), &whole, COUNTER_CAP);
        assert!(hurt.reached(node(2)), "a blow damages");

        let healed = walk(
            &shape(r#"DamageVolition(1);\nHealVolition(1)"#),
            node(0),
            &whole,
            COUNTER_CAP,
        );
        assert!(
            !healed.reached(node(2)),
            "a heal as large as the blow undoes it"
        );

        let already = GameWorld::blank().set_damage("VOLITION", -3.0);
        let partly = walk(&shape("HealVolition(2)"), node(0), &already, COUNTER_CAP);
        assert!(
            partly.reached(node(2)),
            "three damage healed by two leaves one"
        );
    }

    /// Leaving Kim at the church shuts what needs Kim present and opens what needs Kim gone -
    /// and a Kim the world could not read stays unknown until then.
    #[test]
    fn leaving_kim_at_the_church_moves_the_kim_questions() {
        let shape = |script: &str, guard: &str| {
            GraphBuilder::new()
                .add(Entry::new(0).links(&[1]))
                .add(Entry::new(1).script(script).links(&[2]))
                .add(Entry::new(2).guard(guard))
                .build()
        };
        let with_kim = GameWorld::blank()
            .set_query_bool("IsKimHere", true)
            .set_query_bool("IsKimInParty", true);
        let left = "RemoveKitsuragiWaitAtChurch()";

        let kept = walk(&shape("", "IsKimHere()"), node(0), &with_kim, COUNTER_CAP);
        assert!(kept.reached(node(2)), "Kim is here until left somewhere");

        let gone = walk(&shape(left, "IsKimHere()"), node(0), &with_kim, COUNTER_CAP);
        assert!(
            !gone.reached(node(2)),
            "a Kim left at the church is not here"
        );

        let out = walk(
            &shape(left, "not IsKimInParty()"),
            node(0),
            &with_kim,
            COUNTER_CAP,
        );
        assert!(out.reached(node(2)), "nor in the party");
    }

    /// Losing a worn item takes it off: what asks for it, or for anything in its slot, shuts -
    /// and losing something else leaves both alone.
    #[test]
    fn losing_a_worn_item_takes_it_off() {
        let shape = |script: &str, guard: &str| {
            GraphBuilder::new()
                .add(Entry::new(0).links(&[1]))
                .add(Entry::new(1).script(script).links(&[2]))
                .add(Entry::new(2).guard(guard))
                .build()
        };
        let dressed = GameWorld::blank()
            .set_equipped("SHIRT", "shirt_x")
            .set_query_bool("HasShirt", true)
            .set_query_bool("CheckEquipped", true);
        let reached = |script: &str, guard: &str| {
            walk(&shape(script, guard), node(0), &dressed, COUNTER_CAP).reached(node(2))
        };

        assert!(reached(r#"LoseItem("hat_y")"#, "HasShirt()"));
        assert!(!reached(r#"LoseItem("shirt_x")"#, "HasShirt()"));
        assert!(!reached(
            r#"LoseItem("shirt_x")"#,
            r#"CheckEquipped("shirt_x")"#
        ));
        assert!(reached(r#"LoseItem("shirt_x")"#, "not HasShirt()"));
    }

    /// A deadline set from the clock is not already past.
    ///
    /// 1 sets the deadline eight hours ahead and 2 opens once it has passed. The clock does not
    /// move in a crawl, so 2 stays shut - a deadline read as 1 would have opened it.
    #[test]
    fn a_deadline_set_from_the_clock_has_not_passed() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(
                Entry::new(1)
                    .script(r#"SetVariableValue("deadline", TotalHourCount() + 8)"#)
                    .links(&[2]),
            )
            .add(Entry::new(2).guard(r#"TotalHourCount() >= Variable["deadline"]"#))
            .build();
        let world = GameWorld::blank()
            .with_day_counter(2)
            .with_day_minutes(10 * 60)
            .set_variable("deadline", GuardValue::from_number(0.0));

        let walk = walk(&graph, node(0), &world, COUNTER_CAP);
        assert!(
            !walk.reached(node(2)),
            "the deadline is hour 42 and it is hour 34"
        );
    }

    /// A once-only price on an entry the save has shown is not charged again.
    #[test]
    fn a_once_price_on_a_shown_entry_is_not_charged() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).cost(5).cost_once().links(&[2]))
            .add(Entry::new(2).cost(5).links(&[3]))
            .add(Entry::new(3))
            .build();

        let unseen = walk(
            &graph,
            node(0),
            &GameWorld::blank().with_money(5),
            COUNTER_CAP,
        );
        assert!(!unseen.reached(node(3)), "paying both prices needs ten");

        let shown = GameWorld::blank().with_money(5).set_seen(node(1), true);
        let seen = walk(&graph, node(0), &shown, COUNTER_CAP);
        assert!(
            seen.reached(node(3)),
            "the first price was paid on the earlier visit"
        );
    }

    #[test]
    fn a_check_already_resolved_is_closed() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(white(Entry::new(1)).flag("roll").links(&[2]))
            .add(Entry::new(2))
            .build();
        // The roll has already been made and lost, so the check cannot be entered at all.
        let world = GameWorld::blank().set_variable("roll_failed", GuardValue::from_boolean(true));
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
        let world = GameWorld::blank().with_red_checks_failing(true);

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
        let world = GameWorld::blank().set_variable("count", GuardValue::from_number(0.0));
        let walk = walk_branch(&graph, node(0), StartBranch::Either, &world, 1 << 20, 64);
        assert!(walk.exhausted(), "a walk that ran out of room must say so");
    }
}
