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

/// Everything on the hub stack at the end of a request's walk.
///
/// `encountered` is every entry the conversation stepped through since it started, oldest
/// first, group entries and silently passed entries included. THE WHOLE CHAIN AS RECORDED:
/// the plugin reads it off the game's own link traversal, so nothing here recovers a hub from
/// the links - which could never cross an entry the game stepped over without displaying it,
/// such as a passive check that did not fire.
///
/// Empty where no hub is on it, which is also the answer in a group with no candidates.
pub fn since_current_hub(
    order: &IterationOrder,
    hubs: &Hubs,
    encountered: &[DialogueNodeId],
) -> HashSet<DialogueNodeId> {
    follow(order, hubs, encountered).since()
}

/// What a player is taken to have been shown walking from a conversation's start to `menu`, for
/// a request with no player behind it - a measurement's menu, which nobody navigated to.
///
/// THE SHAPE THE PLUGIN SENDS, so the engine does with it exactly what it does in game: every
/// entry on the way from the start, oldest first, group entries included and the menu itself left
/// off. What the plugin records is the game's own traversal, which is a whole chain too.
///
/// THE ROUTE is the shortest along links from `conversation`'s entry 0 to an option of `menu`: to
/// the nearest option arrived at with a hub current, since that is a walk the cut has something to
/// say about, and to the nearest option of all where none is. Ties go to the lower entry, so the
/// same menu always gets the same walk. Guards are ignored, as they are everywhere in this module.
///
/// Empty where the conversation has no entry 0 or no option can be reached from it, which asks
/// the question a request with no walk asks.
///
/// WORKS OUT THE GROUP'S ORDER AND HUBS FOR ITSELF. A measurement builds this in place of a
/// player's walk, before it starts timing; the engine finds the hubs again for the request, inside
/// the timing, as it would in game.
pub fn walk_to_menu(
    graph: &LookAheadGraph,
    conversation: i32,
    menu: &[DialogueNodeId],
) -> Vec<DialogueNodeId> {
    let start = DialogueNodeId::new(conversation, 0);
    if graph.get(start).is_none() {
        return Vec::new();
    }

    let mut came_from: HashMap<DialogueNodeId, DialogueNodeId> = HashMap::new();
    let mut queue = VecDeque::from([start]);
    while let Some(id) = queue.pop_front() {
        for &child in graph
            .get(id)
            .map(|node| node.links.as_slice())
            .unwrap_or_default()
        {
            if child != start && graph.get(child).is_some() && !came_from.contains_key(&child) {
                came_from.insert(child, id);
                queue.push_back(child);
            }
        }
    }

    // Every entry from the start to just before `option`, groups included, oldest first.
    let route_to = |option: DialogueNodeId| {
        let mut route = Vec::new();
        let mut at = option;
        while let Some(&before) = came_from.get(&at) {
            route.push(before);
            at = before;
        }
        route.reverse();
        route
    };

    let order = IterationOrder::of(graph);
    let hubs = Hubs::of(graph);
    let Some((.., route)) = menu
        .iter()
        .copied()
        .filter(|option| came_from.contains_key(option))
        .map(|option| {
            let route = route_to(option);
            let without_hub = follow(&order, &hubs, &route).innermost().is_none();
            (
                without_hub,
                route.len(),
                option.conversation_id,
                option.entry_id,
                route,
            )
        })
        .min_by_key(|(without_hub, length, conversation, entry, _)| {
            (*without_hub, *length, *conversation, *entry)
        })
    else {
        return Vec::new();
    };

    route
}

/// The entries a branch off a hub is spent in: it shows nothing new and changes nothing, so
/// walking it puts the player back where they started.
///
/// ## What "spent" means, and why it is worth cutting
///
/// The walk cut says WHERE THE PLAYER HAS JUST BEEN. This says WHAT THEY HAVE USED UP, which is
/// the durable version of the same idea: the hub stack gives a branch back the moment the
/// player returns to the hub above it, where a branch whose one-time effects have fired stays
/// spent for the rest of the game.
///
/// Three conditions, and all three are needed:
///
/// 1. NOTHING IN IT IS UNREAD, so the branch is not itself the destination.
/// 2. EVERY ENTRY IN IT IS A NO-OP ON RE-ENTRY: the world has shown it, and every action it
///    carries is a `once` action. `DialogueAction::apply` short-circuits a fired once, so such
///    an entry does nothing the second time. An entry carrying an ordinary action does
///    something every time and is never spent.
/// 3. EVERY LINK LEAVING IT STAYS IN THE HUB'S COMPONENT, so it loops back rather than leading
///    on. THIS IS THE ONE THAT KEEPS IT SOUND: a branch that fires a once and then leaves the
///    component is a one-way door, and cutting it would refuse the only route onward.
///
/// With all three, any route through the branch can be replaced by standing at the hub - same
/// state, nothing new shown - so cutting it removes no route to anything.
///
/// ## Where the answer comes from
///
/// NOTHING NEW IS TRACKED FOR THIS. `state::seed_state` seeds a node's `once_slot` from
/// `world.is_seen`, because `GenericLuaFunctions.Once` is a test on whether the entry has been
/// shown - so "this branch's incrementor has fired" is exactly "the player has seen this entry",
/// the same per-entry data the novelty markers are drawn from.
///
/// ## What it said, 2026-09-17: it cuts, and nothing moves
///
/// Whole game, three runs, walked profile, against the same arm without it:
///
/// ```text
///   entries cut          2,029 over 95 groups of 389
///   menus whose stars changed        0
///   sum of medians       8,698 -> 8,608 ms
/// ```
///
/// NOT ONE ANSWER IN THE GAME MOVED, with the walk cut running at 1,704 entries beside it. The
/// reason looks structural rather than incidental: a spent branch holds nothing unread by
/// condition 1, so it is only ever TRANSIT, and the routes through it are loops back to the hub
/// - which is exactly what the walk cut already refuses. The two cuts overlap where it counts.
///
/// It is therefore an opt-in arm that no default path calls, kept because the analysis is sound
/// and cheap to re-measure if the walk or the profile changes. It has no row in
/// `docs/modelling-gaps.md`, because a row there is for an answer that can differ and this one
/// does not. See de-wi02.
pub fn spent_branches<F>(
    graph: &LookAheadGraph,
    order: &IterationOrder,
    hub: DialogueNodeId,
    seen: &dyn Fn(DialogueNodeId) -> bool,
    novelty: &F,
    unread: crate::core::types::Novelty,
) -> HashSet<DialogueNodeId>
where
    F: Fn(DialogueNodeId) -> crate::core::types::Novelty,
{
    let Some(component) = order.component_of(hub) else {
        return HashSet::new();
    };
    let Some(node) = graph.get(hub) else {
        return HashSet::new();
    };

    let mut spent = HashSet::new();
    for &option in &node.links {
        if let Some(branch) =
            branch_if_spent(graph, order, hub, component, option, seen, novelty, unread)
        {
            spent.extend(branch);
        }
    }
    spent
}

/// One branch's entries where it is spent, and `None` where anything about it says otherwise.
///
/// The branch is everything reachable from `option` WITHOUT PASSING THE HUB, since arriving back
/// at the hub is where the branch ends.
#[allow(clippy::too_many_arguments)]
fn branch_if_spent<F>(
    graph: &LookAheadGraph,
    order: &IterationOrder,
    hub: DialogueNodeId,
    component: u32,
    option: DialogueNodeId,
    seen: &dyn Fn(DialogueNodeId) -> bool,
    novelty: &F,
    unread: crate::core::types::Novelty,
) -> Option<HashSet<DialogueNodeId>>
where
    F: Fn(DialogueNodeId) -> crate::core::types::Novelty,
{
    let mut branch = HashSet::new();
    let mut pending = VecDeque::from([option]);
    while let Some(id) = pending.pop_front() {
        if id == hub || !branch.insert(id) {
            continue;
        }
        let node = graph.get(id)?;
        if novelty(id) >= unread {
            return None;
        }
        if !is_no_op_on_re_entry(node, seen) {
            return None;
        }
        for &next in &node.links {
            if next == hub {
                continue;
            }
            // LEAVING THE HUB'S COMPONENT is a one-way door rather than a loop back, and a
            // branch holding one cannot be cut whatever else is true of it.
            if order.component_of(next) != Some(component) {
                return None;
            }
            pending.push_back(next);
        }
    }
    (!branch.is_empty()).then_some(branch)
}

/// Whether walking this entry again would do nothing at all.
///
/// A GROUP ENTRY IS ALWAYS ONE: the game never displays it and it carries no actions of its own.
fn is_no_op_on_re_entry(
    node: &crate::graph::node::LookAheadNode,
    seen: &dyn Fn(DialogueNodeId) -> bool,
) -> bool {
    if node.actions.is_empty() {
        return true;
    }
    // AN ORDINARY ACTION FIRES EVERY TIME, so one of those is enough to make the entry matter
    // however often it has been walked.
    node.actions.iter().all(|action| action.is_once()) && seen(node.id)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::core::types::Novelty;
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

    /// The kitchen menu's options, and the chain the game stepped through on the way to it.
    pub(crate) const KITCHEN_MENU: [i32; 3] = [11, 12, 13];
    pub(crate) const KITCHEN_WALK: [i32; 7] = [0, 1, 2, 5, 6, 7, 10];

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
    pub(crate) const DEEPER_TOPIC_WALK: [i32; 13] = [0, 1, 2, 4, 5, 6, 9, 5, 8, 11, 13, 14, 5];
    pub(crate) const DEEPER_TOPIC_UNREAD: [i32; 2] = [12, 17];

    fn nodes(ids: &[i32]) -> Vec<DialogueNodeId> {
        ids.iter().map(|id| node(*id)).collect()
    }

    fn set(ids: &[i32]) -> HashSet<DialogueNodeId> {
        nodes(ids).into_iter().collect()
    }

    /// The hub stack at the end of a request's walk.
    fn stack(graph: &LookAheadGraph, walk: &[i32]) -> HubStack {
        follow(&IterationOrder::of(graph), &Hubs::of(graph), &nodes(walk))
    }

    /// A menu nobody navigated to is walked to the way the plugin would have recorded it: the
    /// start, then every entry on the shortest route, group entries included.
    #[test]
    fn a_walk_to_a_menu_is_what_the_plugin_would_have_recorded() {
        let graph = kitchen();
        let walk = walk_to_menu(
            &graph,
            crate::test_graph::DEFAULT_CONVERSATION,
            &nodes(&KITCHEN_MENU),
        );

        assert_eq!(walk, nodes(&KITCHEN_WALK));
    }

    /// Of two options, the one arrived at with a hub current gets the walk, though another is
    /// nearer: that is a walk the cut has something to say about.
    #[test]
    fn a_walk_prefers_an_option_behind_a_hub_to_a_nearer_one() {
        // 13 is two links from the start with no hub; 11 is further, behind the main hub.
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 13]))
            .add(Entry::new(1).group().links(&[2, 3]))
            .add(Entry::new(2).player().links(&[4]))
            .add(Entry::new(3).player().links(&[1]))
            .add(Entry::new(4).links(&[1, 11]))
            .add(Entry::new(11).player())
            .add(Entry::new(13).player())
            .build();

        let walk = walk_to_menu(
            &graph,
            crate::test_graph::DEFAULT_CONVERSATION,
            &nodes(&[13, 11]),
        );

        assert_eq!(walk, nodes(&[0, 1, 2, 4]));
    }

    /// No option reachable from the start is no walk, which asks what a request without one asks.
    #[test]
    fn a_menu_no_route_reaches_gets_no_walk() {
        let graph = kitchen();
        let walk = walk_to_menu(
            &graph,
            crate::test_graph::DEFAULT_CONVERSATION,
            &nodes(&[99]),
        );

        assert!(walk.is_empty());
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
        assert_eq!(stack(&graph, &[0, 1, 2]), HubStack::default());
    }

    /// Both hubs are passed on the way to the kitchen, so everything from the main hub on is
    /// behind the player - the sub-hub included, though it was never shown.
    #[test]
    fn what_was_passed_since_the_hub_is_everything_after_it() {
        let graph = kitchen();
        let since = since_current_hub(
            &IterationOrder::of(&graph),
            &Hubs::of(&graph),
            &nodes(&KITCHEN_WALK),
        );

        assert_eq!(since, set(&[1, 2, 5, 6, 7, 10]));
    }

    /// The sub-hub is passed inside the main hub's loop, so it goes on top of it.
    #[test]
    fn a_sub_hub_is_stacked_on_its_hub() {
        let reached = stack(&kitchen(), &KITCHEN_WALK);

        assert_eq!(reached.hubs(), nodes(&[1, 6]));
        assert_eq!(reached.innermost(), Some(node(6)));
    }

    /// Going back to the main hub drops the sub-hub and starts the list again.
    #[test]
    fn returning_to_the_hub_starts_again() {
        let reached = stack(&kitchen(), &[0, 1, 2, 5, 6, 9, 1]);

        assert_eq!(reached.hubs(), nodes(&[1]));
        assert_eq!(reached.since(), set(&[1]));
    }

    /// Arriving back at the sub-hub gives its own topics back - the door's route included - and
    /// keeps the way down from the outer hub behind the player.
    #[test]
    fn returning_to_a_sub_hub_gives_its_topics_back() {
        let reached = stack(&deeper_topic(), &DEEPER_TOPIC_WALK);

        assert_eq!(reached.hubs(), nodes(&[1, 5]));
        assert_eq!(reached.since(), set(&[1, 2, 4, 5]));
    }

    /// Leaving the first loop by a way that never returns empties the stack, and the second
    /// loop's hub starts it again.
    #[test]
    fn a_point_of_no_return_empties_the_stack() {
        let reached = stack(&two_loops(), &[0, 1, 2, 4, 1, 3, 5, 6, 7, 9, 6]);

        assert_eq!(reached.hubs(), nodes(&[6]));
        assert_eq!(reached.since(), set(&[6]));
    }

    /// A hub with two topics, each a player line that fires a one-time increment and returns.
    ///
    /// ```text
    ///    0  start            -> 1
    ///    1  HUB              -> 2, 4
    ///    2    topic one      -> 3 -> 1     (+once)
    ///    4    topic two      -> 5 -> 1     (+once)
    /// ```
    fn two_spendable_topics() -> LookAheadGraph {
        GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).group().links(&[2, 4]))
            .add(
                Entry::new(2)
                    .player()
                    .script("SetVariableValue(\"count\", Variable[\"count\"] +once(1))")
                    .links(&[3]),
            )
            .add(Entry::new(3).links(&[1]))
            .add(
                Entry::new(4)
                    .player()
                    .script("SetVariableValue(\"count\", Variable[\"count\"] +once(1))")
                    .links(&[5]),
            )
            .add(Entry::new(5).links(&[1]))
            .build()
    }

    /// Nothing unread anywhere, which is condition one out of the way.
    fn all_read(_: DialogueNodeId) -> Novelty {
        Novelty::SeenThisGame
    }

    fn spent_with(
        graph: &LookAheadGraph,
        seen: &[i32],
        novelty: &dyn Fn(DialogueNodeId) -> Novelty,
    ) -> HashSet<DialogueNodeId> {
        let seen: HashSet<DialogueNodeId> = seen.iter().map(|id| node(*id)).collect();
        let was_seen = |id: DialogueNodeId| seen.contains(&id);
        spent_branches(
            graph,
            &IterationOrder::of(graph),
            node(1),
            &was_seen,
            &novelty,
            Novelty::UnseenThisGame,
        )
    }

    /// A branch whose one-time effects have fired and which shows nothing unread is spent: going
    /// round it again returns the same state having shown nothing.
    #[test]
    fn a_branch_whose_once_has_fired_is_spent() {
        let graph = two_spendable_topics();
        let spent = spent_with(&graph, &[2, 3], &all_read);

        assert!(spent.contains(&node(2)), "the topic the world has shown");
        assert!(spent.contains(&node(3)), "and what it leads to");
        assert!(
            !spent.contains(&node(4)),
            "the other topic has not been walked, so its once is still to fire"
        );
    }

    /// AN UNSEEN ENTRY STILL DOES SOMETHING. Until the world has shown it, its once has not
    /// fired, so walking it is not a no-op however little it shows.
    #[test]
    fn a_branch_nobody_has_walked_is_not_spent() {
        let graph = two_spendable_topics();
        assert!(spent_with(&graph, &[], &all_read).is_empty());
    }

    /// CONDITION ONE. A branch holding something unread is the destination rather than a spent
    /// loop, whatever its actions have done.
    #[test]
    fn a_branch_holding_something_unread_is_not_spent() {
        let graph = two_spendable_topics();
        let unread_at_three = |id: DialogueNodeId| {
            if id == node(3) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        };
        let spent = spent_with(&graph, &[2, 3], &unread_at_three);

        assert!(
            !spent.contains(&node(2)),
            "the branch leads to something unread, so it is where the player should go"
        );
    }

    /// CONDITION THREE, WHICH IS WHAT KEEPS IT SOUND. A branch that fires its once and then
    /// leaves the hub's component is a one-way door, not a loop back - cutting it would refuse
    /// the only route onward.
    #[test]
    fn a_branch_that_leaves_the_hubs_component_is_never_spent() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).group().links(&[2, 4]))
            .add(
                Entry::new(2)
                    .player()
                    .script("SetVariableValue(\"count\", Variable[\"count\"] +once(1))")
                    .links(&[3]),
            )
            // 3 goes ON rather than back to the hub, so 2 is a door out of the loop.
            .add(Entry::new(3))
            .add(
                Entry::new(4)
                    .player()
                    .script("SetVariableValue(\"count\", Variable[\"count\"] +once(1))")
                    .links(&[5]),
            )
            .add(Entry::new(5).links(&[1]))
            .build();

        let spent = spent_with(&graph, &[2, 3, 4, 5], &all_read);

        assert!(
            !spent.contains(&node(2)),
            "2 leaves the hub's component, so it is a one-way door however spent it looks"
        );
        assert!(
            spent.contains(&node(4)),
            "4 loops back and is genuinely spent"
        );
    }

    /// AN ORDINARY ACTION FIRES EVERY TIME, so an entry carrying one is never a no-op on
    /// re-entry however often it has been walked.
    #[test]
    fn a_branch_with_an_ordinary_action_is_never_spent() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).group().links(&[2]))
            .add(
                Entry::new(2)
                    .player()
                    .script("SetVariableValue(\"flag\", true)")
                    .links(&[3]),
            )
            .add(Entry::new(3).links(&[1]))
            .build();

        assert!(spent_with(&graph, &[2, 3], &all_read).is_empty());
    }
}
