// SPDX-License-Identifier: MIT
//! The adversarial menu profile the whole-menu measurements share.
//!
//! ## Why this is a module rather than a paragraph repeated in each file
//!
//! `menu_residue` says of its own copy: "Restated rather than shared because an example
//! cannot import another example's helpers, and the rule is six lines." The first half is
//! not quite true - an example can pull a module in with `#[path]`, which is how every
//! measurement here already reaches `tests/common` - and the second half stopped being true
//! once the profile grew a reachability filter and a deterministic tie-break. Three files
//! wanting the same forty lines is what a module is for.
//!
//! ## What the profile IS, and why it has to be this one
//!
//! ADVERSARIAL. Exactly the structurally deepest entries are unseen and everything else is
//! seen, so every start that can reach one has something better than its own class beyond
//! it, `bridge::class_worth_hunting` refuses none of them, and every start pays for a real
//! search.
//!
//! THE ALTERNATIVE MEASURES NOTHING, and it is an easy mistake to make: the first cut of
//! `menu_residue` took the SHALLOWEST entries in the group and got twenty-four refusals and
//! zero candidates, which reads in a closing line exactly like a clean run. A percentage
//! profile has the same problem for the same reason - it refuses most starts before a
//! diagram is touched.
//!
//! Depth is by EDGE ANALYSIS ALONE - links followed, guards ignored - so it over-approximates
//! reachability, which makes the quarry at least structurally fair.

// The consumers use different halves - some want the seen state function, some build their own
// from `unseen` - and a warning on every build of every one of them would hide the ones
// worth reading. The same reason `seen_profile.rs` carries this.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;

/// A menu to ask about: what the player has seen ACROSS PLAYTHROUGHS, and which starts to ask.
///
/// HALF OF THE TWO FACTS, and only half on purpose. The game has two scopes and they are not the
/// same question. What THIS game has displayed is the WORLD'S to say - it is the save's own
/// record and it is what fires a `once` - so a profile does not carry it and cannot contradict
/// it. What ANY game has displayed is this, standing in for the global conversation state.
/// `world::seen_state` is where the two meet, and it is the only place the three states are
/// decided.
pub struct MenuProfile {
    /// Never seen in ANY game, which is the highest seen state there is. Its complement is the
    /// seen-any-game set - see [`MenuProfile::seen_any_game`].
    pub unseen: HashSet<DialogueNodeId>,
    /// Starts that can reach something unseen, shallowest first - which is what an option in a
    /// response menu is: an entry with the group's depth still in front of it.
    pub starts: Vec<DialogueNodeId>,
}

impl MenuProfile {
    /// Builds LINK-DEEPEST-X for one group, where X is `unseen_wanted`, or `None` if every start
    /// would be refused.
    ///
    /// The unseen entries are the deepest by LINK DISTANCE, asserted rather than walked to: no
    /// play is known to stand where they are still unread. [`walk_deepest_unseen`] is the set a
    /// walk vouches for.
    ///
    /// `None` rather than an empty profile, because a run of refusals measures nothing and
    /// a caller has to be able to tell that apart from a group that was simply fast.
    pub fn of(
        graph: &LookAheadGraph,
        root: DialogueNodeId,
        unseen_wanted: usize,
        starts_wanted: usize,
    ) -> Option<Self> {
        let unseen = link_deepest_unseen(graph, root, unseen_wanted);
        if unseen.is_empty() {
            return None;
        }
        let starts = synthetic_menu(graph, root, &unseen, starts_wanted);
        (!starts.is_empty()).then_some(Self { unseen, starts })
    }

    /// The starts a menu offers, with the targets they were chosen to reach.
    ///
    /// `None` where either half is empty, because a menu with nothing to hunt and a hunt with
    /// nowhere to start from both measure nothing, and a caller has to be able to tell that
    /// apart from a group that was simply fast.
    pub fn aimed_at(targets: HashSet<DialogueNodeId>, starts: Vec<DialogueNodeId>) -> Option<Self> {
        (!targets.is_empty() && !starts.is_empty()).then_some(Self {
            unseen: targets,
            starts,
        })
    }

    /// What the global conversation state holds: every entry some playthrough has shown.
    ///
    /// HALF OF WHAT DECIDES A SEEN STATE, and it is handed to `world::seen_state` beside the
    /// world rather than turned into one here. A profile that answered with a state of its own
    /// would be a second rule, and a row could then assert one the world disagrees with.
    pub fn seen_any_game(&self) -> impl Fn(DialogueNodeId) -> bool + '_ {
        move |id| !self.unseen.contains(&id)
    }
}

/// Everything a greedy playthrough reaches from the conversation's start, in the order it
/// reaches it, or `None` where it reaches no more than `unseen_wanted` entries.
///
/// THE RANKING EVERY WALK-DEEPEST SET IS TAKEN OFF THE END OF, and the walk that establishes
/// how much of a group any play can reach at all. `None` rather than a short set, because a
/// group with nothing left over has nothing to leave unread and that is an answer about the
/// group rather than a failure. THE START IS ALWAYS SHOWN - opening a conversation displays its
/// entry 0 - so a walk that reached nothing else comes back here as `None`.
fn whole_walk(
    graph: &LookAheadGraph,
    world: &dyn lookahead_engine::world::ILookAheadWorld,
    conversation: i32,
    ceiling: usize,
    unseen_wanted: usize,
) -> Option<Vec<DialogueNodeId>> {
    use lookahead_engine::walkthrough::{Until, greedy_playthrough};

    let none = HashSet::new();
    let whole = greedy_playthrough(graph, world, conversation, ceiling, &none, Until::default());
    (whole.shown.len() > unseen_wanted).then_some(whole.shown)
}

/// WALK-DEEPEST-X: the last X entries a greedy playthrough reaches, where X is `unseen_wanted`.
///
/// A SET A PLAY CAN LEAVE UNREAD, which is the whole difference from the link-deepest set
/// [`MenuProfile::of`] asserts: a walk reached everything before these, so "everything seen
/// except these" is a state some number of playthroughs can arrive at. The walk itself is
/// thrown away here - only which entries it reached last is kept - so this says nothing about
/// what THIS save has displayed.
pub fn walk_deepest_unseen(
    graph: &LookAheadGraph,
    world: &dyn lookahead_engine::world::ILookAheadWorld,
    conversation: i32,
    ceiling: usize,
    unseen_wanted: usize,
) -> Option<HashSet<DialogueNodeId>> {
    let shown = whole_walk(graph, world, conversation, ceiling, unseen_wanted)?;
    Some(deepest_of(&shown, shown.len() - unseen_wanted))
}

/// The entries of a walk from `left` onwards, which is the part of it a profile calls unseen.
///
/// Taken from the tail rather than by a count, since a caller that stopped a second walk short
/// knows how far THAT one actually got and the two need not agree.
fn deepest_of(shown: &[DialogueNodeId], left: usize) -> HashSet<DialogueNodeId> {
    shown[left..].iter().copied().collect()
}

/// How many presses a walked profile may play on for before giving up on finding a menu.
///
/// Generous, because the cost is paid once per group outside any timing, and a walk that needs
/// twenty continues to reach a menu has still reached one.
const TO_A_MENU: usize = 64;

/// A profile and the world the walk that produced it stopped in.
pub struct Walked {
    pub profile: MenuProfile,
    /// The world the menu is asked in: the save's, less what the walk has now shown and what
    /// its variables now hold. Filled by the caller, which is what knows the save.
    pub world: lookahead_engine::bridge::WorldRawData,
    /// Entries the walk put on screen before it stopped: what the world should call seen.
    pub seen: Vec<DialogueNodeId>,
    /// The dialogue variables at the point it stopped, by name - the counters at the values
    /// the walk drove them to, and the flags it set.
    pub variables: HashMap<String, lookahead_engine::bridge::WireValue>,
    /// How far the walk got before it was stopped, and how far it could have gone.
    pub shown: usize,
    pub reachable: usize,
    /// What the conversation has shown the player since it last started, oldest first: the
    /// walk a request carries as `encountered`.
    ///
    /// THE LAST SITTING ONLY, not the whole playthrough. A request's walk "must begin at the
    /// conversation's start, since the hubs are followed from there", and a restart IS the
    /// conversation starting again - so the entries belonging to earlier sittings are a
    /// different visit and following the hub stack through them would stack hubs the player
    /// has since left. See `Leg::restarted`.
    pub walk: Vec<DialogueNodeId>,
}

/// The menu a player is standing at when they have just walked up to it, having never opened
/// this conversation before.
///
/// ## What it builds, and why each part is what a save could hold
///
/// THE WALK-UP IS THE WHOLE HISTORY. The conversation is opened - which shows its start, and
/// nothing else - and then played forward taking lines as they come until a menu is on screen.
/// Nothing is chosen. So the entries put on screen are EXACTLY the ones it takes to reach the
/// menu, their one-time effects have fired, and every other entry in the group is still unseen
/// this game with its `once` still pending.
///
/// THE MENU IS THE GAME'S OWN. `AtAMenu::menu` is what the game would draw, so the starts are
/// the options a player is actually being offered. That is not true of [`MenuProfile::of`],
/// which picks starts structurally - entries that can reach the unseen set, shallowest first -
/// and they need not be the options of any one hub. Measured on conversation 631 the two sets
/// are disjoint; on 761 no menu is reachable from where a greedy walk stops at all.
///
/// THE UNSEEN SET IS STILL WALK-DEEPEST-X, taken from a walk to exhaustion, so it is a set a
/// play can leave unread. It is GLOBAL - never seen in any game - and everything between it and
/// the walk-up is `UnseenThisGame`.
///
/// `None` where the conversation ends or refuses before a menu appears, which is an answer about
/// the group rather than a failure.
pub fn first_menu_profile(
    graph: &LookAheadGraph,
    world: &dyn lookahead_engine::world::ILookAheadWorld,
    conversation: i32,
    ceiling: usize,
    unseen: HashSet<DialogueNodeId>,
) -> Option<Walked> {
    use lookahead_engine::walkthrough::{Until, greedy_playthrough, on_to_a_menu};

    let none = HashSet::new();
    if unseen.is_empty() {
        return None;
    }
    // HOW MUCH OF THE GROUP ANY PLAY REACHES, for the row's own reporting. The targets are the
    // caller's, so this is no longer what decides them.
    let reachable =
        whole_walk(graph, world, conversation, ceiling, 0).map_or(0, |shown| shown.len());

    // JUST OPENED, AND NOTHING MORE. One entry shown is the conversation's start, which opening
    // it displays - so this is the player arriving, before any choice.
    let arrived = greedy_playthrough(
        graph,
        world,
        conversation,
        ceiling,
        &none,
        Until { shown: Some(1) },
    );
    let at = on_to_a_menu(graph, world, &arrived, TO_A_MENU)?;

    let mut seen: Vec<DialogueNodeId> = arrived.shown.clone();
    for id in at.displayed() {
        if !seen.contains(&id) {
            seen.push(id);
        }
    }
    let mut walk: Vec<DialogueNodeId> = last_sitting(&arrived);
    walk.extend(at.encountered());

    let starts: Vec<DialogueNodeId> = at.menu.clone();
    if starts.is_empty() {
        return None;
    }

    Some(Walked {
        // WHAT THE WALK SHOWED IS THE WORLD'S, and it travels as `seen` below rather than in
        // the profile, so there is one statement of it - see `world::seen_state`.
        profile: MenuProfile { unseen, starts },
        world: Default::default(),
        variables: variables_of(graph, world, &at.state),
        seen,
        shown: 1,
        reachable,
        walk,
    })
}

/// Everything the conversation stepped through since it last started.
///
/// A playthrough is a run of sittings, each beginning at the conversation's start and ending
/// where the game sent the player away. What a request carries is the CURRENT one, so this
/// takes the legs from the last restart onwards. A playthrough with no legs at all has walked
/// nothing but the start, which cuts nothing and says so.
fn last_sitting(done: &lookahead_engine::walkthrough::Playthrough) -> Vec<DialogueNodeId> {
    let began = done.legs.iter().rposition(|leg| leg.restarted).unwrap_or(0);
    done.legs[began..]
        .iter()
        .flat_map(|leg| leg.encountered())
        .collect()
}

/// The dialogue variables a state holds, by name, as a world answers them.
///
/// ONLY THE ONES THAT NAME A VARIABLE. A slot table also carries `seen:`, `once:`, `item:` and
/// the rest, which a world answers from its own sets rather than from its variables - see
/// `core::state::seed_state` for which prefix goes where.
fn variables_of(
    graph: &LookAheadGraph,
    world: &dyn lookahead_engine::world::ILookAheadWorld,
    state: &lookahead_engine::core::state::LookAheadState,
) -> HashMap<String, lookahead_engine::bridge::WireValue> {
    use lookahead_engine::bridge::WireValue;
    use lookahead_engine::core::guard_value::GuardValueKind;

    let symbols = graph.symbols();
    let mut found = HashMap::new();
    for slot in 0..symbols.count() {
        let Some(name) = symbols.name_of(slot) else {
            continue;
        };
        if !lookahead_engine::core::state::names_a_variable(name) {
            continue;
        }
        let Some(variable) = symbols.variable_ref(name) else {
            continue;
        };
        // THE KIND THE WORLD ALREADY GIVES IT, with only the value changed. A slot holds a
        // number whatever the variable was declared as, and answering a boolean with a number
        // is a different answer to a guard that compares against true - so the kind is taken
        // from what this world already says rather than guessed from the slot.
        let value = state.get(slot);
        let wire = match world.get_variable(variable).kind() {
            GuardValueKind::Boolean => WireValue::Bool { value: value != 0 },
            _ => WireValue::Number {
                value: f64::from(value),
            },
        };
        found.insert(name.to_string(), wire);
    }
    found
}

/// The `wanted` entries furthest from `root` by LINK DISTANCE, asserted rather than walked to.
///
/// No play is known to stand where these are still unread - [`walk_deepest_unseen`] is the set a
/// walk vouches for. What this has instead is that it depends on the links alone, so it is the
/// same set in every world.
pub fn link_deepest_unseen(
    graph: &LookAheadGraph,
    root: DialogueNodeId,
    wanted: usize,
) -> HashSet<DialogueNodeId> {
    deepest_first(graph, root)
        .into_iter()
        .take(wanted)
        .collect()
}

/// A menu aimed at `targets`: the shallowest entries that can reach one, `starts_wanted` of them.
///
/// NOT A MENU THE GAME DRAWS. These entries are real and each is reachable from the start, but
/// nothing offers them together - so there is no position a player can stand in where this is
/// what they are looking at.
///
/// ADVERSARIAL BY CONSTRUCTION, which is what it is for: every start has something better beyond
/// it, so none is refused before a diagram is touched and each pays for a real search.
pub fn synthetic_menu(
    graph: &LookAheadGraph,
    root: DialogueNodeId,
    targets: &HashSet<DialogueNodeId>,
    starts_wanted: usize,
) -> Vec<DialogueNodeId> {
    let reaching = can_reach(graph, targets);
    deepest_first(graph, root)
        .iter()
        .rev()
        .filter(|id| reaching.contains(*id) && !targets.contains(*id))
        .copied()
        .take(starts_wanted)
        .collect()
}

/// Every entry reachable from `start` by links, deepest first.
fn deepest_first(graph: &LookAheadGraph, start: DialogueNodeId) -> Vec<DialogueNodeId> {
    let mut depth: HashMap<DialogueNodeId, usize> = HashMap::new();
    let mut queue = VecDeque::from([(start, 0usize)]);
    depth.insert(start, 0);
    while let Some((id, here)) = queue.pop_front() {
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if graph.get(child).is_some() && !depth.contains_key(&child) {
                depth.insert(child, here + 1);
                queue.push_back((child, here + 1));
            }
        }
    }

    let mut ranked: Vec<DialogueNodeId> = depth
        .keys()
        .copied()
        // GROUP ENTRIES ARE NOT SCORED - they are walked through and never named as a
        // destination - so they make poor starts and worse quarry.
        .filter(|id| *id != start && graph.get(*id).map(|node| !node.is_group).unwrap_or(false))
        .collect();
    // DETERMINISTIC, so two arms of a comparison rank the same entries the same way.
    // DialogueNodeId is not Ord, so the tie-break is spelled out from its parts.
    ranked.sort_unstable_by_key(|id| {
        (
            std::cmp::Reverse(depth[id]),
            id.conversation_id,
            id.entry_id,
        )
    });
    ranked
}

/// Every entry from which some member of `unseen` is link-reachable.
fn can_reach(graph: &LookAheadGraph, unseen: &HashSet<DialogueNodeId>) -> HashSet<DialogueNodeId> {
    let mut parents: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
    for node in graph.nodes() {
        for &child in &node.links {
            parents.entry(child).or_default().push(node.id);
        }
    }

    let mut reaching: HashSet<DialogueNodeId> = HashSet::new();
    let mut queue: VecDeque<DialogueNodeId> = unseen.iter().copied().collect();
    while let Some(id) = queue.pop_front() {
        for &parent in parents.get(&id).map(|v| v.as_slice()).unwrap_or(&[]) {
            if reaching.insert(parent) {
                queue.push_back(parent);
            }
        }
    }
    reaching
}
