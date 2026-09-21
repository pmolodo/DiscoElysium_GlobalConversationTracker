// SPDX-License-Identifier: MIT
//! Does an option reach unread content WITHOUT coming back through the menu?
//!
//! The marking asks how far away the nearest unread line is, and proving an exact distance is
//! what costs: twenty-four layers and 64 million diagram nodes on conversation 761, against a
//! worklist pass that meets in under half a second on the same group. de-0jsf.17 asks whether
//! a coarser question is worth as much to a player and costs almost nothing.
//!
//! THE QUESTION HERE IS BINARY. Cut every other option of the menu, then ask plain
//! reachability: can this option still reach anything unread? A route that had to return
//! through a sibling option cannot survive the cut, so a yes means the option leads ONWARD
//! and a no means it only ever gets there by looping back through the menu first.
//!
//! That is the distinction de-0jsf.17 says a player would actually notice - 761's ten
//! candidates sit at distances 23 to 25, so the exact answer is separating options nobody
//! could tell apart - and it is asked with the fixed point this engine is fastest at.
//!
//! ## What it prints
//!
//! One row per option: whether it reaches unread content with its siblings cut, whether it
//! reaches any with them left in, and what each cost. The `onward` column is the proposed
//! marker; `at all` is what a reachability-only marking would have said.
//!
//! THE ROW THAT DECIDES THE IDEA is the count at the bottom. A marker that says the same
//! thing about every option in a menu is worth nothing - that is what the marking before
//! de-0jsf.7 did, and why the nearest rule was introduced at all.
//!
//! ## What it said, 2026-09-11: it separates, and it does it at the player's budget
//!
//! The seven heavy groups, eight options, ten unread, THE PLAYER'S OWN 256 MB:
//!
//! ```text
//!   conv   lead onward   reach anything   ms per option
//!    761        4 of 8           5 of 8       97 - 1,028
//!    631        3 of 8           7 of 8          23 - 57
//!    640        5 of 8           7 of 8          19 - 43
//!     16        3 of 8           7 of 8          10 - 23
//!    368        0 of 8           0 of 8          33 - 88
//!     14        0 of 8           0 of 8          19 - 33
//!   1030        0 of 8           0 of 8          12 - 50
//! ```
//!
//! 761's EIGHT OPTIONS TOGETHER COST ABOUT 1.9 SECONDS AT 256 MB, where the exact distance
//! cannot be answered at 256 MB at all and costs 10.8 seconds at six gigabytes.
//!
//! AND IT CARRIES REAL INFORMATION over plain reachability, which is the other half of being
//! worth having. On 631 a reachability marker lights seven options of eight - the
//! uselessness the nearest rule was introduced to fix - where this lights three. On 16 it is
//! seven against three, on 640 seven against five. The three groups that mark nothing mark
//! nothing either way, which matches their zero rounds under the exact search.
//!
//! ## THE WHOLE GAME, 2026-09-11: 2.6 seconds, and it discriminates
//!
//! All 395 menus, one process per group, the player's 256 MB, nothing crashed.
//!
//! ```text
//!   onward, whole game        2,589 ms    median menu 0   p90 9   max 967
//!   the exact marking        17,346 ms    median menu 20  p90 34  max 2,886
//! ```
//!
//! SEVEN TIMES CHEAPER OVER THE GAME, and the shape is better than the total suggests: the
//! median menu costs NOTHING measurable and the worst in the game is 761 at 967 ms - the
//! group that cannot be answered at this allowance at all by an exact distance.
//!
//! ```text
//!     conv   onward_ms   onward   reachable
//!      761         967      4/8         5/8
//!      368         254      0/8         0/8
//!       14         155      0/8         0/8
//!      631         152      3/8         7/8
//!     1030         144      0/8         0/8
//!      640         139      5/8         7/8
//!       16          82      3/8         7/8
//! ```
//!
//! AND IT ACTUALLY POINTS SOMEWHERE, which is the whole reason the nearest rule was
//! introduced. Over the 371 menus where anything unread is reachable at all:
//!
//! ```text
//!   menus where onward separates some options from others   321 of 371
//!   menus where every reachable option is onward             25
//!   menus where no option is onward - every route loops back 25
//!
//!   options lit by plain reachability   2,484 of 2,778 offered   (89%)
//!   options lit by onward                 831 of 2,778           (30%)
//! ```
//!
//! Reachability lights nearly nine options in ten, which is a marker that tells a player
//! nothing. Onward lights three in ten and separates the menu in 87 per cent of the cases
//! where there is anything to separate.
//!
//! THE 25 MENUS WHERE NOTHING IS ONWARD are the honest cost of this marker: unread content
//! is reachable but every route returns through the menu first, so the player is shown no
//! guidance where a nearest-distance marking would have picked a winner. Whether that should
//! draw as "nothing" or as a third state is a decision de-0jsf.17 has not taken.
//!
//! ## Against the exact marking: 22 of 32 agree, and the other 10 are not mistakes
//!
//! ```text
//!   conv   agree   marked but not onward   onward but not marked
//!    761     6/8                       1                       1
//!    631     5/8                       3                       0
//!    640     6/8                       2                       0
//!     16     5/8                       1                       2
//! ```
//!
//! THE TWO ANSWER DIFFERENT QUESTIONS AND ARE MEANT TO. "Marked" is competitive and relative:
//! the greedy marks at most one option a round, so an option is marked because no rival
//! reached that line sooner. "Onward" is absolute: this option gets there without returning
//! through the menu. Neither implies the other, and both directions actually occur.
//!
//! 16:918 IS MARKED AT DISTANCE 9 AND IS NOT ONWARD - it is the nearest way to a line, and it
//! still has to loop back to get there. 761:848 is onward and unmarked - it leads somewhere
//! new directly, and some rival happened to reach that line first. Which of those a player
//! wants told is the whole question de-0jsf.17 was opened on, and the answer there is the
//! second: eliminate the options that make you come back when others do not.
//!
//! ONWARD IS THE MORE SELECTIVE OF THE TWO on the groups where it matters - 3 against 6 on
//! 631, 5 against 7 on 640 - and it never marked nothing where the exact marking marked
//! something. Both of those are the right way round for a marker meant to point somewhere
//! rather than to light up.
//!
//! ## And the bound's slack is CONSTANT within a group, which is de-0jsf.18's answer
//!
//! `--compare` also prints each option's OWN nearest distance - the same marking asked
//! with no rivals - beside the structural lower bound, which is a zero-one walk over links
//! with no diagrams at all. The gap between them does not vary:
//!
//! ```text
//!   conv   own distances        bounds   slack
//!    761   23 23 23 - 24 - 24 -    8 8 8 8 9 8 9 8      15 on every reachable option
//!    631   16 18 18 18 17 17 - 18  10 12 12 12 11 11 11 12    6
//!    640   14 13 14 14 13 14 14 -   8  7  8  8  7  8  8  7    6
//!     16    - - - - - - - 4        ...                        1
//! ```
//!
//! A CONSTANT OFFSET PRESERVES ORDER, so the free bound ranks the reachable options exactly
//! as their true distances do - on 761 every bound of 8 is a true 23 and every bound of 9 is
//! a true 24, and on 631 the bound's order is the true order with nothing out of place.
//!
//! ITS ONE FAILURE IS THE UNREACHABLE. 761:1007, :856 and :806 all carry a bound of 8, tied
//! with the best, and none of them arrives at all. 631:75 and 640:380 are the same. A bound
//! is a lower bound and cannot tell "close" from "never", so ranking by it alone would put
//! three dead options at the top of 761's menu.
//!
//! THE TWO HALVES FIT TOGETHER. Reachability - or the onward form of it above - says which
//! options arrive, and it is the cheap pass this engine is fastest at. The structural bound
//! orders the survivors, and costs no diagram work whatsoever. Between them that is a
//! complete marking without a layered pass anywhere in it.
//!
//! THE OBVIOUS OBJECTION IS THE PROFILE, and `--scatter` answers it. The runs above
//! mark the ten DEEPEST entries unread, which clusters the targets: every option's route
//! shares one long tail and the options differ only near the front, which is exactly where a
//! structural walk is accurate. A real save's unread lines are spread through the group.
//!
//! Spreading them makes the bound BETTER, not worse:
//!
//! ```text
//!   conv   own distances          bounds          slack
//!    640    1  2  1  1  2  3  1 -    1 2 1 1 2 3 1 2    0 everywhere
//!    631    4  6  6  6  2  5  - 6    3 5 5 5 2 4 4 5    0 or 1
//! ```
//!
//! On 640 the bound IS the distance. On 631 it is short by one on six options and exact on
//! the seventh, and the ordering it gives is the true ordering with nothing out of place.
//! Scattered targets sit nearer, so there is less room between an option and its content for
//! a guard to add a detour the structural walk cannot see.
//!
//! So the ordering holds under both profiles, and the clustered one is the harder of the two.
//!
//! ## How to run it
//!
//! `--compare` also runs the exact marking in the same manager and prints which options
//! it marked, which is the check that says whether the cheap answer is the RIGHT answer: a
//! marker that separates the options but separates the wrong ones is worse than none.
//!
//! ```text
//! cargo run --release --example onward_or_back -- --conversation 761 \
//!   tools/run-logged.sh cargo onward -- cargo run --release --example onward_or_back
//! ```
//!
//! `--starts`, `--unseen` and `--budget-mb` mean what they mean in `menu_matrix`.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use lookahead_engine::bridge::{GameWorld, WorldRawData};
use lookahead_engine::core::types::{DialogueNodeId, SeenState, StartBranch};
use lookahead_engine::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::backward::{Backward, Budget as PassBudget};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::known::GroupShape;
use lookahead_engine::symbolic::menu;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::search::Search;
use lookahead_engine::symbolic::seen_state_search::Where;
use lookahead_engine::symbolic::vars::DataVars;

use gct_measure::common;

use gct_measure::options;

use gct_measure::menu_profile;
use menu_profile::MenuProfile;

const CONVERSATIONS: [i32; 7] = [761, 631, 640, 368, 14, 1030, 16];
/// What a player's search gets, TAKEN FROM THE ENGINE rather than restated. A menu is
/// answered under the shipped allowance, so a number typed here would go stale the day
/// that one moved and the row would quietly stop being what it claims to be.
const BUDGET_MB: usize = DiagramBudget::DEFAULT_MEMORY_BUDGET / (1024 * 1024);
const STARTS: usize = 8;
const UNSEEN: usize = 10;
const COUNTER_CAP: i32 = 16;
const EACH_MS: u64 = 60_000;

/// What this driver takes.
///
/// HANDED DOWN, because the scatter flag, the compare flag and the per-pass ration are read
/// well below `main` - in `ask` and in `reaches` - and reading them there is how an option
/// comes to be decided where no caller can see it.
#[derive(clap::Parser)]
#[command(about = "Whether the onward question or the backward one answers a menu sooner.")]
struct Options {
    #[command(flatten)]
    groups: options::Groups,
    #[command(flatten)]
    starts: options::Starts<STARTS>,
    #[command(flatten)]
    unseen: options::Unseen<UNSEEN>,
    #[command(flatten)]
    budget: options::Budget<BUDGET_MB>,
    /// Scatter the unseen entries through the group rather than taking the deepest
    #[arg(long)]
    scatter: bool,
    /// Also ask the question the other way, and report both
    #[arg(long)]
    compare: bool,
    /// What one pass is allowed, in milliseconds
    #[arg(long = "each-ms", value_name = "MS", default_value_t = EACH_MS)]
    each_ms: u64,
}

fn main() {
    let asked = <Options as clap::Parser>::parse();
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");
    let budget = DiagramBudget::new(asked.budget.bytes());
    for conversation in asked.groups.or(&CONVERSATIONS) {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            eprintln!("conversation {conversation}: no group builds from it; skipping.");
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        let Some(profile) = MenuProfile::of(&graph, root, asked.unseen.unseen, asked.starts.starts)
        else {
            println!("conversation {conversation}: no menu");
            continue;
        };
        isolated::on_its_own_thread(|| {
            ask(conversation, &graph, &profile, budget, &asked);
            Some(())
        });
    }
}

fn ask(
    conversation: i32,
    graph: &LookAheadGraph,
    profile: &MenuProfile,
    budget: DiagramBudget,
    asked: &Options,
) {
    let symbols = graph.symbols().clone();
    let world = GameWorld::declaring(
        WorldRawData {
            day_minutes: 720,
            day_counter: 1,
            ..Default::default()
        },
        common::declared(),
    );
    let layout = DataLayout::for_group(graph, &world, COUNTER_CAP);
    let Some(vars) = DataVars::try_new(&layout, &symbols, budget) else {
        eprintln!("conversation {conversation}: no room for the variables.");
        return;
    };
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(&world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, &world, &vars).expect("room for a seed");
    let shape = GroupShape::of(graph);
    let options: HashSet<_> = profile.starts.iter().copied().collect();

    // SCATTERED RATHER THAN CLUSTERED, on request. The profile marks the structurally
    // DEEPEST entries unread, which puts every target in one place: each option's route then
    // shares a long tail and the options differ only near their own end, which is exactly
    // where a structural walk is accurate. A real save's unread lines are spread through the
    // group, and whether the bound's slack survives that is the question de-0jsf.18 turns on.
    let unseen: HashSet<_> = if asked.scatter {
        let mut all: Vec<_> = graph
            .nodes()
            .filter(|n| !n.is_group && !options.contains(&n.id))
            .map(|n| n.id)
            .collect();
        all.sort_by_key(|id| (id.conversation_id, id.entry_id));
        let wanted = profile.unseen.len().max(1);
        let stride = (all.len() / wanted).max(1);
        all.into_iter().step_by(stride).take(wanted).collect()
    } else {
        profile.unseen.iter().copied().collect()
    };
    let seen_state = move |id: DialogueNodeId| match unseen.contains(&id) {
        true => SeenState::UnseenAnyGame,
        false => SeenState::SeenThisGame,
    };

    let targets: Vec<_> = graph
        .nodes()
        .filter(|n| {
            !n.is_group && seen_state(n.id) > SeenState::SeenThisGame && !options.contains(&n.id)
        })
        .map(|n| n.id)
        .collect();

    println!(
        "\n== conversation {conversation}: {} options, {} unread",
        profile.starts.len(),
        targets.len()
    );
    println!(
        "   {:>10}  {:>7}  {:>8}  {:>7}  {:>8}",
        "option", "onward", "ms", "at all", "ms"
    );

    let mut onward = 0usize;
    let mut at_all = 0usize;
    for &start in &profile.starts {
        // SIBLINGS CUT. A route that had to return through another option of this menu
        // cannot survive it, so what is left is what this option reaches on its own.
        let siblings: HashSet<_> = options.iter().copied().filter(|id| *id != start).collect();
        let (cut_yes, cut_ms) = reaches(
            Search {
                graph,
                compiler: &mut compiler,
                world: &world,
                counter_cap: COUNTER_CAP as u32,
                arms: Default::default(),
            },
            &targets,
            &siblings,
            &seed,
            &shape,
            start,
            asked,
        );
        let (open_yes, open_ms) = reaches(
            Search {
                graph,
                compiler: &mut compiler,
                world: &world,
                counter_cap: COUNTER_CAP as u32,
                arms: Default::default(),
            },
            &targets,
            &HashSet::new(),
            &seed,
            &shape,
            start,
            asked,
        );
        onward += usize::from(cut_yes);
        at_all += usize::from(open_yes);
        println!(
            "   {:>5}:{:<4}  {:>7}  {:>8}  {:>7}  {:>8}",
            start.conversation_id,
            start.entry_id,
            if cut_yes { "YES" } else { "no" },
            cut_ms,
            if open_yes { "YES" } else { "no" },
            open_ms
        );
    }
    println!(
        "   SEPARATES: {onward} of {} lead onward, {at_all} reach something at all",
        profile.starts.len()
    );

    if !asked.compare {
        return;
    }
    // AGAINST THE EXACT MARKING, which is the only thing that says whether the cheap answer
    // is the RIGHT answer. A marker that separates the options but separates the wrong ones
    // is worse than no marker at all.
    let contestants: Vec<_> = profile
        .starts
        .iter()
        .map(|&start| menu::Contestant {
            position: Where::of(
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
    let began = Instant::now();
    let found = menu::mark_menu(
        Search {
            graph,
            compiler: &mut compiler,
            world: &world,
            counter_cap: COUNTER_CAP as u32,
            arms: Default::default(),
        },
        &seen_state,
        &contestants,
        &menu::Budget {
            wall: Duration::from_secs(1200),
            each: Duration::from_secs(600),
        },
        &shape,
    );
    println!("   exact marking in {} ms:", began.elapsed().as_millis());
    println!(
        "   {:>10}  {:>6}  {:>8}  {:>5}  {:>5}  {:>5}",
        "option", "marked", "distance", "own", "bound", "slack"
    );
    for (index, (start, mark)) in profile.starts.iter().zip(&found.marks).enumerate() {
        // ITS OWN NEAREST, not the one the competition left it. A menu of one contestant is
        // the same marking asked without rivals, so round one's distance is this option's
        // own distance to the nearest unread line - which is what a RANKING would order by
        // and what the greedy's answer deliberately is not (de-0jsf.18).
        let alone = menu::mark_menu(
            Search {
                graph,
                compiler: &mut compiler,
                world: &world,
                counter_cap: COUNTER_CAP as u32,
                arms: Default::default(),
            },
            &seen_state,
            std::slice::from_ref(&contestants[index]),
            &menu::Budget {
                wall: Duration::from_secs(600),
                each: Duration::from_secs(300),
            },
            &shape,
        );
        let own = alone.marks[0].distance;
        // The structural lower bound this option has on the nearest unread line: a zero-one
        // walk over links with no guards and no diagrams at all.
        let bound = lookahead_engine::symbolic::seen_state_search::choice_bounds(
            graph,
            &contestants[index].position,
            &HashSet::new(),
        );
        let least = targets.iter().filter_map(|id| bound.get(id)).min().copied();
        println!(
            "   {:>5}:{:<4}  {:>6}  {:>8}  {:>5}  {:>5}  {:>5}",
            start.conversation_id,
            start.entry_id,
            if mark.round.is_some() { "YES" } else { "no" },
            mark.distance.map_or("-".into(), |d| d.to_string()),
            own.map_or("-".into(), |d| d.to_string()),
            least.map_or("-".into(), |d| d.to_string()),
            match (own, least) {
                (Some(o), Some(b)) => (o.saturating_sub(b)).to_string(),
                _ => "-".into(),
            }
        );
    }
}

/// Whether anything in `targets` is reachable from `start` with `cut` refused.
fn reaches(
    mut search: Search<'_, '_>,
    targets: &[DialogueNodeId],
    cut: &HashSet<DialogueNodeId>,
    seed: &oxidd::bdd::BDDFunction,
    shape: &GroupShape,
    start: DialogueNodeId,
    asked: &Options,
) -> (bool, u128) {
    let began = Instant::now();
    let position = Where::of(
        search.graph,
        start,
        StartBranch::Either,
        seed,
        search.compiler,
        search.world,
        search.counter_cap,
    )
    .position(start);
    let mut known = shape.known_from(search.graph, start);
    for &entry in &position.entries {
        known = known.from(entry, &position.holding);
    }
    let pass = Backward::reaching_any_knowing(
        search.reborrow(),
        targets,
        cut,
        &PassBudget {
            time: Duration::from_millis(asked.each_ms),
            steps: usize::MAX,
            ..Default::default()
        },
        Some(&known),
    );
    let met = pass.stats().met_at.is_some();
    (met, began.elapsed().as_millis())
}
