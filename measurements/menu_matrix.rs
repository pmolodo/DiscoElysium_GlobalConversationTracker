// SPDX-License-Identifier: MIT
//! What a whole MENU costs, over every group in the game.
//!
//! ## Why a whole menu and not one search
//!
//! A search from one start is not what a player waits for. A request is a
//! whole response menu: `bridge::answer_starts` runs every option against ONE manager and
//! ONE compiler, three options in the ordinary case and twenty-four when every option is a
//! rolled check. So a menu is not the sum of its options - the second option answers against
//! a store the first one filled, and the twenty-fourth against one that has seen
//! twenty-three options' worth of subproblems.
//!
//! THAT IS WHAT A PLAYER WAITS FOR. An option-level total can move while a menu's does not,
//! and the other way round, so the two readings are separate baselines rather than one
//! reading at two scales. de-dt75.1 turned on exactly this distinction and said the per-menu
//! number overrides the per-option one where they disagree.
//!
//! ## What it does
//!
//! One group per row. Builds the adversarial profile - `menu_profile`, shared with
//! `menu_residue` and `menu_wall` - takes its starts as the menu, and marks the whole menu
//! against one manager through `bridge::mark_menu_as_shipped`, so each group gets the marking
//! the product gives it: the onward question first, and the exact marking by branch and bound
//! only where that marks nothing. `DEGCT_MARKING=bnb` puts the exact marking on every group
//! instead, and `DEGCT_MARKING=hybrid-bnb` names the default.
//!
//! EVERY START IS A CONTESTANT, because the marking is competitive: an option that has
//! nothing to hunt for is refused against the class being hunted, by the baseline it
//! already lands on, and that happens inside `mark_menu` rather than before it. So
//! `options` counts the width of the menu rather than the searches it provoked, and it is
//! `offered` over again.
//!
//! ONE MANAGER PER MENU, ON A THREAD OF ITS OWN. Building a second manager on a thread that
//! has already built one is what overflows a stack (de-fpax), and the menu's own warmth is
//! the thing this measurement exists to capture, so the manager is built inside and dropped
//! with the thread.
//!
//! ## What the columns say
//!
//! `menu ms` is the whole thing, setup included, because that is what a request costs.
//! `setup ms` is how much of it was building the layout, the manager, the compiled guards
//! and the seed rather than searching, so the searching is the difference.
//!
//! `asked` is passes run across the whole menu: one worklist pass per round to ask whether
//! anything is still reachable, and one single-target pass per target the branch and bound
//! did not skip. The default adds the sibling-cut passes the onward question asks first.
//! `rounds` is how many rounds ended in a marker, which is how many options the menu claimed
//! content for.
//!
//! `settled` and `partly` split the options by whether their answer is final. A menu that
//! is mostly `partly` is one where the budget bound, and its milliseconds are a floor
//! rather than a cost.
//!
//! `nodes` is what the manager holds when the menu ends, which is the number a parallel
//! split has to clear a group against.
//!
//! ## What it said, 2026-09-09: the whole game, and two menus that are slow
//!
//! 395 menus of eight options, at the player's own 256 MB and the shipped budget, one group
//! per process on a quiet machine. 126 of the 521 measurable groups have no menu at all -
//! no start of theirs has anything worth hunting beyond it.
//!
//! ```text
//!   median 14 ms   p90 48   p99 345   max 3084
//!   total 16.2 s over 395 menus, mean 41 ms
//!
//!   over   250 ms:   5 of 395  (1.3%)
//!   over   500 ms:   3 of 395  (0.8%)
//!   over  1000 ms:   2 of 395  (0.5%)
//! ```
//!
//! 2,970 options and 5,377 candidates asked, and EVERY OPTION SETTLED: none was answered at
//! the start without searching, and none came back a bound. So no menu here is a menu whose
//! budget bound, and every millisecond below is work actually done.
//!
//! ### The two menus over a second, which is what this measurement is for
//!
//! ```text
//!     conv  entries  options  menu ms  setup  asked        nodes
//!      761     3975        8     3084     33     26    1,198,484
//!      368     4724        8     2277     26     56      224,750
//!       14     3594        8      819     27     48       98,158
//! ```
//!
//! `LookAheadTimeBudgetMs` BOUNDS ONE OPTION AND A REQUEST IS A WHOLE MENU. Every option in
//! those rows finished inside its own budget - that is what a `partly` of zero says - so
//! nothing here is the wall failing to hold. Eight options that each behave are still eight
//! options, and on 761 they add to three seconds.
//!
//! That is the question `measurements/menu_wall.rs` asks, and this is the first whole-game
//! answer to it: TWO GROUPS, NAMED, out of 395. A per-option reading cannot produce that
//! list - 761's worst option is well inside its budget - which is the whole reason this
//! measurement exists beside the option matrix rather than instead of it.
//!
//! 761 is also the only row anywhere near the manager, at 1.2 million diagram nodes on the
//! player's 256 MB.
//!
//! ### What a menu costs is mostly not setup
//!
//! 5.6 of the 16.2 seconds, so about a third - and on the slow rows far less, 33 ms of
//! 3,084. That is the opposite way round from the option matrix, where setup is eighty-nine
//! per cent of the time, and the difference is the point: a menu amortises one setup over
//! eight options where a matrix row pays it for one.
//!
//! ## How to run it
//!
//! One group per process, because a manager that runs out of nodes takes the process with
//! it and a crash on one group should not cost the rest:
//!
//! ```text
//! DEGCT_CONVERSATION=631 \
//!   tools/run-logged.sh cargo menu-matrix -- cargo run --release --example menu_matrix
//! ```
//!
//! `DEGCT_HEADER=1` prints the column names and measures nothing, which is how a driver
//! writing one file out of many processes gets a header without parsing a row.
//!
//! `DEGCT_GROUPS_ONLY=1` prints one line per distinct group in the game - `start`,
//! `conversations`, `entries`, `reachable`, most reachable first - and measures nothing. It is
//! how `tools/measure-menus.py all` learns which groups there are, and a zero in `reachable` is
//! how it skips a group with nothing to measure. See [`group_list`].
//!
//! `DEGCT_STARTS` sets the menu's width, `DEGCT_UNSEEN` how many of the deepest entries are
//! unread, and `DEGCT_BUDGET_MB` what the manager is given.
//!
//! `DEGCT_NOLIMIT=1` takes the limits off: a 6144 MB manager and a five-minute wall, which is
//! also each pass's ration.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use lookahead_engine::bridge::{SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, discover_group, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::known::GroupShape;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::symbolic::{menu, novelty_search};

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

#[path = "seen_profile.rs"]
mod seen_profile;
use seen_profile::candidates;

/// The columns, written down here and nowhere else.
///
/// A DRIVER ASKS FOR THESE rather than parsing them off a row, so that a file assembled from
/// many processes cannot get a header that disagrees with its rows.
const COLUMNS: [&str; 12] = [
    "conv", "entries", "options", "offered", "menu_ms", "setup_ms", "asked", "rounds", "settled",
    "partly", "nodes", "starred",
];

/// The groups to measure when nothing is named: the heavy list the matrix has always meant.
const CONVERSATIONS: [i32; 6] = [362, 368, 631, 14, 28, 1030];

/// What a player's search gets, TAKEN FROM THE ENGINE rather than restated. A menu is
/// answered under the shipped allowance, so a number typed here would go stale the day
/// that one moved and the row would quietly stop being what it claims to be.
const BUDGET_MB: usize = DiagramBudget::DEFAULT_MEMORY_BUDGET / (1024 * 1024);

/// What `DEGCT_NOLIMIT` gives the manager: a measurement's six gigabytes rather than a
/// player's allowance. `DEGCT_BUDGET_MB` still overrides it.
const NOLIMIT_BUDGET_MB: usize = 6144;

/// What `DEGCT_NOLIMIT` gives the menu, as a wall AND as each pass's ration.
///
/// BOTH, because one pass to a deep target can carry most of a round's work, so a per-pass
/// ration shorter than the wall would stop it where the wall would not. Five minutes, so that the
/// row says where the search stops rather than where a player's patience would.
const NOLIMIT_TIME: Duration = Duration::from_secs(300);

/// Whether this run is taken with the limits off. See [`NOLIMIT_BUDGET_MB`] and
/// [`NOLIMIT_TIME`].
fn nolimit() -> bool {
    lookahead_engine::core::env::is_set("NOLIMIT")
}

/// Which menu marking a row is taken with. See [`marking`].
#[derive(Clone, Copy)]
enum Marking {
    /// What the product marks with: the onward question first, and the exact marking by branch
    /// and bound only where that marks nothing - see `bridge::mark_menu_as_shipped`.
    HybridBranchAndBound,
    /// The exact marking by branch and bound on every group, with no onward question first -
    /// see `menu::mark_menu`.
    BranchAndBound,
}

/// What `DEGCT_MARKING` says for each marking.
const HYBRID_BRANCH_AND_BOUND: &str = "hybrid-bnb";
const BRANCH_AND_BOUND: &str = "bnb";

/// The marking `DEGCT_MARKING` names: `hybrid-bnb`, the default and what the product marks
/// with, or `bnb` for the exact marking on every group, so row files can be taken both ways and
/// compared on the same profile and the same allowance (de-0jsf.20).
///
/// # Panics
///
/// On any other value, so a misspelt run does not quietly measure the default.
fn marking() -> Marking {
    match lookahead_engine::core::env::var("MARKING")
        .unwrap_or_default()
        .as_str()
    {
        "" | HYBRID_BRANCH_AND_BOUND => Marking::HybridBranchAndBound,
        BRANCH_AND_BOUND => Marking::BranchAndBound,
        other => panic!(
            "DEGCT_MARKING={other:?}: expected {HYBRID_BRANCH_AND_BOUND} or {BRANCH_AND_BOUND}"
        ),
    }
}

/// How many options the menu asks about.
///
/// EIGHT, which is what `workspace_menus` uses, so a figure here is comparable with one
/// there. A wider menu is measurable with `DEGCT_STARTS`; twenty-four is what a menu of
/// rolled checks costs, since de-fes makes each outcome its own start.
const STARTS: usize = 8;

/// How many of the group's deepest entries are unread.
const UNSEEN: usize = 10;

const COUNTER_CAP: i32 = 16;

/// A verdict for a row that could not be measured, which is not a slow row.
const NOT_MEASURED: &str = "NOT-MEASURED";

/// A verdict for a group with no menu to ask about, which is not an empty one.
const NO_MENU: &str = "NO-MENU";

/// What one menu cost.
#[derive(Default)]
struct Menu {
    took: Duration,
    setup: Duration,
    options: usize,
    asked: usize,
    rounds: usize,
    settled: usize,
    partly: usize,
    nodes: usize,
    /// Which entries this marking starred, comma-separated and in menu order.
    ///
    /// WHICH RATHER THAN HOW MANY, and the distinction is the whole reason this column
    /// exists. Two markings can star the same NUMBER of options and not the same options,
    /// and `rounds` cannot tell those apart - so a run that changed which options a menu
    /// recommends while keeping the count would read as no change at all. Comparing two
    /// markings over the game is exactly what that would hide. See de-2p8j.2.
    starred: String,
}

fn main() {
    if lookahead_engine::core::env::is_set("HEADER") {
        println!("{}", COLUMNS.join("\t"));
        return;
    }

    // ASKED ONCE UP FRONT, so a misspelt DEGCT_MARKING stops the run before a group is built
    // rather than inside the thread each menu is marked on.
    let _ = marking();

    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    // ASKED FOR ON ITS OWN, like the header, and for the same reason: a whole-game run has to
    // know which groups there are before it measures any, and a list kept anywhere else can
    // omit a group and never say so. See `group_list`.
    if lookahead_engine::core::env::is_set("GROUPS_ONLY") {
        for (start, conversations, entries, reachable) in group_list(&index) {
            println!("{start}\t{conversations}\t{entries}\t{reachable}");
        }
        return;
    }

    let budget = DiagramBudget::new(
        from_env(
            "BUDGET_MB",
            if nolimit() {
                NOLIMIT_BUDGET_MB
            } else {
                BUDGET_MB
            },
        ) * 1024
            * 1024,
    );
    let starts_wanted = from_env("STARTS", STARTS);
    let unseen_wanted = from_env("UNSEEN", UNSEEN);

    for conversation in numbers("CONVERSATION", &CONVERSATIONS) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            eprintln!("conversation {conversation}: no group builds from it; skipping.");
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            eprintln!("conversation {conversation}: no entry 0; skipping.");
            continue;
        }

        // THROUGH `MenuProfile`, for the reason it exists: a menu whose starts have nothing
        // better beyond them is refused before a diagram is touched, and the whole row reads
        // as a fast engine while measuring nothing.
        let Some(profile) = MenuProfile::of(&graph, root, unseen_wanted, starts_wanted) else {
            row(conversation, &graph, 0, NO_MENU, None);
            continue;
        };

        let novelty = profile.novelty();
        match menu(&graph, &profile.starts, &novelty, budget) {
            Some(measured) => row(
                conversation,
                &graph,
                profile.starts.len(),
                "",
                Some(&measured),
            ),
            // THE MACHINE COULD NOT SUPPLY THE BUDGET, which is not a finding about the
            // menu. Loud, and a different word from a slow row, so a folder holding one is
            // not read as a measurement.
            None => row(
                conversation,
                &graph,
                profile.starts.len(),
                NOT_MEASURED,
                None,
            ),
        }
    }
}

/// Every distinct group in the game, most reachable first, as `(start, conversations,
/// entries, reachable)`.
///
/// A CANONICAL START IS NOT SIMPLY THE SMALLEST MEMBER. `discover_group` is the FORWARD closure
/// of a start, not an equivalence relation, so the smallest conversation in a group may reach
/// only part of it - a group of {3, 5} where 5 leads to 3 and 3 leads nowhere has `closure(3) =
/// {3}`. The start named is the smallest one whose own closure IS the whole set, which is the
/// only kind of start that reproduces the group it came from.
///
/// `reachable` counts the entries a profile could be built from, and a zero is how a run skips
/// a group with nothing to measure - see [`reachable_from`].
///
/// ORDERED BY WHAT A RUN CAN SEE, not by how big the group is: `entries` counts everything in a
/// group's conversations whether anything can walk to it or not, and a group can hold 4,035
/// entries and reach 32. Ties go by start, so the list is the same list every time it is asked
/// for, which a resume depends on.
fn group_list(index: &lookahead_engine::index::Index) -> Vec<(i32, usize, usize, usize)> {
    let mut conversations: Vec<i32> = index.keys().copied().collect();
    conversations.sort_unstable();

    let mut canonical: HashMap<BTreeSet<i32>, i32> = HashMap::new();
    for &conversation in &conversations {
        let group: BTreeSet<i32> = discover_group(index, conversation).into_iter().collect();
        // Ascending, so the first start to produce a set is the smallest that reaches it.
        canonical.entry(group).or_insert(conversation);
    }

    let mut groups: Vec<(i32, usize, usize, usize)> = canonical
        .into_iter()
        .map(|(group, start)| {
            let entries = group.iter().map(|id| index[id].entries.len()).sum();
            (start, group.len(), entries, reachable_from(index, start))
        })
        .collect();
    groups.sort_unstable_by(|a, b| b.3.cmp(&a.3).then(a.0.cmp(&b.0)));
    groups
}

/// How many entries a profile could be built from in `start`'s group: reachable from entry 0,
/// not the start, and not groups - see `seen_profile::candidates`.
///
/// ON STDERR, the reason a group has none, so the group list stays a clean TSV and a driver can
/// still keep why each group was skipped.
fn reachable_from(index: &lookahead_engine::index::Index, start: i32) -> usize {
    let Ok((graph, _)) = build_group_graph(index, start) else {
        eprintln!("conversation {start}: no group builds from it; skipping.");
        return 0;
    };
    let root = DialogueNodeId::new(start, 0);
    if graph.get(root).is_none() {
        eprintln!("conversation {start}: no entry 0; skipping.");
        return 0;
    }
    let reachable = candidates(&graph, root).len();
    if reachable == 0 {
        eprintln!("conversation {start}: nothing is reachable from its start; skipping.");
    }
    reachable
}

/// One menu: every option answered against one manager, warmed by the menu itself.
fn menu<F>(
    graph: &LookAheadGraph,
    starts: &[DialogueNodeId],
    novelty: &F,
    budget: DiagramBudget,
) -> Option<Menu>
where
    // SYNC, because the search runs on a thread of its own - de-fpax - and the closure
    // carried over is a shared reference to this one.
    F: Fn(DialogueNodeId) -> Novelty + Sync,
{
    isolated::on_its_own_thread(|| {
        let began = Instant::now();
        let symbols = graph.symbols().clone();
        let world = SnapshotWorld::declaring(
            WorldSnapshot {
                day_minutes: 720,
                day_counter: 1,
                ..Default::default()
            },
            None,
        );
        let layout = DataLayout::for_group(graph, &world, COUNTER_CAP);
        let vars = DataVars::try_new(&layout, &symbols, budget)?;
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
        let seed = seed_of(graph, &world, &vars).expect("room for a seed");
        // WORKED OUT ONCE FOR THE MENU, which is what the bridge does: the parent map and
        // the order are facts about the links, so a menu of eight options builds them once
        // rather than eight times. See `GroupShape::of`.
        let shape = GroupShape::of(graph);
        let setup = began.elapsed();

        // WHAT THE PLUGIN ASKS FOR, taken from the product rather than restated here, so a
        // change to the shipped budget moves this row with it.
        let search = lookahead_engine::bridge::LookAheadRequest {
            memory_budget_mb: budget.memory() / (1024 * 1024),
            ..Default::default()
        }
        .search_budget();

        let mut counted = Menu {
            setup,
            ..Default::default()
        };

        let contestants: Vec<_> = starts
            .iter()
            .map(|&start| menu::Contestant {
                position: novelty_search::Where::of(
                    graph,
                    start,
                    StartBranch::Either,
                    &seed,
                    &mut compiler,
                    &world,
                    COUNTER_CAP as u32,
                )
                .position(start),
                baseline: novelty(start),
                landing: vec![start],
            })
            .collect();
        let marking_search = lookahead_engine::symbolic::search::Search {
            graph,
            compiler: &mut compiler,
            world: &world,
            counter_cap: COUNTER_CAP as u32,
        };
        let marking_budget = if nolimit() {
            menu::Budget {
                wall: NOLIMIT_TIME,
                each: NOLIMIT_TIME,
            }
        } else {
            menu::Budget {
                wall: search.overall.saturating_mul(starts.len() as u32),
                each: search.each,
            }
        };
        // THE MARKING THE PRODUCT MARKS WITH by default, through the one function that chooses
        // it, so a default row measures what a player waits for; `DEGCT_MARKING=bnb` puts the
        // exact marking on every group instead. See [`marking`].
        let found = match marking() {
            Marking::HybridBranchAndBound => lookahead_engine::bridge::mark_menu_as_shipped(
                marking_search,
                novelty,
                &contestants,
                &marking_budget,
                &shape,
            ),
            Marking::BranchAndBound => menu::mark_menu(
                marking_search,
                novelty,
                &contestants,
                &marking_budget,
                &shape,
            ),
        };
        counted.options = contestants.len();
        counted.asked = found.passes;
        counted.rounds = found.rounds;
        counted.settled = found.marks.iter().filter(|mark| mark.complete).count();
        counted.partly = counted.options - counted.settled;
        counted.starred = starts
            .iter()
            .zip(&found.marks)
            .filter(|(_, mark)| mark.round.is_some())
            .map(|(start, _)| start.entry_id.to_string())
            .collect::<Vec<_>>()
            .join(",");

        counted.took = began.elapsed();
        counted.nodes = vars.node_count();
        Some(counted)
    })
}

/// One row, as a tab-separated line.
///
/// `why` is a word for a row that was not measured, and empty for one that was. It goes in
/// the `menu_ms` column rather than in a column of its own, so a row that says nothing says
/// so where a reader is already looking.
fn row(
    conversation: i32,
    graph: &LookAheadGraph,
    offered: usize,
    why: &str,
    measured: Option<&Menu>,
) {
    let cells: Vec<String> = match measured {
        Some(m) => vec![
            conversation.to_string(),
            graph.count().to_string(),
            m.options.to_string(),
            offered.to_string(),
            format!("{:.0}", ms(m.took)),
            format!("{:.0}", ms(m.setup)),
            m.asked.to_string(),
            m.rounds.to_string(),
            m.settled.to_string(),
            m.partly.to_string(),
            m.nodes.to_string(),
            // A DASH RATHER THAN AN EMPTY CELL for a menu that starred nothing, so a reader
            // and a splitter both see a value where the column is.
            if m.starred.is_empty() {
                "-".to_string()
            } else {
                m.starred.clone()
            },
        ],
        None => {
            let mut cells = vec![
                conversation.to_string(),
                graph.count().to_string(),
                "0".to_string(),
                offered.to_string(),
                why.to_string(),
            ];
            cells.extend(COLUMNS.iter().skip(5).map(|_| "?".to_string()));
            cells
        }
    };
    println!("{}", cells.join("\t"));
}

fn ms(took: Duration) -> f64 {
    took.as_secs_f64() * 1000.0
}

fn from_env(name: &str, fallback: usize) -> usize {
    lookahead_engine::core::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(fallback)
}

fn numbers(name: &str, fallback: &[i32]) -> Vec<i32> {
    match lookahead_engine::core::env::var(name) {
        Ok(named) => named
            .split(',')
            .map(str::trim)
            .filter(|piece| !piece.is_empty())
            .map(|piece| {
                piece
                    .parse()
                    .unwrap_or_else(|_| panic!("{name}={piece:?} is not a number"))
            })
            .collect(),
        Err(_) => fallback.to_vec(),
    }
}
