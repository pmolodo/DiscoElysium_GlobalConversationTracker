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
//! only where that marks nothing. `--marking hybrid-bnb` names that default.
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
//! --conversation 631 \
//!   tools/run-logged.sh cargo menu-matrix -- cargo run --release --example menu_matrix
//! ```
//!
//! `--header` prints the column names and measures nothing, which is how a driver
//! writing one file out of many processes gets a header without parsing a row.
//!
//! WHICH GROUPS THERE ARE IS A DIFFERENT COMMAND, `performance/group_list.rs`, which is how
//! `tools/measure-menus.py all` learns what to measure.
//!
//! `--starts` sets the menu's width, `--unseen` how many of the deepest entries are
//! unread, and `--budget-mb` what the manager is given.
//!
//! `--nolimit` takes the limits off: a 6144 MB manager and a five-minute wall, which is
//! also each pass's ration.
//!
//! ## The two things a row varies
//!
//! `--menu` says WHICH MENU is asked - the first one a walk-up reaches, which the game really
//! draws, or a synthetic set of the shallowest entries that can reach a target, which no player
//! can stand at. `--targets` says WHICH ENTRIES are called never-seen - the last a greedy
//! playthrough reaches, or the furthest by link distance. See [`MenuKind`] and [`Targets`].
//!
//! EVERYTHING ELSE IS FIXED, because a measurement does not want the easy cases. Every row
//! STARTS from the template save, which has not opened this conversation, so every one-time
//! effect in the group is still pending and every slot a `once` would fire is a live variable
//! rather than a constant. The world a playthrough STOPPED in was once a setting and is the
//! easy case: it has spent most of those already. See de-fox9.
//!
//! WHAT THE WALK-UP SHOWS IS THE WORLD'S. `--menu=first` plays to its menu, so the entries it
//! displayed and the variables it moved are in the world the row measures - the save is where
//! that walk STARTED, not where it ends. A synthetic menu has no such walk to play: its route
//! is built structurally to one of its options, which is a walk the hub cut can read and not a
//! sequence anybody pressed, so it moves nothing.
//!
//! THE DEFAULT IS THE NEAREST THING TO A REAL REQUEST: `--menu=first --targets=walk-deepest`,
//! a menu the game draws, walked up to, asked about content a play can leave unread.
//!
//! NOTHING RECONCILES THE TWO SCOPES, because nothing has to: a profile says what ANY game has
//! shown and a world says what THIS one has, and `world::seen_state` maps the pair onto the
//! three states. Neither is a claim about the other, so there is no setting for making them
//! agree and no way for a row to assert a state they disagree about.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use lookahead_engine::bridge::{NodeRef, SnapshotWorld, WorldSnapshot};
use lookahead_engine::core::types::{DialogueNodeId, SeenState, StartBranch};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::symbolic::arms::Arms;
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::var_order::Ordering;
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::symbolic::{menu, seen_state_search};

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "options.rs"]
mod options;

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

/// What `--nolimit` gives the manager: a measurement's six gigabytes rather than a
/// player's allowance. `--budget-mb` still overrides it.
const NOLIMIT_BUDGET_MB: usize = 6144;

/// What `--nolimit` gives the menu, as a wall AND as each pass's ration.
///
/// BOTH, because one pass to a deep target can carry most of a round's work, so a per-pass
/// ration shorter than the wall would stop it where the wall would not. Five minutes, so that the
/// row says where the search stops rather than where a player's patience would.
const NOLIMIT_TIME: Duration = Duration::from_secs(300);

/// Everything this driver takes on its command line.
///
/// ONE STRUCT, HANDED DOWN, rather than each function asking the world for what it needs. `menu`
/// below takes six explicit parameters and still read two options out of the environment from
/// inside the thread the search runs on, which is how an option comes to be decided somewhere no
/// caller can see. A struct threads one argument and carries all of them, so a function that
/// needs another option later gains a field rather than a parameter.
/// A FIELD'S DOC COMMENT IS ITS `--help` TEXT, which is why every one below is a single line and
/// the reasoning sits in ordinary `//` comments beside it. `clap` prints a doc comment verbatim,
/// so the dense rationale this file is written in would arrive at whoever typed `--help` as
/// several paragraphs about why a struct is shaped the way it is.
#[derive(clap::Parser, Debug, Clone)]
#[command(
    about = "One row per group: what a whole menu costs and what it marks. \
             With no group named, this driver's own heavy list.",
    long_about = None
)]
struct Options {
    #[command(flatten)]
    groups: options::Groups,

    #[command(flatten)]
    starts: options::Starts<STARTS>,

    #[command(flatten)]
    unseen: options::Unseen<UNSEEN>,

    // NOT `options::Budget`, whose default is a constant: this one's depends on whether
    // `--nolimit` was passed, which is not known until the arguments are parsed. See
    // `Options::budget`.
    /// How much the diagram manager may commit, in MB [default: the shipped allowance]
    #[arg(long = "budget-mb", value_name = "MB")]
    budget_mb: Option<usize>,

    /// Measure with the limits off: a measurement's memory and a five-minute wall
    #[arg(long)]
    nolimit: bool,

    // A MODE RATHER THAN AN OPTION, and it is how a driver writing one file out of many
    // processes gets a header without parsing a row.
    /// Print the column names and measure nothing
    #[arg(long)]
    header: bool,

    /// Which menu marking a row is taken with
    #[arg(long, value_enum, default_value_t = Marking::HybridBranchAndBound)]
    marking: Marking,

    // THE TEMPLATE IS THE FAIR COMMON DENOMINATOR - the blank slate every committed scenario is
    // a diff over, so no group is favoured by a save that happens to suit it. What it is not is
    // a state anybody reached: on 761 a walk from it shows 44 entries of 2,263, where a walk
    // from a real playthrough's save shows 144. Naming a save asks the same question of a world
    // a player was actually in.
    /// Fold every run of entries a play cannot stop inside into one entry before measuring
    #[arg(long = "collapse-runs")]
    collapse_runs: bool,

    /// Which save a walked profile is built from
    #[arg(long, value_name = "NAME", default_value = save_world::TEMPLATE)]
    save: String,

    /// Which menu a row asks about
    #[arg(long, value_enum, default_value_t = MenuKind::First)]
    menu: MenuKind,

    /// Which entries a row calls never seen in any game
    #[arg(long, value_enum, default_value_t = Targets::WalkDeepest)]
    targets: Targets,

    #[command(flatten)]
    caching: prepared::Caching,

    /// How the layout orders its variables
    #[arg(long = "var-order", value_enum, default_value_t = Ordering::Slot)]
    var_order: Ordering,
}

impl Options {
    /// Which arms a row is taken under. `Arms::default()` is the shipped algorithm, so a run
    /// that names neither of these measures what the game does.
    fn arms(&self) -> Arms {
        Arms {
            var_order: self.var_order,
        }
    }
}

impl Options {
    /// What the diagram manager may spend, in bytes.
    ///
    /// The limits being off raises the default and nothing else: a run that names a budget gets
    /// the budget it named either way.
    fn budget(&self) -> DiagramBudget {
        let mb = self.budget_mb.unwrap_or(if self.nolimit {
            NOLIMIT_BUDGET_MB
        } else {
            BUDGET_MB
        });
        DiagramBudget::new(mb * 1024 * 1024)
    }
}

/// Which menu marking a row is taken with. Row files can be taken each way and compared on the
/// same profile and the same allowance (de-0jsf.20).
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
/// A MISSPELT ARM IS REFUSED BY NAME, with the arms listed, because `clap` will not accept a
/// value that is not one of these - which is what a hand-written parser had to panic to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum Marking {
    /// What the product marks with, told where the player walked from - see `hub::walk_to_menu`
    /// for the walk - so the onward question cuts what they passed since their current hub, and
    /// the exact marking by branch and bound only answers where that marks nothing. See
    /// `bridge::mark_menu_as_shipped`, which the plugin's requests reach too.
    #[value(name = HYBRID_BRANCH_AND_BOUND, help = "what the product marks with, walk and hub cut included")]
    HybridBranchAndBound,
    /// The shipped hybrid with the SPENT BRANCHES cut beside the walk: a branch off a hub the
    /// player is inside whose one-time effects have all fired and which shows nothing unread
    /// cannot be the way on, so it is refused like the walk itself. See de-wi02.
    #[value(name = HYBRID_SPENT, help = "the shipped hybrid, with spent branches cut beside the walk")]
    HybridSpent,
    /// STEP 1 AND NOTHING AFTER IT - the onward question with the cut the default rule would
    /// ask it with, stopping whether or not it starred anything.
    ///
    /// NOT A MARKING ANYONE WOULD SHIP, and it is not offered as one: a menu it leaves bare is
    /// a menu the product would have gone on to answer exactly. It exists to name the menus
    /// that FALL THROUGH, cheaply - a `rounds` of zero here is a menu the expensive half runs
    /// for - so a before-and-after of step 2 can be taken on the menus step 2 actually
    /// touches, without paying for step 2 to find out which those are. See de-qy5t.
    #[value(name = ONWARD, help = "step 1 alone, to name the menus that fall through to step 2")]
    Onward,
}

/// What `--marking` says for each marking.
const HYBRID_BRANCH_AND_BOUND: &str = "hybrid-bnb";
const HYBRID_SPENT: &str = "hybrid-spent";
const ONWARD: &str = "onward";

/// How many options the menu asks about.
///
/// EIGHT, which is what `workspace_menus` uses, so a figure here is comparable with one
/// there. A wider menu is measurable with `--starts`; twenty-four is what a menu of
/// rolled checks costs, since de-fes makes each outcome its own start.
const STARTS: usize = 8;

/// How many of the group's deepest entries are unread.
const UNSEEN: usize = 10;

/// What one leg of a profile's walk may hold, where a menu or a target set asks for
/// one. The same bound `greedy_playthrough` uses, so a profile built here is the one that
/// generator caches.
const WALK_CEILING: usize = 200_000;

/// WHICH MENU a row asks about, which is one of the two things that vary.
///
/// THE WORLD IS NOT A CHOICE: every row starts from `save_world::of_save` of the template save,
/// which knows its variables, its inventory and its check outcomes, and
/// it has not opened this conversation - so every one-time effect in the group is still pending
/// and every slot a `once` would fire is still a live variable rather than a constant. The world
/// a playthrough STOPPED in was the other option and is the easy case: it has spent most of
/// those already.
///
/// NEITHER PAIRING IS A STATE A PLAYER IS LITERALLY IN. Reaching a menu means having walked to
/// it, and walking to it shows entries this world says are unshown. The default is the closest
/// reachable approximation rather than a claim about a real save. See de-fox9.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum MenuKind {
    /// THE FIRST MENU A WALK-UP REACHES, which is a menu the game would really draw: its options
    /// are offered together and a player can stand where they are all in front of them.
    ///
    /// ITS OPTIONS ARE NOT CHOSEN TO REACH THE TARGETS, unlike the synthetic menu - it is a real
    /// menu and gets to be whatever it is - so an option with nothing to hunt is refused by the
    /// baseline it already lands on, and a row can honestly mark nothing.
    First,
    /// THE SHALLOWEST ENTRIES THAT CAN REACH A TARGET, `--starts` of them. Adversarial by
    /// construction: every option has something better beyond it, so none is refused before a
    /// diagram is touched. Not a menu any player can stand at - nothing offers these together.
    SyntheticFarthest,
}

/// WHICH ENTRIES a row calls never seen in any game, which is the other thing that varies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum Targets {
    /// The last `--unseen` entries a greedy playthrough reaches. A SET A PLAY CAN LEAVE UNREAD:
    /// a walk got to everything before them, so "all seen but these" is a state some number of
    /// playthroughs arrives at.
    WalkDeepest,
    /// The `--unseen` entries furthest from the start by LINK DISTANCE. Asserted rather than
    /// walked to, so no play is known to stand where these are still unread - but it depends on
    /// the links alone, so it is the same set in every world.
    LinkDeepest,
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
    // PARSED BEFORE ANYTHING IS BUILT, so a misspelt option stops the run before a group is
    // built rather than inside the thread each menu is marked on.
    let asked = <Options as clap::Parser>::parse();

    if asked.header {
        println!("{}", COLUMNS.join("\t"));
        return;
    }

    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    // NOT READ HERE, and that is the point: a group whose graph and whose world are both kept
    // never needs the index at all. See `prepared::Shipped`, and `Prep::index` for the column
    // that says what it cost when something did need it.
    let shipped = Shipped::at(path, asked.caching);

    // ASKED FOR ON ITS OWN, like the header, and for the same reason: a whole-game run has to
    // know which groups there are before it measures any, and a list kept anywhere else can
    // omit a group and never say so. See `group_list`.

    let budget = asked.budget();
    let starts_wanted = asked.starts.starts;
    let unseen_wanted = asked.unseen.unseen;

    for conversation in asked.groups.or(&CONVERSATIONS) {
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
        let mut graph = group.graph;
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            eprintln!("conversation {conversation}: no entry 0; skipping.");
            continue;
        }

        // THROUGH `MenuProfile`, for the reason it exists: a menu whose starts have nothing
        // better beyond them is refused before a diagram is touched, and the whole row reads
        // as a fast engine while measuring nothing.
        // THE WORLD EVERY ROW IS TAKEN IN, whichever menu and whichever targets: the template
        // save as it stands, which has not opened this conversation. See `Menu`.
        let mut snapshot = save_world::of_save(&graph, conversation, &shipped, &asked.save);
        // DECLARED FOR THE WALK, which cannot decide a variable nothing declares and would stop
        // short without the table - see de-qy5t. The measured world is built from the same
        // snapshot below, as a request builds it.
        let base = SnapshotWorld::declaring(snapshot.clone(), save_world::declared());
        let Some(unseen) = (match asked.targets {
            Targets::LinkDeepest => Some(menu_profile::link_deepest_unseen(
                &graph,
                root,
                unseen_wanted,
            )),
            Targets::WalkDeepest => menu_profile::walk_deepest_unseen(
                &graph,
                &base,
                conversation,
                WALK_CEILING,
                unseen_wanted,
            ),
        }) else {
            no_profile(conversation);
            continue;
        };

        let (profile, walk) = match asked.menu {
            MenuKind::SyntheticFarthest => {
                let starts = menu_profile::synthetic_menu(&graph, root, &unseen, starts_wanted);
                // NO PLAYER BEHIND IT, so the walk a request carries is built instead: the
                // shortest route from the conversation's start to one of these options. See
                // `hub::walk_to_menu`, and `MenuKind::SyntheticFarthest` for why no route can reach
                // them all.
                let walk =
                    lookahead_engine::symbolic::hub::walk_to_menu(&graph, conversation, &starts);
                match MenuProfile::aimed_at(unseen, starts) {
                    Some(found) => (found, walk),
                    None => {
                        no_profile(conversation);
                        continue;
                    }
                }
            }
            MenuKind::First => {
                match menu_profile::first_menu_profile(
                    &graph,
                    &base,
                    conversation,
                    WALK_CEILING,
                    unseen,
                ) {
                    Some(found) => {
                        // WHICH MENU THE WALK-UP LANDED ON, and how far it had to go. The first
                        // menu a conversation offers is usually its main hub, and in a long one
                        // it may be an intro menu in front of that - the row cannot say which,
                        // so this does.
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
                        // THE WALK-UP REALLY HAPPENED, so what it showed and what it left the
                        // variables at are the world's now. The template save is where it
                        // STARTED, not where it ends: a row that asked about this menu while
                        // claiming the entries leading to it were unshown would describe a
                        // player who is not standing where the menu is.
                        snapshot.seen = found.seen.iter().copied().map(NodeRef::from).collect();
                        snapshot.variables = found.variables;
                        (found.profile, found.walk)
                    }
                    None => {
                        no_profile(conversation);
                        continue;
                    }
                }
            }
        };

        // THE GRAPH AND THE WALK ARE BEHIND US, and nothing a menu costs is. Taken here rather
        // than where the row is written, or a measured row's prep would swallow its search.
        let prep = Prep::of(&shipped, before, built, started);

        let seen_any_game = profile.seen_any_game();
        // FITTED AS A REQUEST FITS IT, and before the fold and the layout that follow, both of
        // which read what a fitting decides. `bridge::answer` does this to every graph it
        // answers over - prices for the game mode, an action's condition settled, a passive
        // check's outcome decided, and a slot whose value the world settles put into the guards
        // that read it - so a row taken over an UNFITTED graph is a row about an engine the
        // game does not run. See the rule in CLAUDE.md about measuring the shipped algorithm,
        // and de-j4kg for what this was measured to move.
        fitted_to(&mut graph, &snapshot);
        // FOLDED HERE, BEFORE THE LAYOUT, because the point of folding is the slots as much as
        // the entries and the layout is built from the graph below. The profile stays the one
        // the FULL graph produced - the same menu, the same entries called unseen - and is
        // translated onto the folded group, so a folded row and a plain one answer the same
        // question. See `LookAheadGraph::collapsing_runs` and de-f75o.
        let folded = match asked.collapse_runs {
            true => Some(graph.collapsing_runs()),
            false => None,
        };
        let mut members: HashMap<DialogueNodeId, Vec<DialogueNodeId>> = HashMap::new();
        if let Some(folded) = folded.as_ref() {
            for (&member, &head) in &folded.into_head {
                members.entry(head).or_default().push(member);
            }
        }
        let graph = folded.as_ref().map_or(&graph, |folded| &folded.graph);
        let into_head = |id: DialogueNodeId| {
            folded
                .as_ref()
                .and_then(|folded| folded.into_head.get(&id).copied())
                .unwrap_or(id)
        };
        // AN ENTRY STANDS FOR ITS WHOLE RUN, so it has been seen in some earlier game only where
        // every entry it stands for has. Anything less and a run holding something unread would
        // read as read, which is the one way folding could hide content from the marking.
        let seen_any_game = |id: DialogueNodeId| {
            seen_any_game(id)
                && members
                    .get(&id)
                    .is_none_or(|run| run.iter().all(|member| seen_any_game(*member)))
        };
        let starts: Vec<_> = profile.starts.iter().map(|id| into_head(*id)).collect();
        match menu(
            graph,
            conversation,
            &starts,
            &seen_any_game,
            budget,
            &snapshot,
            &walk,
            &asked,
        ) {
            Some(measured) => row(conversation, graph, starts.len(), "", prep, Some(&measured)),
            // THE MACHINE COULD NOT SUPPLY THE BUDGET, which is not a finding about the
            // menu. Loud, and a different word from a slow row, so a folder holding one is
            // not read as a measurement.
            None => row(conversation, graph, starts.len(), NOT_MEASURED, prep, None),
        };
    }
}

/// One menu: every option answered against one manager, warmed by the menu itself.
///
/// `conversation` is the row's, whose start the `hybrid-hub` marking walks from.
/// Fits a group's graph to the world its row will be measured in.
///
/// THE SAME WORLD `menu` MEASURES IN, wrapped the same way, because it has to be: a graph fitted
/// to one world and measured in another describes neither.
fn fitted_to(graph: &mut LookAheadGraph, world: &WorldSnapshot) {
    let world = SnapshotWorld::declaring(world.clone(), None);
    graph.fit(&lookahead_engine::graph::Fitting::read(graph, &world));
}

fn menu<F>(
    graph: &LookAheadGraph,
    conversation: i32,
    starts: &[DialogueNodeId],
    seen_any_game: &F,
    budget: DiagramBudget,
    snapshot: &WorldSnapshot,
    walk: &[DialogueNodeId],
    asked: &Options,
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
        // THE WALK IS THE CALLER'S, built before the clock starts: it stands in for the walk the
        // plugin records as the conversation plays, which costs the engine nothing. What the
        // engine does with it - the group's hubs, the cut - is inside the timing below. Which
        // walk it is depends on the menu asked about; see `MenuKind`.
        let began = Instant::now();
        let symbols = graph.symbols().clone();
        // THE SAVE AS IT STANDS, taken WHOLE rather than patched: it is a world a playthrough
        // really was in, and editing a field of it would put it back among the worlds nobody
        // walked to. Nothing has opened this conversation, so no `once` has fired and no `seen`
        // slot is set - the entries it calls read were read in an EARLIER playthrough, which is
        // the seen-any-game set and the profile's to say.
        //
        // THE SAME WORLD `main` FITTED THE GRAPH TO, which it must be - see `fitted_to`.
        let world = SnapshotWorld::declaring(snapshot.clone(), None);
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
            arms: asked.arms(),
        };
        let marking_budget = if asked.nolimit {
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
        let found = match asked.marking {
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
