// SPDX-License-Identifier: MIT
//! How often do two options of one menu ask about the SAME target?
//!
//! ## The one number de-a88z turns on, and it comes before any of it is built
//!
//! That issue would remember what a backward pass settled about a target, so a later
//! candidate list does not pay for it again. Its own notes say where the reuse could be and
//! where it cannot:
//!
//! WITHIN ONE OPTION THERE IS NOTHING TO LOOK UP. The driver walks a candidate list and
//! every entry in it is a different target, so no target is asked about twice. Sharing
//! inside one option needs a different relation entirely, which is de-kqgq's dominance.
//!
//! BETWEEN OPTIONS IS THE ORDINARY CASE. `bridge::answer_within` answers a whole menu
//! against one manager and one compiler - one world, one seed - and `candidates_from`
//! selects each start's link-reachable unseen entries. Two options of one node lead into
//! largely the same subgraph, so their lists should hold largely the same entries, differing
//! mostly in ORDER because distance is measured from each start.
//!
//! Should. Nothing had measured it, and if the options ask disjoint questions the whole idea
//! is worth nothing.
//!
//! ## What this measures, and what it deliberately does not
//!
//! THE LISTS, NOT THE VERDICTS. A candidate list is a function of the link graph and the
//! novelty function, so this needs no diagrams, no guards and no world, and a whole menu
//! costs milliseconds. What it reports is therefore a CEILING on the reuse: it counts every
//! repeated ASK, where a memo only pays on a repeated ask whose first answer was a settled
//! refusal.
//!
//! That is the right shape for a gate. A ceiling near one says stop; a ceiling well above
//! one says the expensive half is worth measuring.
//!
//! ## PER OPTION, NOT PER REQUEST, which is the trap this exists to avoid
//!
//! The reuse is between options, so what it is worth scales with MENU WIDTH - and the widths
//! are far apart. de-8hh2.13's realistic menu is three options; twenty-four is the rolled
//! check worst case, where every option is two starts. A figure quoted for a request of
//! twenty-four would be about the worst case wearing the name of the ordinary one, so every
//! row here is reported at three, eight and twenty-four and the three are not summarised
//! into one.
//!
//! ## What it said, 2026-09-08: the options ask very nearly the SAME LIST
//!
//! Real response menus - one node's own links as the options - over nine groups, forty
//! menus each, widest first, under the seven percentage profiles:
//!
//! ```text
//!      profile      asks   targets    repeat    share  asks each
//!     5pc-seen   2418469    412147   2006322   83.0%       5.87
//!    50pc-seen   1273903    217273   1056630   82.9%       5.86
//!    95pc-seen    126771     21620    105151   82.9%       5.86
//! ```
//!
//! and split by how wide the menu is, which is the split that matters:
//!
//! ```text
//!   options    menus      asks   targets  asks each
//!         3      349    364301    122795       2.97
//!         4      692   1256189    319935       3.93
//!         5      607   2268961    458438       4.95
//!         7      203   1182634    171578       6.89
//!        10       42    384130     39272       9.78
//!        28        7     90812      3363      27.00
//! ```
//!
//! `asks each` IS THE MENU'S WIDTH, to within a few per cent, at every width. Three options
//! ask about the average target 2.97 times out of a possible 3. So de-a88z's premise holds
//! and holds strongly: two options of one node do lead into the same subgraph, and their
//! candidate lists are very nearly the same list.
//!
//! AND IT DOES NOT MOVE WITH THE PROFILE - 83.0 per cent repeated asks at five per cent seen
//! and 82.9 at ninety-five. The overlap is a fact about the graph rather than about what the
//! player has read, which is worth knowing because it means no profile makes the reuse go
//! away and none has to be argued about.
//!
//! ## AND THEN de-rn59.4 TOOK NEARLY ALL OF IT AWAY, which is the finding that decides it
//!
//! The figures above are candidate LISTS. Since the driver refuses a dominated candidate
//! with no fixed point, what a search actually ASKS is a small fraction of its list, and a
//! candidate that costs nothing is one there is nothing to remember about. The same menus,
//! counting only what is asked:
//!
//! ```text
//!   options    menus      asks  asks each     asked  each after
//!         3      349    364301       2.97      8375        1.91
//!         5      607   2268961       4.95     50649        3.05
//!        10       42    384130       9.78      8793        5.39
//!        31        7     80793      29.05      3993       11.06
//! ```
//!
//! A THREE-OPTION MENU ASKS ABOUT 3.4 TARGETS IN TOTAL, of which 1.8 are distinct - so a
//! memo would save about 1.6 fixed points a menu. On the three heavy groups whose search does
//! not settle, where a fixed point is expensive rather than cheap, it is 0.8.
//!
//! That is what a memo is worth now, and it is not worth a per-request object threaded
//! through five signatures. The saving does grow with width - eleven fixed points a menu at
//! five options - so this is a decision about menu width and not a permanent one. See
//! de-a88z, which is closed on these numbers rather than on the idea being wrong.
//!
//! ## The adversarial menu says 100 per cent, and that number should not be quoted
//!
//! `deepest` reports every option asking about exactly the same ten entries, at every width.
//! That is `MenuProfile` talking, not the graph: it picks the shallowest entries that can
//! reach the deepest ones, so every option is selected for reaching one small deep set and
//! reaches all of it. It is the right fixture for what a menu COSTS and the wrong one for
//! what two options SHARE. The arm is kept because the contrast is the point - a fixture can
//! flatter a number as easily as it can flatten one, and this is the direction nobody checks.
//!
//! ## BETWEEN MENUS IS A DIFFERENT AND MUCH LARGER NUMBER, 2026-09-08
//!
//! Everything above is one node's options, which is what a memo living inside one request
//! could collect. A request is one menu; a session is hours of them in a handful of groups,
//! and the `walk` arm counts that population - the group walked in link order, marking what
//! it passes as seen, forty menus each over the same nine groups:
//!
//! ```text
//!      profile   menus     asked  distinct   already      share   asks each
//!     5pc-seen     347      1861      1592       269      14.5%        1.17
//!    50pc-seen     347      6119      1766      4353      71.1%        3.46
//!    95pc-seen     346     11213       528     10685      95.3%       21.24
//! ```
//!
//! IT MOVES WITH THE PROFILE, which is the opposite of what `links` found and is the whole
//! finding. Overlap between the options of one node is a fact about the graph and sits at 83
//! per cent whatever the player has read. Overlap between successive menus is a fact about
//! how much is LEFT: at five per cent seen a walk barely repeats itself, because there are
//! unseen entries everywhere and each menu has its own; at ninety-five there are a handful
//! left in the group and every menu asks about the same handful.
//!
//! At the top end that is 32 asks a menu of which 31 are repeats. The regime where a memo
//! would pay is therefore the late one - a player deep in a conversation they have mostly
//! read - and it is not a small effect there.
//!
//! WHAT THE NUMBER DOES NOT SAY is whether any of it is collectable. Nothing in the arm
//! invalidates anything, and the walk moves the seen set at every step, so a memo keyed on
//! the world snapshot would discard all of it. See de-znov: the ceiling decides that an
//! invalidation rule is worth designing, and says nothing about which of the three shapes
//! there could work.
//!
//! AND TWO THIRDS OF THESE ASKS NEVER REACH THE BACKWARD DRIVER, 2026-09-09. An option whose
//! start already carries what is hunted runs no pass, so its candidates cost nothing and there is
//! nothing about them to remember - the same subtraction dominance makes, one stage later.
//! `cacheable_asks` runs the very asks this arm counts and puts it at 3361 of 9882. Of those
//! that do reach the driver, all 3361 settled without meeting and 68.6 per cent were repeats,
//! so the share above survives the subtraction: read against passes actually spent rather
//! than against this table.
//!
//! ## How to run it
//!
//! ```text
//! DEGCT_RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo candidate-recurrence -- \
//!   cargo run --release --example candidate_recurrence
//! DEGCT_RUN_LOG_DIR=measurements/logs tools/run-logged.sh cargo recurrence-walk -- \
//!   cargo run --release --example candidate_recurrence walk
//! ```
//!
//! `links` is the default and is the faithful one; `deepest` is the adversarial contrast;
//! `walk` is successive menus rather than one menu's options. `CONVERSATION` picks the
//! groups, `PROFILES` the percentages, `MENUS` how many menus to take from a group, `UNSEEN`
//! and `WIDTHS` the deepest arm's profile and widths.

use std::collections::{HashMap, HashSet};

use lookahead_engine::core::types::{DialogueNodeId, Novelty};
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::novelty_search::{Nearest, candidates_from};

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

#[path = "seen_profile.rs"]
mod seen_profile;

#[path = "menu_walk.rs"]
mod menu_walk;

/// The groups to ask about: the heavy six the measurements share, plus the three de-kqgq
/// found the refusals concentrated in.
const GROUPS: [i32; 9] = [362, 368, 631, 14, 28, 1030, 825, 587, 640];

/// The menu widths reported side by side. Three is de-8hh2.13's realistic menu; twenty-four
/// is the rolled-check worst case the other whole-menu measurements use.
const WIDTHS: [usize; 3] = [3, 8, 24];

/// How many of the deepest entries the profile leaves unseen, and how many options to build.
///
/// The same ten `menu_residue`, `prune_on_menus` and `cache_split_menu` take, so a row here
/// is about the menu they measure.
const UNSEEN: usize = 10;
const STARTS: usize = 24;

/// The percentage-seen profiles the `links` arm sweeps, which are the matrix grid's.
const PERCENTS: [u32; 7] = [95, 90, 75, 50, 25, 10, 5];

/// How many menus to take from one group, widest first.
///
/// Widest first because that is where the reuse, if any, is largest, so a sample that found
/// nothing there found nothing anywhere.
const MENUS: usize = 40;

/// How many links make a menu, which is [`menu_walk::MIN_OPTIONS`] because the two arms
/// have to agree about what a menu is.
const MIN_OPTIONS: usize = menu_walk::MIN_OPTIONS;

fn main() {
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    match std::env::args().nth(1).as_deref() {
        Some("deepest") => deepest(&index),
        Some("walk") => walk(&index),
        _ => links(&index),
    }
}

/// How many menus one walk through a group counts before it stops.
///
/// Forty, the same sample `links` takes, so the two arms are the same size of thing said
/// about the same groups. A walk that runs out of unvisited menus first stops there.
const WALK_MENUS: usize = 40;

/// Successive menus along a walk through one group, which is the population de-znov is about.
///
/// ## The question, and why `links` cannot answer it
///
/// `links` counts what the options of ONE node share, which is what a memo living inside one
/// request could collect. de-a88z was closed on that number: after de-rn59.4 refuses a
/// dominated candidate for free, a three-option menu asks about 3.4 targets of which 1.8 are
/// distinct, and 1.6 saved fixed points does not pay for the plumbing.
///
/// A request is one menu. de-znov is about what survives BETWEEN them, and a session is
/// hours of menus in a handful of groups - a different and larger population, which nothing
/// had counted.
///
/// ## What a walk is here
///
/// [`menu_walk`], which is where it lives because `cacheable_asks` runs the very asks this
/// counts and the two numbers are multiplied together. A walk that runs out of unvisited
/// menus before [`WALK_MENUS`] stops there.
///
/// ## It is a ceiling, and deliberately
///
/// Nothing here invalidates anything. A verdict is counted as reusable whenever a later menu
/// asks about a target an earlier one already asked about, whatever the world did in between
/// - and the world does move, since the seed carries what has been seen and this walk is
/// changing exactly that. A memo keyed on the world snapshot would throw all of it away.
///
/// So the number decides whether an invalidation rule is worth designing, not whether one
/// would work. Near one says close de-znov unbuilt.
///
/// AND IT COUNTS ASKS THAT NEVER REACH THE BACKWARD DRIVER, which `cacheable_asks` measures
/// and this cannot: an option whose start already carries what is hunted runs no pass, so there is
/// nothing to remember about its candidates. That is the largest subtraction from this
/// number after dominance.
fn walk(index: &lookahead_engine::index::Index) {
    let groups = env_list("CONVERSATION", &GROUPS);
    let percents: Vec<u32> = match lookahead_engine::core::env::var("PROFILES") {
        Ok(value) => value
            .split(',')
            .filter_map(|p| p.trim().parse().ok())
            .collect(),
        Err(_) => PERCENTS.to_vec(),
    };
    let menus_wanted = from_env("MENUS", WALK_MENUS);

    println!(
        "SUCCESSIVE MENUS IN ONE GROUP: the group walked in link order, marking what it \
         passes\nas seen. `asked` is what the menus ask about once dominance has refused what \
         it can,\nsummed; `already` is how much of that an EARLIER menu of the same walk had \
         asked about.\n"
    );
    println!(
        "A CEILING. Nothing here invalidates anything, and the walk moves the seen set at \
         every\nstep - so this says whether an invalidation rule is worth designing, not \
         whether one\nwould work.\n"
    );
    println!(
        "{:>6}  {:>12}  {:>6}  {:>8}  {:>8}  {:>8}  {:>9}",
        "conv", "profile", "menus", "asked", "distinct", "already", "share",
    );

    let mut overall: HashMap<u32, menu_walk::Recurrence> = HashMap::new();

    for conversation in groups {
        let Ok((graph, _)) = build_group_graph(index, conversation) else {
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            continue;
        }
        let pool = seen_profile::candidates(&graph, root);

        for &percent in &percents {
            let menus = menu_walk::menus(
                &graph,
                root,
                seen_profile::percent_unseen(&pool, percent),
                menus_wanted,
            );
            let counted = menu_walk::Recurrence::of(&menus);
            if counted.menus == 0 {
                continue;
            }
            println!(
                "{:>6}  {:>12}  {:>6}  {:>8}  {:>8}  {:>8}  {:>9}",
                conversation,
                format!("{percent}pc-seen"),
                counted.menus,
                counted.asked,
                counted.first,
                counted.repeat,
                share(counted.repeat, counted.asked),
            );
            overall.entry(percent).or_default().add(&counted);
        }
        println!();
    }

    let mut seen: Vec<u32> = overall.keys().copied().collect();
    seen.sort_unstable();
    println!("OVER EVERY GROUP, per profile:\n");
    println!(
        "{:>12}  {:>6}  {:>8}  {:>8}  {:>8}  {:>9}  {:>10}",
        "profile", "menus", "asked", "distinct", "already", "share", "asks each",
    );
    for percent in seen {
        let totals = &overall[&percent];
        println!(
            "{:>12}  {:>6}  {:>8}  {:>8}  {:>8}  {:>9}  {:>10.2}",
            format!("{percent}pc-seen"),
            totals.menus,
            totals.asked,
            totals.first,
            totals.repeat,
            share(totals.repeat, totals.asked),
            asks_each(totals),
        );
    }
    println!(
        "\n`asks each` is how many menus of one walk ask about the average target. At one, \
         no\nmenu ever asks about a target another already settled and there is nothing for a \
         memo\nto carry between requests, whatever key it were given.\n"
    );
}

/// The faithful arm: a menu's options are ONE NODE'S OWN CHILDREN.
///
/// ## Why this and not the adversarial profile
///
/// A response menu in the game is a node and the entries it links to. `MenuProfile` builds
/// something else on purpose - the shallowest entries that can reach the deepest ones - which
/// is right for measuring what a menu COSTS, because it guarantees every option pays for a
/// real search. It is wrong for measuring what two options SHARE, and wrong in the direction
/// that flatters the answer: the profile picks its options for reaching one small deep set,
/// so every one of them reaches all of it and the overlap is total by construction. See
/// `deepest`, which reports exactly that and is kept for the contrast.
///
/// Here the options are a real node's real links, and the profile is the percentage-seen draw
/// a matrix row uses, so the unseen set is scattered over the group rather than pooled at the
/// bottom of it.
fn links(index: &lookahead_engine::index::Index) {
    let groups = env_list("CONVERSATION", &GROUPS);
    let percents: Vec<u32> = match lookahead_engine::core::env::var("PROFILES") {
        Ok(value) => value
            .split(',')
            .filter_map(|p| p.trim().parse().ok())
            .collect(),
        Err(_) => PERCENTS.to_vec(),
    };
    let menus_wanted = from_env("MENUS", MENUS);

    println!(
        "REAL RESPONSE MENUS: one node's own links are the options, under the \
         percentage-seen\nprofiles a matrix row uses. `asks` is every option's candidate \
         list summed, `targets` is\nhow many distinct entries those asks are about, and \
         `asks each` is how many options ask\nabout the average target - the number a memo \
         is worth per entry it holds.\n"
    );
    println!(
        "The last two columns are the same question of what a search ACTUALLY ASKS now that\n\
         de-rn59.4 refuses a dominated candidate for free: a candidate costing no fixed \
         point\nis one there is nothing to remember about.\n"
    );
    println!(
        "{:>6}  {:>12}  {:>6}  {:>7}  {:>8}  {:>7}  {:>9}  {:>8}  {:>9}",
        "conv",
        "profile",
        "menus",
        "options",
        "asks",
        "targets",
        "asks each",
        "asked",
        "each after",
    );

    let mut overall: HashMap<u32, Totals> = HashMap::new();
    let mut widths: HashMap<usize, Totals> = HashMap::new();
    let mut overall_after: HashMap<u32, Totals> = HashMap::new();
    let mut widths_after: HashMap<usize, Totals> = HashMap::new();

    for conversation in groups {
        let Ok((graph, _)) = build_group_graph(index, conversation) else {
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            continue;
        }
        let menus = wide_nodes(&graph, root, menus_wanted);
        if menus.is_empty() {
            continue;
        }
        let pool = seen_profile::candidates(&graph, root);

        for &percent in &percents {
            let unseen = seen_profile::percent_unseen(&pool, percent);
            let novelty = |id: DialogueNodeId| {
                if unseen.contains(&id) {
                    Novelty::UnseenAnyGame
                } else {
                    Novelty::SeenThisGame
                }
            };

            let mut group = Counted::default();
            let mut group_after = Counted::default();
            let mut menus_counted = 0usize;
            for options in &menus {
                let lists: Vec<Vec<DialogueNodeId>> = options
                    .iter()
                    .map(|start| candidates_from(&graph, &[*start], &novelty, Nearest::First))
                    .collect();
                // A MENU WHOSE OPTIONS ASK NOTHING is not a disjoint menu, it is no menu.
                // Counting one would put a zero into the average that is about the profile
                // rather than about sharing.
                if lists.iter().all(|list| list.is_empty()) {
                    continue;
                }
                menus_counted += 1;
                let counted = count(&lists, lists.len());
                group.merge(&counted);
                widths.entry(counted.options).or_default().add(&counted);

                // AND THE SAME QUESTION OF WHAT IS LEFT AFTER DOMINANCE, which is the one
                // that matters now that de-rn59.4 ships. Most of a candidate list is refused
                // for free by something above it, and a candidate that costs no fixed point
                // is one there is nothing to remember about.
                let asked: Vec<Vec<DialogueNodeId>> = options
                    .iter()
                    .zip(&lists)
                    .map(|(start, list)| menu_walk::minimal(&graph, *start, list))
                    .collect();
                let after = count(&asked, asked.len());
                group_after.merge(&after);
                widths_after.entry(after.options).or_default().add(&after);
            }

            if menus_counted == 0 {
                continue;
            }
            overall.entry(percent).or_default().add(&group);
            overall_after.entry(percent).or_default().add(&group_after);
            println!(
                "{:>6}  {:>12}  {:>6}  {:>7}  {:>8}  {:>7}  {:>9.2}  {:>8}  {:>9.2}",
                conversation,
                format!("{percent}pc-seen"),
                menus_counted,
                group.options,
                group.asks,
                group.targets,
                group.asks_each(),
                group_after.asks,
                group_after.asks_each(),
            );
        }
        println!();
    }

    let mut seen: Vec<u32> = overall.keys().copied().collect();
    seen.sort_unstable();
    println!("OVER EVERY GROUP, per profile:\n");
    println!(
        "{:>12}  {:>8}  {:>8}  {:>9}  {:>8}  {:>8}  {:>10}",
        "profile", "asks", "targets", "asks each", "asked", "targets", "each after",
    );
    for percent in &seen {
        let totals = &overall[percent];
        let after = &overall_after[percent];
        println!(
            "{:>12}  {:>8}  {:>8}  {:>9.2}  {:>8}  {:>8}  {:>10.2}",
            format!("{percent}pc-seen"),
            totals.asks,
            totals.targets,
            totals.asks_each(),
            after.asks,
            after.targets,
            after.asks_each(),
        );
    }

    let mut sizes: Vec<usize> = widths.keys().copied().collect();
    sizes.sort_unstable();
    println!("\nBY HOW WIDE THE MENU IS, since the reuse is between options:\n");
    println!(
        "{:>7}  {:>7}  {:>8}  {:>9}  {:>8}  {:>10}",
        "options", "menus", "asks", "asks each", "asked", "each after",
    );
    for size in &sizes {
        let totals = &widths[size];
        let after = widths_after.get(size);
        println!(
            "{:>7}  {:>7}  {:>8}  {:>9.2}  {:>8}  {:>10.2}",
            size,
            totals.rows,
            totals.asks,
            totals.asks_each(),
            after.map(|a| a.asks).unwrap_or(0),
            after.map(|a| a.asks_each()).unwrap_or(0.0),
        );
    }

    println!(
        "\n`each after` IS THE NUMBER THE DESIGN TURNS ON, and `asks each` is what it was \
         before\nde-rn59.4. Both say how many options ask about the average target, which is \
         what a memo\nis asked for per entry it holds; at one, nothing is ever looked up \
         twice and there is\nnothing to build. Read only the second: a candidate the \
         dominance rule refuses for free\ncosts no fixed point, so there is nothing about it \
         to remember."
    );
    println!(
        "\nBOTH ARE CEILINGS. A memo pays only where the first ask was a SETTLED REFUSAL, \
         and this\ncounts every repeated ask - so a menu whose options find something early \
         is counted at\nfull price here and would pay far less."
    );
}

/// Menus, widest first: the nodes with the most links to entries a search could ask about.
///
/// A GROUP ENTRY IS NOT AN OPTION - the game expands one in place and never writes its
/// SimStatus - so a node whose links are all groups is not a menu however many it has.
fn wide_nodes(
    graph: &lookahead_engine::graph::graph::LookAheadGraph,
    root: DialogueNodeId,
    wanted: usize,
) -> Vec<Vec<DialogueNodeId>> {
    let reachable = seen_profile::structurally_reachable(graph, root);
    let mut menus: Vec<Vec<DialogueNodeId>> = Vec::new();
    for (&id, _) in reachable.iter() {
        let Some(node) = graph.get(id) else { continue };
        let options: Vec<DialogueNodeId> = node
            .links
            .iter()
            .copied()
            .filter(|child| graph.get(*child).is_some_and(|node| !node.is_group))
            .collect();
        if options.len() >= MIN_OPTIONS {
            menus.push(options);
        }
    }

    // Widest first, and the identifiers break the tie so the sample is the same sample on
    // every run rather than whatever the hash map happened to yield.
    menus.sort_unstable_by(|a, b| {
        b.len().cmp(&a.len()).then_with(|| {
            let key = |m: &Vec<DialogueNodeId>| {
                m.first()
                    .map(|id| (id.conversation_id, id.entry_id))
                    .unwrap_or_default()
            };
            key(a).cmp(&key(b))
        })
    });
    menus.truncate(wanted);
    menus
}

/// The adversarial arm, kept for the contrast it draws rather than for its number.
fn deepest(index: &lookahead_engine::index::Index) {
    let groups = env_list("CONVERSATION", &GROUPS);
    let widths: Vec<usize> = match lookahead_engine::core::env::var("WIDTHS") {
        Ok(value) => value
            .split(',')
            .filter_map(|w| w.trim().parse().ok())
            .collect(),
        Err(_) => WIDTHS.to_vec(),
    };
    let unseen_wanted = from_env("UNSEEN", UNSEEN);

    println!(
        "HOW OFTEN TWO OPTIONS OF ONE MENU ASK ABOUT THE SAME TARGET, the structurally \
         deepest\n{unseen_wanted} entries unseen. `asks` is every option's list summed; \
         `targets` is how many\ndistinct entries those asks are about; `repeat` is the \
         difference, which is what a memo\nwould answer without a fixed point.\n"
    );
    println!(
        "{:>6}  {:>7}  {:>7}  {:>8}  {:>7}  {:>7}  {:>9}",
        "conv", "options", "asks", "targets", "repeat", "share", "asks each",
    );

    let mut overall: HashMap<usize, Totals> = HashMap::new();
    let mut widest: Vec<(i32, Vec<Option<Overlap>>)> = Vec::new();

    for conversation in groups {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            continue;
        }
        let Some(profile) = MenuProfile::of(&graph, root, unseen_wanted, STARTS) else {
            continue;
        };

        let novelty = |id: DialogueNodeId| {
            if profile.unseen.contains(&id) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        };
        // ONE LIST PER OPTION, from the driver's own function so this cannot drift from what
        // it asks. Built once and sliced per width, since a menu of three is the first three
        // options of the menu of twenty-four rather than a different menu.
        let lists: Vec<Vec<DialogueNodeId>> = profile
            .starts
            .iter()
            .map(|start| candidates_from(&graph, &[*start], &novelty, Nearest::First))
            .collect();

        for &width in &widths {
            let counted = count(&lists, width);
            overall.entry(width).or_default().add(&counted);
            println!(
                "{:>6}  {:>7}  {:>7}  {:>8}  {:>7}  {:>6}  {:>9.2}",
                conversation,
                counted.options,
                counted.asks,
                counted.targets,
                counted.repeat,
                share(counted.repeat, counted.asks),
                counted.asks_each(),
            );
        }
        println!();

        widest.push((
            conversation,
            per_option(&lists, *widths.iter().max().unwrap_or(&STARTS)),
        ));
    }

    println!("PER OPTION at the widest menu: how much of each option's list an EARLIER");
    println!("option had already asked about. Option one can never have any, by definition.\n");
    println!(
        "{:>6}  {:>7}  {:>6}  {:>7}  {:>6}",
        "conv", "option", "asks", "already", "share"
    );
    for (conversation, options) in &widest {
        for (at, overlap) in options.iter().enumerate() {
            let Some(overlap) = overlap else { continue };
            println!(
                "{:>6}  {:>7}  {:>6}  {:>7}  {:>5}",
                conversation,
                at + 1,
                overlap.asks,
                overlap.already,
                share(overlap.already, overlap.asks),
            );
        }
        println!();
    }

    let mut widths_seen: Vec<usize> = overall.keys().copied().collect();
    widths_seen.sort_unstable();
    println!("OVER EVERY GROUP, per menu width:\n");
    println!(
        "{:>7}  {:>8}  {:>8}  {:>8}  {:>7}  {:>9}",
        "options", "asks", "targets", "repeat", "share", "asks each",
    );
    for width in &widths_seen {
        let totals = &overall[width];
        println!(
            "{:>7}  {:>8}  {:>8}  {:>8}  {:>6}  {:>9.2}",
            width,
            totals.asks,
            totals.targets,
            totals.repeat,
            share(totals.repeat, totals.asks),
            totals.asks_each(),
        );
    }

    println!(
        "\n`asks each` IS THE NUMBER THE DESIGN TURNS ON. It is how many options ask about \
         the\naverage target, so it is what a memo is asked for per entry it holds. At one, \
         nothing\nis ever looked up twice and there is nothing to build. The three rows are \
         three menu\nwidths and not three estimates of one figure: the reuse is between \
         options, so what it\nis worth scales with how many there are."
    );
}

/// What one menu of a given width yields.
#[derive(Debug, Clone, Copy, Default)]
struct Counted {
    options: usize,
    /// Every option's list length, summed: the questions the menu puts.
    asks: usize,
    /// How many distinct entries those questions are about.
    targets: usize,
    /// The difference, which is what a memo would answer without a fixed point.
    repeat: usize,
}

impl Counted {
    /// How many options ask about the average target.
    fn asks_each(&self) -> f64 {
        if self.targets == 0 {
            return 0.0;
        }
        self.asks as f64 / self.targets as f64
    }

    /// Adds another menu's count to this one.
    ///
    /// TARGETS ARE SUMMED, NOT UNIONED, and that is the honest way round: a memo lives inside
    /// one request answering one menu, so an entry two DIFFERENT menus both ask about is two
    /// entries as far as any memo is concerned. Unioning would credit sharing that no memo in
    /// this design could collect.
    fn merge(&mut self, other: &Counted) {
        self.options += other.options;
        self.asks += other.asks;
        self.targets += other.targets;
        self.repeat += other.repeat;
    }
}

/// One option's overlap with the options before it.
#[derive(Debug, Clone, Copy)]
struct Overlap {
    asks: usize,
    already: usize,
}

fn count(lists: &[Vec<DialogueNodeId>], width: usize) -> Counted {
    let taken = &lists[..width.min(lists.len())];
    let mut seen: HashSet<DialogueNodeId> = HashSet::new();
    let mut asks = 0usize;
    for list in taken {
        asks += list.len();
        seen.extend(list.iter().copied());
    }

    Counted {
        options: taken.len(),
        asks,
        targets: seen.len(),
        repeat: asks - seen.len(),
    }
}

/// For each option, how much of its list an earlier option had already asked about.
fn per_option(lists: &[Vec<DialogueNodeId>], width: usize) -> Vec<Option<Overlap>> {
    let mut seen: HashSet<DialogueNodeId> = HashSet::new();
    lists
        .iter()
        .take(width)
        .map(|list| {
            let already = list.iter().filter(|id| seen.contains(*id)).count();
            seen.extend(list.iter().copied());
            Some(Overlap {
                asks: list.len(),
                already,
            })
        })
        .collect()
}

#[derive(Debug, Default)]
struct Totals {
    /// How many menus went into this, which is what the per-width table counts.
    rows: usize,
    asks: usize,
    targets: usize,
    repeat: usize,
}

impl Totals {
    fn add(&mut self, counted: &Counted) {
        self.rows += 1;
        self.asks += counted.asks;
        // SUMMED PER GROUP rather than over a union of the groups: two groups share no
        // entries, so a union would be the same number spelled more expensively - and a
        // memo lives inside one request, which is inside one group.
        self.targets += counted.targets;
        self.repeat += counted.repeat;
    }

    fn asks_each(&self) -> f64 {
        if self.targets == 0 {
            return 0.0;
        }
        self.asks as f64 / self.targets as f64
    }
}

/// How many menus of one walk ask about the average target.
///
/// The ratio this arm is FOR, which is why it lives here rather than on the count: the arm
/// that runs the asks reports what they cost and never takes a ratio of them.
fn asks_each(counted: &menu_walk::Recurrence) -> f64 {
    if counted.first == 0 {
        return 0.0;
    }
    counted.asked as f64 / counted.first as f64
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
