// SPDX-License-Identifier: MIT
//! Two allowances, six conversations, five profiles: the whole grid. Plus seven more
//! profiles held back for being too easy, and a census of the game two of the five depend on.
//!
//! A DEFAULT RUN MEASURES ONE OF THE TWO: `ingame`, the search the game runs at a player's
//! own settings, which is the column every tuning decision is read off. `nolimit` is the
//! same search with the limits taken off, and costs up to two minutes a row against the
//! other's one second, so it is asked for rather than assumed - `DEGCT_ENGINES=all` for
//! both, or either name for one column. See [`engines`].
//!
//! The measurements this repository already has each ask one question well. This asks the
//! same question of every combination, because what is wanted is a surface rather than a
//! handful of points: how the cost moves with the group, with how much of it the player has
//! read, and with what the search is allowed to spend.
//!
//! ## The two columns, and what each one is
//!
//! ONE SEARCH, TWO ALLOWANCES. Both run `answer::best_novelty` over the backward driver -
//! ONE CANDIDATE AT A TIME, best novelty class first, stopping at the first candidate proved
//! reachable - so the name says what the column may spend and nothing else:
//!
//! | column | allowance |
//! |---|---|
//! | `ingame` | the player's own settings, through [`in_game_request`] |
//! | `nolimit` | a two-minute wall and a measurement's manager |
//!
//! ASKING ABOUT ONE HAND-PICKED TARGET WOULD MEASURE A QUESTION NOBODY ASKS. The candidate
//! ordering is what makes the short circuit sound, and the per-candidate cost when the
//! answer is no is the cost a player actually waits for.
//!
//! THESE ARE THE NAMES `DEGCT_ENGINES=` TAKES, and [`Engine::label`] is where they live.
//!
//! ## What the search costs, and why that is the whole question
//!
//! When the answer is NO it pays one fixed point PER CANDIDATE. So the cost of a row should
//! fall out of two numbers it already has - how many candidates were waiting, and what one
//! pass cost - and a row where it does not is the interesting one.
//!
//! IT PRUNES ITSELF, which is why one pass is cheaper than its shape suggests: a variable
//! enters a backward formula only if a guard on some path to the target reads it, and a
//! write erases its slot. That is the cone-of-influence reduction, got for nothing rather
//! than from a separate analysis.
//!
//! ## The grid
//!
//! Six conversations - the five heaviest plus 362, the largest in the game - against five
//! profiles describing how much of the group the player has read:
//!
//! - the deepest 1, 5 and 10 entries unseen;
//! - the deepest 1 and 5 entries NO PATH CAN REACH.
//!
//! ALL FIVE ARE DEEP PROFILES, and the seven percentage-seen ones that used to sit beside
//! them are held back now. See [`PROFILES`] and [`TOO_EASY`] for the measurement that split
//! them: the deep profiles reach twenty-two to fifty-four seconds on the `nolimit` column
//! while the percentage ones top out at 1.1, and the seven of them give one single answer
//! on 92.5 per cent of groups. They are still runnable by name.
//!
//! ## The two that need a census: `deepest-unreach-1` and `-5`
//!
//! The unseen profiles measure the direction the search stops early in. A deep entry that IS
//! reachable is proved the moment the backward pass meets the seed, and that is usually
//! instant - so "deepest-N" is the adversarial SHAPE without the adversarial COST. The
//! expensive question is an entry no path can reach, because a no has to be proved, which
//! means driving the fixed point to completion rather than stumbling on a yes.
//!
//! Which entries those are is not something a row can work out for itself. It takes a
//! bounded backward pass per candidate, so two runs could disagree about what the profile
//! even is, and the cost would land inside the clock the row exists to report. So it is
//! measured once, over every group, and written down - `DEGCT_CENSUS=1`, driven by
//! `tools/measure-census.sh` - and the rows READ it:
//!
//! ```text
//! DEGCT_CENSUS_OUT=measurements/logs/2026-09-08_census tools/measure-census.sh all
//! DEGCT_CENSUS_FILE=measurements/logs/2026-09-08_census/census.tsv \
//!   DEGCT_PROFILES=deepest-unreach-1,deepest-unreach-5 tools/measure-matrix.sh all
//! ```
//!
//! THE SCRIPT TAKES ONE WHEN THE GRID NEEDS IT AND NOTHING NAMED ONE, into the run's own
//! folder, before the first row - so the two commands above collapse into the ordinary one
//! and the census still lands beside the rows drawn from it. It is taken only when
//! `CENSUS_FILE` is unset and the folder holds no census yet; a named file is used exactly
//! as given, and NOTHING CHECKS WHETHER IT IS CURRENT. A census taken under a different
//! world from the one being measured is a real way to get wrong rows, and the run says so
//! when it sees the contradiction - see the note on `found` below - but it cannot see it in
//! advance and does not pretend to.
//!
//! Running the binary by hand with an unreachable profile and no `CENSUS_FILE` is still
//! refused outright rather than classified on the spot: see [`Census::of`].
//!
//! TWO ROWS ARE SKIPPED RATHER THAN RUN, and the TSV says which rule skipped them. A group
//! with no unreachable entries poses no such question at all; a group with exactly one
//! produces a five-entry profile that is one hard question and four instant ones wearing the
//! name of a hard profile.
//!
//! WHERE THE CENSUS FOUND FEWER THAN N, THE SET IS SMALLER - it is not padded out with
//! reachable entries. So every entry in a `deepest-unreach-N` set is one no path can reach,
//! which is what the name says, and `unseen` is how many that turned out to be. Anything
//! comparing these rows reads that column rather than the number in the name.
//!
//! THAT MAKES `found` IMPOSSIBLE ON THESE ROWS, which is worth more than the tidiness: with
//! nothing reachable in the set there is nothing to find, so a `found` means the census and
//! the search disagree. The run says so loudly instead of recording it, and names the likely
//! cause - a `CENSUS_FILE` taken under a different world from the one being measured, which
//! nothing checks.
//!
//! ## Why "deepest" for the small counts and "random" for the percentages
//!
//! They are asking different things and the difference is the point.
//!
//! DEEPEST IS THE ADVERSARIAL CASE. An entry at the end of the longest chain is the one the
//! search reaches last, so seeding it and nothing else poses the hardest question the group
//! can pose. Depth here is by EDGE ANALYSIS ALONE - links followed, guards ignored - which
//! makes it an upper bound on reachability: an entry it cannot find is unreachable for
//! certain, so a quarry drawn from it is at least structurally fair.
//!
//! What it is NOT is a question the search can necessarily answer. Entries that deep are
//! often ones the guards shut, so these rows frequently read "explore everything, find
//! nothing" - and that is exactly the worst case for cost, which is what these rows are for.
//! THE FALLOFF CURVE IS A DIFFERENT MEASUREMENT AND NOBODY HAS ONE, DELIBERATELY. It needs
//! seeding by REACH ORDER rather than by depth, because a flat "found nothing" series
//! measures nothing about falloff. `tests/unseen_falloff.rs` did it and went in 15969c1
//! with the state-at-a-time engine; de-bnjy.5 is the decision not to replace it, since
//! nobody wants the curve. Said plainly rather than cited, because this file pointed at
//! that path for months after it stopped existing.
//!
//! RANDOM IS THE TYPICAL CASE. A save does not read a conversation depth-first; it reads
//! whatever the conversation led it to. Drawing uniformly from the structurally reachable
//! entries is the closest thing to a real profile that needs no real profile.
//!
//! The seed is the percentage, so a row is reproducible and two rows are not accidentally
//! the same draw.
//!
//! ## Why there is no all-seen row any more
//!
//! There was one, as a ceiling: nothing unseen means nothing can stop the search early, so
//! it said how much there was to explore at all. It is gone for two reasons.
//!
//! IT IS NOT A SCENARIO ANYBODY RUNS. The mod never asks it - the bridge's no-improvement
//! shortcut answers a fully-read save without starting a search, so the row measured the raw
//! engine being asked a question it is never asked. On conversation 28 it read 222,400
//! states where the same profile through the bridge costs zero.
//!
//! AND A BACKWARD SEARCH CANNOT RUN IT AT ALL. Backward starts from a target and works out
//! which states reach it; with nothing unseen there is no target, so the row has no meaning
//! on that side and cannot be compared across engines - which is what this file is for.
//!
//! ## The row that made the case for this search, 2026-09-05
//!
//! Conversation 14 with its one structurally deepest entry unseen - so ONE CANDIDATE - at
//! the measurement budget:
//!
//! ```text
//!   conv  entries  profile    unseen  verdict         ms   nodes  asked  cands
//!     14     3594  deepest-1       1  not-there  326-476  28,467      1      1
//! ```
//!
//! Run twice, on a quiet machine and a busy one. The time moved and the set size did not,
//! which is the reassuring way round for a measurement of a representation.
//!
//! NOT-THERE IS A SETTLED ANSWER here, not a budget running out: every candidate was asked
//! about and every pass reached a fixed point. The approximation also runs the safe way -
//! the backward sets are over-approximations, so a state missing from one genuinely cannot
//! reach the target.
//!
//! WHAT IT DOES NOT SAY is anything about a long candidate list. One candidate is the shape
//! this is best at, and the expensive case - a long list none of which is reachable, where
//! a refusal is paid for once per candidate - is untouched at `cands` of one. That is what
//! the percentage profiles are for.
//!
//! ## Running it
//!
//! One conversation per process, because a diagram manager that runs out of nodes takes the
//! whole process with it and a crash in the fourth row should not cost the other five:
//!
//!     DEGCT_CONVERSATION=368 cargo run --release --example performance_matrix
//!
//! `CONVERSATION`, `PROFILE` and `ENGINES` each narrow the grid, and all three take a
//! comma-separated list. A third engine triples what a full run costs, so being able to
//! ask one question of one group is not a convenience:
//!
//!     DEGCT_CONVERSATION=14 DEGCT_PROFILE=deepest-1 DEGCT_ENGINES=bwd cargo run --release \
//!         --example performance_matrix
//!
//! THE HEADER FOLLOWS THE SELECTION - a run that names one engine prints that engine's
//! columns and no others, so a narrowed run is never a wide row with holes in it. Ask for
//! the header alone with `DEGCT_HEADER_ONLY=1`, which is how `tools/measure-matrix.py` learns the
//! column names rather than keeping its own copy of them.
//!
//! `DEGCT_CONSTANTS_ONLY=1` prints, in the same spirit, the numbers the driver has to do
//! arithmetic with - the memory budget in megabytes, the bytes a diagram node costs, and the
//! per-engine cap in seconds. They live in `src/symbolic/budget.rs` and the driver used to
//! carry its own copies.
//!
//! ## Every group in the game, with `DEGCT_GROUPS_ONLY=1`
//!
//! Prints one line per DISTINCT group - `start`, `conversations`, `entries`, `reachable` -
//! and measures nothing. It is how a whole-game run enumerates its rows, for the same
//! reason `HEADER_ONLY` exists: the alternative is a list written by hand somewhere else,
//! which can silently omit what nobody thought of.
//!
//! `reachable` IS HOW A RUN SKIPS WHAT IT CANNOT MEASURE. It counts the entries a profile
//! could be built from, and a zero means the group has no rows - see [`NoRows`] for the
//! three ways that happens, one of which is said on stderr per group. 901 of the game's
//! 1,422 groups are zero, nearly all of them the two-entry ORB stubs the database is full
//! of, and a run that measured them anyway spent 9,010 processes to be told so one row at
//! a time. The count is asked and not written down, exactly as the group list is: a
//! committed file of empty groups would be a second list to go stale, and the one thing
//! worse than paying for those processes is skipping a group that does have rows.
//!
//! WHY A CANONICAL START IS NOT SIMPLY THE SMALLEST MEMBER. `discover_group` is the FORWARD
//! closure of a start, not an equivalence relation, so the smallest conversation in a group
//! may reach only part of it - a group of {3, 5} where 5 leads to 3 and 3 leads nowhere has
//! `closure(3) = {3}`. The start named here is the smallest one whose own closure IS the
//! whole set, which is the only kind of start that reproduces the group it came from.
//!
//! The rows are printed as TAB-SEPARATED VALUES, so a run can be piped straight into a
//! file and read by something else later - which is what de-raed asks for when it says the
//! logs should be kept for analysis.

use std::collections::{BTreeSet, HashMap, HashSet};

use lookahead_engine::core::state::StateSymbols;
use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, discover_group, read_index};
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated;
use lookahead_engine::symbolic::known::GroupShape;

use lookahead_engine::symbolic::answer;
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::world::world::ILookAheadWorld;

#[path = "../tests/common/mod.rs"]
mod common;

// ONE DEFINITION OF "UNREACHABLE", pulled in rather than written again. The census below and
// the `deepest-unreach-N` profiles both turn on which entries no path can reach under the
// world's conditions, and that is a bounded backward pass per candidate that already exists
// next door. Two copies would be two answers, and the whole point of writing the census down
// is that a profile can be built from the same set a later reader sees.
//
// ALLOWED TO BE MOSTLY UNUSED, because it is a whole measurement rather than a library: its
// own `main`, its own conversation list and its own profile builder come along with the one
// function wanted here, and every one of them would otherwise be a dead-code warning in this
// build. The alternative is a third file holding the classifier, which buys a clean warning
// list and costs the thing that made this the right shape - that symbolic_answers.rs is
// where somebody looking for "what does unreachable mean" already goes.
#[path = "symbolic_answers.rs"]
#[allow(dead_code)]
mod symbolic_answers;

// The structural half of what a row is measured under: the depths, the entries a profile may
// name, and the draw a percentage makes over them. Shared because a conversation and a
// profile name have to mean ONE set of unseen entries wherever the pair is written down.
#[path = "seen_profile.rs"]
mod seen_profile;
use seen_profile::{candidates, percent_unseen};

/// The six heaviest groups, 362 included.
const HEAVIEST: [i32; 6] = [362, 368, 631, 14, 28, 1030];

/// The same allowance for every engine, so the columns can be read against each other.
///
/// THE MEASUREMENT ALLOWANCE, NOT THE PLUGIN'S. This used to be `DEFAULT_MEMORY_BUDGET`,
/// which is a product decision about what a player's machine should give a response menu -
/// a fine ceiling to run with and the wrong one to measure against, because a row that
/// says "no room" then reports the ration rather than the algorithm. What is wanted here
/// is where the search actually stops, so it gets the shared measurement budget and the
/// default the plugin runs under is left alone.
///
/// Both engines take it: the search in bytes directly, the diagram through
/// [`DiagramBudget`], which turns it into a node capacity and a cache capacity. A hand-
/// picked capacity is what made this unequal before - 2^22 nodes is a hard ceiling of
/// about 134 MB, half what the search was allowed, and conversations 631 and 14 reported
/// "no room" at exactly 4,194,304 nodes, which was that ceiling and not the budget.
/// Override with `ROW_MEMORY_MB`, for the one conversation that wants more than the rest.
///
/// A run reported as a measurement should say which allowance it used, the same way it
/// should say which time cap - two rows given different budgets are not comparable, and
/// nothing in a TSV records the budget.
fn memory() -> usize {
    lookahead_engine::core::env::var("ROW_MEMORY_MB")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(|mb| mb * 1024 * 1024)
        .unwrap_or_else(|| DiagramBudget::measurement().memory())
}

fn budget() -> DiagramBudget {
    DiagramBudget::new(memory())
}

/// How long one engine may spend on one row.
///
/// TEN MINUTES, not the sixty seconds this used to be, and the reason is the budget above.
/// The matrix exists to find where a search actually stops and to answer "how long until
/// every scenario has a CONCRETE answer" - found or not-there rather than gave-up. A cap
/// that fires first answers neither: the row says `gave-up` having never come near
/// spending the memory, and raising the memory to six gigabytes buys nothing at all.
///
/// So the cap is meant to be the thing that does NOT stop a row, and it is here only
/// because a row that will never finish still has to end. Where it fires, the row's own
/// wall time says so and the verdict is `gave-up` as before.
///
/// Override with `ROW_SECONDS` to bound a run that has to fit in an afternoon; a run
/// reported as a measurement should say which cap it used.
const DEFAULT_ROW_SECONDS: u64 = 600;

fn row_time() -> std::time::Duration {
    let seconds = lookahead_engine::core::env::var("ROW_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(DEFAULT_ROW_SECONDS);
    std::time::Duration::from_secs(seconds)
}

/// How often a row should say where it has got to, or None to say nothing until it ends.
///
/// ## Why a row needs this at all
///
/// Because a heavy row is half an hour of silence. Both engines already had the hook - the
/// search reports on a clock, the fixed point every so many steps - and both were passed
/// None here, so a run that took an hour and twenty-seven minutes printed sixty-six lines
/// and nothing in between. There is no percentage to give: neither engine knows how much
/// is left, only how much it has spent. So progress is what it HAS spent, which is the
/// number that matters anyway, because spending the budget is how these rows end.
///
/// ## Thirty seconds by default, since de-fahb
///
/// It used to be off unless `PROGRESS_SECONDS` was set, on the reasoning that "the lines go
/// into the row's log, and a run that is not being watched does not want them". Two things
/// undid that. `tools/measure-matrix.sh` FILTERS - the row log gets everything and the run
/// log gets only the lines carrying [`PROGRESS`] - so an unwatched run pays a few
/// lines in a per-row file nobody opens. And the runs got longer: a whole-game sweep is
/// hours and thousands of rows, where "is it still going" is asked constantly and was
/// answered by nothing, because the cap is 600 seconds PER ENGINE and a heavy row could be
/// silent for half an hour.
///
/// `PROGRESS_SECONDS` still overrides, and `DEGCT_PROGRESS_SECONDS=0` still turns it off.
///
/// LIVES IN `symbolic_answers.rs`, along with [`symbolic_answers::PROGRESS`] and
/// [`symbolic_answers::mmss`], because the census narrates itself on the same clock and two
/// copies of "how often does a long run say where it is" would be two things to keep in
/// step. That file is pulled in here with `#[path]`, so it is the one both can see.

/// The verdict for a row nothing was learned from, in every engine's columns.
///
/// LOUD, and not a word either engine can produce on its own, because the failure it
/// reports is not theirs: the machine could not supply the budget, so the row was never
/// run. A gap or a quiet `gave-up` here would read as a finding about the search.
const NOT_MEASURED: &str = "NOT-MEASURED";

/// The verdict for a row the GAME would not have searched at all.
///
/// de-qh27. `scored` computes a baseline and refuses when nothing link-reachable beats it,
/// returning a complete answer having run nothing; this column exists to be that method, so
/// it has to refuse in the same places.
///
/// ITS OWN WORD, because the obvious alternative - `not-there` with `asked=0` - cannot be
/// told from a backward driver that ran and found no candidates, and the two mean opposite
/// things: one is "nothing was worth looking for", the other is "we looked and there was
/// nothing". A row that conflated them would be read as evidence about the search.
///
/// NOT A FAILURE. The answer is settled and complete; only the work is absent.
const NOT_WORTH_HUNTING: &str = "not-worth-hunting";

const COUNTER_CAP: i32 = 16;

/// How much of a group a profile has read.
#[derive(Debug, Clone, Copy)]
enum Profile {
    /// Everything seen. Nothing to find, and the shortcut should say so without searching.

    /// The n structurally deepest entries unseen: the adversarial case.
    DeepestUnseen(usize),
    /// The n deepest entries NO PATH CAN REACH: the case that has to prove a no.
    ///
    /// THE HARD ONE, and the reason it exists as a profile of its own. A deep entry that IS
    /// reachable is proved the moment the backward pass meets the seed, and that is usually
    /// instant - so [`Profile::DeepestUnseen`] measures the direction that stops early. An
    /// entry nothing can reach has to be refused, which means driving the fixed point to
    /// completion rather than stumbling on a yes.
    ///
    /// THE SET IS READ, NOT DERIVED. Which entries those are comes out of a census taken
    /// beforehand (`DEGCT_CENSUS=1`, see [`census`]) and named by `CENSUS_FILE`. Classifying per
    /// row instead would be wrong twice over: the classification is a bounded pass, so two
    /// runs could disagree about what the profile even IS, and its cost would land inside
    /// the clock the row exists to report.
    DeepestUnreachable(usize),
    /// This percentage of entries seen, the rest unseen, drawn at random: the typical case.
    PercentSeen(u32),
}

impl Profile {
    fn label(self) -> String {
        match self {
            Profile::DeepestUnseen(n) => format!("deepest-{n}"),
            // THE SAME NAME measurements/symbolic_answers.rs uses, so the two measurements
            // do not spell one profile two ways. FIXED rather than carrying how many of the
            // set were genuinely unreachable, which the label used to say there: a label
            // that varies per group cannot be asked for by name, and both the resume and
            // `DEGCT_PROFILE=` key on it. What was real is the `real` column instead.
            Profile::DeepestUnreachable(n) => format!("deepest-unreach-{n}"),
            Profile::PercentSeen(p) => format!("{p}pc-seen"),
        }
    }
}

/// The default grid: the deep profiles, and only the deep profiles.
///
/// A PROFILE EARNS ITS PLACE HERE BY BEING EXPENSIVE, because this is a performance matrix
/// and a row that is instant whatever the search does costs a run its time and tells it
/// nothing. Measured over the 200 largest groups, 2026-09-08
/// (`measurements/logs/2026-09-08_slice-price`, `tools/matrix-profile-cost.py`), on the
/// `nolimit` column - the one that CAN be slow:
///
/// ```text
///   profile             under 1s   median    p90      p99      max
///   deepest-unreach-5      93.5%      144    372    24197    36455
///   deepest-10             96.0%      164    312    22802    53641
///   deepest-5              96.0%      164    311    22423    36628
///   deepest-unreach-1      93.5%      150    391    22373    24553
///   deepest-1              96.0%      155    331    21932    22636
///   25pc-seen              99.5%      173    298      489     1127
///   75pc-seen             100.0%      178    299      429      437
///   ... the other five percentage profiles, all within that band
/// ```
///
/// The five deep profiles reach twenty-two to fifty-four SECONDS; the seven percentage ones
/// top out at 1.1, and six of the seven never pass half a second. That is a fiftyfold gap in
/// the ninety-ninth percentile and it is the whole reason for the split below.
///
/// READ ON `nolimit`, NOT `ingame`, AND THAT IS NOT A DETAIL. The in-game column is walled by
/// the player's own `LookAheadTimeBudgetMs`, so no row of it can be slow - every profile is
/// under a second there, and asking which profiles are expensive in that column deletes the
/// entire grid. A column bounded by construction cannot say what work costs.
const PROFILES: [Profile; 5] = [
    Profile::DeepestUnseen(1),
    Profile::DeepestUnseen(5),
    Profile::DeepestUnseen(10),
    // IN THE DEFAULT GRID, and they were held out of it because they cannot run without a
    // census to read. `tools/measure-matrix.sh` now TAKES one into the run's own folder when
    // a grid needs it and none was named, so the objection is answered rather than
    // outstanding. Running the binary by hand without a census is still refused outright -
    // see `Census::of`, which will not classify on the spot.
    Profile::DeepestUnreachable(1),
    Profile::DeepestUnreachable(5),
];

/// The percentage-seen profiles: runnable by name, and NOT part of the default grid.
///
/// KEPT OUT OF [`PROFILES`] because they are too easy to be worth a whole-game run's time.
/// The table above is the measurement; what it says is that these seven answer in about a
/// fifth of a second whatever the group, so seven tenths of every run was spent confirming
/// that the typical case is still typical.
///
/// THEY ARE ALSO REDUNDANT WITH EACH OTHER, which is the second reason and the one that
/// says why the answer is none of them rather than one of them. Over the same 200 groups all
/// seven give a single answer on 185 of them - 92.5 per cent - while the five deep profiles
/// agree with each other on 33 per cent. Seven readings of one question is not seven
/// questions.
///
/// They remain a sweep of their own, and nothing about a run that names them has changed:
///
///     DEGCT_PROFILES=95pc-seen,50pc-seen,5pc-seen tools/measure-matrix.sh all
///
/// A RUN WITH THIS GRID DOES NOT COMPARE ROW FOR ROW WITH ONE TAKEN BEFORE THE SPLIT. The
/// rows that survive compare exactly as they always did - a profile's definition has not
/// moved - but "a whole-game run" now means five profiles over 1,422 groups rather than ten,
/// and a summary that divides by the row count will not agree with an older one.
const TOO_EASY: [Profile; 7] = [
    Profile::PercentSeen(95),
    Profile::PercentSeen(90),
    Profile::PercentSeen(75),
    Profile::PercentSeen(50),
    Profile::PercentSeen(25),
    Profile::PercentSeen(10),
    Profile::PercentSeen(5),
];

/// Every profile a run can name, which is the default grid plus the ones held back from it.
fn known_profiles() -> impl Iterator<Item = Profile> {
    PROFILES.into_iter().chain(TOO_EASY)
}

/// The two searches a row can hold, named for what tells them apart.
///
/// ONE METHOD, TWO ALLOWANCES, which is the whole of the distinction now: both columns run
/// the same search over the same graph, and they differ in what they are allowed to spend.
/// So the names say the allowance rather than the algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    /// `answer::best_novelty` AT THE PLAYER'S OWN SETTINGS: what somebody waits for.
    ///
    /// de-xegj split this from the column below, because one column cannot answer both of
    /// the questions asked of it - "what does a player wait for" and "where does this method
    /// actually stop" - and the single column answered neither. It ran at
    /// `answer::Budget::default()` on a six-gigabyte manager: a player gets 1000 ms and
    /// 256 MB, and two seconds is neither the shipped default nor a meaningful no-limit.
    ///
    /// ITS BUDGETS ARE ASKED OF THE PRODUCT, not restated here - see [`in_game_request`].
    /// The matrix keeping its own copy of the game's inputs is what produced that bug and
    /// the two filed beside it.
    InGame,
    /// The same method with the limits taken off, walled at two minutes.
    ///
    /// WHERE DOES THIS METHOD ACTUALLY STOP, which the in-game column cannot say because it
    /// stops where the player's settings stop it. The wall is the contract; the rations
    /// under it are estimates aimed at landing inside it.
    ///
    /// BUILT DIRECTLY RATHER THAN THROUGH THE PRODUCT, and that is the one place restating
    /// rations is right: `search_budget` hands a candidate the whole wall, so a two-minute
    /// clock built through it would be two minutes on one candidate. This column wants its
    /// own numbers, and is deliberately not the product's configuration.
    NoLimit,
}

const ALL_ENGINES: [Engine; 2] = [Engine::InGame, Engine::NoLimit];

/// What a run measures when it does not say: the shipped method at the shipped settings.
///
/// [`Engine::InGame`] is the search a player actually waits for, and it is the column every
/// tuning decision is read off. [`Engine::NoLimit`] answers a different question - where
/// this method stops when nothing stops it - and costs up to two minutes a row against the
/// in-game column's one second, so a whole-game run asks for it rather than assuming it.
///
/// `DEGCT_ENGINES=all` measures both. See [`engines`].
const DEFAULT_ENGINES: [Engine; 1] = [Engine::InGame];

/// What to pass for the whole grid, since naming one engine no longer implies the rest.
const ALL: &str = "all";

impl Engine {
    /// The name `ENGINES` selects it by, and the prefix its columns carry.
    fn label(self) -> &'static str {
        match self {
            Engine::InGame => "ingame",
            Engine::NoLimit => "nolimit",
        }
    }

    /// The columns it fills, in order. THE ONE PLACE THE COLUMN NAMES LIVE: the header is
    /// built from these and `tools/measure-matrix.sh` asks the test for it.
    fn columns(self) -> &'static [&'static str] {
        // `setup` is how much of `ms` was building the layout, the manager, the compiled
        // guards and the seed rather than searching - see [`Cells::of`], and note that `ms`
        // still carries the whole of it.
        //
        // `nodes` is what the manager holds at the end of the row, which is what the
        // parallel split clears a group against.
        //
        // `by` is whether the search settled, stopped part way, or answered at the start
        // without searching at all - see `answer::Answered`.
        //
        // BOTH COLUMNS REPORT THE SAME SIX, so the two can be read against each other
        // directly: the same row, the same question, one held to the player's settings and
        // one not.
        //
        // THE COST COLUMNS ARE MACHINE-DEPENDENT AND THE VERDICT IS NOT, which is de-12wr.3
        // rather than a caveat added for safety. A refactor is checked on `verdict`; `ms`
        // and `nodes` are read for their shape across many rows and not row by row.
        &["verdict", "ms", "setup", "nodes", "by", "asked"]
    }

    fn headers(self) -> Vec<String> {
        self.columns()
            .iter()
            .map(|name| format!("{}_{name}", self.label()))
            .collect()
    }
}

/// Which engines this run measures.
///
/// THE SHIPPED SETTINGS BY DEFAULT - see [`DEFAULT_ENGINES`]. `nolimit` answers a different
/// question, where this method stops when nothing stops it, and it is expensive out of all
/// proportion to how often that is wanted: a two-minute wall a row may actually reach,
/// against the in-game column's one second. `DEGCT_ENGINES=all` measures both, and naming
/// either works.
///
/// WHY A SELECTION AT ALL, rather than always measuring everything: a question is usually
/// about one column, and spending hours on the other to get it is how a measurement stops
/// being run.
///
/// A narrowed run is a NARROWER ROW, not a wide one with holes in it: the header follows
/// the selection, so nothing has to be told apart from a result later.
fn engines() -> Vec<Engine> {
    let named = lookahead_engine::core::env::var("ENGINES").unwrap_or_default();
    let wanted: Vec<&str> = named
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();

    // SET BUT EMPTY MEANS THE DEFAULT, the same as unset. A driver script that passes the
    // selection through has nothing to pass when there is no selection, and this is what it
    // looked like when that was an error instead: a row printed with no engine columns at
    // all, past a header with none either, recorded as a measurement. Empty must keep
    // meaning something sensible; since de-8xcd the sensible thing is the default rather
    // than everything.
    if wanted.is_empty() {
        return DEFAULT_ENGINES.to_vec();
    }

    // A NAME FOR THE WHOLE GRID, so a caller that wants the comparison does not have to
    // spell out three engine names - which is the kind of written-out list that went stale
    // here before, and is now one place instead of every call site.
    if wanted == [ALL] {
        return ALL_ENGINES.to_vec();
    }

    // A misspelling would otherwise measure nothing and say nothing about why.
    //
    // THE ALTERNATIVES ARE ASKED OF THE ENGINES rather than written out here. A hand-written
    // copy of the names goes stale, and a refusal that offers names nothing answers to
    // misdirects worse than no refusal at all. `all` is offered alongside them, since it is
    // a name a caller can pass and a reader looking for the grid should not have to spell
    // the columns out.
    for name in &wanted {
        assert!(
            ALL_ENGINES.iter().any(|engine| engine.label() == *name),
            "no engine called {name:?}: the names are {}, or {ALL} for every one of them",
            ALL_ENGINES.map(Engine::label).join(", "),
        );
    }

    ALL_ENGINES
        .into_iter()
        .filter(|engine| wanted.contains(&engine.label()))
        .collect()
}

/// One canonical start per DISTINCT group, heaviest first: (start, conversations, entries).
///
/// ## Which start, and why it is not the smallest member
///
/// `discover_group` is a forward closure, so two starts in the same group can reach
/// different sets and only some of them reach all of it. The start kept here is the
/// SMALLEST ONE WHOSE OWN CLOSURE IS THE WHOLE SET, which is what makes the line
/// reproducible: handing it back as `DEGCT_CONVERSATION=` rebuilds exactly the group it came
/// from. Taking the smallest member instead would sometimes name a start that reaches a
/// smaller group, and the row would quietly be about something else.
///
/// The set is the key rather than the start, because `build_group_graph` walks the group
/// in ascending conversation order and so produces an identical graph from any start whose
/// closure is that set - measured in `measurements/group_census.rs`, which counts 1,422
/// distinct groups over 1,501 conversations.
///
/// ## Heaviest first, deliberately - but this is no longer the order a run uses
///
/// A whole-game run is long and will be interrupted. Fifty spanning groups carry
/// fifty-five per cent of the entries and every group the measurements have ever been
/// about; the other 1,372 average forty-three entries and cost microseconds apiece. So
/// ordering by entries puts all of the information and all of the risk at the front, and
/// leaves a cheap tail that any resumed run can finish quickly.
///
/// SINCE de-xp9s THE CALLER RE-SORTS BY WHAT EACH GROUP CAN REACH, which is a better proxy
/// for the same intent: `entries` counts a group's conversations whether the search can walk
/// to them or not, and group 7 is the case that shows the difference - 4,035 entries, 32
/// reachable. What this ordering still decides is the order the groups are WALKED in while
/// the caller counts them, and therefore the order of the reasons in `groups.log`. Keep it
/// total for that reason alone.
fn group_starts(index: &lookahead_engine::index::Index) -> Vec<(i32, usize, usize)> {
    let mut conversations: Vec<i32> = index.keys().copied().collect();
    conversations.sort_unstable();

    let mut canonical: HashMap<BTreeSet<i32>, i32> = HashMap::new();
    for &conversation in &conversations {
        let group: BTreeSet<i32> = discover_group(index, conversation).into_iter().collect();
        // Ascending, so the first start to produce a set is the smallest that reaches it.
        canonical.entry(group).or_insert(conversation);
    }

    let mut groups: Vec<(i32, usize, usize)> = canonical
        .into_iter()
        .map(|(group, start)| {
            let entries = group.iter().map(|id| index[id].entries.len()).sum();
            (start, group.len(), entries)
        })
        .collect();

    // Entries first, then the start, so the order is total and a run's row list is the
    // same list every time it is asked for.
    groups.sort_unstable_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    groups
}

/// Why a group has nothing to measure. Three reasons, and they are not the same thing.
///
/// A group that yields no rows used to be discovered one row at a time, by a process that
/// built the graph, found nothing, said so on stderr and exited. Over the whole game that
/// is 901 groups of 1,422 and 9,010 processes that measure nothing - see
/// [`NoRows::message`] for what the line says and `tools/measure-matrix.sh` for what now
/// asks the question once per group instead of once per row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoRows {
    /// `build_group_graph` refused the start.
    NoGroup,
    /// The group built, but the start is not in it.
    NoEntryZero,
    /// The start is there and reaches no entry a profile could be built from.
    NothingReachable,
}

impl NoRows {
    /// The line the log has always carried, and which the script matches on `; no rows$`.
    ///
    /// SAID OUT LOUD, all three of them. A conversation that yields no rows used to
    /// `continue` in silence, and silence is indistinguishable from a dead process to the
    /// script, which decides a row crashed by the absence of a row line. The row is still
    /// absent - there is genuinely nothing to measure - but the log says which of the three
    /// reasons it was, rather than leaving CRASHED to be read as a finding.
    fn message(self, conversation: i32) -> String {
        let why = match self {
            NoRows::NoGroup => "no group builds from it",
            NoRows::NoEntryZero => "the group has no entry 0",
            NoRows::NothingReachable => "nothing is reachable from its start",
        };
        format!("conversation {conversation}: {why}; no rows")
    }
}

/// What a start offers to measure: the group's graph, the start itself, and the entries a
/// profile can be built from - or why there is nothing.
///
/// ONE PLACE, ASKED TWICE, and that is the whole point of lifting it out of `main`. The row
/// loop asks before it measures, and `GROUPS_ONLY` asks so that a whole-game run can skip
/// the empty groups without starting a process to be told. Two copies of this would be two
/// chances for the enumeration to promise rows the row loop then declines to produce, which
/// is a disagreement nothing would report: the script would simply record NO-ROWS for a
/// group it never ran, or run 901 groups it did not need to.
fn measurable(
    index: &lookahead_engine::index::Index,
    conversation: i32,
) -> Result<(LookAheadGraph, DialogueNodeId, Vec<DialogueNodeId>), NoRows> {
    let Ok((graph, _)) = build_group_graph(index, conversation) else {
        return Err(NoRows::NoGroup);
    };
    let start = DialogueNodeId::new(conversation, 0);
    if graph.get(start).is_none() {
        return Err(NoRows::NoEntryZero);
    }
    let reachable = candidates(&graph, start);
    if reachable.is_empty() {
        return Err(NoRows::NothingReachable);
    }
    Ok((graph, start, reachable))
}

fn conversations(default: &[i32]) -> Vec<i32> {
    match lookahead_engine::core::env::var("CONVERSATION") {
        Ok(named) => named
            .split(',')
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| {
                id.parse().unwrap_or_else(|_| {
                    refuse(&format!(
                        "{}={id:?} is not a conversation id",
                        lookahead_engine::core::env::qualified("CONVERSATION")
                    ))
                })
            })
            .collect(),
        Err(_) => default.to_vec(),
    }
}

/// Stops the run, saying why, rather than measuring something nobody asked for.
///
/// ## Why this is worth an exit rather than a shrug
///
/// Both selections used to DROP what they did not recognise - an unparseable id fell out of
/// a `filter_map`, an unknown label out of a `filter` - so a typo produced an empty
/// selection, no rows, and a silent exit 0. `tools/measure-matrix.sh` decides a row crashed
/// by the absence of a row line, so every row of a mistyped run came back CRASHED.
///
/// That is a FOURTH outcome landing in the most alarming of the three the matrix exists to
/// keep apart - `no-room` and `CRASHED` are results, `NOT-MEASURED` is a run to repeat - and
/// a full run is hours, so it would not be noticed until a TSV full of CRASHED was being
/// read as a discovery about the search. See de-uxyw.
///
/// Exit 2 rather than a panic: a panic would print a backtrace into the row log and still
/// leave the script guessing, where a named refusal on stderr is the whole message.
fn refuse(why: &str) -> ! {
    eprintln!("{why}");
    eprintln!(
        "profiles: {}",
        known_profiles()
            .map(|p| p.label())
            .collect::<Vec<_>>()
            .join(", "),
    );
    eprintln!("conversations: any group id the index carries, comma separated");
    std::process::exit(2);
}

/// Which profiles this process should measure, by label.
///
/// ONE ROW PER PROCESS is the intended way to run this, driven by `tools/measure-matrix.sh`.
/// A row can take the whole process down - measured: conversation 28 with its five deepest
/// entries unseen overflows the stack inside a recursive diagram operation - and with every
/// row in one process the first crash destroys every row after it. The 28 run reported two
/// rows out of eleven and lost the rest.
///
/// Answering the whole list when nothing is named keeps the test runnable on its own; the
/// script is what makes the results survivable.
fn profiles() -> Vec<Profile> {
    match lookahead_engine::core::env::var("PROFILE") {
        Ok(named) => named
            .split(',')
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(|label| {
                known_profiles()
                    .find(|profile| profile.label() == label)
                    .unwrap_or_else(|| {
                        refuse(&format!(
                            "{}={label:?} is not a profile",
                            lookahead_engine::core::env::qualified("PROFILE")
                        ))
                    })
            })
            .collect(),
        Err(_) => PROFILES.to_vec(),
    }
}

/// The entries a profile leaves unseen.
/// A census read back: which entries a group's own census proved unreachable, deepest first.
///
/// EMPTY IS A FINDING, not a missing row - a group whose census found nothing unreachable is
/// exactly the group whose unreachable profiles are skipped. A group ABSENT from the census
/// is a different thing entirely and is refused rather than guessed at; see [`Census::of`].
struct Census(HashMap<i32, Vec<DialogueNodeId>>);

impl Census {
    /// Reads the census named by `CENSUS_FILE`, or refuses if there is none to read.
    ///
    /// REFUSES RATHER THAN CLASSIFYING ON THE SPOT, which is the whole design of these
    /// profiles: a row built from a classification of its own is a row whose profile nobody
    /// can look up afterwards, and it would carry the classification's cost inside its own
    /// clock. A run that names an unreachable profile without a census has asked for
    /// something that cannot be produced honestly, and saying so on stderr and stopping is
    /// the answer - the same shape [`refuse`] already takes for an unknown profile.
    fn of(profiles: &[Profile]) -> Option<Self> {
        if !profiles
            .iter()
            .any(|p| matches!(p, Profile::DeepestUnreachable(_)))
        {
            return None;
        }

        let Ok(path) = lookahead_engine::core::env::var("CENSUS_FILE") else {
            refuse(
                "an unreachable profile needs CENSUS_FILE, naming the census.tsv that says \
                 which entries are unreachable. Take one with tools/measure-census.sh.",
            )
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            refuse(&format!(
                "{}={path:?} could not be read",
                lookahead_engine::core::env::qualified("CENSUS_FILE")
            ))
        };

        let mut rows = HashMap::new();
        for line in text.lines().skip(1) {
            let cells: Vec<&str> = line.split('\t').collect();
            // A CRASHED census row has the group in column one and nothing usable after it,
            // so it parses to no entries - and a group whose census crashed is a group whose
            // unreachable set is unknown. It is deliberately NOT recorded as "none
            // unreachable", which would silently turn an unmeasured group into a skipped one.
            let Some(conversation) = cells.first().and_then(|c| c.parse::<i32>().ok()) else {
                continue;
            };
            if cells.get(4).copied() == Some("all") || cells.get(4).copied() == Some("at-least") {
                rows.insert(conversation, entries(cells.get(6).copied().unwrap_or("")));
            }
        }

        eprintln!("census: {} group(s) read from {path}", rows.len());
        Some(Self(rows))
    }

    /// The unreachable entries recorded for a group, deepest first.
    fn of_group(&self, conversation: i32) -> &[DialogueNodeId] {
        match self.0.get(&conversation) {
            Some(found) => found,
            // NOT AN EMPTY ANSWER. The census either covers this group or it does not, and
            // "not in the file" cannot be read as "nothing unreachable here" - that would
            // turn a census that crashed, or one taken over a different set of groups, into
            // a run full of confidently skipped rows.
            None => refuse(&format!(
                "the census has no row for group {conversation}, so what is unreachable in \
                 it is unknown. Census that group before asking for an unreachable profile."
            )),
        }
    }
}

/// Ids as `conv:entry,conv:entry,...`, which is how a census names its lists.
///
/// The inverse of [`entries`], and next to it so the two spellings cannot drift apart.
fn listed(ids: &[DialogueNodeId]) -> String {
    ids.iter()
        .map(|id| format!("{}:{}", id.conversation_id, id.entry_id))
        .collect::<Vec<String>>()
        .join(",")
}

/// `conv:entry,conv:entry,...` as ids, which is how a census names its list.
fn entries(list: &str) -> Vec<DialogueNodeId> {
    list.split(',')
        .filter(|cell| !cell.is_empty())
        .filter_map(|cell| {
            let (conversation, entry) = cell.split_once(':')?;
            Some(DialogueNodeId::new(
                conversation.parse().ok()?,
                entry.parse().ok()?,
            ))
        })
        .collect()
}

/// Why a profile declined to produce a row for a group.
///
/// A SKIPPED ROW IS STILL A ROW. A silently absent one cannot be told from one that was never
/// run, which is the mistake the whole-game run's NO-ROWS outcome exists to avoid, so each of
/// these is written into the TSV with the rule that produced it.
#[derive(Debug, Clone, Copy)]
enum Skipped {
    /// The group has no unreachable entries, so there is no such question to ask in it.
    NoneUnreachable,
    /// The group has exactly one, so a five-entry profile would be four easy questions.
    ///
    /// THE RULE WORTH UNDERSTANDING RATHER THAN OBEYING. The set is topped up from the
    /// deepest remaining entries, so `deepest-unreach-5` over a group with one unreachable
    /// entry is one hard question and four instant ones wearing the name of a hard profile.
    /// The number it reported would be mostly the easy case.
    OneUnreachable,
}

impl Skipped {
    fn rule(self) -> &'static str {
        match self {
            Skipped::NoneUnreachable => "SKIPPED-none-unreachable",
            Skipped::OneUnreachable => "SKIPPED-one-unreachable",
        }
    }
}

fn unseen_for(profile: Profile, candidates: &[DialogueNodeId]) -> HashSet<DialogueNodeId> {
    match profile {
        Profile::DeepestUnseen(n) => candidates.iter().take(n).copied().collect(),
        // Built by `unreachable_for`, which needs the census this does not have. Reaching
        // here would mean the row loop stopped asking it first.
        Profile::DeepestUnreachable(_) => unreachable!("an unreachable profile reads the census"),
        Profile::PercentSeen(percent) => percent_unseen(candidates, percent),
    }
}

/// The entries an unreachable profile asks about: the deepest `n` the census proved
/// unreachable, or all of them where it found fewer.
///
/// SO THE SET CAN BE SMALLER THAN THE NAME SAYS, and `deepest-unreach-5` over a group with
/// three unreachable entries asks about three. The name is the question; `unseen` is what
/// could be found to ask it with, and a reader comparing rows within a profile has to read
/// that column rather than assume five.
///
/// The skip rules are applied here rather than by the caller because they are part of what
/// the profile MEANS: zero unreachable entries is not a hard question made easy, it is no
/// question at all, and one is one hard question that would sit in a set named for five.
fn unreachable_for(
    n: usize,
    census: &Census,
    conversation: i32,
) -> Result<HashSet<DialogueNodeId>, Skipped> {
    let unreachable = census.of_group(conversation);
    match unreachable.len() {
        0 => return Err(Skipped::NoneUnreachable),
        1 if n > 1 => return Err(Skipped::OneUnreachable),
        _ => {}
    }

    // ONLY WHAT IS ACTUALLY UNREACHABLE, and fewer than N where that is all there is.
    //
    // de-x8ms.3, reversing what this did first. It used to top the set up to N from the
    // deepest remaining CANDIDATES - which are reachable - on the argument that a set of
    // three when five were asked for is a different measurement, and comparing a 3-entry
    // row against a 5-entry one conflates set size with difficulty.
    //
    // The user weighed that and decided the other way, and the argument is better: the
    // profile then MEANS what its name says, every entry in it one no path can reach.
    // Before, that was only true of groups with five or more.
    //
    // AND IT BUYS AN INVARIANT. With nothing reachable in the set there is nothing to find,
    // so a `found` verdict on one of these rows is impossible - and therefore a
    // contradiction between the census and the search rather than a result. See where the
    // row is written for what is done about that.
    //
    // The set size is now the honest count and travels in the `unseen` column, which every
    // profile already has; the separate `real` column this used to need is gone.
    Ok(unreachable.iter().copied().take(n).collect())
}

/// What one engine did with one profile: one cell per column it names, in that order.
///
/// STRINGS RATHER THAN NUMBERS, so that a column a row could not fill says `?` rather than
/// a zero the reader cannot tell from a measurement.
///
/// `ms` IS THE WHOLE OF WHAT THE ENGINE TOOK and `setup` is how much of it was not
/// searching, so the search is the difference. Narrowing `ms` to the search alone would be
/// the tidier definition and would silently change what every recorded folder's `ms` column
/// means against every new one, so the split adds a column and redefines none. It answers
/// two live confusions at once (de-x8ms.1): a floor row reads about three hundred
/// milliseconds per engine while doing no searching at all, and 16/deepest-1 recorded
/// 647,709 ms against a 600,000 ms cap, which looked like an overrun and was a cap plus its
/// setup.
struct Cells(Vec<String>);

impl Cells {
    /// The cells of an engine that did not run, one `?` per column it would have filled.
    fn absent(verdict: &str, engine: Engine) -> Self {
        let mut cells = vec![verdict.to_string()];
        cells.extend(engine.columns().iter().skip(1).map(|_| "?".to_string()));
        Self(cells)
    }
}

/// The search the game runs.
///
/// ONE CANDIDATE AT A TIME, best novelty class first, stopping at the first candidate proved
/// reachable - which is `novelty_search` driven by `answer::best_novelty`, and is what
/// `bridge::answer_within` calls. The two columns differ in what they are allowed to spend
/// and in nothing else.
///
/// The budget is the search's own - the one the bridge hands it, scaled by nothing here
/// - because what this column is for is what a player waits for. A row measured under a
/// measurement-sized budget would answer a question nobody asks.
/// A request carrying the PLUGIN'S OWN DEFAULTS, so the in-game column asks the product for
/// its budgets instead of restating them.
///
/// The numbers are the plugin's: `LookAheadTimeBudgetMs = 1000` and
/// `LookAheadMemoryBudgetMb = 256`, both from
/// src/GlobalConversationTracker.Plugin/Plugin.cs, and `state_budget = 0` because no such
/// setting is on the wire any more. THOSE TWO NUMBERS ARE THE ONLY THING RESTATED, and they
/// have to be - they live in C# and nothing here can read them. Everything downstream of them
/// - how a dial becomes rations, how megabytes become a node capacity - is asked of
/// `search_budget` and `diagram_budget` rather than copied, because the matrix keeping its own
/// copy of the game's configuration is what de-qh27, de-cluo and de-xegj all are.
///
/// IF THE PLUGIN'S DEFAULTS CHANGE, this goes stale silently. There is no shared constant to
/// bind them to; a test that fails when they diverge would need to read the C#.
fn in_game_request() -> lookahead_engine::bridge::LookAheadRequest {
    lookahead_engine::bridge::LookAheadRequest {
        time_budget_ms: 1000,
        memory_budget_mb: 256,
        state_budget: 0,
        ..Default::default()
    }
}

/// What each column is allowed.
///
/// THE BACKWARD-ONLY ARMS ARE THEIR PAIR WITH ONE FIELD CHANGED, and are written that way
/// rather than spelled out, so that a change to either budget reaches its control
/// automatically. A second copy of the rations is exactly how the in-game column drifted
/// from the game (de-xegj), and a control that drifts from what it controls for measures
/// nothing at all.
fn search_budget_for(engine: Engine) -> answer::Budget {
    match engine {
        Engine::InGame => in_game_request().search_budget(),
        // NO LIMIT, WALLED. The wall is the contract and the rations under it are estimates
        // aimed at landing inside it; an implementer may tune them, and 120 seconds is what
        // the column promises.
        Engine::NoLimit => answer::Budget {
            overall: std::time::Duration::from_secs(120),
            backwards: std::time::Duration::from_secs(100),
            each: std::time::Duration::from_secs(10),
        },
    }
}

/// The switching method, over EVERY profile of one group, on one manager.
///
/// ## Why the loop is inside here rather than outside
///
/// de-x8ms.1. Nothing above the profile loop depends on the profile - the layout, the manager,
/// the compiled guards and the seed are functions of the graph, the world and the symbols -
/// and building them is most of what a row costs. Measured on the whole-game run of
/// 2026-09-09: setup was 76.9 per cent of the `ingame` column's time, and building it once
/// per group instead of once per row removed 64.4 per cent of all the engine time in the run.
///
/// IT COULD NOT SIMPLY BE HOISTED OUT, which is what made this a task rather than an edit.
/// Each engine runs on `isolated::on_its_own_thread` because what overflows a stack is
/// BUILDING A SECOND MANAGER ON A THREAD THAT HAS ALREADY BUILT ONE (de-fpax, de-w0rw), so
/// the setup cannot cross the thread boundary to be shared, and rebuilding it per profile on
/// one long-lived thread is the very thing that overflows. Inverting the loops keeps the
/// invariant - ONE MANAGER PER THREAD - and pays for it once per engine per group.
///
/// ## What that changes about the measurement, and how a row says so
///
/// The profiles after the first answer on a WARM manager: the compiled guards and the apply
/// cache the first profile paid for are still there. That is a different measurement from one
/// profile per manager, and quite possibly a better model of the mod - a game session is one
/// process for hours - but a folder must not hold both without saying which.
///
/// IT SAYS SO IN THE `setup` COLUMN, which is self-describing rather than a flag somewhere
/// else: the first profile of a group reports the setup it paid, and every profile after it
/// reports ZERO, because that is what each of them actually spent. Summing the column over a
/// folder gives the true total either way.
fn search_all(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn lookahead_engine::world::world::ILookAheadWorld,
    symbols: &StateSymbols,
    work: &[HashSet<DialogueNodeId>],
    engine: Engine,
) -> Vec<Cells> {
    let began = std::time::Instant::now();

    // THE MANAGER DIFFERS BETWEEN THE TWO, and by far more than the clocks do. A player gets
    // 256 MB; the run's own allowance is 6144 serial or a worker's share in parallel - a
    // twenty-four-fold gap, against a two-fold gap in time. It is the divergence most likely
    // to hide `no-room` rows a player would actually hit, which is the in-game column's most
    // useful output rather than a regression.
    let manager = match engine {
        Engine::InGame => in_game_request().diagram_budget(),
        Engine::NoLimit => budget(),
    };

    let layout = DataLayout::for_group(graph, world, COUNTER_CAP);
    let Some(vars) = DataVars::try_new(&layout, symbols, manager) else {
        return work
            .iter()
            .map(|_| Cells::absent(NOT_MEASURED, engine))
            .collect();
    };
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, world, &vars).expect("room for a seed");
    // See the same reading in `symbolic_forward_all`: everything above is profile-independent,
    // which is the whole reason this function takes a list.
    let shared_setup = began.elapsed().as_millis();

    work.iter()
        .enumerate()
        .map(|(index, unseen)| {
            search_one(
                graph,
                start,
                world,
                &vars,
                &mut compiler,
                &seed,
                unseen,
                engine,
                // THE FIRST PROFILE CARRIES THE SETUP AND THE REST CARRY NONE, because that
                // is what each of them spent. See the note above.
                if index == 0 { shared_setup } else { 0 },
            )
        })
        .collect()
}

/// One profile of one group, against a manager and a compiler the caller built.
#[allow(clippy::too_many_arguments)]
fn search_one(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn lookahead_engine::world::world::ILookAheadWorld,
    vars: &DataVars,
    compiler: &mut GuardCompiler,
    seed: &oxidd::bdd::BDDFunction,
    unseen: &HashSet<DialogueNodeId>,
    engine: Engine,
    setup: u128,
) -> Cells {
    // THIS ROW'S OWN CLOCK, which starts after the shared setup and so measures the search.
    // The first row of a group adds the setup back on, below, so that summing the column over
    // a folder still gives what the run cost.
    let began = std::time::Instant::now();

    let novelty = |id: DialogueNodeId| {
        if unseen.contains(&id) {
            Novelty::UnseenAnyGame
        } else {
            Novelty::SeenThisGame
        }
    };

    // THE GATE THE GAME APPLIES, and this column exists to be the game. de-qh27.
    //
    // src/bridge.rs `scored` computes a baseline and refuses to search when nothing
    // link-reachable beats it, returning a COMPLETE answer of "none" having run nothing.
    // This used to skip that and search unconditionally, which recorded backward
    // work the game never pays for in the column that claims to be the game's method.
    //
    // THE SHARED PREDICATE, NOT A SECOND COPY - a second copy is how this drifted the first
    // time. For StartBranch::Either, which is what this row uses, `scored` reduces its
    // `from` to the one start and its baseline to that start's own novelty
    // (src/bridge.rs:1231-1239), so the gate here is that call with those arguments.
    let Some(hunting) =
        lookahead_engine::bridge::class_worth_hunting(graph, &[start], novelty(start), &novelty)
    else {
        // ITS OWN VERDICT, because `not-there` with `asked=0` cannot be told from a backward
        // driver that had no candidates - and the two mean opposite things. The run's other
        // outcomes are kept apart for the same reason: NO-ROWS, no-room, CRASHED and
        // NOT-MEASURED all say something different about why a row has no number in it.
        //
        // COMPLETE, NOT A GIVE-UP. The game's answer here is settled: nothing link-reachable
        // outranks where the option already lands, so there is nothing to look for. A row
        // that read as a failure would invite somebody to fix it.
        return Cells(vec![
            NOT_WORTH_HUNTING.to_string(),
            (setup + began.elapsed().as_millis()).to_string(),
            // A GATED ROW IS ALL SETUP, and saying so is the point of the column: what it
            // spent went on building an apparatus the gate then declined to use.
            setup.to_string(),
            // Nothing was built, nothing answered, nothing asked. `by` says which half
            // answered and the honest value is neither.
            "0".to_string(),
            "Gated".to_string(),
            "0".to_string(),
        ]);
    };

    let answer = answer::best_novelty(
        graph,
        start,
        StartBranch::Either,
        seed,
        compiler,
        world,
        COUNTER_CAP as u32,
        &novelty,
        hunting,
        &search_budget_for(engine),
        // ONE START PER ROW HERE, so this builds a shape per search - which is what every
        // row of this matrix has always paid, and holding that constant is what lets a row
        // measured today be read against one measured before de-bnjy.10.
        &GroupShape::of(graph),
        None,
    );

    let verdict = match answer.by {
        _ if answer.best > Novelty::SeenThisGame => "found",
        answer::Answered::Partly => "gave-up",
        _ => "not-there",
    };

    // WHAT THE MANAGER HOLDS, which is memory in use and therefore what the budget and the
    // parallel split both watch.
    //
    // READ AT THE END OF THE ROW rather than tracked as a high-water mark. For one search
    // over one manager those are near enough the same thing, because nodes are not
    // reclaimed eagerly; it would stop being true if a row ever ran two searches with
    // managers of their own.
    Cells(vec![
        verdict.to_string(),
        (setup + began.elapsed().as_millis()).to_string(),
        setup.to_string(),
        vars.node_count().to_string(),
        format!("{:?}", answer.by),
        answer.targets_asked.to_string(),
    ])
}

/// The columns every row starts with, whichever engines ran.
///
/// THERE WAS A `real` COLUMN HERE BRIEFLY, added by de-thlz.2 and removed by de-x8ms.3. It
/// held how many of a topped-up unreachable set were genuinely unreachable - and once the
/// top-up went, every entry in such a set is genuinely unreachable, so it always equalled
/// `unseen`. A column that always equals its neighbour is worse than no column, because a
/// reader assumes it means something.
///
/// `unseen` NOW CARRIES IT. For a `deepest-unreach-N` row it is how many the census could
/// find, which may be fewer than N; anything comparing such rows has to read it rather than
/// assume the number in the name.
const ROW_COLUMNS: [&str; 4] = ["conv", "entries", "profile", "unseen"];

/// The columns of a census row, written down here and nowhere else.
const CENSUS_COLUMNS: [&str; 8] = [
    "conv",
    "candidates",
    "unreachable",
    "undecided",
    "exact",
    "ms",
    "deepest_unreachable",
    "undecided_entries",
];

/// How many unreachable entries a census stops after.
///
/// TEN, where the profiles need five, because the LIST is the artefact worth having rather
/// than the count: it is the answer to "where are the hard questions in this game", and
/// re-deriving it later costs the same pass again. de-thlz.2.
const CENSUS_WANTED: usize = 10;

/// How many a census stops after, which `CENSUS_WANTED` sets and `CENSUS_ALL` removes.
///
/// TEN IS A PROFILE'S APPETITE, NOT A CENSUS'S. It was chosen because the deepest-unreach
/// profiles need five and ten is twice that - and for naming the hard questions it is the
/// right number. It is the wrong number for an ARTEFACT: de-x8ms.5 wants a status per entry,
/// and a census that stops at ten leaves everything past the tenth unexamined. Measured on
/// the whole game: 250 of 521 groups hit the cap, and only 10.9 per cent of link-reachable
/// entries came out with a world-conditioned status.
///
/// `DEGCT_CENSUS_ALL=1` scans every candidate instead. That is a different and much longer run -
/// classification is a bounded backward pass per candidate - so it is asked for rather than
/// assumed.
fn census_wanted() -> usize {
    if lookahead_engine::core::env::is_set("CENSUS_ALL") {
        usize::MAX
    } else {
        CENSUS_WANTED
    }
}

/// Which of a group's entries no path can reach, deepest first, and what that cost.
///
/// ## Why this is a mode of the matrix and not a measurement of its own
///
/// Because everything it needs is here. `candidates` defines the deepest-first order the
/// profiles are built in, `measurable` decides what a group even has to offer, and the
/// conversation list is the same one the rows use - so a census run and a row run cannot
/// disagree about which groups exist or which entries are in play. What it borrows from
/// elsewhere is the one thing that is genuinely elsewhere: the classification itself.
///
/// ## What the columns mean, because two of them are easy to misread
///
/// `unreachable` is capped at [`CENSUS_WANTED`], so it is a COUNT only when `exact` says
/// `all`; `at-least` means the scan stopped with ten in hand and the group has that many or
/// more. `exact` is what the skip rules read: nothing to skip on a group that has none, and
/// a `deepest-unreach-5` on a group with one real entry would be one hard question wearing
/// the name of a hard profile.
///
/// `undecided` is not a rounding error to be ignored. A candidate whose pass does not settle
/// inside the cap has proved nothing either way, so a deeper entry may be missing from the
/// list for no better reason than that it was expensive to ask about. A group with a large
/// `undecided` is telling you its census is a lower bound.
///
/// `undecided_entries` NAMES THEM, and that is what makes the rest of the row usable. A
/// census names only what it proved unreachable, so which entries are REACHABLE is recovered
/// by subtracting that list from the candidates - and an undecided candidate is one the
/// subtraction would hand a status it never earned. Knowing only the count leaves nowhere to
/// put the doubt but the whole group. de-x8ms.5.
///
/// THE CAP IS SAID OUT LOUD ON STDERR, because two censuses taken under different caps are
/// not the same artefact and nothing in the TSV records it.
fn census(index: &lookahead_engine::index::Index, world: &dyn ILookAheadWorld) {
    if !lookahead_engine::core::env::is_set("NO_HEADER") {
        println!("{}", CENSUS_COLUMNS.join("\t"));
    }
    let wanted = census_wanted();
    eprintln!(
        "census: {} per group, {} seconds per candidate",
        if wanted == usize::MAX {
            "every candidate".to_string()
        } else {
            format!("up to {wanted}")
        },
        symbolic_answers::CLASSIFY_CAP.as_secs(),
    );

    for conversation in conversations(&HEAVIEST) {
        // A GROUP WITH NOTHING TO OFFER STILL GETS A ROW. A silently absent row cannot be
        // told from one that was never run, which is the mistake the whole-game run's
        // NO-ROWS outcome exists to avoid; the reason goes to stderr, as it does for
        // `GROUPS_ONLY`.
        let (graph, start, reachable) = match measurable(index, conversation) {
            Ok(measurable) => measurable,
            Err(why) => {
                eprintln!("{}", why.message(conversation));
                println!("{conversation}\t0\t0\t0\tall\t0\t\t");
                continue;
            }
        };

        let began = std::time::Instant::now();
        let (unreachable, undecided) =
            symbolic_answers::classify(&graph, start, world, &reachable, wanted);
        let millis = began.elapsed().as_millis();

        // `all` means the scan ran out of candidates, so the count is the whole truth for
        // this group; `at-least` means it ran out of room.
        let exact = if unreachable.len() == wanted {
            "at-least"
        } else {
            "all"
        };

        println!(
            "{conversation}\t{}\t{}\t{}\t{exact}\t{millis}\t{}\t{}",
            reachable.len(),
            unreachable.len(),
            undecided.len(),
            listed(&unreachable),
            listed(&undecided),
        );
    }
}

fn main() {
    // THE TRIMMED INDEX, WHICH IS THE MOD'S - 15 MB against the full index's 50, and it
    // reads in about 135 ms against 429 to 678. de-12wr.2: the driver runs one process per
    // row, so the read is paid once per row, and it was 40 to 70 per cent of a floor row's
    // whole cost. `measurements/row_overhead.rs` is where those figures come from.
    //
    // THE ROWS ARE THE SAME ROWS, and that is proved rather than argued: `tests/
    // shipped_index.rs` requires every field the engine reads to survive the trim, requires
    // the group graph built from each index to be identical entry for entry and symbol for
    // symbol, and requires the WHOLE GAME's group list - `discover_group` over all 1,422
    // conversations, which is what decides the rows a run has - to be the same list. A row
    // is a function of the graph, the world, the budget, the cap and the profile, and only
    // the graph comes out of the index.
    let Some(path) = common::shipped_index() else {
        return;
    };
    let engines = engines();

    // Tab separated, so a run pipes straight into a file that something else can read -
    // see the note at the top about keeping the logs. The header names exactly the engines
    // that ran, and is the only place the column names are written down.
    let header: Vec<String> = ROW_COLUMNS
        .iter()
        .map(|name| name.to_string())
        .chain(engines.iter().flat_map(|engine| engine.headers()))
        .collect();

    // ASKED FOR ON ITS OWN by the driver, which needs the column names before it has a row
    // and should not keep a second copy of them to go stale.
    if lookahead_engine::core::env::is_set("HEADER_ONLY") {
        println!("{}", header.join("\t"));
        return;
    }

    // THE NUMBERS THE DRIVER HAS TO DO ARITHMETIC WITH, printed rather than copied.
    //
    // The driver divides the memory budget between its workers and converts a group's
    // `_nodes` column into megabytes to decide whether that group would fit a worker's
    // share. Both need constants that live in `src/symbolic/budget.rs`, and until this they
    // were transcribed into the shell - "the two numbers here that have to be kept in step
    // with the Rust by hand", as the driver put it. A hand-kept copy of a constant is wrong
    // silently, and this one is wrong in the direction that manufactures rows: too large a
    // share and workers race for memory the machine does not have.
    //
    // ONE PAIR PER LINE, `name<tab>value`, so a reader with no parser gets the same answer
    // as the driver. `memory_mb` follows ROW_MEMORY_MB where a run sets one, which is what
    // makes the printed value the budget this run will actually use rather than the default.
    if lookahead_engine::core::env::is_set("CONSTANTS_ONLY") {
        println!("memory_mb\t{}", memory() / (1024 * 1024));
        println!("bytes_per_node\t{}", DiagramBudget::BYTES_PER_NODE);
        println!("row_seconds\t{}", row_time().as_secs());
        return;
    }

    let index = read_index(&path).expect("the index reads");

    // ASKED FOR ON ITS OWN, like the header, and for the same reason: a whole-game run has
    // to know what the rows ARE before it measures any, and a list kept anywhere else can
    // omit a group and never say so.
    if lookahead_engine::core::env::is_set("GROUPS_ONLY") {
        let counted: Vec<(i32, usize, usize, usize)> = group_starts(&index)
            .into_iter()
            .map(|(start, conversations, entries)| {
                // THE FOURTH COLUMN IS WHY THIS COSTS MORE THAN IT USED TO, and it is worth
                // it. Building each group's graph and walking it from the start is what
                // tells the caller whether there is anything here at all, and 901 of the
                // game's 1,422 groups answer no. Learning that here costs one walk;
                // learning it the old way cost ten processes, each of which read the index
                // and built the same graph to reach the same conclusion.
                let reachable = match measurable(&index, start) {
                    Ok((_, _, reachable)) => reachable.len(),
                    Err(why) => {
                        // ON STDERR, so the counts stay a clean TSV and the reason is still
                        // recorded. `tools/measure-matrix.sh` keeps this stream as its
                        // groups.log; the wording is the one the row logs have always used.
                        eprintln!("{}", why.message(start));
                        0
                    }
                };
                (start, conversations, entries, reachable)
            })
            .collect();

        // ORDERED BY WHAT A RUN CAN SEE, not by how big the group is. de-xp9s.
        //
        // `entries` counts everything in the group's conversations, reachable or not, and
        // the two come apart badly. Group 7 holds 4,035 entries of which 32 are reachable -
        // conversation 7 is a stage-directions test dialogue whose single "Jump to:" link
        // drags in 3,957 entries of Klaasje that nothing in it can walk to - so it sorted
        // FOURTH of 1,422 and is one of the cheapest groups in the game. Group 275 sorted
        // ahead of 498 groups that have rows while having none at all.
        //
        // WHY IT MATTERS MORE THAN TIDINESS: tools/measure-matrix.sh runs groups one at a
        // time until the cost bottoms out, then goes parallel, and it decides that from the
        // cost of the groups as they finish. A trivial group sorted near the front is noise
        // in that signal - and noise this ordering was introducing, rather than anything
        // inherent in the game. Reading a curve that actually descends is worth the sort.
        //
        // TIES BY START, so the order stays TOTAL. That is not tidiness either: the resume
        // depends on a run's list being the same list every time it is asked for.
        //
        // THE EMPTY GROUPS ALL LAND AT THE END, since their count is zero, which pairs with
        // de-cziy - they are skipped rather than recorded, and now they are skipped from
        // one end of the list rather than scattered through it.
        let mut counted = counted;
        counted.sort_by(|a, b| b.3.cmp(&a.3).then(a.0.cmp(&b.0)));

        for (start, conversations, entries, reachable) in counted {
            println!("{start}\t{conversations}\t{entries}\t{reachable}");
        }
        return;
    }

    let world = common::measurement_save();

    // ASKED FOR ON ITS OWN, like the header and the group list, and it is the one mode that
    // answers a question about the GAME rather than about a search: which of a group's
    // entries no path can reach. de-thlz.2 wants that written down before any unreachable
    // profile is run, so that the profiles are built from a recorded set rather than from a
    // classification each row repeats - which would also put the classification's cost
    // inside the row's clock.
    if lookahead_engine::core::env::is_set("CENSUS") {
        census(&index, &world);
        return;
    }

    // ONCE, BEFORE ANY ROW, and only when a row is going to want it. Reading it per row would
    // be the same file parsed for every group of a whole-game sweep, and asking for it when
    // no unreachable profile was named would make a census a precondition of every run.
    let census = Census::of(&profiles());

    // SUPPRESSIBLE, because the driver script runs one row per process and wants one header
    // in the file rather than one per row.
    if !lookahead_engine::core::env::is_set("NO_HEADER") {
        println!("{}", header.join("\t"));
    }

    for conversation in conversations(&HEAVIEST) {
        // ASKED HERE TOO, and by the same function `GROUPS_ONLY` asks. A whole-game run
        // will have pruned this group already; a run that names its conversations has not,
        // so the check stays where it always was.
        let (graph, start, reachable) = match measurable(&index, conversation) {
            Ok(measurable) => measurable,
            Err(why) => {
                eprintln!("{}", why.message(conversation));
                continue;
            }
        };
        let symbols = graph.symbols().clone();

        // EVERY PROFILE'S QUARRY, RESOLVED BEFORE ANY ENGINE STARTS, which is what lets the
        // engines loop over the profiles instead of the other way round. de-x8ms.1: the setup
        // an engine builds is profile-independent and is most of what a row costs, so the
        // profiles have to be known before the manager is built rather than discovered one at
        // a time inside it.
        //
        // THE SKIPPED PROFILES ARE PRINTED HERE AND LEFT OUT OF THE LIST, exactly as they
        // were when this was one loop: a skipped row is still a row, with the rule that
        // skipped it in its verdict columns, and it costs no engine anything.
        let mut work: Vec<(Profile, HashSet<DialogueNodeId>)> = Vec::new();
        for profile in profiles() {
            // BUILT BEFORE ANYTHING IS SPENT, because for an unreachable profile this is
            // also where the run learns there is no question to ask in this group.
            let unseen = match profile {
                Profile::DeepestUnreachable(n) => {
                    let census = census.as_ref().expect("a census, or the profile refused");
                    match unreachable_for(n, census, conversation) {
                        Ok(unseen) => unseen,
                        Err(skipped) => {
                            eprintln!(
                                "SKIPPED: {conversation} {} - {}",
                                profile.label(),
                                match skipped {
                                    Skipped::NoneUnreachable =>
                                        "no entry in this group is unreachable, so there is \
                                         no such question to ask here",
                                    Skipped::OneUnreachable =>
                                        "exactly one entry is unreachable, so a five-entry \
                                         profile would be one hard question and four \
                                         instant ones",
                                },
                            );
                            // THE RULE GOES IN THE VERDICT COLUMNS, so the TSV says not only
                            // that the row was skipped but which rule skipped it - and so a
                            // resume counts the row as done rather than retrying it forever.
                            let absent: Vec<String> = engines
                                .iter()
                                .flat_map(|engine| Cells::absent(skipped.rule(), *engine).0)
                                .collect();
                            println!(
                                "{conversation}\t{}\t{}\t0\t{}",
                                graph.count(),
                                profile.label(),
                                absent.join("\t"),
                            );
                            continue;
                        }
                    }
                }
                _ => unseen_for(profile, &reachable),
            };

            // ASKED PER ROW, AND BEFORE ANYTHING IS SPENT. A machine that cannot supply the
            // budget makes the RUN invalid rather than the row a result - there is nothing
            // to record about a search that never happened - so the row says NOT-MEASURED
            // and wants running again when the memory is free. Per row rather than once at
            // the top because what else is running on the machine changes underneath a run
            // that takes hours.
            //
            // AND EACH SYMBOLIC ENGINE ASKS AGAIN, through DataVars::try_new, which is not
            // redundant. This decides the ROW - every engine is skipped, because one
            // verdict measured beside another that never ran is half a row, and two are
            // only comparable when they were rationed alike (de-e23q). That one is the
            // backstop for the race this check openly cannot close: another process can
            // take the memory between the answer here and the allocation there, and the
            // allocation aborts rather than failing.
            //
            // BOTH COLUMNS WANT ONE, so a machine that cannot supply the budget has
            // nothing to measure at all - which is what NOT-MEASURED says.
            if !budget().can_be_supplied() {
                eprintln!(
                    "NOT MEASURED: {conversation} {} - this machine could not supply the \
                     {} MB budget. The row is not a result; run it again with the memory \
                     free.",
                    profile.label(),
                    memory() / (1024 * 1024),
                );
                let absent: Vec<String> = engines
                    .iter()
                    .flat_map(|engine| Cells::absent(NOT_MEASURED, *engine).0)
                    .collect();
                println!(
                    "{conversation}\t{}\t{}\t{}\t{}",
                    graph.count(),
                    profile.label(),
                    unseen.len(),
                    absent.join("\t"),
                );
                continue;
            }

            work.push((profile, unseen));
        }

        if work.is_empty() {
            continue;
        }

        // EACH ENGINE ON A THREAD OF ITS OWN, which is the one thing this file was not
        // doing and six other measurements were - de-w0rw. The 6 GB run of 2026-09-06
        // lost five of sixty rows to `thread 'main' has overflowed its stack`, and the
        // thread it names is the process's own.
        //
        // WHAT MAKES IT THE FIX rather than a bigger stack, measured on de-fpax: what
        // accumulates is BUILDING A SECOND MANAGER ON A THREAD THAT HAS ALREADY BUILT
        // ONE, and the third overflows - which is why the deaths were always a third
        // row. A thread per engine means ONE MANAGER PER THREAD, which is the arrangement
        // that did not fail in twenty runs where the main thread failed in eight of twenty.
        // A half-gigabyte stack only moved the rate.
        //
        // AND NOW EVERY PROFILE OF THE GROUP RUNS ON THAT ONE THREAD AND THAT ONE MANAGER -
        // de-x8ms.1. The invariant is unchanged, because what overflows is building a second
        // manager rather than running a second search; what changes is that the setup is
        // built once per engine per group instead of once per engine per row, which the
        // whole-game run of 2026-09-09 measured at 64.4 per cent of all its engine time.
        //
        // THE HELPER'S CONSTRAINT IS STILL MET: each of these builds its layout, its
        // manager, its compiled guards and its seed itself, and hands back `Cells` -
        // strings - so nothing borrowed from a manager crosses the boundary.
        let quarries: Vec<HashSet<DialogueNodeId>> =
            work.iter().map(|(_, unseen)| unseen.clone()).collect();
        let per_engine: Vec<Vec<Cells>> = engines
            .iter()
            .map(|engine| {
                isolated::on_its_own_thread(|| {
                    search_all(&graph, start, &world, &symbols, &quarries, *engine)
                })
            })
            .collect();

        // TRANSPOSED BACK INTO ROWS, because the engines answered profile-major and a row is
        // one profile across every engine. The order within each engine's answers is the
        // order of `work`, which is the order the profiles are printed in - so the row a
        // reader sees is assembled from the same index in every column.
        for (index, (profile, unseen)) in work.iter().enumerate() {
            let measured: Vec<String> = per_engine
                .iter()
                .flat_map(|answers| answers[index].0.clone())
                .collect();

            // A `found` ON AN UNREACHABLE PROFILE IS IMPOSSIBLE, so it is reported rather
            // than recorded. de-x8ms.3. Every entry in the set was proved unreachable by the
            // census, so there is nothing in it to find: a search that found something means
            // the census and the search disagree, and one of the two is wrong.
            //
            // THE LIKELY CAUSE IS NAMED because there is an obvious one and nothing checks
            // it: CENSUS_FILE names a file, and no part of this verifies that the census was
            // taken under the same world the row is being measured under. A census from one
            // scenario read into a run of another would build every profile from the wrong
            // entries, and this is the only place that would show.
            if matches!(profile, Profile::DeepestUnreachable(_))
                && measured.iter().any(|cell| cell == "found")
            {
                eprintln!(
                    "CONTRADICTION: {conversation} {} came back 'found', which cannot \
                     happen - every entry in this set was proved unreachable by the \
                     census. Either the census is wrong or the search is. The likeliest \
                     cause is a CENSUS_FILE taken under a different world from the one \
                     this row was measured under; nothing checks that.",
                    profile.label(),
                );
            }

            println!(
                "{conversation}\t{}\t{}\t{}\t{}",
                graph.count(),
                profile.label(),
                unseen.len(),
                measured.join("\t"),
            );
        }
    }
}
