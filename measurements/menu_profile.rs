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

// The consumers use different halves - some want the novelty function, some build their own
// from `unseen` - and a warning on every build of every one of them would hide the ones
// worth reading. The same reason `seen_profile.rs` carries this.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;

/// A menu to ask about: which entries are unseen, and which starts to ask.
pub struct MenuProfile {
    /// The deepest entries, which are the only unseen ones.
    pub unseen: HashSet<DialogueNodeId>,
    /// Starts that can reach one of them, shallowest first - which is what an option in a
    /// response menu is: an entry with the group's depth still in front of it.
    pub starts: Vec<DialogueNodeId>,
}

impl MenuProfile {
    /// Builds the profile for one group, or `None` if every start would be refused.
    ///
    /// `None` rather than an empty profile, because a run of refusals measures nothing and
    /// a caller has to be able to tell that apart from a group that was simply fast.
    pub fn of(
        graph: &LookAheadGraph,
        root: DialogueNodeId,
        unseen_wanted: usize,
        starts_wanted: usize,
    ) -> Option<Self> {
        let ranked = deepest_first(graph, root);
        let unseen: HashSet<DialogueNodeId> = ranked.iter().take(unseen_wanted).copied().collect();
        if unseen.is_empty() {
            return None;
        }

        let reaching = can_reach(graph, &unseen);
        let starts: Vec<DialogueNodeId> = ranked
            .iter()
            .rev()
            .filter(|id| reaching.contains(*id) && !unseen.contains(*id))
            .copied()
            .take(starts_wanted)
            .collect();

        (!starts.is_empty()).then_some(Self { unseen, starts })
    }

    /// The novelty function this profile describes.
    pub fn novelty(
        &self,
    ) -> impl Fn(DialogueNodeId) -> lookahead_engine::core::types::Novelty + '_ {
        use lookahead_engine::core::types::Novelty;
        move |id| {
            if self.unseen.contains(&id) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        }
    }
}

/// A profile and the world the walk that produced it stopped in.
pub struct Walked {
    pub profile: MenuProfile,
    /// Entries the walk put on screen before it stopped: what the world should call seen.
    pub seen: Vec<DialogueNodeId>,
    /// The dialogue variables at the point it stopped, by name - the counters at the values
    /// the walk drove them to, and the flags it set.
    pub variables: HashMap<String, lookahead_engine::bridge::WireValue>,
    /// How far the walk got before it was stopped, and how far it could have gone.
    pub shown: usize,
    pub reachable: usize,
}

/// The profile a greedy playthrough leaves when it is stopped with `unseen_wanted` entries
/// still to come.
///
/// ## Why this exists beside [`MenuProfile::of`]
///
/// `of` RANKS BY STRUCTURE and asserts the result: the deepest entries by link depth are
/// called unseen and everything else seen, and nothing checks that any play could stand
/// there. An asserted state can contradict itself - a `seen:` slot shuts an entry that shuts
/// once seen, so declaring most of a conversation seen can close the routes to the rest, and
/// on 761 that left its unread content link-reachable and symbolically unreachable.
///
/// This walks instead. The playthrough is taken twice: once to exhaustion, to learn how many
/// entries any play reaches at all, and once stopped that many less `unseen_wanted`. What
/// comes back is the seen set and the DATA STATE at the moment of stopping, so the unseen
/// entries are the last ones a nearest-first play would reach and the world around them is the
/// one it walked into.
///
/// ## What it does not do, and why
///
/// THE STARTS ARE CHOSEN AS `of` CHOOSES THEM - entries that can reach something unseen,
/// shallowest first - rather than being the menu the walk was standing at. A walk stops at the
/// entry it went for, which is not generally a menu, so standing it at one would mean walking
/// further on a different rule. Keeping the same start rule also keeps a row the same width as
/// the rows already measured, so only the world differs. See de-aqxa.2.
pub fn walked_profile(
    graph: &LookAheadGraph,
    world: &dyn lookahead_engine::world::ILookAheadWorld,
    conversation: i32,
    ceiling: usize,
    unseen_wanted: usize,
    starts_wanted: usize,
) -> Option<Walked> {
    use lookahead_engine::walkthrough::{Until, greedy_playthrough};

    let none = HashSet::new();
    let whole = greedy_playthrough(graph, world, conversation, ceiling, &none, Until::default());
    // THE START IS ALWAYS SHOWN - opening a conversation displays its entry 0 - so a walk that
    // reached nothing else has nothing to take an unseen set from.
    let reachable = whole.shown.len();
    if reachable <= unseen_wanted {
        return None;
    }

    let stop_at = reachable - unseen_wanted;
    let stopped = greedy_playthrough(
        graph,
        world,
        conversation,
        ceiling,
        &none,
        Until {
            shown: Some(stop_at),
        },
    );
    let seen = stopped.shown.clone();
    let unseen: HashSet<DialogueNodeId> = whole.shown[seen.len()..].iter().copied().collect();
    if unseen.is_empty() {
        return None;
    }

    let reaching = can_reach(graph, &unseen);
    let ranked = deepest_first(graph, DialogueNodeId::new(conversation, 0));
    let starts: Vec<DialogueNodeId> = ranked
        .iter()
        .rev()
        .filter(|id| reaching.contains(*id) && !unseen.contains(*id))
        .copied()
        .take(starts_wanted)
        .collect();
    if starts.is_empty() {
        return None;
    }

    Some(Walked {
        profile: MenuProfile { unseen, starts },
        variables: variables_of(graph, world, &stopped.ended),
        seen,
        shown: stop_at,
        reachable,
    })
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
