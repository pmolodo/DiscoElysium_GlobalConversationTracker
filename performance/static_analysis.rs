// SPDX-License-Identifier: MIT
//! Facts about the dialogue graph that never change, worked out once and committed.
//!
//! Two passes, both over the LINK STRUCTURE ALONE - no guards, no actions, no world.
//! That is what makes their answers static: they depend on the shipped database and
//! nothing else, so they can be computed once, checked in, and read back instantly
//! instead of being recomputed per option at runtime.
//!
//! 1. WHICH CONVERSATIONS FORM A GROUP. Conversations link to each other, and the search
//!    loads the whole group a start belongs to. Partitioning the database into groups
//!    says how big that job ever gets.
//!
//! 2. WHAT EACH START CAN REACH. From a given entry, following links and ignoring
//!    everything else, which entries can be arrived at. This is a strict upper bound on
//!    what any search can reach, however clever - a guard can only ever refuse a link, it
//!    cannot create one.
//!
//! ## Why the second one is worth having
//!
//! `LookAheadEngine::has_potential_improvement` currently asks whether ANY entry in the
//! group outranks the option. That is the whole group, including everything no path from
//! this option leads to. Asked against the reachable set instead, it refuses more searches
//! and never refuses a search that could have found something.
//!
//! A stateless prefilter of this shape was built in C#, measured and reverted - see
//! de-asw.3 - because it pruned only 0.5 to 4 per cent in the hub-connected case and cost
//! more than it saved ONCE WIRED PER OPTION. Precomputing it removes exactly that
//! objection: the cost moves to a build step that runs once.
//!
//! Regenerate both committed files with:
//!   cargo run --release --example static_analysis
//!
//! or one stage at a time: groups, reachability, coverage.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use lookahead_engine::index::{Index, links_of, read_index};

use gct_measure::common;

/// THREE PASSES OVER THE SAME DATABASE, run in order by default and singly by name.
///
/// The first two write the committed files under `analysis/` and share the whole link-
/// walking apparatus below; the third reports what a start can reach and writes nothing.
/// Regenerating one of the two committed files without the other is the case the argument
/// exists for - they are separate answers and a reader should be able to refresh one.
///
/// NOTHING IN THE REPOSITORY READS `analysis/` YET. The files are committed and the
/// prefilter they exist for is not built; that is de-asw.3's story rather than this file's,
/// but it is worth knowing before treating a stale file as a bug.
fn main() {
    match std::env::args().nth(1).as_deref() {
        None => {
            partition_the_database_into_conversation_groups();
            measure_link_reachability_from_every_entry();
            how_much_of_a_loaded_group_can_a_start_actually_reach();
        }
        Some("groups") => partition_the_database_into_conversation_groups(),
        Some("reachability") => measure_link_reachability_from_every_entry(),
        Some("coverage") => how_much_of_a_loaded_group_can_a_start_actually_reach(),
        Some(other) => {
            eprintln!(
                "unknown pass {other:?}; expected groups, reachability or coverage, or no \
                 argument at all to run the three in order"
            );
            std::process::exit(2);
        }
    }
}

/// Where the committed answers live.
const ANALYSIS_DIR: &str = "analysis";

/// The conversations each conversation links TO, and each conversation linked FROM.
///
/// Both directions, because a group is a weakly connected component: two conversations
/// belong together if either can link to the other. `index::discover_group` follows links
/// FORWARDS only, which gives what one start can pull in rather than a partition - a
/// different and also useful question, measured separately below.
fn conversation_links(index: &Index) -> HashMap<i32, HashSet<i32>> {
    let mut adjacent: HashMap<i32, HashSet<i32>> = HashMap::new();
    for (&id, conversation) in index {
        adjacent.entry(id).or_default();
        for entry in &conversation.entries {
            for &destination in &entry.to_conversation {
                if destination != id && index.contains_key(&destination) {
                    adjacent.entry(id).or_default().insert(destination);
                    adjacent.entry(destination).or_default().insert(id);
                }
            }
        }
    }

    adjacent
}

/// The weakly connected components: a true partition of the database.
fn partition(index: &Index) -> Vec<Vec<i32>> {
    let adjacent = conversation_links(index);
    let mut unassigned: BTreeSet<i32> = index.keys().copied().collect();
    let mut groups: Vec<Vec<i32>> = Vec::new();

    while let Some(&seed) = unassigned.iter().next() {
        let mut group = BTreeSet::new();
        let mut pending = VecDeque::new();
        unassigned.remove(&seed);
        group.insert(seed);
        pending.push_back(seed);

        while let Some(id) = pending.pop_front() {
            for &neighbour in adjacent.get(&id).into_iter().flatten() {
                if unassigned.remove(&neighbour) {
                    group.insert(neighbour);
                    pending.push_back(neighbour);
                }
            }
        }

        groups.push(group.into_iter().collect());
    }

    // Biggest first, which is the order anybody reading this wants.
    groups.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));
    groups
}

/// Every entry, keyed by conversation, with the entries it links to.
///
/// Flattened out of the index once so the reachability pass does not re-read it per start.
fn entry_links(index: &Index) -> HashMap<(i32, i32), Vec<(i32, i32)>> {
    let mut links = HashMap::new();
    for (&conversation_id, conversation) in index {
        for entry in &conversation.entries {
            let destinations = links_of(entry, conversation_id)
                .into_iter()
                .map(|id| (id.conversation_id, id.entry_id))
                .collect();
            links.insert((conversation_id, entry.id), destinations);
        }
    }

    links
}

/// How many entries are reachable from `start` by following links alone.
fn reachable_from(
    links: &HashMap<(i32, i32), Vec<(i32, i32)>>,
    start: (i32, i32),
) -> BTreeSet<(i32, i32)> {
    let mut seen = BTreeSet::new();
    let mut pending = VecDeque::new();
    if !links.contains_key(&start) {
        return seen;
    }

    seen.insert(start);
    pending.push_back(start);

    while let Some(id) = pending.pop_front() {
        for &child in links.get(&id).into_iter().flatten() {
            if links.contains_key(&child) && seen.insert(child) {
                pending.push_back(child);
            }
        }
    }

    seen
}

fn analysis_path(name: &str) -> PathBuf {
    let path = common::repo_root().join(ANALYSIS_DIR);
    std::fs::create_dir_all(&path).expect("the analysis directory");
    path.join(name)
}

/// Writes a committed answer, with the trailing newline the repo's hooks insist on.
///
/// Without it `end-of-file-fixer` adds one at commit time and the next regeneration takes
/// it away again, so the file shows as changed on every run whether or not the answer
/// did. A committed artefact is only useful if regenerating it is a no-op when nothing
/// has moved.
fn write_answer(name: &str, body: &str) {
    let out = analysis_path(name);
    std::fs::write(&out, format!("{body}\n")).expect("writing the answer");
    println!("wrote {} ({} bytes)", out.display(), body.len() + 1);
}

fn partition_the_database_into_conversation_groups() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");

    let groups = partition(&index);
    let entries: usize = index.values().map(|c| c.entries.len()).sum();

    println!(
        "{} conversations, {entries} entries, {} groups",
        index.len(),
        groups.len(),
    );
    println!("\nthe ten largest groups, by conversations:");
    for group in groups.iter().take(10) {
        let held: usize = group
            .iter()
            .filter_map(|id| index.get(id))
            .map(|c| c.entries.len())
            .sum();
        println!(
            "  {:>4} conversations, {held:>6} entries, starting {:?}",
            group.len(),
            &group[..group.len().min(6)],
        );
    }

    let alone = groups.iter().filter(|g| g.len() == 1).count();
    println!("\n{alone} conversations are a group of one");

    // Written as a map from the group's lowest conversation id to its members, which is
    // stable across runs and reads better in a diff than an array index would.
    let by_root: BTreeMap<String, &Vec<i32>> =
        groups.iter().map(|g| (g[0].to_string(), g)).collect();
    let written = serde_json::to_string_pretty(&by_root).expect("the groups serialise");
    write_answer("conversation_groups.json", &written);

    assert!(!groups.is_empty(), "the database yielded no groups");
}

fn measure_link_reachability_from_every_entry() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let links = entry_links(&index);

    // Every entry is a possible start: the look-ahead searches from whichever option the
    // player is looking at, not from a conversation's first line.
    println!("{} entries, each a possible start", links.len());

    let mut summary: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0usize;
    let mut widest = (0usize, (0, 0));

    for &start in links.keys() {
        let reached = reachable_from(&links, start).len();
        total += reached;
        if reached > widest.0 {
            widest = (reached, start);
        }
        summary.insert(format!("{}:{}", start.0, start.1), reached);
    }

    println!(
        "mean {} entries reachable per start; widest {} from {}:{}",
        total / links.len().max(1),
        widest.0,
        (widest.1).0,
        (widest.1).1,
    );

    let written = serde_json::to_string(&summary).expect("the summary serialises");
    write_answer("link_reachability.json", &written);

    assert!(!links.is_empty(), "the database yielded no entries");
}

/// How much smaller is the question, once it is asked of the reachable set?
///
/// The number that decides whether refining the short circuit is worth anything. Its C#
/// ancestor was reverted for pruning 0.5 to 4 per cent, so this has to be compared
/// against that rather than admired on its own.
///
/// Measured per start as the reachable entries against the entries in the group the search
/// would LOAD - `index::discover_group`'s forward closure, which is what
/// `build_group_graph` builds and therefore what the group-wide check scans.
fn how_much_of_a_loaded_group_can_a_start_actually_reach() {
    let Some(path) = common::conversation_index() else {
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let links = entry_links(&index);

    // The loaded group is per CONVERSATION, so it is worth computing once each rather
    // than once per entry.
    let mut loaded: HashMap<i32, usize> = HashMap::new();
    for &conversation in index.keys() {
        let held = lookahead_engine::index::discover_group(&index, conversation)
            .iter()
            .filter_map(|id| index.get(id))
            .map(|c| c.entries.len())
            .sum();
        loaded.insert(conversation, held);
    }

    println!(
        "{:>10} {:>10} {:>10} {:>8}",
        "starts", "reachable", "loaded", "share"
    );

    let mut buckets: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
    for &start in links.keys() {
        let reached = reachable_from(&links, start).len();
        let held = *loaded.get(&start.0).unwrap_or(&reached);
        // Grouped by how big the loaded group is, because the whole question only matters
        // where the group is large - a search over 40 entries costs nothing either way.
        let bucket = match held {
            0..=99 => "     <100",
            100..=999 => "  100-999",
            1000..=4999 => " 1k-5k",
            _ => "     >5k",
        };
        let row = buckets.entry(bucket).or_insert((0, 0, 0));
        row.0 += 1;
        row.1 += reached;
        row.2 += held;
    }

    for (bucket, (starts, reached, held)) in &buckets {
        println!(
            "{bucket} {starts:>10} {:>10} {:>10} {:>7.1}%",
            reached / starts.max(&1),
            held / starts.max(&1),
            100.0 * *reached as f64 / *held as f64,
        );
    }

    let (starts, reached, held) = buckets
        .values()
        .fold((0, 0, 0), |acc, r| (acc.0 + r.0, acc.1 + r.1, acc.2 + r.2));
    println!(
        "\nover all {starts} starts: a start reaches {:.1}% of the group loaded for it, \
         so {:.1}% of every loaded group is entries no path from the option leads to",
        100.0 * reached as f64 / held as f64,
        100.0 - 100.0 * reached as f64 / held as f64,
    );

    assert!(starts > 0, "no starts were measured");
}
