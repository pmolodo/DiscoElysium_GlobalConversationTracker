// SPDX-License-Identifier: MIT
//! Which conversation groups can be measured at all.
//!
//! ## Why this is a command of its own
//!
//! Enumerating the game is a different job from measuring a menu in it, and a driver has to do
//! the first before it can do the second. A measurement that answered this when a variable told
//! it to would be one command doing two unrelated things, told apart by a flag - so this is its
//! own command, and `menu_matrix` only ever measures.
//!
//! ## What "can be measured" means, and why it is one question
//!
//! A group drops out for two reasons, and they are the same kind of fact - properties of the
//! dialogue, found by edge analysis, with no world and no walk:
//!
//! ```text
//! it reaches nothing from its start        901 of the game's 1,422 conversations
//! nothing it reaches offers the player     92 of the 521 groups that remain
//! ```
//!
//! A menu is a set of player options, so a group where nothing reachable offers the player a
//! choice has no menu under any walk, however it is asked.
//!
//! Neither is a measurement waiting to be taken, and neither can change because a run was asked
//! differently. That is what makes them worth keeping between runs.
//!
//! WHAT IS NOT DECIDED HERE: whether a particular run can build the profile it wants in a group
//! that DOES have menus. That depends on the world it walks into and on what it was asked for -
//! how many of the deepest entries to treat as unread, which profile to walk - so it is a
//! per-run finding rather than a fact, and `menu_matrix` reports it on stderr and measures
//! nothing. The two have been confused before, which is why this one is spelt out.
//!
//! ## It answers from what it derived last time
//!
//! Deriving the list builds a graph for every conversation in the game to see what each group
//! reaches, which is most of what asking costs. So the answer is kept - transparently, the way a
//! memo is, except on disk - and a later call reads it. Nothing about the call says which
//! happened; `--no-cache` derives it, as it does everything else kept.
//!
//! ## How to run it
//!
//! ```text
//! tools/run-logged.sh --kind analysis cargo group-list -- cargo run --release --example group_list
//! ```
//!
//! One line per group, most reachable first:
//!
//! ```text
//! start   conversations   entries   reachable   menus
//! ```

use std::collections::{BTreeSet, HashMap, HashSet};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{Index, build_group_graph, discover_group};

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "prepared.rs"]
mod prepared;
use prepared::{Group, Shipped};

#[path = "seen_profile.rs"]
mod seen_profile;
use seen_profile::candidates;

/// Every group worth measuring, most reachable first.
pub fn valid_groups(shipped: &Shipped) -> Vec<Group> {
    derived(shipped)
        .into_iter()
        .filter(|group| group.reachable > 0 && group.menus > 0)
        .collect()
}

/// Every distinct group in the game with what it reaches, kept between runs.
fn derived(shipped: &Shipped) -> Vec<Group> {
    prepared::group_list(shipped, || {
        let index = shipped.index();
        let mut groups: Vec<Group> = groups_of(index)
            .into_iter()
            .map(|(start, members)| {
                let (reachable, menus) = what_is_there(index, start);
                Group {
                    start,
                    conversations: members.len(),
                    entries: members.iter().map(|id| index[id].entries.len()).sum(),
                    reachable,
                    menus,
                    content: prepared::content_of(index, &members),
                }
            })
            .collect();
        groups.sort_unstable_by(|a, b| b.reachable.cmp(&a.reachable).then(a.start.cmp(&b.start)));
        groups
    })
}

/// Every distinct group in the game, as `(canonical start, the conversations it is made of)`,
/// by start.
///
/// A CANONICAL START IS NOT SIMPLY THE SMALLEST MEMBER. `discover_group` is the FORWARD closure
/// of a start, not an equivalence relation, so the smallest conversation in a group may reach
/// only part of it - a group of {3, 5} where 5 leads to 3 and 3 leads nowhere has `closure(3) =
/// {3}`. The start named is the smallest one whose own closure IS the whole set, which is the
/// only kind of start that reproduces the group it came from.
fn groups_of(index: &Index) -> Vec<(i32, Vec<i32>)> {
    let mut conversations: Vec<i32> = index.keys().copied().collect();
    conversations.sort_unstable();

    let mut canonical: HashMap<BTreeSet<i32>, i32> = HashMap::new();
    for &conversation in &conversations {
        let group: BTreeSet<i32> = discover_group(index, conversation).into_iter().collect();
        // Ascending, so the first start to produce a set is the smallest that reaches it.
        canonical.entry(group).or_insert(conversation);
    }

    let mut groups: Vec<(i32, Vec<i32>)> = canonical
        .into_iter()
        .map(|(group, start)| (start, group.into_iter().collect()))
        .collect();
    groups.sort_unstable_by_key(|(start, _)| *start);
    groups
}

/// What there is to measure in `start`'s group: how many entries a profile could be built from,
/// and how many of them offer the player anything.
///
/// BY EDGE ANALYSIS, from the graph alone. No world is built and no walk is taken, so neither
/// number depends on a setting or on what a particular run can reach - they are properties of
/// the dialogue, which is what makes them worth keeping between runs.
///
/// AN ENTRY WITH A PLAYER CHILD IS WHERE A MENU CAN BE. A menu is a set of player options, which
/// the engine composes from an entry's links when it gets there; whether it gets there depends
/// on the world, but whether there is anything there to compose does not. So a group with none
/// has no menu under any walk, and a group with some may still refuse a particular profile -
/// that is a different question, and a per-run one.
///
/// ON STDERR, the reason a group has nothing, so the list stays a clean TSV and a driver can
/// still keep why each group was left out.
fn what_is_there(index: &Index, start: i32) -> (usize, usize) {
    let Ok((graph, _)) = build_group_graph(index, start) else {
        eprintln!("conversation {start}: no group builds from it; skipping.");
        return (0, 0);
    };
    let root = DialogueNodeId::new(start, 0);
    if graph.get(root).is_none() {
        eprintln!("conversation {start}: no entry 0; skipping.");
        return (0, 0);
    }

    let reachable = candidates(&graph, root);
    if reachable.is_empty() {
        eprintln!("conversation {start}: nothing is reachable from its start; skipping.");
        return (0, 0);
    }

    let menus = std::iter::once(root)
        .chain(reachable.iter().copied())
        .filter(|id| offers_the_player(&graph, *id))
        .count();
    if menus == 0 {
        eprintln!(
            "conversation {start}: nothing it reaches offers the player anything, so there is no \
             menu in it to measure; skipping."
        );
    }
    (reachable.len(), menus)
}

/// Whether anything `id` links to is the player's to choose.
///
/// THROUGH GROUPS, WHICH IS WHAT THE ENGINE DOES. `walkthrough`'s `offer` expands a group link
/// into what the group shows, so an entry whose only link is a group holding player options DOES
/// offer a menu. Stopping at the direct children instead said thirteen groups had no menu when a
/// measurement had already found one in each of them - which is the failure that matters here,
/// since a group left out of the list is never measured again.
///
/// GUARDS ARE IGNORED ON PURPOSE. This asks whether a menu can be there at all, not whether a
/// particular world reaches it: an over-estimate costs a group that gets measured and answers
/// nothing, and an under-estimate loses a measurement silently.
fn offers_the_player(graph: &lookahead_engine::graph::LookAheadGraph, id: DialogueNodeId) -> bool {
    let Some(node) = graph.get(id) else {
        return false;
    };
    let mut seen: HashSet<DialogueNodeId> = HashSet::new();
    let mut queue: Vec<DialogueNodeId> = node.links.clone();
    while let Some(child) = queue.pop() {
        if !seen.insert(child) {
            continue;
        }
        let Some(child) = graph.get(child) else {
            continue;
        };
        if child.is_group {
            queue.extend(child.links.iter().copied());
        } else if child.player {
            return true;
        }
    }
    false
}

/// What this driver takes. It enumerates the whole game, so there is no group to name.
#[derive(clap::Parser)]
#[command(about = "Which groups are worth measuring, one row each.")]
struct Options {
    #[command(flatten)]
    caching: prepared::Caching,
}

fn main() {
    let asked = <Options as clap::Parser>::parse();
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; nothing to enumerate.");
        return;
    };
    let shipped = Shipped::at(path, asked.caching);
    for group in valid_groups(&shipped) {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            group.start, group.conversations, group.entries, group.reachable, group.menus
        );
    }
}
