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
//! only where that marks nothing. `DEGCT_MARKING=hybrid-bnb` names that default.
//!
//! WITH THE WALK, because the product asks with one. A profile's menu has nobody behind it, so
//! the walk a player would have been shown is built from the conversation's start before the
//! clock starts - `hub::walk_to_menu` - and the cut it drives is worked out inside the timing by
//! `bridge::passed_since_hub`, the call a player's request goes through. A default row is the
//! shipped algorithm, walk and hub cut included (de-r2xf.11).
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
//! and the seed rather than searching, so the searching is the difference. `layout ms` is the
//! part of that which was the layout alone - 6 to 10 ms on 761, against a setup of 400 and a
//! menu of 5,600, which is what a cost nobody had timed turned out to be. See de-mau4.
//!
//! `prep ms` is what came BEFORE either of those: building the group's graph and walking its
//! profile. Every group pays it, whether or not a menu is ever measured, so it is its own
//! column rather than a second meaning for `setup ms` - a column that means two things cannot
//! be totalled, and these two can. `graph ms` is the part of it that was the graph build, so
//! the walk and whatever else precedes the profile is the difference.
//!
//! `index ms` is earlier still: reading the shipped index, which a process does once and only
//! if something needs it. It is taken OUT of `prep ms` rather than left inside it, so the
//! columns can be added up - see [`Prep::of`] - and a ZERO there is a group whose graph and
//! whose world were both kept, which is a group that never needed the index at all.
//!
//! WHAT IS FLAT IS WHAT IS LARGE HERE, which is why these are split at all: a cost paid once
//! per group whatever its size is a different problem from one that grows with the graph, and
//! the totals say the flat ones dominate. See de-ealo and de-9z1u.
//!
//! ## What is not a row, and the distinction that was being lost
//!
//! EVERY ROW IS A MEASUREMENT. Two things that are not:
//!
//! A GROUP WITH NO MENU IN IT, which is a fact about the dialogue - nothing it reaches offers the
//! player anything - found by edge analysis in `performance/group_list.rs` and never asked about
//! here, because such a group is not in the list this measures.
//!
//! A PROFILE THIS RUN COULD NOT BUILD, which is a finding about the run: the profile is walked,
//! in a world, with as many of the deepest entries called unread as the run was told. The same
//! group can refuse under one question and answer under another. It is reported on stderr - see
//! [`no_profile`] - and nothing is written, because there is nothing to write.
//!
//! THE TWO WERE ONE WORD, "NO-MENU", in a column where a measurement goes. That made a per-run
//! finding look like a property of the group, and a quarter of every whole-game run's rows were
//! it. They are not even the same size: 92 of the game's 521 groups contain no menu at all,
//! while 130 of the 429 that do refuse the profile the current default asks for. See de-ealo.
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
//! **IT IS A READING OF THE MANAGER, NOT A COUNT OF THE SEARCH, AND IT MOVES A LITTLE ON ITS
//! OWN.** What it counts is the unique table, which holds dead nodes until a collection
//! sweeps them, and a collection that finds another already running declines rather than
//! waits - so what is left unswept when the menu ends depends on thread timing. Measured over
//! three whole-game runs (de-jitt): 7 of 299 groups read differently every run - 1030, 602,
//! 605, 786, 368, 944, 1105 - the widest by 0.7%, while `rounds`, `settled` and `starred`
//! were identical for all 299. The search decided the same thing each time; only the sweep
//! landed elsewhere.
//!
//! So a difference under about one per cent is not a difference, and a nodes figure quoted to
//! the last digit is quoting the noise with it. Comparing two branches on this column means
//! comparing a spread against a spread; `tools/measure-menus.py --runs 3` prints which groups
//! moved and by how much, for exactly that reason.
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
//! That is the question `performance/menu_wall.rs` asks, and this is the first whole-game
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
//! WHICH GROUPS THERE ARE IS A DIFFERENT COMMAND, `performance/group_list.rs`, which is how
//! `tools/measure-menus.py all` learns what to measure.
//!
//! `DEGCT_STARTS` sets the menu's width, `DEGCT_UNSEEN` how many of the deepest entries are
//! unread, and `DEGCT_BUDGET_MB` what the manager is given.
//!
//! `DEGCT_NOLIMIT=1` takes the limits off: a 6144 MB manager and a five-minute wall, which is
//! also each pass's ration.
//!
//! EVERY ROW IS WALK-DEEPEST-X BY DEFAULT, where X is `DEGCT_UNSEEN`: each menu is asked in a
//! state a greedy playthrough reached, with the deepest entries THAT WALK REACHES still to come.
//! The world it hands the engine IS the walk's own, so there is one account of what the player
//! has read rather than two that can disagree. `DEGCT_WALKED_PROFILE=menu` asks instead about
//! the menu the player is standing at; it is the more honest profile and the weaker
//! measurement, and `menu_profile::Starts` carries the numbers. `=link-deepest` takes the
//! deepest entries by LINK DISTANCE instead, whether or not a play can stand where they are
//! still unread - see [`walked_profile`] for why that is not the default, and for why it does
//! not reproduce the runs taken before the world became mandatory.
//!
//! NOTHING RECONCILES THE TWO SCOPES, because nothing has to: a profile says what ANY game has
//! shown and a world says what THIS one has, and `world::seen_state` maps the pair onto the
//! three states. Neither is a claim about the other, so there is no setting for making them
//! agree and no way for a row to assert a state they disagree about.

use std::time::{Duration, Instant};

use lookahead_engine::bridge::{NodeRef, SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::{DialogueNodeId, SeenState, StartBranch};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::symbolic::{menu, seen_state_search};

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

#[path = "prepared.rs"]
mod prepared;
use prepared::Shipped;

#[path = "save_world.rs"]
mod save_world;

/// The columns, written down here and nowhere else.
///
/// A DRIVER ASKS FOR THESE rather than parsing them off a row, so that a file assembled from
/// many processes cannot get a header that disagrees with its rows.
const COLUMNS: [&str; 17] = [
    "conv",
    "entries",
    "options",
    "offered",
    "menu_ms",
    "setup_ms",
    "layout_ms",
    "index_ms",
    "graph_ms",
    "prep_ms",
    "asked",
    "rounds",
    "settled",
    "partly",
    "nodes",
    "starred",
    "exact",
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

/// Which save a walked profile is built from: the template, or what `DEGCT_SAVE` names.
///
/// THE TEMPLATE IS THE FAIR COMMON DENOMINATOR - the blank slate every committed scenario is a
/// diff over, so no group is favoured by a save that happens to suit it. What it is not is a
/// state anybody reached: on 761 a walk from it shows 44 entries of 2,263, where a walk from a
/// real playthrough's save shows 144. Naming a save asks the same question of a world a player
/// was actually in, and `target_cost` already reads this variable.
fn save() -> String {
    lookahead_engine::core::env::var("SAVE").unwrap_or_else(|_| save_world::TEMPLATE.to_string())
}

/// Whether this run is taken with the limits off. See [`NOLIMIT_BUDGET_MB`] and
/// [`NOLIMIT_TIME`].
fn nolimit() -> bool {
    lookahead_engine::core::env::is_set("NOLIMIT")
}

/// Which menu marking a row is taken with. See [`marking`].
#[derive(Clone, Copy)]
enum Marking {
    /// What the product marks with, told where the player walked from - see `hub::walk_to_menu`
    /// for the walk - so the onward question cuts what they passed since their current hub, and
    /// the exact marking by branch and bound only answers where that marks nothing. See
    /// `bridge::mark_menu_as_shipped`, which the plugin's requests reach too.
    HybridBranchAndBound,
    /// The shipped hybrid with the SPENT BRANCHES cut beside the walk: a branch off a hub the
    /// player is inside whose one-time effects have all fired and which shows nothing unread
    /// cannot be the way on, so it is refused like the walk itself. See de-wi02.
    HybridSpent,
    /// STEP 1 AND NOTHING AFTER IT - the onward question with the cut the default rule would
    /// ask it with, stopping whether or not it starred anything.
    ///
    /// NOT A MARKING ANYONE WOULD SHIP, and it is not offered as one: a menu it leaves bare is
    /// a menu the product would have gone on to answer exactly. It exists to name the menus
    /// that FALL THROUGH, cheaply - a `rounds` of zero here is a menu the expensive half runs
    /// for - so a before-and-after of step 2 can be taken on the menus step 2 actually
    /// touches, without paying for step 2 to find out which those are. See de-qy5t.
    Onward,
}

/// What `DEGCT_MARKING` says for each marking.
const HYBRID_BRANCH_AND_BOUND: &str = "hybrid-bnb";
const HYBRID_SPENT: &str = "hybrid-spent";
const ONWARD: &str = "onward";

/// The marking `DEGCT_MARKING` names: `hybrid-bnb`, the default and what the product marks
/// with, walk and all. Row files can be taken each way and compared on the same profile and the
/// same allowance (de-0jsf.20).
///
/// THE DEFAULT IS THE SHIPPED ALGORITHM, walk included, because a measurement that is not the
/// game's algorithm describes code no player runs (de-r2xf.11). There is no default without the
/// walk to fall back to.
///
/// EVERY ARM HERE STILL ASKS THE ONWARD QUESTION FIRST, because the product always does. The
/// exact marking alone is not offered: there is no state in which a player's engine marks a
/// menu without asking the cheap question first, so an arm that did would describe code the
/// game cannot run - and rows taken that way were repeatedly compared against shipped rows as
/// though the two answered one question. `menu::mark_menu` is still what step 2 calls, and a
/// test that wants the exact answer calls it directly. See de-eo76.
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
        HYBRID_SPENT => Marking::HybridSpent,
        ONWARD => Marking::Onward,
        other => panic!(
            "DEGCT_MARKING={other:?}: expected {HYBRID_BRANCH_AND_BOUND}, {HYBRID_SPENT} or \
             {ONWARD}"
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

/// What one leg of a walked profile's search may hold, where `DEGCT_WALKED_PROFILE` asks for
/// one. The same bound `greedy_playthrough` uses, so a profile built here is the one that
/// generator caches.
const WALK_CEILING: usize = 200_000;

/// Where a walked profile left the player: the world it stopped in, and what the conversation
/// has shown them since it last started.
///
/// The two travel together because they are one reading of one moment - the walk that produced
/// the world is the walk the hubs are followed along - and separating them is how a measurement
/// ends up asking a menu in one world about a player standing in another.
struct Standing {
    world: WorldSnapshot,
    walk: Vec<DialogueNodeId>,
}

/// The default. The X unseen entries are the deepest ones A WALK REACHES: `DEGCT_UNSEEN` is the
/// X in walk-deepest-X, and a run of keypresses is the witness that a player can stand there.
const WALK_DEEPEST: &str = "walk-deepest";
/// A start set nobody can stand at, asked on a save that has shown nothing: the structural menu
/// of `MenuProfile::of` with link-deepest-X globally unseen and no walk-up. See `Scenario`.
const SYNTHETIC_MENU: &str = "synthetic-menu";
/// The FIRST menu a walk-up from the conversation's start reaches, with walk-deepest-X globally
/// unseen. See `Scenario`.
const FIRST_MENU: &str = "first-menu";
/// The X unseen entries are the deepest ones BY LINK DISTANCE, picked off the dialogue graph
/// with no regard for whether any play can be standing where they are still unread -
/// link-deepest-X. No walk vouches for it.
const LINK_DEEPEST: &str = "link-deepest";
/// The menu the player is standing at, rather than starts chosen for reaching the unseen.
const WALKED_ON_SCREEN: &str = "menu";
/// What 21 rows already in `performance/logs` name `WALK_DEEPEST` as, kept so they stay
/// reproducible. Nothing else spells it this way any more.
const WALKED_FLAG: &str = "1";

/// Which unseen set a row is taken on: walk-deepest-X by default, link-deepest-X on request.
///
/// ## The world says what is seen, and nothing else does
///
/// walk-deepest-X takes a greedy playthrough from the template save, stops it with `DEGCT_UNSEEN`
/// entries still to come, and measures THAT: the unseen entries are the last ones a nearest-first
/// play reaches, the seen set is what it displayed, and the variables are what its walk left them
/// at. There is ONE account of what the player has read - the world - and the seen state function
/// agrees with it because it was derived from it.
///
/// link-deepest-X takes the structurally deepest entries by link depth instead, and asserts them
/// rather than reaching them. IT IS A SAVE THAT HAS NEVER OPENED THIS CONVERSATION: the deepest X
/// are unseen in any game, everything else was read in an EARLIER playthrough, and nothing at all
/// is seen this game - so no `once` has fired and no `seen` slot is set. That is a state a player
/// can be in, and the one in which the most once-slots are still live variables rather than
/// constants, which is what makes it adversarial.
///
/// WHAT IT CANNOT BE is a save that has read almost everything IN THIS GAME while none of its
/// one-time effects have fired. Saying so was what made the profile incoherent, and the
/// arithmetic of it is worth keeping: a `seen` slot shuts an entry that shuts once seen, so a
/// world told that most of the conversation was read this game closes the routes to the rest,
/// and 761 answered in 511 ms STARRING NOTHING - the cost of proving an empty menu. The unseen
/// entries here are unseen ANY game, which closes nothing.
///
/// ## link-deepest-X does not reproduce the runs it descends from
///
/// It is the nearest thing still available to them, not a rerun of them. Those rows were taken
/// when a row could be asked with NO world at all, and the world is mandatory now - so
/// link-deepest-X pairs the old asserted unseen set with a world that has to be there. Where an
/// old figure and a link-deepest-X figure differ, that gap is a candidate explanation and not a
/// regression. Treat the old numbers as history, and re-take anything a decision rests on.
///
/// ROWS ARE NOT COMPARABLE ACROSS IT, which is why it is in `COMPARED_VARIABLES`: it is a
/// different question about a different world, not the same question measured better.
///
/// # Panics
///
/// On any other value, so a misspelt run does not quietly measure something else.
fn walked_profile() -> Scenario {
    match lookahead_engine::core::env::var("WALKED_PROFILE") {
        Err(_) => Scenario::Walked(menu_profile::Starts::Reaching),
        Ok(value) => match value.as_str() {
            "" | WALKED_FLAG | WALK_DEEPEST => Scenario::Walked(menu_profile::Starts::Reaching),
            WALKED_ON_SCREEN => Scenario::Walked(menu_profile::Starts::OnScreen),
            LINK_DEEPEST => Scenario::LinkDeepest,
            SYNTHETIC_MENU => Scenario::SyntheticMenu,
            FIRST_MENU => Scenario::FirstMenu,
            other => panic!(
                "DEGCT_WALKED_PROFILE={other:?}: expected {WALK_DEEPEST} (or {WALKED_FLAG}), \
                 {WALKED_ON_SCREEN}, {LINK_DEEPEST}, {SYNTHETIC_MENU} or {FIRST_MENU}"
            ),
        },
    }
}

/// Which scenario a row is taken in: which menu is asked, in what world, with what globally
/// unseen.
///
/// THE GAME HAS TWO SCOPES OF SEEN and these differ in both, so each names both. Global state is
/// what this player has ever seen; save state is what THIS game has displayed, and is what fires
/// a `once`. See `menu_profile::MenuProfile::seen_state`.
enum Scenario {
    /// Walk-deepest-X globally unseen, asked in the world the walk stopped in. The walk has
    /// shown a great deal, so most one-time effects have already fired.
    Walked(menu_profile::Starts),
    /// Link-deepest-X globally unseen, everything else seen this game. The profile the
    /// baselines were taken on, kept for comparing against them.
    LinkDeepest,
    /// ARTIFICIAL MENU, NOTHING SHOWN. The structural start set of `MenuProfile::of`, which is
    /// not a menu any player can stand at, asked on a save that has never opened the
    /// conversation - so every `once` in the group is still pending. Link-deepest-X is globally
    /// unseen and everything else is `UnseenThisGame`.
    ///
    /// The most adversarial state that is still internally consistent: nothing here contradicts
    /// anything, it simply is not a position a player can be in.
    SyntheticMenu,
    /// THE REAL MENU, WALKED UP TO. The conversation is opened and played forward until a menu
    /// is on screen, choosing nothing - so the starts are the options the game would draw and
    /// the only entries shown this game are the ones it takes to get there. Walk-deepest-X is
    /// globally unseen.
    ///
    /// What a veteran player meets on a fresh save: they have read almost all of this before,
    /// in another game, and none of it in this one.
    ///
    /// THE FIRST MENU, WHICH IS USUALLY BUT NOT ALWAYS THE MAIN HUB. A conversation generally
    /// opens with a few lines and then offers its topics; a long one may put an intro menu in
    /// front of that. Taking the first is what makes the walk-up short and the arrival honest -
    /// it is where a player IS a few presses in. On 761 it is a seven-option menu three entries
    /// from the start, which is a menu worth asking about rather than a two-option doorway.
    FirstMenu,
}

const COUNTER_CAP: i32 = 16;

/// A verdict for a row that could not be measured, which is not a slow row.
const NOT_MEASURED: &str = "NOT-MEASURED";

/// What one menu cost.
#[derive(Default)]
struct Menu {
    took: Duration,
    setup: Duration,
    /// The part of `setup` that was building the layout, which is the only part de-ct6n
    /// changed and the part nothing had ever timed. See de-mau4.
    layout: Duration,
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
    /// Whether the EXACT marking answered, rather than the onward question.
    ///
    /// WHAT SELECTS THE MENUS AN OPTIMISATION TO THE EXPENSIVE HALF COULD HAVE MOVED. Step 1
    /// settles most menus and step 2 never runs for them, so a whole-game total is mostly
    /// rows that could not have changed, and a difference in the few that did arrives diluted
    /// into the noise. A run wants both readings - the whole game for what a session costs,
    /// this subset for whether the change did anything - and this column is how the second is
    /// taken. See de-qy5t.
    fell_through: bool,
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
    // NOT READ HERE, and that is the point: a group whose graph and whose world are both kept
    // never needs the index at all. See `prepared::Shipped`, and `Prep::index` for the column
    // that says what it cost when something did need it.
    let shipped = Shipped::at(path);

    // ASKED FOR ON ITS OWN, like the header, and for the same reason: a whole-game run has to
    // know which groups there are before it measures any, and a list kept anywhere else can
    // omit a group and never say so. See `group_list`.

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
        // FROM BEFORE THE GRAPH BUILD, because that is what a row which never measures
        // anything is made of - see [`Prep`].
        let started = Instant::now();
        // WHAT THE INDEX HAD ALREADY COST THIS PROCESS, so that what it costs THIS group is the
        // difference. The index is read when something first needs it, which is inside the
        // group's own preparation - so without this the read would be counted twice, once in
        // its own column and again inside `graph` and `total`.
        let before = shipped.took();
        let Ok(group) = prepared::group_graph(&shipped, conversation) else {
            eprintln!("conversation {conversation}: no group builds from it; skipping.");
            continue;
        };
        let built = started.elapsed() - (shipped.took() - before);
        let graph = group.graph;
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            eprintln!("conversation {conversation}: no entry 0; skipping.");
            continue;
        }

        // THROUGH `MenuProfile`, for the reason it exists: a menu whose starts have nothing
        // better beyond them is refused before a diagram is touched, and the whole row reads
        // as a fast engine while measuring nothing.
        let scenario = walked_profile();
        let (profile, walked) = if let Scenario::SyntheticMenu = scenario {
            // NO WALK-UP AND NOTHING SHOWN. The world is the save as it is - no `seen` slot set,
            // no `once` fired - and the starts are the structural set, which is why there is no
            // walk to stand at them by. `walked` stays None, so no hub cut is taken either.
            match MenuProfile::of(&graph, root, unseen_wanted, starts_wanted) {
                Some(found) => (found, None),
                None => {
                    no_profile(conversation);
                    continue;
                }
            }
        } else if let Scenario::FirstMenu = scenario {
            let base = SnapshotWorld::declaring(
                save_world::of_save(&graph, conversation, &shipped, &save()),
                save_world::declared(),
            );
            match menu_profile::first_menu_profile(
                &graph,
                &base,
                conversation,
                WALK_CEILING,
                unseen_wanted,
            ) {
                Some(found) => {
                    // WHICH MENU THE WALK-UP LANDED ON, and how far it had to go. The first menu
                    // a conversation offers is usually its main hub, and in a long one it may be
                    // an intro menu in front of that - the row cannot say which, so this does.
                    eprintln!(
                        "conversation {conversation}: walked up {} entries to a menu of {} \
                         options {:?}, with {} of {} entries globally unseen",
                        found.seen.len(),
                        found.profile.starts.len(),
                        found
                            .profile
                            .starts
                            .iter()
                            .map(|id| id.entry_id)
                            .collect::<Vec<_>>(),
                        found.profile.unseen.len(),
                        found.reachable,
                    );
                    let mut world = save_world::of_save(&graph, conversation, &shipped, &save());
                    world.seen = found.seen.iter().copied().map(NodeRef::from).collect();
                    world.variables = found.variables;
                    (
                        found.profile,
                        Some(Standing {
                            world,
                            walk: found.walk,
                        }),
                    )
                }
                None => {
                    no_profile(conversation);
                    continue;
                }
            }
        } else if let Scenario::Walked(which) = scenario {
            // THE SAME WORLD THE DATASET'S WALK USES, declared table included. A walk cannot
            // decide a variable nothing declares without it, so it refuses and stops short -
            // and this walk and `greedy_playthrough`'s would then be two different walks
            // called by one name. Giving the table to one and not the other is what made 761's
            // cached playthrough run fourteen legs while the profile measured here stopped at
            // seven. See de-qy5t.
            let base = SnapshotWorld::declaring(
                save_world::of_save(&graph, conversation, &shipped, &save()),
                save_world::declared(),
            );
            match menu_profile::walked_profile(
                &graph,
                &base,
                conversation,
                WALK_CEILING,
                unseen_wanted,
                starts_wanted,
                which,
            ) {
                // THE WALK'S OWN WORLD, less what it has now shown and what its variables now
                // hold. Everything else - the character sheet, the checks, the inventory - is
                // the save's, since the walk never changed those.
                Some(found) => {
                    // HOW MUCH OF THE GROUP THE WALK REACHED, on stderr beside the row. A
                    // walked row is read very differently depending on whether the playthrough
                    // behind it covered most of the group or a corner of it, and the row cannot
                    // say, being the same shape as every other row. On 761 the walk reaches 44
                    // entries of 2,263 from the template save, which is the difference between
                    // a menu asked in a well-explored conversation and one asked in a doorway.
                    //
                    // NOT THE UNSEEN COUNT, which is `DEGCT_UNSEEN` and the same every time by
                    // construction: printing it would have looked like a measurement and been
                    // a restatement of the setting.
                    let entries = graph.nodes().filter(|node| !node.is_group).count();
                    eprintln!(
                        "conversation {conversation}: the walk reaches {} of {entries} entries, \
                         and the profile is taken {} in",
                        found.reachable, found.shown,
                    );
                    let mut world = save_world::of_save(&graph, conversation, &shipped, &save());
                    world.seen = found.seen.iter().copied().map(NodeRef::from).collect();
                    world.variables = found.variables;
                    (
                        found.profile,
                        Some(Standing {
                            world,
                            walk: found.walk,
                        }),
                    )
                }
                None => {
                    no_profile(conversation);
                    continue;
                }
            }
        } else {
            match MenuProfile::of(&graph, root, unseen_wanted, starts_wanted) {
                Some(found) => (found, None),
                None => {
                    no_profile(conversation);
                    continue;
                }
            }
        };

        // THE GRAPH AND THE WALK ARE BEHIND US, and nothing a menu costs is. Taken here rather
        // than where the row is written, or a measured row's prep would swallow its search.
        let prep = Prep::of(&shipped, before, built, started);

        let seen_any_game = profile.seen_any_game();
        match menu(
            &graph,
            conversation,
            &profile.starts,
            &seen_any_game,
            budget,
            walked.as_ref(),
        ) {
            Some(measured) => row(
                conversation,
                &graph,
                profile.starts.len(),
                "",
                prep,
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
                prep,
                None,
            ),
        };
    }
}

/// One menu: every option answered against one manager, warmed by the menu itself.
///
/// `conversation` is the row's, whose start the `hybrid-hub` marking walks from.
fn menu<F>(
    graph: &LookAheadGraph,
    conversation: i32,
    starts: &[DialogueNodeId],
    seen_any_game: &F,
    budget: DiagramBudget,
    walked: Option<&Standing>,
) -> Option<Menu>
where
    // SYNC, because the search runs on a thread of its own - de-fpax - and the closure
    // carried over is a shared reference to this one.
    //
    // THE SET, NOT A READY-MADE CLASSIFIER, because the other half of what decides a seen state
    // is the world, and the world is built below. Handing one in would mean deciding the states
    // before the world that half-decides them exists.
    F: Fn(DialogueNodeId) -> bool + Sync,
{
    isolated::on_its_own_thread(|| {
        // WHERE A PLAYER WOULD HAVE WALKED FROM, built before the clock starts: it stands in for
        // the walk the plugin records as the conversation plays, which costs the engine nothing.
        // What the engine does with it - the group's hubs, the cut - is inside the timing below.
        //
        // THE PLAYTHROUGH'S OWN WALK WHERE THERE IS ONE. A walked profile knows what the
        // conversation has shown the player since it last started, which is what a request
        // carries. `hub::walk_to_menu` is the stand-in for a profile with no player behind it,
        // and it leaves a far shallower hub stack - measured on 761, a walk cut of ONE ENTRY
        // against a sitting of twenty-six presses. See de-aqxa.9.
        let walk = match walked {
            Some(standing) => standing.walk.clone(),
            None => lookahead_engine::symbolic::hub::walk_to_menu(graph, conversation, starts),
        };

        let began = Instant::now();
        let symbols = graph.symbols().clone();
        // THE WALKED WORLD WHERE THERE IS ONE, and it is taken WHOLE rather than patched: it
        // came out of a playthrough that reached the state it describes, and editing a field of
        // it would put it back among the worlds nobody walked to.
        let world = SnapshotWorld::declaring(
            match walked {
                Some(standing) => standing.world.clone(),
                None => WorldSnapshot {
                    day_minutes: 720,
                    day_counter: 1,
                    // NOTHING SEEN THIS GAME, which is the whole of what a profile with no walk
                    // behind it asserts: a save that has never opened this conversation, so no
                    // `once` has fired and no `seen` slot is set. The entries it calls read were
                    // read in an EARLIER playthrough, which is the seen-any-game set and the
                    // profile's to say.
                    ..Default::default()
                },
            },
            None,
        );
        let layout = DataLayout::for_group(graph, &world, COUNTER_CAP);
        // WHAT THE LAYOUT ALONE COST, so that "the cost is in building the layout" is a number
        // rather than the only unmeasured thing left in setup. See de-mau4.
        let built_layout = began.elapsed();
        let vars = DataVars::try_new(&layout, &symbols, budget)?;
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
        let seed = seed_of(graph, &world, &vars).expect("room for a seed");
        // THE GROUP AS THIS MENU CAN WALK IT, and its shape, through the call the bridge makes,
        // so a row pays what a player's menu pays for it. See `bridge::walkable_menu`.
        let (trimmed, shape) =
            lookahead_engine::bridge::walkable_menu(graph, &mut compiler, starts);
        // THE ONE RULE, off the world just built and the set the profile supplied - see
        // `world::seen_state`.
        let whole_states = lookahead_engine::world::seen_states(&world, seen_any_game);
        let reachable_states = trimmed.seen_state(&whole_states);
        let seen_state = &reachable_states;
        let graph = &trimmed.graph;
        let setup = began.elapsed();
        let layout = built_layout;

        // WHAT THE MARKING ACTUALLY HUNTS, counted AFTER the trim and outside the timing. A
        // scenario is a claim about the seen states, and the row cannot say which claim it made
        // - every row is the same shape. Counting BEFORE `walkable_menu` counts a graph the
        // marking never sees, so a row could announce ten entries unseen anywhere and hunt none
        // of them. See de-rnrb.
        //
        // OUT OF REACH IS COUNTED APART FROM READ, because `Trimmed::seen_state` reads an entry the
        // request cannot arrive at as seen-this-game - which is right for the marking and wrong
        // for a reader. Adding the two together would report a save that has read hundreds of
        // entries beside a world whose seen set is empty.
        {
            let mut any_game = 0;
            let mut this_game = 0;
            let mut seen = 0;
            let mut out_of_reach = 0;
            for node in graph.nodes() {
                if !trimmed.reachable.contains(&node.id) {
                    out_of_reach += 1;
                    continue;
                }
                match seen_state(node.id) {
                    SeenState::UnseenAnyGame => any_game += 1,
                    SeenState::UnseenThisGame => this_game += 1,
                    SeenState::SeenThisGame => seen += 1,
                }
            }
            eprintln!(
                "conversation {conversation}: {any_game} unseen-any-game, {this_game} \
                 unseen-this-game, {seen} seen-this-game, and {out_of_reach} this menu cannot \
                 reach"
            );
        }

        // WHAT THE PLUGIN ASKS FOR, taken from the product rather than restated here, so a
        // change to the shipped budget moves this row with it.
        let search = lookahead_engine::bridge::LookAheadRequest {
            memory_budget_mb: budget.memory() / (1024 * 1024),
            ..Default::default()
        }
        .search_budget();

        let mut counted = Menu {
            setup,
            layout,
            ..Default::default()
        };

        let contestants: Vec<_> = starts
            .iter()
            .map(|&start| menu::Contestant {
                position: seen_state_search::Where::of(
                    graph,
                    start,
                    StartBranch::Either,
                    &seed,
                    &mut compiler,
                    &world,
                    COUNTER_CAP as u32,
                )
                .position(start),
                baseline: seen_state(start),
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
        // it, so a default row measures what a player waits for. See [`marking`].
        let found = match marking() {
            // INSIDE THE TIMED REGION, the group's hubs included. The WALK ITSELF is handed
            // over and both cuts are derived from it in the bridge, through the one call a
            // request carrying a walk goes through - so this row pays what a player's menu
            // pays, the deriving included, and cannot drift from the rule by restating it.
            Marking::HybridBranchAndBound => lookahead_engine::bridge::mark_menu_as_shipped(
                marking_search,
                seen_state,
                &contestants,
                &marking_budget,
                &shape,
                &walk,
            ),
            // THE SAME WALK CUT AS THE DEFAULT ARM in step 1, and the spent branches given to
            // STEP 2, which is the only place they can do anything. Step 1 already refuses
            // every loop back through a hub, and a spent branch is one, so a cut handed there
            // is subsumed before it is asked - which is what this arm did until de-qy5t, and
            // why it appeared to find nothing.
            //
            // INSIDE THE TIMING, as the walk's own cut is, since a player's request would have
            // to work this out too.
            Marking::HybridSpent => {
                let cut = lookahead_engine::bridge::passed_since_hub(graph, &shape, &walk);
                let spent = lookahead_engine::bridge::spent_since_hub(
                    graph, &shape, &world, seen_state, &walk,
                );
                // ON STDERR, because a cut that finds nothing and a cut that is not running
                // produce the same row, and only one of those is a finding. What matters here
                // is what the spent cut holds that the walk cut does NOT - entries already in
                // the walk cut are refused by step 1 and never reach step 2 to be blocked.
                // THE IDS, NOT JUST THE COUNT, AND THE OPTIONS BESIDE THEM. A count says how
                // much was cut and cannot say WHAT, and the question a changed marker asks is
                // whether the entry that lost its star is one of these. Printing both is what
                // showed that conversation 45's option 73 is itself a spent-branch entry, which
                // is the flaw in the whole cut - see `hub::spent_branches`.
                let mut ids: Vec<i32> = spent.iter().map(|id| id.entry_id).collect();
                ids.sort_unstable();
                let mut options: Vec<i32> = contestants
                    .iter()
                    .flat_map(|c| c.landing.iter().map(|id| id.entry_id))
                    .collect();
                options.sort_unstable();
                eprintln!(
                    "conversation {conversation}: spent cut is {} entries, {} of them beyond \
                     a walk cut of {}\n  spent: {ids:?}\n  options: {options:?}",
                    spent.len(),
                    spent.difference(&cut).count(),
                    cut.len(),
                );
                menu::mark_menu_hybrid(
                    marking_search,
                    seen_state,
                    &contestants,
                    &marking_budget,
                    &shape,
                    &cut,
                    &spent,
                )
            }
            // THROUGH THE PRODUCT'S OWN STEP 1, not a restatement of it: `mark_step_one`
            // decides which cut the question is asked with, and a copy of that decision here
            // would be free to drift from the rule it is supposed to be selecting against.
            Marking::Onward => menu::mark_step_one(
                marking_search,
                seen_state,
                &contestants,
                &marking_budget,
                &shape,
                &lookahead_engine::bridge::passed_since_hub(graph, &shape, &walk),
            ),
        };
        counted.options = contestants.len();
        counted.asked = found.passes;
        counted.rounds = found.rounds;
        counted.fell_through = found.fell_through;
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

/// What a group spent before any menu existed, which EVERY group pays whether or not a menu
/// is ever measured.
///
/// SPLIT, because the total alone does not say what to do about it. Measured whole-game on
/// 2026-09-18, preparation was 93 per cent of the run - 101 seconds against 7.7 of menu
/// measuring - and it barely tracks the group's size: three entries cost 110 ms and 4,724
/// cost 297. Something flat dominates, and `graph` against `total` says whether the flat part
/// is the graph build or what follows it. See de-ealo and de-9z1u.
#[derive(Debug, Clone, Copy)]
struct Prep {
    /// Reading the shipped index, where this group's preparation is what needed it read.
    ///
    /// ZERO WHERE NOTHING NEEDED IT, which is the whole point: a group whose graph and whose
    /// world are both kept never asks for the index, and the column says so. It is reported per
    /// row rather than once because the driver runs one process per group, so a whole-game run
    /// pays it once per group.
    index: Duration,
    /// Building the group's graph from the index.
    graph: Duration,
    /// That, and everything else up to the profile being ready to measure against.
    total: Duration,
}

impl Prep {
    /// What this group spent, with the index read taken out of it.
    ///
    /// TAKEN OUT SO THE COLUMNS CAN BE ADDED UP. The index is read when the first thing needs
    /// it, which is inside a group's own preparation - so leaving it in would report it twice,
    /// once in `index` and again inside `graph` and `total`, and no total over the row would
    /// mean anything.
    ///
    /// `before` is what the index had cost this process when this group started, so a process
    /// measuring several groups charges the read to the one that caused it.
    fn of(shipped: &Shipped, before: Duration, graph: Duration, started: Instant) -> Self {
        let index = shipped.took() - before;
        Self {
            index,
            graph,
            total: started.elapsed() - index,
        }
    }
}

/// This run could not build the profile it wanted in this group: said on stderr, and measured
/// as nothing.
///
/// NOT "NO MENU", WHICH IS A DIFFERENT AND STRONGER CLAIM. Whether a group contains a menu at all
/// is a property of the dialogue, answered by edge analysis in `performance/group_list.rs`, and a
/// group that has none never reaches this code. What is refused HERE is the adversarial profile:
/// it is built under a walk, in a world, with as many of the deepest entries called unread as
/// this run was told to - so the same group can refuse under one question and answer under
/// another. Calling that "no menu" is what made a per-run finding look like a fact.
///
/// NOT A ROW EITHER, because a row is a measurement and this is the absence of one.
fn no_profile(conversation: i32) {
    eprintln!(
        "conversation {conversation}: no profile can be built here - nothing it reaches is worth \
         hunting under this unseen set and this walk. Measuring nothing."
    );
}

/// One row, as a tab-separated line: printed, and handed back for a caller that wants to keep
/// it.
///
/// `why` is a word for a row that was not measured, and empty for one that was. It goes in
/// the `menu_ms` column rather than in a column of its own, so a row that says nothing says
/// so where a reader is already looking.
///
fn row(
    conversation: i32,
    graph: &LookAheadGraph,
    offered: usize,
    why: &str,
    prep: Prep,
    measured: Option<&Menu>,
) -> String {
    let cells: Vec<String> = match measured {
        Some(m) => vec![
            conversation.to_string(),
            graph.count().to_string(),
            m.options.to_string(),
            offered.to_string(),
            format!("{:.0}", ms(m.took)),
            format!("{:.0}", ms(m.setup)),
            format!("{:.0}", ms(m.layout)),
            format!("{:.0}", ms(prep.index)),
            format!("{:.0}", ms(prep.graph)),
            format!("{:.0}", ms(prep.total)),
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
            if m.fell_through { "yes" } else { "no" }.to_string(),
        ],
        None => {
            let mut cells = vec![
                conversation.to_string(),
                graph.count().to_string(),
                "0".to_string(),
                offered.to_string(),
                why.to_string(),
                // NOTHING WAS SET UP: no layout, no manager, no compiled guards. Zero rather
                // than "?", because it is known and it is none.
                "0".to_string(),
                "0".to_string(),
                format!("{:.0}", ms(prep.index)),
                format!("{:.0}", ms(prep.graph)),
                format!("{:.0}", ms(prep.total)),
            ];
            // The rest are answers a measurement would have given, and there was none.
            cells.extend(COLUMNS.iter().skip(10).map(|_| "?".to_string()));
            cells
        }
    };
    let line = cells.join("\t");
    println!("{line}");
    line
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
