// SPDX-License-Identifier: MIT
//! How often is a candidate DOMINATED by one the driver already asked about?
//!
//! ## The relation, and why reachability was the wrong one to test
//!
//! de-cnjw closed with one sentence: between two backward runs the sets do not compose,
//! because `B_t` and `B_u` are about different targets and neither contains the other in
//! general, even when `t` and `u` are on a path. That is right about REACHABILITY - a guard
//! on the connecting path can block, so `t` being link-reachable from `u` says nothing - and
//! it never named the relation that does give containment.
//!
//! DOMINANCE DOES. Write `B_t(s)` for the data states at the start `s` from which the target
//! `t` is still reachable. If every path from `s` to `u` passes through `t`, then any run
//! that reaches `u` reached `t` on the way, so `B_u(s)` is contained in `B_t(s)`. Two
//! consequences, worth very different amounts:
//!
//! - A YES ABOUT `u` IMPLIES A YES ABOUT `t`. Nearly worthless: the driver stops at the
//!   first candidate it proves, so it was never going to ask about `t`.
//! - A NO ABOUT `t` REFUSES EVERY `u` THAT `t` DOMINATES, with no fixed point at all. That
//!   is the whole prize, because refusals are where the cost is - de-cnjw measured
//!   conversation 28 at 25 refusals out of 26 candidates asked, each needing a complete
//!   fixed point, and nothing else in the engine makes one cheaper.
//!
//! ## What this measures, which is the cheapest half of the question
//!
//! ONLY THE STRUCTURE. No diagrams, no guards, no world, no search: the driver's candidate
//! order is a function of the link graph and the novelty function, and dominance is a
//! function of the link graph alone. So the addressable share can be counted for the whole
//! game in seconds, and if it is near zero the expensive half is never worth running.
//!
//! For each row - one start, one profile - it builds the candidate list in exactly the order
//! [`novelty_search::candidates_from`] hands the driver, then walks it counting how many
//! candidates have a STRICT DOMINATOR EARLIER IN THAT SAME LIST. Those are the ones a
//! refusal of the dominator would have answered for free.
//!
//! WHY EARLIER IN THE LIST IS THE RIGHT TEST, rather than dominance on its own. Within one
//! novelty class the driver walks nearest first, and a strict dominator of `u` is strictly
//! nearer than `u` - every path to `u` goes through it - so a dominator inside the class
//! always precedes what it dominates. Across classes it need not, and a dominator asked
//! about after its dominated candidate arrives too late to save anything. Counting bare
//! dominance would report a prize the driver's own order cannot collect.
//!
//! ## The two columns that say why the answer came out as it did
//!
//! `has-dom` counts candidates with any strict dominator at all among the candidates,
//! earlier or later. It is the structural ceiling: `dominated` can never exceed it, and the
//! gap between them is what the ORDER costs rather than what the GRAPH refuses.
//!
//! `scc` is the share of the reachable subgraph inside its largest strongly connected
//! component. Entries inside one component cannot dominate each other at all, so a group
//! that is mostly one component is one this cannot help however the order falls - which is
//! the hub-and-spoke shape de-asw.3 already found raw link reachability defeated by.
//!
//! ## What it said, 2026-09-08, over the whole game: 87.7 per cent
//!
//! 1,422 distinct groups, one start each, the seven percentage profiles - 3,647 rows and
//! 260,352 candidates, walked in a second and a half.
//!
//! ```text
//!   rows                       3647
//!   rows with >1 candidate     3224 (88.4%)
//!   candidates               260352
//!   with a strict dominator  228305 (87.7%)
//!   with an EARLIER one      228305 (87.7%)
//! ```
//!
//! and on the heavy groups the matrix measures, per row:
//!
//! ```text
//!   conv       profile   cands  dominated    share    scc
//!    631      5pc-seen    2703       2630    97.3%    85%
//!    640      5pc-seen    2278       2196    96.4%    86%
//!     14      5pc-seen    2104       1949    92.6%    77%
//!   1177      5pc-seen    1819       1818    99.9%    36%
//!     28     50pc-seen     707        587    83.0%    54%
//!    362     95pc-seen      61         17    27.9%    86%
//! ```
//!
//! and over an adversarial menu of twenty-four options, 960 of 2,160 candidates: 44.4 per
//! cent.
//!
//! SO de-cnjw'S SENTENCE IS OVERTURNED, and the phrase carrying it was "in general". Two
//! backward passes do not compose under REACHABILITY, exactly as it said. Under DOMINANCE
//! they compose almost always, and the driver's own candidate order collects it for free.
//!
//! ## The two things that keep this from being 87.7 per cent off the bill
//!
//! A DOMINATOR ONLY PAYS WHEN IT IS REFUSED. The rule is that a NO about `t` refuses every
//! `u` it dominates; a yes about `t` ends the search anyway. So the share above is exactly
//! the saving on a row that refuses every candidate, and an over-estimate on one that finds
//! something. de-kqgq measured that population directly: of 2,238 rows, 329 refuse every
//! candidate and complete 2,623 fixed points doing it, and those are the rows this collapses.
//!
//! SCC MEMBERSHIP DOES NOT PROTECT A GROUP THE WAY IT LOOKS AS THOUGH IT SHOULD. Two entries
//! inside one strongly connected component cannot dominate each other, and 54 to 93 per cent
//! of these groups is one component - yet the share is high anyway, because the dominator
//! that does the work is usually OUTSIDE the component and above it. A hub reached through
//! one entry has that entry dominating everything in the hub. So `scc` explains which PAIRS
//! are unavailable and predicts the total badly; 1177 at 36 per cent in one component scores
//! 99.9, and 362 at 86 per cent scores 27.9 on its sparsest profile.
//!
//! ## Why the link graph is the sound one to take dominance over
//!
//! Guards are ignored here, and that is the safe direction rather than an approximation to
//! apologise for. Real paths are a subset of link paths, so a target on every LINK path to
//! `u` is on every real path to it. Ignoring guards can only make the relation report FEWER
//! dominators than hold, never more.
//!
//! ## How to run it
//!
//! ```text
//! tools/run-logged.sh cargo dominance-share -- \
//!   cargo run --release --example dominance_share
//! ```
//!
//! `rows` (the default) takes the heavy groups across the percentage profiles of the matrix
//! grid, including the four the prize was measured to be concentrated in. `menu` asks the
//! same question of a whole adversarial menu, where each option is a start of its own. `all`
//! sweeps every group in the game, which is the answer that settles it. `verify` re-derives
//! the dominator relation by deleting entries and comparing, which is what makes the share
//! believable rather than merely printed - 1,328,348 pairs over nine groups, no
//! disagreements.
//!
//! `CONVERSATION` overrides which groups `rows` walks, comma separated; `PROFILES` overrides
//! which percentages.

use std::collections::{HashMap, HashSet};

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, discover_group, read_index};
use lookahead_engine::symbolic::dominators::Dominators;
use lookahead_engine::symbolic::novelty_search::{Nearest, candidates_from};
use lookahead_engine::symbolic::order::IterationOrder;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

#[path = "seen_profile.rs"]
mod seen_profile;
use seen_profile::{candidates as profile_candidates, percent_unseen, structurally_reachable};

/// The groups `rows` walks unless `CONVERSATION` says otherwise: the heavy six, plus the
/// four the concentrated rows live in.
const GROUPS: [i32; 9] = [362, 368, 631, 14, 28, 1030, 825, 587, 640];

/// The percentages `rows` sweeps unless `PROFILES` says otherwise. The matrix grid's, minus
/// the deepest-N profiles, which name one to ten candidates and so have nothing to dominate.
const PERCENTS: [u32; 7] = [95, 90, 75, 50, 25, 10, 5];

/// The menu `menu` asks about: how many entries are unseen, and how many options.
///
/// THE SAME NUMBERS THE OTHER WHOLE-MENU MEASUREMENTS USE - `menu_residue`, `prune_on_menus`
/// and `cache_split_menu` all take ten unseen and twenty-four starts - so a row here is about
/// the same menu they are. Ten rather than one matters here in particular: one unseen entry
/// gives every option a candidate list of length one, which nothing can dominate and which
/// would report a zero that is arithmetic rather than a finding. `UNSEEN` and `STARTS` move
/// them.
const MENU_UNSEEN: usize = 10;
const MENU_STARTS: usize = 24;

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    match std::env::args().nth(1).as_deref() {
        Some("menu") => menu(&index),
        Some("all") => all(&index),
        Some("verify") => verify(&index),
        _ => rows(&index),
    }
}

/// One start per group, swept over the percentage profiles: the matrix's own population.
fn rows(index: &lookahead_engine::index::Index) {
    let groups = env_list("CONVERSATION", &GROUPS);
    let percents: Vec<u32> = match lookahead_engine::core::env::var("PROFILES") {
        Ok(value) => value
            .split(',')
            .filter_map(|p| p.trim().parse().ok())
            .collect(),
        Err(_) => PERCENTS.to_vec(),
    };

    println!("ONE START PER GROUP, over the percentage profiles the matrix rows use.\n");
    header();

    let mut totals = Totals::default();
    for conversation in groups {
        let Ok((graph, _)) = build_group_graph(index, conversation) else {
            continue;
        };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }
        let shape = Shape::of(&graph, &[start]);
        let profile_pool = profile_candidates(&graph, start);

        for &percent in &percents {
            let unseen = percent_unseen(&profile_pool, percent);
            let counted = count(&graph, &[start], &unseen);
            totals.add(&counted);
            row(
                &format!("{conversation}"),
                &format!("{percent}pc-seen"),
                &counted,
                &shape,
            );
        }
    }

    println!();
    totals.report("all rows");
    verdict(&totals);
}

/// Every option of one adversarial menu, which is the population de-a88z cares about.
fn menu(index: &lookahead_engine::index::Index) {
    let groups = env_list("CONVERSATION", &GROUPS);
    let starts_wanted = from_env("STARTS", MENU_STARTS);
    let unseen_wanted = from_env("UNSEEN", MENU_UNSEEN);

    println!(
        "EVERY OPTION OF ONE ADVERSARIAL MENU, {starts_wanted} starts over each group, the \
         structurally\ndeepest {unseen_wanted} entries unseen - the profile the whole-menu \
         measurements share.\n"
    );
    header();

    let mut totals = Totals::default();
    for conversation in groups {
        let Ok((graph, _)) = build_group_graph(index, conversation) else {
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            continue;
        }
        let Some(profile) = MenuProfile::of(&graph, root, unseen_wanted, starts_wanted) else {
            continue;
        };

        let mut group = Totals::default();
        for (option, &start) in profile.starts.iter().enumerate() {
            let shape = Shape::of(&graph, &[start]);
            let counted = count(&graph, &[start], &profile.unseen);
            group.add(&counted);
            totals.add(&counted);
            row(
                &format!("{conversation}"),
                &format!("option {}", option + 1),
                &counted,
                &shape,
            );
        }
        group.report(&format!("conversation {conversation}"));
        println!();
    }

    totals.report("all options");
    verdict(&totals);
}

/// Every group in the game, one start each, over the percentage profiles.
fn all(index: &lookahead_engine::index::Index) {
    let mut conversations: Vec<i32> = index.keys().copied().collect();
    conversations.sort_unstable();

    // One canonical start per distinct group: the smallest conversation whose own closure is
    // the whole set, which is what `performance_matrix` hands back as `DEGCT_CONVERSATION=`.
    let mut canonical: HashMap<Vec<i32>, i32> = HashMap::new();
    for &conversation in &conversations {
        let mut group = discover_group(index, conversation);
        group.sort_unstable();
        canonical.entry(group).or_insert(conversation);
    }
    let mut starts: Vec<i32> = canonical.into_values().collect();
    starts.sort_unstable();

    println!(
        "EVERY GROUP IN THE GAME: {} distinct groups over {} conversations, one start each,\n\
         over the {} percentage profiles.\n",
        starts.len(),
        conversations.len(),
        PERCENTS.len(),
    );

    let began = std::time::Instant::now();
    let mut totals = Totals::default();
    let mut with_any = 0usize;
    let mut walked = 0usize;
    // The rows worth naming: where the order actually collects something.
    let mut best: Vec<(i32, u32, Counted, Shape)> = Vec::new();

    for conversation in starts {
        let Ok((graph, _)) = build_group_graph(index, conversation) else {
            continue;
        };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }
        let shape = Shape::of(&graph, &[start]);
        let profile_pool = profile_candidates(&graph, start);
        if profile_pool.is_empty() {
            continue;
        }
        walked += 1;

        let mut any = false;
        for &percent in &PERCENTS {
            let unseen = percent_unseen(&profile_pool, percent);
            let counted = count(&graph, &[start], &unseen);
            totals.add(&counted);
            if counted.dominated > 0 {
                any = true;
                best.push((conversation, percent, counted, shape));
            }
        }
        if any {
            with_any += 1;
        }
    }

    println!("groups with rows        {walked}");
    println!("groups collecting any   {with_any}");
    println!("walked in               {:.1?}\n", began.elapsed());

    best.sort_unstable_by(|a, b| b.2.dominated.cmp(&a.2.dominated));
    if !best.is_empty() {
        println!("THE ROWS THAT COLLECT THE MOST, at most twenty:\n");
        header();
        for (conversation, percent, counted, shape) in best.iter().take(20) {
            row(
                &format!("{conversation}"),
                &format!("{percent}pc-seen"),
                counted,
                shape,
            );
        }
        println!();
    }

    totals.report("the whole game");
    verdict(&totals);
}

/// What one row's candidate list yields.
#[derive(Debug, Clone, Copy, Default)]
struct Counted {
    /// Candidates in the driver's order.
    candidates: usize,
    /// Of those, how many have a strict dominator anywhere in the list.
    has_dominator: usize,
    /// Of those, how many have one EARLIER in the list, which is what the order can collect.
    dominated: usize,
}

/// The structural facts about one start's reachable subgraph, computed once per start.
#[derive(Debug, Clone, Copy)]
struct Shape {
    reachable: usize,
    largest_component: usize,
}

impl Shape {
    fn of(graph: &LookAheadGraph, starts: &[DialogueNodeId]) -> Self {
        let reachable = starts
            .iter()
            .map(|start| structurally_reachable(graph, *start).len())
            .max()
            .unwrap_or(0);
        let order = IterationOrder::of(graph);
        Self {
            reachable,
            largest_component: order.largest_component(),
        }
    }

    /// The share of the group inside its largest strongly connected component, where
    /// dominance cannot hold between any two members.
    fn scc_share(&self) -> f64 {
        if self.reachable == 0 {
            return 0.0;
        }
        self.largest_component as f64 / self.reachable as f64
    }
}

/// Walks one row's candidate list in the driver's own order, counting what dominance gives.
fn count(
    graph: &LookAheadGraph,
    starts: &[DialogueNodeId],
    unseen: &HashSet<DialogueNodeId>,
) -> Counted {
    let novelty = |id: DialogueNodeId| {
        if unseen.contains(&id) {
            Novelty::UnseenAnyGame
        } else {
            Novelty::SeenThisGame
        }
    };
    // THE DRIVER'S OWN ORDER, from the driver's own function, so this cannot drift from what
    // it actually asks. `Nearest::First` is what a look-ahead uses; a census takes the other
    // end and is not what this is about.
    let ordered = candidates_from(graph, starts, &novelty, Nearest::First);
    // THE SHIPPED RELATION, not a copy of it. `Dominators` is what the driver skips
    // candidates with, so this measures the thing that runs and `verify` checks the thing
    // that runs. A second implementation here would be free to be right while the engine's
    // was wrong, which is the one arrangement worth avoiding.
    let doms = Dominators::of(graph, starts);

    let in_list: HashSet<DialogueNodeId> = ordered.iter().copied().collect();
    let mut asked: HashSet<DialogueNodeId> = HashSet::new();
    let mut counted = Counted {
        candidates: ordered.len(),
        ..Default::default()
    };

    for &target in &ordered {
        let mut has_dominator = false;
        let mut earlier = false;
        for node in doms.above(target) {
            if in_list.contains(&node) {
                has_dominator = true;
                if asked.contains(&node) {
                    earlier = true;
                    break;
                }
            }
        }

        counted.has_dominator += usize::from(has_dominator);
        counted.dominated += usize::from(earlier);
        asked.insert(target);
    }

    counted
}

/// Running totals over many rows.
#[derive(Debug, Default)]
struct Totals {
    rows: usize,
    candidates: usize,
    has_dominator: usize,
    dominated: usize,
    /// Rows with more than one candidate, which are the only ones dominance can touch.
    multi: usize,
    multi_candidates: usize,
}

impl Totals {
    fn add(&mut self, counted: &Counted) {
        self.rows += 1;
        self.candidates += counted.candidates;
        self.has_dominator += counted.has_dominator;
        self.dominated += counted.dominated;
        if counted.candidates > 1 {
            self.multi += 1;
            self.multi_candidates += counted.candidates;
        }
    }

    fn report(&self, what: &str) {
        println!("OVER {what}:");
        println!("  rows                       {}", self.rows);
        println!(
            "  rows with >1 candidate     {} ({})",
            self.multi,
            share(self.multi, self.rows),
        );
        println!("  candidates                 {}", self.candidates);
        println!(
            "  ... in those rows          {} ({})",
            self.multi_candidates,
            share(self.multi_candidates, self.candidates),
        );
        println!(
            "  with a strict dominator    {} ({} of candidates)",
            self.has_dominator,
            share(self.has_dominator, self.candidates),
        );
        println!(
            "  with an EARLIER one        {} ({} of candidates)",
            self.dominated,
            share(self.dominated, self.candidates),
        );
    }
}

fn verdict(totals: &Totals) {
    println!();
    println!(
        "THE PRIZE IS THE LAST LINE: those candidates are the ones a refusal of an earlier \
         one\nwould have answered with no fixed point at all. Multiply it by the measured \
         cost of a\nrefusal to price the idea; the line above it is the ceiling a better \
         ORDER could reach."
    );
    if totals.dominated == 0 {
        println!(
            "\nZERO IS AN ANSWER, and it is the one de-cnjw's sentence predicted: nothing \
             to build."
        );
    }
}

fn header() {
    println!(
        "{:>6}  {:>12}  {:>6}  {:>9}  {:>9}  {:>7}  {:>5}",
        "conv", "profile", "cands", "has-dom", "dominated", "share", "scc",
    );
}

fn row(conversation: &str, profile: &str, counted: &Counted, shape: &Shape) {
    println!(
        "{:>6}  {:>12}  {:>6}  {:>9}  {:>9}  {:>7}  {:>4.0}%",
        conversation,
        profile,
        counted.candidates,
        counted.has_dominator,
        counted.dominated,
        share(counted.dominated, counted.candidates),
        shape.scc_share() * 100.0,
    );
}

fn share(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "-".to_string();
    }
    format!("{:.1}%", part as f64 * 100.0 / whole as f64)
}

fn from_env(name: &str, fallback: usize) -> usize {
    lookahead_engine::core::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

fn env_list(name: &str, fallback: &[i32]) -> Vec<i32> {
    match lookahead_engine::core::env::var(name) {
        Ok(value) => value
            .split(',')
            .filter_map(|c| c.trim().parse().ok())
            .collect(),
        Err(_) => fallback.to_vec(),
    }
}

/// Checks the dominator tree against the definition, by deletion.
///
/// ## Why this arm exists rather than a unit test
///
/// The share this measurement reports is large, and a large number out of a graph algorithm
/// is exactly the kind of finding that should not be believed on the strength of the
/// algorithm looking right. The definition is directly checkable: `t` dominates `u` from `s`
/// exactly when deleting `t` makes `u` unreachable from `s`. So this re-derives the whole
/// relation the slow, obvious way and compares.
///
/// IT CHECKS THE SHIPPED RELATION. `symbolic::dominators::Dominators` is what the backward
/// driver skips candidates with, and it is what this asks - so this is a soundness check on
/// the engine rather than on a measurement's private copy. The unit tests beside that module
/// cover the shapes; this covers the real database, where the shapes are all mixed together.
///
/// It is quadratic and deliberately so - one reachability walk per entry - which is why it
/// runs over the small groups and a sample of the large ones rather than the game.
fn verify(index: &lookahead_engine::index::Index) {
    let groups = env_list("CONVERSATION", &GROUPS);
    println!(
        "CHECKING THE DOMINATOR TREE AGAINST THE DEFINITION, by deleting each entry and \
         asking\nwhich others stop being reachable. Disagreement is a defect in this \
         measurement.\n"
    );

    let mut checked = 0usize;
    let mut wrong = 0usize;
    for conversation in groups {
        let Ok((graph, _)) = build_group_graph(index, conversation) else {
            continue;
        };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let doms = Dominators::of(&graph, &[start]);
        let reachable: Vec<DialogueNodeId> =
            structurally_reachable(&graph, start).into_keys().collect();

        // A SAMPLE OF THE CUT POINTS, because this is one walk per entry deleted and the
        // heavy groups hold thousands. Every entry of a small group, and a spread over a
        // large one, taken by stride so the sample is not the shallow end.
        let stride = (reachable.len() / SAMPLED_CUTS).max(1);
        let mut disagreed: Vec<(DialogueNodeId, DialogueNodeId, bool)> = Vec::new();

        for cut in reachable.iter().step_by(stride) {
            if *cut == start {
                continue;
            }
            let without = reachable_without(&graph, start, *cut);
            for &id in &reachable {
                if id == start || id == *cut {
                    continue;
                }
                let by_deletion = !without.contains(&id);
                let by_tree = doms.dominates(*cut, id);
                checked += 1;
                if by_deletion != by_tree {
                    wrong += 1;
                    if disagreed.len() < SHOWN_DISAGREEMENTS {
                        disagreed.push((*cut, id, by_deletion));
                    }
                }
            }
        }

        println!(
            "{conversation:>6}  {:>5} entries, {:>4} cut points sampled",
            reachable.len(),
            reachable.len().div_ceil(stride),
        );
        for (cut, id, by_deletion) in disagreed {
            println!(
                "        DISAGREES: does {cut:?} dominate {id:?}? deletion says \
                 {by_deletion}, the tree says {}",
                doms.dominates(cut, id),
            );
        }
    }

    println!("\npairs checked   {checked}");
    println!("disagreements   {wrong}");
    if wrong == 0 {
        println!("\nThe tree agrees with the definition on every pair checked.");
    }
}

/// How many entries of one group to delete, at most. One reachability walk each.
const SAMPLED_CUTS: usize = 60;

/// How many disagreements to name per group before the count speaks for itself.
const SHOWN_DISAGREEMENTS: usize = 5;

/// Everything reachable from `start` when `cut` is not there.
fn reachable_without(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    cut: DialogueNodeId,
) -> HashSet<DialogueNodeId> {
    let mut seen: HashSet<DialogueNodeId> = HashSet::new();
    let mut stack = vec![start];
    seen.insert(start);
    while let Some(id) = stack.pop() {
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if child == cut || graph.get(child).is_none() || !seen.insert(child) {
                continue;
            }
            stack.push(child);
        }
    }
    seen
}
