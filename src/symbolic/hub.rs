// SPDX-License-Identifier: MIT
//! Which hubs the player is inside, read off the links as they walk.
//!
//! ## Hubs, and the stack of them
//!
//! Many of the game's conversations are written around pass-through entries the player returns
//! to between topics - Garte's "garte main HUB", and under it "whirlinghub" for the Whirling's
//! own topics. Such an entry is a GROUP entry, which the game never displays, with several
//! entries linking back to it. Those are the CANDIDATES: group entries with at least two
//! distinct parents.
//!
//! A walk always begins at the start of a conversation. The player is inside a STACK of hubs,
//! outermost first, each holding what was passed since the player last arrived at it:
//!
//! - passing a candidate with nothing on the stack puts it there;
//! - passing a candidate inside the same loop pushes it on top - a topic's sub-hub;
//! - arriving back at a hub already on the stack drops everything above it and starts its own
//!   list over, since everything passed after it can be walked again from it;
//! - passing a point of no return - anywhere the stack's hubs can no longer be reached from -
//!   empties the stack.
//!
//! What the player has passed, for the purpose of the cut, is everything still on the stack.
//!
//! ## Why a stack and not one hub
//!
//! With one hub, everything since it stays behind the player until they return to it. Take a
//! sub-hub B under a hub A, with a door off B leading to a menu of its own and an unread line
//! there. A player who went through the door once, backed out to B, and is at B's menu again
//! has the door's whole route in the list - so the door loses its star, though choosing it
//! still reaches the unread line. Arriving back at B has to give B's topics back, while the
//! route from A down to B stays behind the player; that is what the stack keeps apart.
//!
//! ## Why "can get back" is a lookup
//!
//! The player reached where they are FROM the hubs on the stack, so they can return to them
//! along links exactly when their position and the hubs can each reach the other - when all lie
//! in one strongly connected component of the link graph. A hub is only pushed from inside the
//! component of the one below it, so the whole stack shares one component, and [`IterationOrder`]
//! already decomposes the group that way for the searches: the test is a comparison of two
//! component numbers.
//!
//! GUARDS ARE IGNORED, which is what keeps it cheap and is its one known way to be wrong: a way
//! back that a variable closes still counts as a way back, so the stack stays and the cut keeps
//! entries the player could not in fact return through.
//!
//! ## How well it agrees with the names
//!
//! `tests/main_hub.rs` walks the shortest route from each conversation's start to every entry
//! a writer titled as its main hub, and reports whether that hub is the outermost on arrival.
//! Measured 2026-09-14 over the full index:
//!
//! ```text
//!   titled main hubs                          95
//!   titled hubs that are candidates           95
//!   outermost on arrival                      92
//!   conversations holding a candidate        445 of 1,501
//! ```
//!
//! In the other three - 35, 368 and 676 - the route in passes one other candidate inside the
//! main loop first, so the titled hub sits on top of it rather than at the bottom.

use std::collections::{HashMap, HashSet, VecDeque};

use super::order::IterationOrder;
use crate::core::types::DialogueNodeId;
use crate::graph::LookAheadGraph;

/// The fewest distinct back-links a hub has. One is a join, not a hub.
const MIN_BACK_LINKS: usize = 2;

/// Which of a group's entries could be a hub.
pub struct Hubs {
    candidates: HashSet<DialogueNodeId>,
}

impl Hubs {
    /// Works out the candidates, once for a group.
    pub fn of(graph: &LookAheadGraph) -> Self {
        let mut back_links: HashMap<DialogueNodeId, usize> = HashMap::new();
        for node in graph.nodes() {
            // DISTINCT parents: an entry that links to the same child twice is one route there.
            let children: HashSet<DialogueNodeId> = node.links.iter().copied().collect();
            for child in children {
                *back_links.entry(child).or_default() += 1;
            }
        }
        let candidates = back_links
            .into_iter()
            .filter(|(id, count)| {
                *count >= MIN_BACK_LINKS && graph.get(*id).is_some_and(|node| node.is_group)
            })
            .map(|(id, _)| id)
            .collect();
        Self { candidates }
    }

    /// Every group entry with at least [`MIN_BACK_LINKS`] distinct parents.
    pub fn candidates(&self) -> &HashSet<DialogueNodeId> {
        &self.candidates
    }

    pub fn is_candidate(&self, id: DialogueNodeId) -> bool {
        self.candidates.contains(&id)
    }
}

/// One hub on the stack, and what was passed since the player last arrived at it.
#[derive(Debug, PartialEq, Eq)]
struct Frame {
    hub: DialogueNodeId,
    /// The hub first, then everything passed after it until the next hub was pushed.
    passed: Vec<DialogueNodeId>,
}

impl Frame {
    fn at(hub: DialogueNodeId) -> Self {
        Self {
            hub,
            passed: vec![hub],
        }
    }
}

/// The hubs a walk is inside, outermost first, and what was passed since each.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct HubStack {
    frames: Vec<Frame>,
}

impl HubStack {
    /// The hubs, outermost first.
    pub fn hubs(&self) -> Vec<DialogueNodeId> {
        self.frames.iter().map(|frame| frame.hub).collect()
    }

    /// The hub the whole stack sits under, if there is one.
    pub fn outermost(&self) -> Option<DialogueNodeId> {
        self.frames.first().map(|frame| frame.hub)
    }

    /// The hub the player most recently arrived at, if there is one.
    pub fn innermost(&self) -> Option<DialogueNodeId> {
        self.frames.last().map(|frame| frame.hub)
    }

    /// Everything still on the stack: what the player has passed and would be walking back
    /// through. Empty while there is no hub.
    pub fn since(&self) -> HashSet<DialogueNodeId> {
        self.frames
            .iter()
            .flat_map(|frame| frame.passed.iter().copied())
            .collect()
    }
}

/// Follows a walk from the start of a conversation, oldest entry first, and says which hubs it
/// is inside at its end.
///
/// `passed` is every entry walked through, group entries included - see [`passage`] for how a
/// request's shown entries become one.
pub fn follow(order: &IterationOrder, hubs: &Hubs, passed: &[DialogueNodeId]) -> HubStack {
    let mut stack = HubStack::default();
    for &entry in passed {
        if let Some(outermost) = stack.outermost() {
            if order.component_of(entry) != order.component_of(outermost) {
                // A POINT OF NO RETURN. The whole stack shares one component, so none of it can
                // be reached from here.
                stack.frames.clear();
            } else if let Some(at) = stack.frames.iter().position(|frame| frame.hub == entry) {
                // BACK AT A HUB: what was passed after it can be walked again from it.
                stack.frames.truncate(at);
                stack.frames.push(Frame::at(entry));
                continue;
            } else if hubs.is_candidate(entry) {
                stack.frames.push(Frame::at(entry));
                continue;
            } else {
                if let Some(top) = stack.frames.last_mut() {
                    top.passed.push(entry);
                }
                continue;
            }
        }
        if hubs.is_candidate(entry) {
            stack.frames.push(Frame::at(entry));
        }
    }
    stack
}

/// What a request's walk passed through, in order: each shown entry, then the group entries
/// routed through on the way to the next one, and last those on the way to the menu.
///
/// `encountered` is what the conversation has shown since it started, oldest first - lines
/// displayed and options chosen - and `menu` is the options now on offer. Group entries are
/// never shown, so they are recovered from the LINKS between one shown entry and the next.
pub fn passage(
    graph: &LookAheadGraph,
    encountered: &[DialogueNodeId],
    menu: &[DialogueNodeId],
) -> Vec<DialogueNodeId> {
    let mut walked = Vec::new();
    for (at, &from) in encountered.iter().enumerate() {
        walked.push(from);
        let next = match encountered.get(at + 1) {
            Some(next) => std::slice::from_ref(next),
            None => menu,
        };
        walked.extend(passed_between(graph, from, next));
    }
    walked
}

/// Everything on the hub stack at the end of a request's walk.
///
/// Empty where no hub is on it, which is also the answer in a group with no candidates.
pub fn since_current_hub(
    graph: &LookAheadGraph,
    order: &IterationOrder,
    hubs: &Hubs,
    encountered: &[DialogueNodeId],
    menu: &[DialogueNodeId],
) -> HashSet<DialogueNodeId> {
    follow(order, hubs, &passage(graph, encountered, menu)).since()
}

/// The group entries a route from `from` to any of `to` passes through, group entries alone,
/// nearest to `from` first.
fn passed_between(
    graph: &LookAheadGraph,
    from: DialogueNodeId,
    to: &[DialogueNodeId],
) -> Vec<DialogueNodeId> {
    let is_group = |id: &DialogueNodeId| graph.get(*id).is_some_and(|node| node.is_group);
    let links = |id: DialogueNodeId| {
        graph
            .get(id)
            .map(|node| node.links.as_slice())
            .unwrap_or_default()
    };

    let mut reached = HashSet::new();
    let mut pending: Vec<DialogueNodeId> = links(from).iter().copied().filter(is_group).collect();
    while let Some(id) = pending.pop() {
        if reached.insert(id) {
            pending.extend(links(id).iter().copied().filter(is_group));
        }
    }

    // BACK FROM THE DESTINATION, so a group entry the step could have wandered into and out of
    // again is not counted as passed.
    let targets: HashSet<DialogueNodeId> = to.iter().copied().collect();
    let mut on_route: HashSet<DialogueNodeId> = reached
        .iter()
        .copied()
        .filter(|id| links(*id).iter().any(|child| targets.contains(child)))
        .collect();
    loop {
        let more: Vec<DialogueNodeId> = reached
            .iter()
            .copied()
            .filter(|id| !on_route.contains(id))
            .filter(|id| links(*id).iter().any(|child| on_route.contains(child)))
            .collect();
        if more.is_empty() {
            break;
        }
        on_route.extend(more);
    }

    // IN THE ORDER THEY ARE WALKED, because which candidate comes first decides the stack.
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();
    let mut queue: VecDeque<DialogueNodeId> = links(from)
        .iter()
        .copied()
        .filter(|id| on_route.contains(id))
        .collect();
    while let Some(id) = queue.pop_front() {
        if seen.insert(id) {
            ordered.push(id);
            queue.extend(
                links(id)
                    .iter()
                    .copied()
                    .filter(|child| on_route.contains(child)),
            );
        }
    }
    ordered
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::test_graph::{Entry, GraphBuilder, node};

    /// Garte's kitchen, cut down to its shape.
    ///
    /// ```text
    ///    0  greeting                          -> 1
    ///    1  MAIN HUB                          -> 2, 3, 4
    ///    2    "about the Whirling"            -> 5 -> 6
    ///    3    "a new bird"                       (unread)
    ///    4    "good bye"                      -> 19
    ///    6  SUB-HUB                           -> 7, 8, 9
    ///    7    "the kitchen"                   -> 10
    ///    8    "the door"                      -> 20 -> 1
    ///    9    "something else"                -> 1
    ///   10  the kitchen menu                  -> 11, 12, 13
    ///   11    "maybe I am a cook"             -> 14 -> 6
    ///   12    "a warrant"                     -> 16 -> 17, 18
    ///   13    "really hungry"                 -> 15 -> 6
    ///   17      "how did you know"               (unread)
    ///   18      "right"                       -> 21 -> 1
    /// ```
    ///
    /// Every option on the kitchen menu reaches unread content with its siblings cut: 11 and
    /// 13 by going back out through the sub-hub and the main hub to 3, and 12 by going on to
    /// 17. Only 12 leads onward in the sense a player means.
    pub(crate) fn kitchen() -> LookAheadGraph {
        GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).group().links(&[2, 3, 4]))
            .add(Entry::new(2).player().links(&[5]))
            .add(Entry::new(3).player())
            .add(Entry::new(4).player().links(&[19]))
            .add(Entry::new(5).links(&[6]))
            .add(Entry::new(6).group().links(&[7, 8, 9]))
            .add(Entry::new(7).player().links(&[10]))
            .add(Entry::new(8).player().links(&[20]))
            .add(Entry::new(9).player().links(&[1]))
            .add(Entry::new(10).links(&[11, 12, 13]))
            .add(Entry::new(11).player().links(&[14]))
            .add(Entry::new(12).player().links(&[16]))
            .add(Entry::new(13).player().links(&[15]))
            .add(Entry::new(14).links(&[6]))
            .add(Entry::new(15).links(&[6]))
            .add(Entry::new(16).links(&[17, 18]))
            .add(Entry::new(17).player())
            .add(Entry::new(18).player().links(&[21]))
            .add(Entry::new(19))
            .add(Entry::new(20).links(&[1]))
            .add(Entry::new(21).links(&[1]))
            .build()
    }

    /// The kitchen menu's options, and what the player was shown on the way to it.
    pub(crate) const KITCHEN_MENU: [i32; 3] = [11, 12, 13];
    pub(crate) const KITCHEN_WALK: [i32; 5] = [0, 2, 5, 7, 10];

    /// A sub-hub whose new content is one choice further in, behind a door already opened once.
    ///
    /// ```text
    ///    0  greeting                          -> 1
    ///    1  HUB A                             -> 2, 3
    ///    2    "about B"                       -> 4 -> 5
    ///    3    "good bye"
    ///    5  SUB-HUB B                         -> 6, 7, 8, 15
    ///    6    "topic one"                     -> 9 -> 5
    ///    7    "back"                          -> 10 -> 1
    ///    8    "the door"                      -> 11, a menu of its own -> 12, 13
    ///   12      "open it"                        (unread)
    ///   13      "leave it"                    -> 14 -> 5
    ///   15    "the window"                    -> 16 -> 17
    ///   17      "look out"                       (unread)
    /// ```
    ///
    /// The player came from A into B, took topic one, went through the door and left it, and is
    /// at B's menu again. The door still leads to "open it", and it is B's own topic rather than
    /// a way back out - so it must keep its star. The window is there so the menu has an
    /// onward option however much is cut, and the cheap question answers it rather than handing
    /// the menu to the exact marking.
    pub(crate) fn deeper_topic() -> LookAheadGraph {
        GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).group().links(&[2, 3]))
            .add(Entry::new(2).player().links(&[4]))
            .add(Entry::new(3).player())
            .add(Entry::new(4).links(&[5]))
            .add(Entry::new(5).group().links(&[6, 7, 8, 15]))
            .add(Entry::new(6).player().links(&[9]))
            .add(Entry::new(7).player().links(&[10]))
            .add(Entry::new(8).player().links(&[11]))
            .add(Entry::new(9).links(&[5]))
            .add(Entry::new(10).links(&[1]))
            .add(Entry::new(11).links(&[12, 13]))
            .add(Entry::new(12).player())
            .add(Entry::new(13).player().links(&[14]))
            .add(Entry::new(14).links(&[5]))
            .add(Entry::new(15).player().links(&[16]))
            .add(Entry::new(16).links(&[17]))
            .add(Entry::new(17).player())
            .build()
    }

    /// B's menu, the walk that came back to it, and the two unread lines.
    pub(crate) const DEEPER_TOPIC_MENU: [i32; 4] = [6, 7, 8, 15];
    pub(crate) const DEEPER_TOPIC_WALK: [i32; 9] = [0, 2, 4, 6, 9, 8, 11, 13, 14];
    pub(crate) const DEEPER_TOPIC_UNREAD: [i32; 2] = [12, 17];

    fn nodes(ids: &[i32]) -> Vec<DialogueNodeId> {
        ids.iter().map(|id| node(*id)).collect()
    }

    fn set(ids: &[i32]) -> HashSet<DialogueNodeId> {
        nodes(ids).into_iter().collect()
    }

    /// The hub stack at the end of a request's walk.
    fn stack(graph: &LookAheadGraph, walk: &[i32], menu: &[i32]) -> HubStack {
        let walked = passage(graph, &nodes(walk), &nodes(menu));
        follow(&IterationOrder::of(graph), &Hubs::of(graph), &walked)
    }

    /// Two loops, the second reached only through a one-way passage out of the first.
    ///
    /// ```text
    ///   0 -> A1 -> 2 -> 4 -> A1
    ///         A1 -> 3 -> 5 -> B6          (nothing leads back to A1)
    ///                    B6 -> 7 -> 9 -> B6
    ///                    B6 -> 8 -> 10 -> B6
    /// ```
    fn two_loops() -> LookAheadGraph {
        GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).group().links(&[2, 3]))
            .add(Entry::new(2).player().links(&[4]))
            .add(Entry::new(3).player().links(&[5]))
            .add(Entry::new(4).links(&[1]))
            .add(Entry::new(5).links(&[6]))
            .add(Entry::new(6).group().links(&[7, 8]))
            .add(Entry::new(7).player().links(&[9]))
            .add(Entry::new(8).player().links(&[10]))
            .add(Entry::new(9).links(&[6]))
            .add(Entry::new(10).links(&[6]))
            .build()
    }

    #[test]
    fn a_candidate_is_a_group_entry_linked_back_to_more_than_once() {
        assert_eq!(Hubs::of(&kitchen()).candidates(), &set(&[1, 6]));
    }

    /// A displayed line that several branches rejoin is not a candidate, however busy.
    #[test]
    fn only_group_entries_are_candidates() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2, 3]))
            .add(Entry::new(2).player().links(&[1]))
            .add(Entry::new(3).player().links(&[1]))
            .build();

        assert!(Hubs::of(&graph).candidates().is_empty());
        assert_eq!(stack(&graph, &[0, 2], &[2, 3]), HubStack::default());
    }

    /// Both hubs are passed on the way to the kitchen, so everything from the main hub on is
    /// behind the player - the sub-hub included, though it was never shown.
    #[test]
    fn what_was_passed_since_the_hub_is_everything_after_it() {
        let graph = kitchen();
        let since = since_current_hub(
            &graph,
            &IterationOrder::of(&graph),
            &Hubs::of(&graph),
            &nodes(&KITCHEN_WALK),
            &nodes(&KITCHEN_MENU),
        );

        assert_eq!(since, set(&[1, 2, 5, 6, 7, 10]));
    }

    /// The sub-hub is passed inside the main hub's loop, so it goes on top of it.
    #[test]
    fn a_sub_hub_is_stacked_on_its_hub() {
        let reached = stack(&kitchen(), &KITCHEN_WALK, &KITCHEN_MENU);

        assert_eq!(reached.hubs(), nodes(&[1, 6]));
        assert_eq!(reached.innermost(), Some(node(6)));
    }

    /// Going back to the main hub drops the sub-hub and starts the list again.
    #[test]
    fn returning_to_the_hub_starts_again() {
        let reached = stack(&kitchen(), &[0, 2, 5, 9], &[2, 3, 4]);

        assert_eq!(reached.hubs(), nodes(&[1]));
        assert_eq!(reached.since(), set(&[1]));
    }

    /// Arriving back at the sub-hub gives its own topics back - the door's route included - and
    /// keeps the way down from the outer hub behind the player.
    #[test]
    fn returning_to_a_sub_hub_gives_its_topics_back() {
        let reached = stack(&deeper_topic(), &DEEPER_TOPIC_WALK, &DEEPER_TOPIC_MENU);

        assert_eq!(reached.hubs(), nodes(&[1, 5]));
        assert_eq!(reached.since(), set(&[1, 2, 4, 5]));
    }

    /// Leaving the first loop by a way that never returns empties the stack, and the second
    /// loop's hub starts it again.
    #[test]
    fn a_point_of_no_return_empties_the_stack() {
        let reached = stack(&two_loops(), &[0, 2, 4, 3, 5, 7, 9], &[7, 8]);

        assert_eq!(reached.hubs(), nodes(&[6]));
        assert_eq!(reached.since(), set(&[6]));
    }
}
