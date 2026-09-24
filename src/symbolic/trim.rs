// SPDX-License-Identifier: MIT
//! The group as one request can walk it: entries whose guard holds in no state closed, and
//! what the starts can then no longer reach cut off.
//!
//! ## What it is for
//!
//! The searches decide guards state by state, so an entry whose guard can never hold is
//! refused by every search already. What does not refuse it is everything that walks the
//! LINKS and ignores guards to stay cheap: the parent map and iteration order in
//! [`super::known::GroupShape`], the hub candidates and the "can get back" components in
//! [`super::hub`], the choice-distance bound in [`super::seen_state_search::choice_bounds`], the
//! link walk that settles options before a pass in [`crate::graph::LookAheadGraph::best_linked_class`],
//! and the dominator tree. Each is sound because ignoring guards only ever keeps routes, and
//! each is loose for the same reason: a route through a door the world keeps shut still counts.
//!
//! So this closes those doors once, for the request, and every walk that reads the links gets
//! the tighter graph without learning anything about guards.
//!
//! ## A copy with links taken away, not a different graph
//!
//! The nodes, their actions and the symbol table are the group's, unchanged, so the layout, the
//! manager and the compiled guards built from the whole group line up with it entry for entry.
//! Only links go: every link into a closed entry, and every link out of an entry the starts can
//! no longer reach - which also takes those entries out of every parent list, so a backward
//! walk does not step onto them either.
//!
//! ## What closes an entry
//!
//! An ordinary entry whose compiled guard may hold in no state the request can be in. Not only
//! the one the search starts from, since a write on the way can open a guard the seed does not
//! satisfy - but a slot nothing on the way CAN change is pinned where it starts, and a guard that
//! needs it elsewhere is shut. See [`pinned`] for which slots those are.
//!
//! NOT A CHECK BY ITS GUARD. A passive check whose condition fails is stepped over onto its
//! links rather than refusing them, and a rolled check's guard is what offers it; neither is a
//! door in the sense this needs. A rolled check whose roll is RECORDED for good is closed,
//! though - see [`recorded_for_good`].
//!
//! NOT A START. The menu was composed from this world, so a start's guard held when it was
//! offered; a compiled guard that says otherwise is a gap in the compiler, and closing a start
//! on it would draw a menu the player is looking at as unreachable.
//!
//! ## To a fixed point
//!
//! Closing entries removes the writes behind them, so a slot one round could still change can be
//! pinned the next, and that can close more. So the walk repeats, each round pinning what the
//! entries the last one reached cannot change, until the reachable set stops shrinking. It only
//! ever shrinks: a round's writers are the last round's reachable entries, fewer writers pin more
//! slots, and more pins close more guards.
//!
//! PINNED IN THE DIAGRAM, NOT DROPPED FROM THE LAYOUT. Dropping a pinned slot and recompiling
//! over the smaller layout would say the same thing, and would rebuild the manager the
//! conversation's workspace keeps - see `DataLayout::for_group_entered_at`. A conjunction of
//! slot equalities asks the same question of the guards already compiled.

use std::collections::{HashMap, HashSet, VecDeque};

use oxidd::BooleanFunction;
use oxidd::bdd::BDDFunction;

use super::guard_formula::GuardCompiler;
use super::reachability::seed_slot_value;
use crate::core::action::DialogueActionKind;
use crate::core::types::{DialogueCheckKind, DialogueNodeId, SeenState};
use crate::graph::LookAheadGraph;
use crate::world::ILookAheadWorld;

/// A request's view of its group.
pub struct Trimmed {
    /// The group with the links this request cannot take removed.
    pub graph: LookAheadGraph,
    /// Every entry the starts can still reach, closed entries not among them.
    ///
    /// WHAT THERE IS TO FIND. An unread entry outside it is not a target: nothing the request
    /// can do arrives there, so a seen state that counted it would hand every link walk a class
    /// no search can meet. See [`Self::seen_state`].
    pub reachable: HashSet<DialogueNodeId>,
    /// How many entries were closed by their guard.
    pub closed: usize,
    /// How many entries the starts could reach before closing and cannot after, closed
    /// entries not counted.
    pub cut_off: usize,
}

impl Trimmed {
    /// `seen_state`, with every entry the request cannot reach read as already seen.
    pub fn seen_state<'a, F: Fn(DialogueNodeId) -> SeenState + 'a>(
        &'a self,
        seen_state: F,
    ) -> impl Fn(DialogueNodeId) -> SeenState + 'a {
        move |id| {
            if self.reachable.contains(&id) {
                seen_state(id)
            } else {
                SeenState::SeenThisGame
            }
        }
    }
}

/// The group as a request starting at `starts` can walk it, in `world`. See the module doc.
pub fn trimmed(
    graph: &LookAheadGraph,
    compiler: &mut GuardCompiler<'_>,
    world: &dyn ILookAheadWorld,
    starts: &[DialogueNodeId],
) -> Trimmed {
    let before = reached(graph, starts);
    let state = crate::core::state::seed_state(graph, world);

    // THE FIRST ROUND'S WRITERS are everything the links reach, which can only over-count.
    let mut writers = before.clone();
    let (reachable, closed) = loop {
        let pins = pinned(graph, compiler, &state, &writers, starts);
        let (reachable, closed) = walk(graph, compiler, starts, &pins);
        if reachable.len() == writers.len() {
            break (reachable, closed);
        }
        writers = reachable;
    };

    let mut walkable = graph.clone();
    let ids: Vec<DialogueNodeId> = graph.nodes().map(|node| node.id).collect();
    for id in ids {
        let node = walkable.get_mut(id).expect("the id came from this graph");
        if reachable.contains(&id) {
            node.links.retain(|child| !closed.contains(child));
        } else {
            node.links.clear();
        }
    }

    let cut_off = before
        .iter()
        .filter(|id| !reachable.contains(id) && !closed.contains(id))
        .count();
    Trimmed {
        graph: walkable,
        reachable,
        closed: closed.len(),
        cut_off,
    }
}

/// One round: the entries the starts reach with every door that cannot open under `pins`
/// closed, and the entries closed.
fn walk(
    graph: &LookAheadGraph,
    compiler: &mut GuardCompiler<'_>,
    starts: &[DialogueNodeId],
    pins: &Pins,
) -> (HashSet<DialogueNodeId>, HashSet<DialogueNodeId>) {
    let mut closed = HashSet::new();
    let mut reachable = HashSet::new();
    let mut pending: VecDeque<DialogueNodeId> = starts.iter().copied().collect();
    while let Some(id) = pending.pop_front() {
        if !reachable.insert(id) {
            continue;
        }
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if reachable.contains(&child) || closed.contains(&child) {
                continue;
            }
            let Some(entry) = graph.get(child) else {
                continue;
            };
            if !starts.contains(&child)
                && ((entry.kind == DialogueCheckKind::None
                    && holds_nowhere(compiler, child, &entry.guard, &pins.set))
                    || recorded_for_good(entry, pins))
            {
                closed.insert(child);
                continue;
            }
            pending.push_back(child);
        }
    }
    (reachable, closed)
}

/// Whether a rolled check's roll is recorded and nothing can unrecord it.
///
/// A CHECK ALREADY ROLLED IS CLOSED, passed or failed - neither kind can be retried once the
/// roll is recorded, which is how the searches treat it (`Reachability::rolled_cases`). A flag
/// pinned set is one recorded on every route, so the check can never be entered: the failed
/// white check the game keeps locked, most often. Its guard is not asked, which is why this is
/// the one kind of check the trim closes.
fn recorded_for_good(entry: &crate::graph::node::LookAheadNode, pins: &Pins) -> bool {
    entry.is_rolled()
        && (pins.set_for_good(entry.flag_slot) || pins.set_for_good(entry.failed_flag_slot))
}

/// Every slot no entry of `writers` can change, held at its starting value, as one set.
///
/// A SLOT IS PINNED WHERE NO WRITE CAN MOVE IT: nothing in `writers` writes it, or every write
/// there leaves it where it starts - an assignment of the value it already holds, or entering
/// an entry whose flag is already set. Only a write can change a slot, so one no write can
/// change holds its starting value on every route, which is what makes pinning sound.
///
/// NOT A START'S LOCK. A locked option is answered with its locks lifted - see
/// `bridge::answer_starts` - and its failure slot is what a lifted failed check reads; pinning
/// it here would close what the lifted search has to walk.
///
/// THE WHOLE SET WHERE THE MANAGER HAS NO ROOM for the conjunction, which pins nothing and so
/// closes only what the unpinned guards close. The values stand either way: they are facts
/// about the writes, not about the manager.
fn pinned(
    graph: &LookAheadGraph,
    compiler: &GuardCompiler<'_>,
    state: &crate::core::state::LookAheadState,
    writers: &HashSet<DialogueNodeId>,
    starts: &[DialogueNodeId],
) -> Pins {
    let vars = compiler.vars();
    let start_value = |slot: usize| seed_slot_value(vars, state, slot);

    let mut movable = HashSet::new();
    for node in writers.iter().filter_map(|id| graph.get(*id)) {
        for action in node.all_actions() {
            let Ok(slot) = usize::try_from(action.slot()) else {
                continue;
            };
            let moves = match action.kind() {
                DialogueActionKind::Assign => u32::try_from(action.value())
                    .map_or(true, |value| Some(value) != start_value(slot)),
                DialogueActionKind::Increment => action.value() != 0,
                _ => true,
            };
            if moves {
                movable.insert(slot);
            }
        }
        // ENTERING SETS THESE, to one.
        for slot in [
            node.seen_slot,
            node.once_slot,
            node.flag_slot,
            node.failed_flag_slot,
        ] {
            if let Ok(slot) = usize::try_from(slot)
                && start_value(slot) != Some(1)
            {
                movable.insert(slot);
            }
        }
    }
    for start in starts.iter().filter_map(|id| graph.get(*id)) {
        if let Ok(slot) = usize::try_from(start.failed_flag_slot) {
            movable.insert(slot);
        }
    }

    let values: HashMap<usize, u32> = (0..vars.layout().slot_count())
        .filter(|slot| !movable.contains(slot))
        .filter_map(|slot| Some((slot, start_value(slot)?)))
        .collect();
    let mut set = vars.top();
    for (&slot, &value) in &values {
        match vars
            .slot_equals(slot, value)
            .and_then(|holds| set.and(&holds).ok())
        {
            Some(pinned) => set = pinned,
            None => {
                set = vars.top();
                break;
            }
        }
    }
    Pins { set, values }
}

/// What a round pins: the slots no write can move, and the set of states that holds them.
struct Pins {
    /// Every state with each pinned slot at its value.
    set: BDDFunction,
    /// Each pinned slot's value.
    values: HashMap<usize, u32>,
}

impl Pins {
    /// Whether `slot` is pinned set, so an entry it records has been passed through for good.
    fn set_for_good(&self, slot: i32) -> bool {
        usize::try_from(slot).is_ok_and(|slot| self.values.get(&slot) == Some(&1))
    }
}

/// Whether a guard holds in no state `pins` allows.
///
/// A REPUTATION QUESTION IS COMPILED WITHOUT BEING KEPT. The compiler settles reputation ranges
/// on the trimmed graph, after this has run, and a guard kept from before would keep its
/// per-state answer after its range settled.
///
/// A CONJUNCTION THE MANAGER HAS NO ROOM FOR SAYS THE GUARD MAY HOLD, which keeps the entry: an
/// entry kept open is only a looser trim, where one closed wrongly would be a wrong answer.
fn holds_nowhere(
    compiler: &mut GuardCompiler<'_>,
    id: DialogueNodeId,
    guard: &crate::core::guard::Guard,
    pins: &BDDFunction,
) -> bool {
    let compiled = if GuardCompiler::asks_reputation(guard) {
        compiler.compile(guard)
    } else {
        compiler.compile_for(id, guard)
    };
    compiled
        .may_be_true
        .and(pins)
        .is_ok_and(|held| !held.satisfiable())
}

/// Every entry the links reach from `starts`.
fn reached(graph: &LookAheadGraph, starts: &[DialogueNodeId]) -> HashSet<DialogueNodeId> {
    let mut seen = HashSet::new();
    let mut pending: VecDeque<DialogueNodeId> = starts.iter().copied().collect();
    while let Some(id) = pending.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(node) = graph.get(id) {
            pending.extend(node.links.iter().copied());
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::guard_value::GuardValue;
    use crate::symbolic::budget::DiagramBudget;
    use crate::symbolic::data_layout::DataLayout;
    use crate::symbolic::vars::DataVars;
    use crate::test_graph::{Entry, GraphBuilder, node};
    use crate::world::GameWorld;

    const DOOR: &str = r#"Variable["door"] == true"#;

    /// The group trimmed from entry 0, in a world where the door variable is shut.
    fn trim(graph: &LookAheadGraph) -> Trimmed {
        trim_in(
            graph,
            &GameWorld::blank().set_variable("door", GuardValue::from_boolean(false)),
        )
    }

    /// The group trimmed from entry 0, in `world`.
    fn trim_in(graph: &LookAheadGraph, world: &GameWorld) -> Trimmed {
        let layout = DataLayout::for_graph(graph, 16, None, false);
        let vars = DataVars::new(&layout, graph.symbols(), DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(world);
        trimmed(graph, &mut compiler, world, &[node(0)])
    }

    /// A door whose only write leaves it where it already is stays shut: the write cannot open
    /// it, so the slot is pinned and the guard asks for a value it never has.
    #[test]
    fn a_door_a_write_cannot_move_closes() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(
                Entry::new(1)
                    .guard(r#"Variable["door"] == false"#)
                    .links(&[3]),
            )
            .add(
                Entry::new(2)
                    .script(r#"SetVariableValue("door", true)"#)
                    .links(&[1]),
            )
            .add(Entry::new(3))
            .build();
        let world = GameWorld::blank().set_variable("door", GuardValue::from_boolean(true));
        let trimmed = trim_in(&graph, &world);

        assert_eq!(
            trimmed.closed, 1,
            "only ever set to what it holds, so never false"
        );
        assert!(!trimmed.reachable.contains(&node(3)));
    }

    /// A write behind a door that closes is no write at all, so the door it would have opened
    /// closes on the next round.
    #[test]
    fn a_door_whose_only_opener_is_shut_closes_too() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1).guard(DOOR).links(&[3]))
            .add(
                Entry::new(2)
                    .guard(r#"Variable["latch"] == true"#)
                    .links(&[4]),
            )
            .add(Entry::new(3))
            .add(
                Entry::new(4)
                    .script(r#"SetVariableValue("door", true)"#)
                    .links(&[1]),
            )
            .build();
        let world = GameWorld::blank()
            .set_variable("door", GuardValue::from_boolean(false))
            .set_variable("latch", GuardValue::from_boolean(false));
        let trimmed = trim_in(&graph, &world);

        assert_eq!(
            trimmed.closed, 2,
            "the latch shuts 2, and with 4 gone nothing opens 1"
        );
        assert!(!trimmed.reachable.contains(&node(3)));
        assert!(!trimmed.reachable.contains(&node(4)));
    }

    /// A door the world keeps shut and nothing opens closes, and what only it led to is cut
    /// off: its links go, and it is no longer something to find.
    #[test]
    fn a_door_nothing_opens_closes_and_cuts_off_what_is_behind_it() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1).guard(DOOR).links(&[3]))
            .add(Entry::new(2))
            .add(Entry::new(3).links(&[0]))
            .build();
        let trimmed = trim(&graph);

        assert_eq!(trimmed.closed, 1);
        assert_eq!(trimmed.cut_off, 1, "3 is reached only through the door");
        assert_eq!(trimmed.graph.get(node(0)).unwrap().links, vec![node(2)]);
        assert!(trimmed.graph.get(node(3)).unwrap().links.is_empty());
        let seen_state = trimmed.seen_state(|_| SeenState::UnseenAnyGame);
        assert_eq!(seen_state(node(3)), SeenState::SeenThisGame);
        assert_eq!(seen_state(node(2)), SeenState::UnseenAnyGame);
    }

    /// A door something on the way can open is not closed, though the world has it shut.
    #[test]
    fn a_door_a_write_can_open_stays_open() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1).guard(DOOR).links(&[3]))
            .add(
                Entry::new(2)
                    .script(r#"SetVariableValue("door", true)"#)
                    .links(&[1]),
            )
            .add(Entry::new(3))
            .build();
        let trimmed = trim(&graph);

        assert_eq!(trimmed.closed, 0);
        assert!(trimmed.reachable.contains(&node(3)));
    }

    /// A white check the save has already failed, and nothing can unfail, is closed, and what
    /// only it led to is cut off; the same check unfailed stays open.
    #[test]
    fn a_check_failed_for_good_closes() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 3]))
            .add(
                Entry::new(1)
                    .kind(DialogueCheckKind::White)
                    .flag("wc")
                    .links(&[2]),
            )
            .add(Entry::new(2))
            .add(Entry::new(3))
            .build();
        let failed = GameWorld::blank().set_variable("wc_failed", GuardValue::from_boolean(true));
        let open = GameWorld::blank().set_variable("wc_failed", GuardValue::from_boolean(false));

        let trimmed = trim_in(&graph, &failed);
        assert_eq!(trimmed.closed, 1);
        assert!(!trimmed.reachable.contains(&node(2)));

        let trimmed = trim_in(&graph, &open);
        assert_eq!(trimmed.closed, 0);
        assert!(trimmed.reachable.contains(&node(2)));
    }

    /// A check whose guard cannot hold is stepped over rather than refused, so it closes
    /// nothing.
    #[test]
    fn a_check_never_closes_on_its_guard() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(
                Entry::new(1)
                    .guard(DOOR)
                    .kind(DialogueCheckKind::Passive)
                    .links(&[2]),
            )
            .add(Entry::new(2))
            .build();
        let trimmed = trim(&graph);

        assert_eq!(trimmed.closed, 0);
        assert!(trimmed.reachable.contains(&node(2)));
    }

    /// A start is never closed: the menu was offered from this world.
    #[test]
    fn a_start_never_closes() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).guard(DOOR).links(&[1]))
            .add(Entry::new(1))
            .build();
        let trimmed = trim(&graph);

        assert_eq!(trimmed.closed, 0);
        assert!(trimmed.reachable.contains(&node(1)));
    }
}
