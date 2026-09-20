// SPDX-License-Identifier: MIT
//! Whether a profile's globally-unseen set is a state any number of playthroughs could leave.
//!
//! ## The question
//!
//! A fresh-save scenario says: these X entries were never seen in ANY game, and everything else
//! was. For that to describe a real player, every entry it calls seen has to be one some
//! playthrough could have displayed - and displaying an entry means walking to it. So if every
//! route to some entry `e` passes through an unseen entry `u`, then seeing `e` required seeing
//! `u`, and the claim contradicts itself. The set is REACHABLE-CLOSED when no such `e` exists.
//!
//! WALK-DEEPEST-X IS CLOSED BY CONSTRUCTION and is not asked about here: it is the tail of a
//! walk, so a walk reached everything before it. LINK-DEEPEST-X is picked by edge depth off the
//! dialogue graph with nothing vouching for it, and that is what this checks. The expectation is
//! that the deepest entries are mostly leaves and lead nowhere - but "mostly" is not a reason,
//! and what rests on it is whether the synthetic-menu scenario measures a world or a
//! fiction. See de-5sdm.
//!
//! ## What it computes
//!
//! Reachability from the group's root twice: once over the whole graph, once with the unseen set
//! removed. An entry in the first and not the second is one whose every route is through the
//! unseen set, and each one is a counter-example to the scenario's claim.
//!
//! EDGE ANALYSIS ALONE - links followed, guards ignored - which is the same approximation the
//! profile itself is built on. It OVER-approximates what a play can reach, so it is the
//! generous reading: an entry this calls blocked is blocked under any guard as well.
//!
//! ## How to run it
//!
//! It takes the groups to check rather than enumerating them - `--conversation`, or the
//! first column of standard input, which is what `group_list` writes:
//!
//! ```text
//! tools/run-logged.sh --kind analysis cargo group-list -- cargo run --release --example group_list > groups.tsv
//! tools/run-logged.sh --kind analysis cargo profile-closure -- cargo run --release --example profile_closure < groups.tsv
//! ```
//!
//! One line per group, worst first, and a closing count. `--unseen` sets X:
//!
//! ```text
//! conv    reachable   unseen   blocked   worst
//! ```
//!
//! `blocked` is how many entries the set puts out of reach of every play, and `worst` names a
//! few of them. A group with `blocked` 0 is closed: the scenario is a state a player can be in.
//!
//! ## What it said, 2026-09-19: link-deepest-10 is mostly not a state
//!
//! ```text
//! 89 of 395 groups reachable-closed, 306 not
//! worst: 14 blocks 20 entries, 553 blocks 16, 348 blocks 15, 15 and 1030 block 14
//! 761 blocks 2 - 1168:428 and 1168:821
//! ```
//!
//! Few entries each, and in more than three quarters of the game. So `synthetic-menu` is an
//! adversarial UPPER BOUND rather than a player: it says what the engine would cost in the
//! hardest assignment of seen states a group admits, and `synthetic-menu-walk-deepest` says
//! what it costs in the hardest one a player can actually be in. On 761 the two are 2,099 ms
//! and 184 ms.

use std::collections::{HashSet, VecDeque};

use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::graph::LookAheadGraph;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "options.rs"]
mod options;

#[path = "prepared.rs"]
mod prepared;
use prepared::Shipped;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

/// How many of the group's deepest entries the scenario calls unseen, matching `menu_matrix`.
const UNSEEN: usize = 10;

/// How many blocked entries a row names before it stops. Enough to see what KIND of entry is
/// blocked, which is what decides whether a failure is interesting.
const NAMED: usize = 4;

/// Every entry reachable from `root` by links, optionally with `without` removed from the graph.
///
/// The removed entries are not themselves reached, which is what makes the second pass answer
/// the question: a play that has never displayed them cannot walk through them either.
fn reached(
    graph: &LookAheadGraph,
    root: DialogueNodeId,
    without: &HashSet<DialogueNodeId>,
) -> HashSet<DialogueNodeId> {
    let mut seen = HashSet::from([root]);
    let mut queue = VecDeque::from([root]);
    while let Some(id) = queue.pop_front() {
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if without.contains(&child) || graph.get(child).is_none() {
                continue;
            }
            if seen.insert(child) {
                queue.push_back(child);
            }
        }
    }
    seen
}

/// What the link-deepest set puts out of reach in one group, or `None` where no profile builds.
fn blocked_by(
    graph: &LookAheadGraph,
    root: DialogueNodeId,
    unseen_wanted: usize,
) -> Option<(usize, usize, Vec<DialogueNodeId>)> {
    // THE STARTS ARE NOT USED, and asking for one is only how `MenuProfile::of` reports that the
    // group has no usable profile at all. A group it refuses is one no fresh-save row is taken
    // on, so its closure is not a question anybody asks.
    let profile = MenuProfile::of(graph, root, unseen_wanted, 1)?;

    let whole = reached(graph, root, &HashSet::new());
    let without = reached(graph, root, &profile.unseen);
    let mut blocked: Vec<DialogueNodeId> = whole
        .iter()
        .filter(|id| !without.contains(*id) && !profile.unseen.contains(*id))
        .copied()
        .collect();
    // DETERMINISTIC, so two runs name the same entries in the same order. DialogueNodeId is not
    // Ord, so the key is spelled out from its parts.
    blocked.sort_unstable_by_key(|id| (id.conversation_id, id.entry_id));
    Some((whole.len(), profile.unseen.len(), blocked))
}

/// Which groups to check: whatever `--conversation` named, otherwise the first column of
/// whatever is on standard input.
///
/// IT DOES NOT ENUMERATE THE GAME, because `group_list` does and enumerating is a different job
/// from checking - the same division `menu_matrix` keeps. Taking the list rather than deriving
/// it also means this checks exactly the groups a measurement would be taken on, which is the
/// only set the question is about.
fn asked_about(named: &[i32]) -> Vec<i32> {
    use std::io::BufRead;

    if !named.is_empty() {
        return named.to_vec();
    }
    std::io::stdin()
        .lock()
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| {
            line.split('\t')
                .next()
                .and_then(|cell| cell.trim().parse().ok())
        })
        .collect()
}

/// What this driver takes. With no group named it reads them from standard input, which is how a
/// measurement's own list reaches it.
#[derive(clap::Parser)]
#[command(about = "Whether each group's walked profile is a state a playthrough could leave.")]
struct Options {
    #[command(flatten)]
    groups: options::Groups,
    #[command(flatten)]
    unseen: options::Unseen<UNSEEN>,
    #[command(flatten)]
    caching: prepared::Caching,
}

fn main() {
    let asked = <Options as clap::Parser>::parse();
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; nothing to check.");
        return;
    };
    let shipped = Shipped::at(path, asked.caching);
    let unseen_wanted = asked.unseen.unseen;

    let groups = asked_about(&asked.groups.conversations);
    if groups.is_empty() {
        eprintln!(
            "no groups named. Pass --conversation, or feed this the group enumeration:\n  \
             cargo run --release --example group_list > groups.tsv\n  \
             cargo run --release --example profile_closure < groups.tsv"
        );
        return;
    }

    println!("conv\treachable\tunseen\tblocked\tworst");
    let mut rows: Vec<(usize, i32, usize, usize, Vec<DialogueNodeId>)> = Vec::new();
    for conversation in groups {
        let Ok(group) = prepared::group_graph(&shipped, conversation) else {
            eprintln!("conversation {conversation}: no group builds from it; skipping.");
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        let Some((whole, unseen, blocked)) = blocked_by(&group.graph, root, unseen_wanted) else {
            eprintln!("conversation {conversation}: no profile builds; skipping.");
            continue;
        };
        rows.push((blocked.len(), conversation, whole, unseen, blocked));
    }

    rows.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let closed = rows.iter().filter(|row| row.0 == 0).count();
    for (count, conversation, whole, unseen, blocked) in &rows {
        let named: Vec<String> = blocked
            .iter()
            .take(NAMED)
            .map(|id| format!("{}:{}", id.conversation_id, id.entry_id))
            .collect();
        println!(
            "{conversation}\t{whole}\t{unseen}\t{count}\t{}",
            if named.is_empty() {
                "-".to_string()
            } else {
                named.join(",")
            }
        );
    }
    eprintln!(
        "link-deepest-{unseen_wanted}: {closed} of {} groups are reachable-closed, {} are not",
        rows.len(),
        rows.len() - closed
    );
}
