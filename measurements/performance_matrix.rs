// SPDX-License-Identifier: MIT
//! Three engines, six conversations, ten profiles: the whole grid. Plus two more profiles
//! that are deliberately outside it, and a census of the game that makes them possible.
//!
//! A DEFAULT RUN MEASURES ONE OF THE THREE. `fwdbwd` is the engine the game runs and the one
//! being tuned; the other two are evidence for that tuning and cost several times what it
//! does, so they are asked for rather than assumed - `ENGINES=all` for the grid, or any of
//! the names for one column. See [`engines`] for the measured argument. Everything below
//! that describes "three columns" is describing the grid you get when you ask for it.
//!
//! The measurements this repository already has each ask one question well. This asks the
//! same question of every combination, because the thing that is actually wanted - a rule
//! for choosing WHICH SEARCH per option (de-a1wb) - cannot be drawn from a handful of
//! points. It needs a surface.
//!
//! ## The three engines, and what each one actually is
//!
//! DIRECTION IS WHAT TELLS THEM APART. All three carry data states the same way - one
//! decision diagram per entry - so the name says which way each one runs and nothing else:
//!
//! | column | what runs | direction |
//! |---|---|---|
//! | `fwd` | `Reachability::explore_within` | forwards, from the start |
//! | `bwd` | `novelty_search::best_novelty` over `Backward` | backwards, from a target |
//! | `fwdbwd` | `portfolio::best_novelty` | a forward slice, then the backward driver |
//!
//! THESE ARE THE NAMES `ENGINES=` TAKES, and [`Engine::label`] is where they live. The
//! recorded results further down were measured under older names and each says so; the
//! table under "The names have moved twice" translates them.
//!
//! FORWARD walks `node.links` from the start exactly as the game's own search does; what
//! differs is that one decision diagram per entry holds every data state reached there at
//! once.
//!
//! BACKWARD is the only column that reverses the direction. It computes
//! pre-images from a target, and it is driven the way the portfolio would drive it: ONE
//! CANDIDATE AT A TIME, best novelty class first, stopping at the first candidate proved
//! reachable. Asking it about one hand-picked target instead would measure a question
//! nobody asks - the short circuit and the per-candidate cost ARE the approach.
//!
//! FORWARD-BACKWARD is WHAT THE GAME ACTUALLY RUNS, since the bridge was rewired - so it
//! is the column that says what a player waits for, and the other two are what it is made
//! of.
//!
//! ## The names have moved twice, and `fwd` does not mean today what it meant first
//!
//! Kept because the mislabelling outlived several conclusions drawn from it, and because a
//! folder of old rows can only be read by knowing which era named its columns:
//!
//! ```text
//!   oldest    fwd, bwd                   `fwd` was the state-at-a-time search and `bwd`
//!                                        was the SYMBOLIC FORWARD one. Neither was backward.
//!   middle    explicit, symfwd, symbwd   the same two named honestly, plus the first
//!                                        genuine backward column.
//!   current   fwd, bwd, fwdbwd           the state-at-a-time search is gone; what is left
//!                                        differs only in direction, so it is named for that.
//! ```
//!
//! So the oldest table measured EXPLICIT against SYMBOLIC, both going forwards, and the
//! genuine backward engine had never been run by it at all. Any forward-versus-backward
//! reading of a run from before that - including the framing of de-a1wb itself - rests on
//! a column name that did not describe what ran. de-zovl is the correction.
//!
//! `tools/matrix-remaining.awk` and `measurements/README.md` carry this same table,
//! because reading an old folder means translating it.
//!
//! ## What the backward column costs, and why that is the whole question
//!
//! When the answer is NO it pays one fixed point PER CANDIDATE, where the search pays one
//! walk for all of them. So the rule the portfolio needs should fall out of two numbers a
//! row already has - how many candidates were waiting, and what one pass cost:
//!
//! - FEW CANDIDATES favours backward, and it prunes itself: a variable enters a backward
//!   formula only if a guard on some path to the target reads it, and a write erases its
//!   slot, which is the cone-of-influence reduction a forward search needs a separate
//!   analysis to get.
//! - MANY UNREACHABLE CANDIDATES favours forward, because one search refuses all of them.
//!
//! ## The grid
//!
//! Six conversations - the five heaviest plus 362, the largest in the game - against ten
//! profiles describing how much of the group the player has read:
//!
//! - the deepest 1, 5 and 10 entries unseen, which are the deliberately hard cases;
//! - 95, 90, 75, 50, 25, 10 and 5 per cent seen, drawn at random, which are the shapes a
//!   real save actually has.
//!
//! ## And two profiles that are NOT in that grid: `deepest-unreach-1` and `-5`
//!
//! The ten above measure the direction the search stops early in. A deep entry that IS
//! reachable is proved the moment the backward pass meets the seed, and that is usually
//! instant - so "deepest-N" is the adversarial SHAPE without the adversarial COST. The
//! expensive question is an entry no path can reach, because a no has to be proved, which
//! means driving the fixed point to completion rather than stumbling on a yes.
//!
//! Which entries those are is not something a row can work out for itself. It takes a
//! bounded backward pass per candidate, so two runs could disagree about what the profile
//! even is, and the cost would land inside the clock the row exists to report. So it is
//! measured once, over every group, and written down - `CENSUS=1`, driven by
//! `tools/measure-census.sh` - and the rows READ it:
//!
//! ```text
//! CENSUS_OUT=measurements/logs/2026-09-08_census tools/measure-census.sh all
//! CENSUS_FILE=measurements/logs/2026-09-08_census/census.tsv \
//!   PROFILES=deepest-unreach-1,deepest-unreach-5 tools/measure-matrix.sh all
//! ```
//!
//! They are held out of the default grid on purpose: they cannot run without a census, and
//! adding a profile to the grid would make the whole-game run this repository already has
//! not comparable with the next one. de-thlz.2.
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
//! ## What the whole grid said at 256 MB, 2026-09-04
//!
//! SUPERSEDED TWICE OVER, AND KEPT AS HISTORY.
//!
//! THE COLUMNS ARE NOT WHAT THEY SAY. This run predates de-zovl, so its "forward" is the
//! explicit search and its "backward" is the SYMBOLIC FORWARD search - the two columns
//! renamed above. Nothing below is a measurement of a backward search, and the headings
//! are left as `explicit` and `symfwd` to stop it being read as one.
//!
//! AND THE ALLOWANCE WAS THE PLUGIN'S. Both engines were held to 256 MB and 60
//! seconds, and de-e33h is the finding that this measures the ration rather than the
//! algorithm: most heavy rows end in gave-up or no-room having been stopped by the
//! ceiling. The measurement now runs at the shared six-gigabyte budget above, with a cap
//! that is meant not to fire. Every number below is from the old setting and should be
//! read as what a 256 MB ration does, not as what these searches cost.
//!
//! The adversarial rows - everything read, or all but the one, five or ten structurally
//! deepest entries - behave identically within a conversation, so one line stands for all
//! four:
//!
//! ```text
//!   conv  entries   explicit                   symfwd
//!    368     4724   gave-up   400ms  170,870   gave-up   64s   7.5M nodes
//!    631     4514   gave-up   370ms  112,506   gave-up   65s   7.4M nodes
//!     14     3594   gave-up   390ms  133,089   NO ROOM   55s   8.4M nodes (the budget)
//!    362     1860   gave-up   660ms  377,017   gave-up   62s   2.1M nodes
//!     28     2186   gave-up   490ms  222,400   FOUND     50ms  180K nodes
//!   1030     1476   not-there   0ms      410   not-there  6ms  6.2K nodes
//! ```
//!
//! And every random profile, on every conversation, at every percentage:
//!
//! ```text
//!   explicit: found, 0ms, 2-100 states      symfwd: found, 5-15ms, 30-55K nodes
//! ```
//!
//! ### The explicit search wins almost everywhere, and it is not close
//!
//! On the profiles a real save actually has - any of the random percentages - the search
//! answers in under a millisecond and the symbolic forward search takes five to fifteen.
//! Not a disaster in either case, but there is no argument for the diagram there: it is
//! slower on every single row.
//!
//! On the adversarial profiles the search gives up in about four hundred milliseconds and
//! the symbolic one spends A MINUTE to give up as well. Five of the six conversations end
//! that way.
//!
//! ### Conversation 28 is the exception, and the whole case
//!
//! It ANSWERS where the search cannot: 50 milliseconds against 490 spent giving up. That is
//! what a symbolic search is for, and it is one conversation in six. Anything that decides
//! between engines per option (de-a1wb) has to find the 28-shaped groups cheaply, because
//! guessing wrong costs a minute.
//!
//! ### The verdicts changed once the two were really given the same room
//!
//! Worth recording because it was nearly missed. The manager PREALLOCATES its node capacity
//! and refuses to grow past it, and that capacity was a hand-picked 2^22 - about 134 MB,
//! half the search's allowance. On that setting 631 and 14 both read NO ROOM.
//!
//! Derive the capacity from the budget instead and they separate: 631 runs out of TIME at
//! 6.4 million nodes, and only 14 genuinely fails to fit, stopping at exactly the 8,388,608
//! nodes the budget allows. One of those is a search that is too slow and the other is a
//! representation that does not fit, and the earlier setting reported both as the second.
//!
//! ### The adversarial rows are all the same row
//!
//! Within a conversation, deepest-1, -5 and -10 cost the explicit search exactly the same
//! number of states - 170,870 on 368, three times over, and the same as the all-seen row did
//! before it was removed. The deepest entries by edge analysis are the ones the guards shut,
//! so seeding them changes nothing the search can find and it explores the whole space
//! regardless. That is the correct worst case and it is what these rows are for; it is not a
//! falloff curve, and measuring one needs a different seeding entirely - see the note above
//! on why nothing measures it today.
//!
//! ## All three engines on one row, at one budget, 2026-09-05
//!
//! Conversation 14, its one structurally deepest entry unseen, six gigabytes and a
//! ten-minute cap EACH. The comparison de-rfva asked for, and the first one on this file
//! that is like for like.
//!
//! MIDDLE-ERA NAMES BELOW: `symfwd` is today's `fwd` and `symbwd` today's `bwd`, and
//! `explicit` is the state-at-a-time search, which no column measures any more.
//!
//! ```text
//!   engine    verdict         ms       states / nodes
//!   explicit  gave-up     20,101       5,862,108 states
//!   symfwd    gave-up    668,613      89,921,612 nodes (229,729,460 summed over the sets)
//!   symbwd    NOT-THERE      634          28,468 nodes, 1 candidate of 1
//! ```
//!
//! THE BACKWARD SEARCH IS THE ONLY ONE THAT ANSWERS, and it answers in two thirds of a
//! second. The search spends twenty seconds and six gigabytes to give up; the symbolic
//! forward search spends ELEVEN MINUTES and ninety million nodes to give up. Neither
//! failure is the ration talking - both were given the whole measurement budget, which is
//! what de-e33h asked for and what the 256 MB history above could not say.
//!
//! And `not-there` here is settled, not exhausted: the one candidate was asked about and
//! its fixed point completed. The engine's approximations run the safe way, so a state
//! missing from a completed backward set genuinely cannot reach the target.
//!
//! WHAT IT DOES NOT SAY is anything about a long candidate list. One candidate is the
//! shape backward is best at, and the crossover this file exists to find lives in the
//! percentage profiles, where a refusal has to be paid for once per candidate.
//!
//! ## The first row the backward column has been run on, 2026-09-05
//!
//! Conversation 14 with its one structurally deepest entry unseen - so ONE CANDIDATE - at
//! the measurement budget, the backward column alone:
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
//! That is the row both forward searches fail. At the old 256 MB setting the search gave up
//! in 390ms having explored 133,089 states, and the symbolic forward search read NO ROOM
//! at 8.4 million nodes after 55 seconds; this settles it in under half a second on 28
//! thousand. It is one row against a run at a different allowance, so it is a shape rather
//! than a comparison - and the crossover this column is really for, a long candidate list
//! none of which is reachable, is untouched at `cands` of one.
//!
//! ## Running it
//!
//! One conversation per process, because a diagram manager that runs out of nodes takes the
//! whole process with it and a crash in the fourth row should not cost the other five:
//!
//!     CONVERSATION=368 cargo run --release --example performance_matrix
//!
//! `CONVERSATION`, `PROFILE` and `ENGINES` each narrow the grid, and all three take a
//! comma-separated list. A third engine triples what a full run costs, so being able to
//! ask one question of one group is not a convenience:
//!
//!     CONVERSATION=14 PROFILE=deepest-1 ENGINES=bwd cargo run --release \
//!         --example performance_matrix
//!
//! THE HEADER FOLLOWS THE SELECTION - a run that names one engine prints that engine's
//! columns and no others, so a narrowed run is never a wide row with holes in it. Ask for
//! the header alone with `HEADER_ONLY=1`, which is how `tools/measure-matrix.sh` learns the
//! column names rather than keeping its own copy of them.
//!
//! ## Every group in the game, with `GROUPS_ONLY=1`
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

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use lookahead_engine::core::state::StateSymbols;
use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::symbolic::portfolio;
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, discover_group, read_index};
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::known::{GroupShape, Known};
use lookahead_engine::symbolic::novelty_search::{
    best_novelty, Budget as SearchBudget, StoppedBy,
};
use lookahead_engine::symbolic::reachability::{seed_of, Budget, Reachability};
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::isolated;
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
    std::env::var("ROW_MEMORY_MB")
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
    let seconds = std::env::var("ROW_SECONDS")
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
/// `PROGRESS_SECONDS` still overrides, and `PROGRESS_SECONDS=0` still turns it off.
///
/// LIVES IN `symbolic_answers.rs`, along with [`symbolic_answers::PROGRESS`] and
/// [`symbolic_answers::mmss`], because the census narrates itself on the same clock and two
/// copies of "how often does a long run say where it is" would be two things to keep in
/// step. That file is pulled in here with `#[path]`, so it is the one both can see.
use symbolic_answers::progress_every;

/// How often the fixed point looks up from its work, against the five seconds it SPEAKS.
///
/// Two rates because they cost differently: looking up is a clock read and a memory
/// question, and is wanted often enough that the budget cannot be overspent by much;
/// gathering the line walks every entry's set for a node count, and is wanted only as often
/// as somebody can read it. The forward search needs no equivalent - it is already on a
/// clock of its own.
const CHECK_GAP: std::time::Duration = std::time::Duration::from_secs(1);

/// Bytes as gigabytes, for a line a person reads while waiting.
fn gb(bytes: usize) -> String {
    format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

use symbolic_answers::{mmss, PROGRESS};

/// The verdict for a row nothing was learned from, in every engine's columns.
///
/// LOUD, and not a word either engine can produce on its own, because the failure it
/// reports is not theirs: the machine could not supply the budget, so the row was never
/// run. A gap or a quiet `gave-up` here would read as a finding about the search.
const NOT_MEASURED: &str = "NOT-MEASURED";

/// The `fwdbwd` verdict for a row the GAME would not have searched at all.
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
    /// beforehand (`CENSUS=1`, see [`census`]) and named by `CENSUS_FILE`. Classifying per
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
            // `PROFILE=` key on it. What was real is the `real` column instead.
            Profile::DeepestUnreachable(n) => format!("deepest-unreach-{n}"),
            Profile::PercentSeen(p) => format!("{p}pc-seen"),
        }
    }
}

const PROFILES: [Profile; 10] = [

    Profile::DeepestUnseen(1),
    Profile::DeepestUnseen(5),
    Profile::DeepestUnseen(10),
    Profile::PercentSeen(95),
    Profile::PercentSeen(90),
    Profile::PercentSeen(75),
    Profile::PercentSeen(50),
    Profile::PercentSeen(25),
    Profile::PercentSeen(10),
    Profile::PercentSeen(5),
];

/// The unreachable profiles: runnable by name, and NOT part of the default grid.
///
/// KEPT OUT OF [`PROFILES`] ON PURPOSE. Two reasons, and the second is the one that matters.
/// They cannot run without a census to read, so a default run would fail on a machine that
/// had not taken one; and adding a profile to the default grid changes what "a whole-game
/// run" means, which would make the run this repository already has - ten profiles over
/// 1,422 groups - not comparable with the next one. They are a sweep of their own:
///
///     CENSUS_FILE=measurements/logs/<census>/census.tsv \
///       PROFILES=deepest-unreach-1,deepest-unreach-5 tools/measure-matrix.sh all
///
/// TEN IS DELIBERATELY ABSENT, where the seen profiles have deepest-10. de-thlz.2 asks for
/// 1 and 5 and nothing else, and a profile nobody asked for is hours of run time answering
/// a question nobody put.
const UNREACHABLE: [Profile; 2] =
    [Profile::DeepestUnreachable(1), Profile::DeepestUnreachable(5)];

/// Every profile a run can name, which is the default grid plus the ones held back from it.
fn known_profiles() -> impl Iterator<Item = Profile> {
    PROFILES.into_iter().chain(UNREACHABLE)
}

/// The three searches a row can hold, named for what they actually do.
///
/// DIRECTION IS WHAT TELLS THEM APART, which is why the names are what they are: every
/// engine here carries its data the same way - a decision diagram per entry - so a prefix
/// saying so would distinguish nothing.
///
/// READING AN OLD RUN: a folder whose columns say `symfwd`/`symbwd` is this pair under
/// older names, and one whose columns say `fwd`/`bwd` and nothing else is older still,
/// where `bwd` is what this file calls `fwd`. See measurements/README.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    /// `Reachability::explore_within`: a set per entry, forwards.
    Forward,
    /// `novelty_search::best_novelty` over `Backward`: a set per entry, backwards, one
    /// candidate at a time.
    Backward,
    /// `portfolio::best_novelty` AT THE PLAYER'S OWN SETTINGS: what somebody waits for.
    ///
    /// de-xegj split this from the column below, because one column cannot answer both of
    /// the questions asked of it - "what does a player wait for" and "where does this method
    /// actually stop" - and the single column answered neither. It ran at
    /// `portfolio::Budget::default()` on a six-gigabyte manager: a player gets 1000 ms and
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
    /// rations is right: `search_budget` deliberately caps a candidate at 250 ms and the
    /// candidate count at 64 whatever the dial says, so a two-minute clock built through it
    /// would have a real ceiling of about 64 x 250 ms. This is deliberately not the
    /// product's configuration.
    NoLimit,
}

const ALL_ENGINES: [Engine; 4] =
    [Engine::Forward, Engine::Backward, Engine::InGame, Engine::NoLimit];

/// What a run measures when it does not say. See [`engines`] for why it is this one.
const DEFAULT_ENGINES: [Engine; 1] = [Engine::InGame];

/// What to pass for the whole grid, since naming one engine no longer implies the rest.
const ALL: &str = "all";

impl Engine {
    /// The name `ENGINES` selects it by, and the prefix its columns carry.
    fn label(self) -> &'static str {
        match self {
            Engine::Forward => "fwd",
            Engine::Backward => "bwd",
            Engine::InGame => "ingame",
            Engine::NoLimit => "nolimit",
        }
    }

    /// The columns it fills, in order. THE ONE PLACE THE COLUMN NAMES LIVE: the header is
    /// built from these and `tools/measure-matrix.sh` asks the test for it.
    fn columns(self) -> &'static [&'static str] {
        match self {
            // Two sizes because neither bounds the other; see `symbolic_forward`.
            Engine::Forward => &["verdict", "ms", "nodes", "setsum"],
            // `asked` against `cands` is the whole trade this column exists to price: one
            // fixed point per candidate asked about, against one pass over all of them.
            Engine::Backward => &["verdict", "ms", "nodes", "asked", "cands"],
            // `by` is which half answered - the forward slice, the backward driver, or
            // neither completely - which is the whole question the switching method asks.
            //
            // `nodes` IS THE SAME QUANTITY THE OTHER TWO REPORT, added in de-x8ms.6: what
            // the manager holds at the end of the row. It was missing because the question
            // this column answers is "which half answered", not "how big did it get" - and
            // that mattered the moment fwdbwd became the default, because the parallel
            // split clears a group against a worker's share of the budget and had nothing
            // to clear it with. A fwdbwd-only run is exactly the run whose memory this is.
            // BOTH PORTFOLIO COLUMNS REPORT THE SAME THINGS, so the two can be read against
            // each other directly: the same row, the same question, one held to the player's
            // settings and one not.
            Engine::InGame | Engine::NoLimit => &["verdict", "ms", "nodes", "by", "asked"],
        }
    }

    fn headers(self) -> Vec<String> {
        self.columns().iter().map(|name| format!("{}_{name}", self.label())).collect()
    }
}

/// Which engines this run measures.
///
/// FWDBWD ALONE BY DEFAULT, since de-8xcd. It is the engine the game actually runs, and it
/// is the one being tuned; fwd and bwd are evidence for that tuning rather than products in
/// their own right, and they are expensive out of all proportion to how often the evidence
/// is wanted. `ENGINES=all` measures the three, and naming any of them works as it always
/// did.
///
/// WHAT IT SAVES, measured over the two whole-game datasets rather than asserted. Engine time
/// by column on the ten-profile grid (measurements/logs/whole-game, 5,210 measured rows) was
/// fwd 2.06 h / 63.9%, bwd 0.73 h / 22.7%, fwdbwd 0.43 h / 13.4%. On the deepest-unreachable
/// sweep it was starker still - fwd 99.6% - because that profile has no yes to stumble onto
/// and the forward search has to exhaust. So a default run drops to about an eighth of its
/// engine time on the grid and to a five-hundredth on the unreachable profiles.
///
/// WHAT IT COSTS, and it is worth saying rather than discovering: the fwd and bwd columns
/// become a HISTORICAL BASELINE. measurements/logs/whole-game holds them for every group and
/// profile in the game, which is what makes this reasonable now - but that ages the moment
/// either engine changes, and whoever changes one and wants to know what they did to it has
/// to ask for them deliberately, in hours rather than minutes.
///
/// The selection was here before this became the default, because a third column triples what
/// a full run costs and because a question is often about one engine - "what does the backward
/// search do with the one case both forward searches cannot answer" is a single row of a
/// single column, and spending an hour on the other thirty-two to get it is how a measurement
/// stops being run at all.
///
/// A narrowed run is a NARROWER ROW, not a wide one with holes in it: the header follows
/// the selection, so nothing has to be told apart from a result later.
fn engines() -> Vec<Engine> {
    let named = std::env::var("ENGINES").unwrap_or_default();
    let wanted: Vec<&str> = named.split(',').map(str::trim).filter(|n| !n.is_empty()).collect();

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
    // THE ALTERNATIVES ARE ASKED OF THE ENGINES rather than written out here, because a
    // written copy went stale and the message spent an era offering `explicit, symfwd,
    // symbwd` - the MIDDLE era's spelling - which meant it refused `symfwd` in the very
    // sentence that named it, and sent the reader on to two more names that were also
    // gone. A refusal that misdirects costs more than no refusal at all.
    // `all` IS OFFERED HERE TOO, because it is now a name a caller can pass and a refusal
    // that lists only the engines would send a reader looking for the grid to spell out
    // three of them - the same misdirection the note above is about, in a new place.
    for name in &wanted {
        assert!(
            ALL_ENGINES.iter().any(|engine| engine.label() == *name),
            "no engine called {name:?}: the names are {}, or {ALL} for every one of them",
            ALL_ENGINES.map(Engine::label).join(", "),
        );
    }

    ALL_ENGINES.into_iter().filter(|engine| wanted.contains(&engine.label())).collect()
}

/// One canonical start per DISTINCT group, heaviest first: (start, conversations, entries).
///
/// ## Which start, and why it is not the smallest member
///
/// `discover_group` is a forward closure, so two starts in the same group can reach
/// different sets and only some of them reach all of it. The start kept here is the
/// SMALLEST ONE WHOSE OWN CLOSURE IS THE WHOLE SET, which is what makes the line
/// reproducible: handing it back as `CONVERSATION=` rebuilds exactly the group it came
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
    match std::env::var("CONVERSATION") {
        Ok(named) => named
            .split(',')
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| {
                id.parse().unwrap_or_else(|_| {
                    refuse(&format!("CONVERSATION={id:?} is not a conversation id"))
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
        known_profiles().map(|p| p.label()).collect::<Vec<_>>().join(", "),
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
    match std::env::var("PROFILE") {
        Ok(named) => named
            .split(',')
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(|label| {
                known_profiles()
                    .find(|profile| profile.label() == label)
                    .unwrap_or_else(|| refuse(&format!("PROFILE={label:?} is not a profile")))
            })
            .collect(),
        Err(_) => PROFILES.to_vec(),
    }
}

/// Which entries are reachable from `start` by following links alone, and how far.
///
/// Guards ignored entirely, which is what de-raed means by "determined by edge analysis
/// alone". It is the loosest notion of reachable there is, and that is why it is the right
/// one for choosing a question: an entry missing from it is unreachable for certain, so
/// seeding it would make a row meaningless.
fn structurally_reachable(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
) -> HashMap<DialogueNodeId, usize> {
    let mut depth = HashMap::new();
    let mut queue = VecDeque::new();
    depth.insert(start, 0usize);
    queue.push_back(start);

    while let Some(id) = queue.pop_front() {
        let here = depth[&id];
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if graph.get(child).is_some() && !depth.contains_key(&child) {
                depth.insert(child, here + 1);
                queue.push_back(child);
            }
        }
    }

    depth
}

/// The entries a profile can be built from: reachable, not the start, and not groups.
///
/// GROUPS ARE EXCLUDED because the game never writes a group's SimStatus, so every group in
/// the database reads as never displayed and the search refuses to score one. Seeding a group
/// as unseen would add an entry that cannot end a search, which would quietly make a row
/// harder than it claims to be.
///
/// Returned deepest first, ties broken by id, so a run repeats exactly.
fn candidates(graph: &LookAheadGraph, start: DialogueNodeId) -> Vec<DialogueNodeId> {
    let depths = structurally_reachable(graph, start);
    let mut all: Vec<(DialogueNodeId, usize)> = depths
        .into_iter()
        .filter(|(id, _)| *id != start)
        .filter(|(id, _)| graph.get(*id).is_some_and(|node| !node.is_group))
        .collect();

    all.sort_unstable_by_key(|(id, depth)| {
        (std::cmp::Reverse(*depth), id.conversation_id, id.entry_id)
    });
    all.into_iter().map(|(id, _)| id).collect()
}

/// A small deterministic generator, so a row is the same row on every machine.
///
/// Written out rather than taken from a crate: what is wanted is repeatability across runs
/// and platforms, and a named algorithm with the arithmetic in view gives that without a
/// dependency whose version could change the draw underneath a recorded measurement.
/// This is xorshift64*, which is more than good enough for choosing which entries to mark.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // Zero is a fixed point of xorshift, so it can never be the state.
        Self(seed.wrapping_mul(2685821657736338717).max(1))
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(2685821657736338717)
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
        if !profiles.iter().any(|p| matches!(p, Profile::DeepestUnreachable(_))) {
            return None;
        }

        let Ok(path) = std::env::var("CENSUS_FILE") else {
            refuse(
                "an unreachable profile needs CENSUS_FILE, naming the census.tsv that says \
                 which entries are unreachable. Take one with tools/measure-census.sh.",
            )
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            refuse(&format!("CENSUS_FILE={path:?} could not be read"))
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
            Some(DialogueNodeId::new(conversation.parse().ok()?, entry.parse().ok()?))
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

fn unseen_for(
    profile: Profile,
    candidates: &[DialogueNodeId],
) -> HashSet<DialogueNodeId> {
    match profile {

        Profile::DeepestUnseen(n) => candidates.iter().take(n).copied().collect(),
        // Built by `unreachable_for`, which needs the census this does not have. Reaching
        // here would mean the row loop stopped asking it first.
        Profile::DeepestUnreachable(_) => unreachable!("an unreachable profile reads the census"),
        Profile::PercentSeen(percent) => {
            // The seed IS the percentage, as de-raed asks: reproducible, and different for
            // every row so two rows are not accidentally the same draw.
            let mut rng = Rng::new(percent as u64);
            let mut shuffled = candidates.to_vec();

            // Fisher-Yates, so every subset of the right size is equally likely. Taking the
            // first n of a sorted list after a partial shuffle would not be.
            for i in (1..shuffled.len()).rev() {
                let j = (rng.next() % (i as u64 + 1)) as usize;
                shuffled.swap(i, j);
            }

            let seen = (shuffled.len() * percent as usize) / 100;
            shuffled.into_iter().skip(seen).collect()
        }
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
/// STRINGS RATHER THAN NUMBERS, because the three engines do not report the same
/// quantities and a struct wide enough for all of them would have to say what a column
/// means by leaving it zero - which is a value the reader cannot tell from a measurement.
struct Cells(Vec<String>);

impl Cells {
    fn of(verdict: &str, millis: u128, sizes: &[usize]) -> Self {
        let mut cells = vec![verdict.to_string(), millis.to_string()];
        cells.extend(sizes.iter().map(|size| size.to_string()));
        Self(cells)
    }

    /// The cells of an engine that did not run, one `?` per column it would have filled.
    fn absent(verdict: &str, engine: Engine) -> Self {
        let mut cells = vec![verdict.to_string()];
        cells.extend(engine.columns().iter().skip(1).map(|_| "?".to_string()));
        Self(cells)
    }
}

/// The switching method, which is what the game runs.
///
/// THE PORTFOLIO, NOT A THIRD ALGORITHM: a forward slice hunting the best class anything
/// reachable carries, and then the backward driver told what the slice found. Where the
/// slice halts, nothing else runs; where it does not, what it reached is handed on and a
/// backward pass that MEETS it stops there. The other two columns are its halves measured
/// alone, which is what makes this row readable against them.
///
/// The budget is the portfolio's own - the one the bridge hands it, scaled by nothing here
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

/// What each portfolio column is allowed.
fn search_budget_for(engine: Engine) -> portfolio::Budget {
    match engine {
        Engine::InGame => in_game_request().search_budget(),
        // NO LIMIT, WALLED. The wall is the contract and the rations under it are estimates
        // aimed at landing inside it; an implementer may tune them, and 120 seconds is what
        // the column promises.
        //
        // TARGETS AND STEPS ARE RELEASED TOO, which is the point: raising the clock alone
        // would achieve nothing, since 64 candidates at 250 ms is a real ceiling of about
        // sixteen seconds however long the outer clock is.
        _ => portfolio::Budget {
            overall: std::time::Duration::from_secs(120),
            forwards: std::time::Duration::from_secs(20),
            backwards: std::time::Duration::from_secs(100),
            each: std::time::Duration::from_secs(10),
            targets: usize::MAX,
            // The slice is held to the same allowance the manager gets, rather than the
            // 256 MB it would otherwise inherit by default - which would quietly cap this
            // column at the very number it exists to exceed.
            slice_memory: budget().memory(),
            slice_steps: usize::MAX,
            pruning: portfolio::Budget::default().pruning,
        },
    }
}

fn forward_backward(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn lookahead_engine::world::world::ILookAheadWorld,
    symbols: &StateSymbols,
    unseen: &HashSet<DialogueNodeId>,
    engine: Engine,
) -> Cells {
    let began = std::time::Instant::now();

    // THE MANAGER DIFFERS BETWEEN THE TWO, and by far more than the clocks do. A player gets
    // 256 MB; the run's own allowance is 6144 serial or a worker's share in parallel - a
    // twenty-four-fold gap, against a two-fold gap in time. It is the divergence most likely
    // to hide `no-room` rows a player would actually hit, which is the in-game column's most
    // useful output rather than a regression.
    let manager = match engine {
        Engine::InGame => in_game_request().diagram_budget(),
        _ => budget(),
    };

    let layout = DataLayout::for_group(graph, world, COUNTER_CAP);
    let Some(vars) = DataVars::try_new(&layout, symbols, manager) else {
        return Cells::absent(NOT_MEASURED, engine);
    };
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(world)
        .with_constant_clock(DataLayout::group_passes_time(graph));
    let seed = seed_of(graph, world, &vars);

    let novelty = |id: DialogueNodeId| {
        if unseen.contains(&id) { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
    };

    // THE GATE THE GAME APPLIES, and this column exists to be the game. de-qh27.
    //
    // src/bridge.rs `scored` computes a baseline and refuses to search when nothing
    // link-reachable beats it, returning a COMPLETE answer of "none" having run nothing.
    // This used to skip that and call the portfolio unconditionally, which recorded backward
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
            began.elapsed().as_millis().to_string(),
            // Nothing was built, nothing answered, nothing asked. `by` says which half
            // answered and the honest value is neither.
            "0".to_string(),
            "Gated".to_string(),
            "0".to_string(),
        ]);
    };

    let answer = portfolio::best_novelty(
        graph,
        start,
        StartBranch::Either,
        &seed,
        &mut compiler,
        world,
        COUNTER_CAP as u32,
        &novelty,
        hunting,
        &search_budget_for(engine),
        // ONE START PER ROW HERE, so this builds a shape per search - which is what every
        // row of this matrix has always paid, and holding that constant is what lets a row
        // measured today be read against one measured before de-bnjy.10.
        &GroupShape::of(graph),
    );

    let verdict = match answer.by {
        _ if answer.best > Novelty::SeenThisGame => "found",
        portfolio::Answered::Partly => "gave-up",
        _ => "not-there",
    };

    // THE SAME `node_count` THE OTHER TWO REPORT - what the manager holds, which is memory
    // in use and therefore what the budget and the parallel split both watch.
    //
    // READ AT THE END OF THE ROW, not tracked as a high-water mark, which is what fwd and
    // bwd do too. For a single search those are near enough the same thing; for a portfolio
    // that runs a forward slice and then a backward driver over ONE manager they are also
    // near enough, because nodes are not reclaimed eagerly between the halves. It would
    // stop being true if the halves ever got managers of their own.
    //
    // EXPECT IT TO BE SMALLER THAN THE fwd COLUMN for the same group, and that is the point
    // rather than a discrepancy: fwdbwd stops as soon as either half can answer, so it
    // genuinely holds less. A split decided on this number is deciding against what the run
    // in front of it actually costs.
    Cells(vec![
        verdict.to_string(),
        began.elapsed().as_millis().to_string(),
        vars.node_count().to_string(),
        format!("{:?}", answer.by),
        answer.targets_asked.to_string(),
    ])
}

fn symbolic_forward(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn lookahead_engine::world::world::ILookAheadWorld,
    symbols: &StateSymbols,
    unseen: &HashSet<DialogueNodeId>,
) -> Cells {
    let began = std::time::Instant::now();

    let layout = DataLayout::for_group(graph, world, COUNTER_CAP);
    // From the same allowance, so the diagram and the search are held to one number rather than two
    // that happen to agree.
    //
    // FALLIBLY, because the alternative is not a wrong number but a dead process: the
    // manager preallocates its node store and that allocation aborts. A None here means the
    // machine could not supply the budget, which is not a finding about the search - the
    // row is NOT MEASURED and wants running again with the memory free.
    let Some(vars) = DataVars::try_new(&layout, symbols, budget()) else {
        return Cells::absent(NOT_MEASURED, Engine::Forward);
    };
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(world)
        .with_constant_clock(DataLayout::group_passes_time(graph));

    let seed = seed_of(graph, world, &vars);
    let quarry: HashSet<DialogueNodeId> = unseen.clone();
    let allowance = memory();
    let every = progress_every();
    let sym_budget = Budget {
        steps: usize::MAX,
        time: row_time(),
        memory: allowance,
        // A STARTING GUESS ONLY when progress is on: `check_gap` retunes it from here to
        // whatever holds a second, because no fixed step count can - twenty thousand steps
        // is a moment early on and minutes once the sets are large, and the minutes end is
        // exactly where somebody is watching. Checking every second also bounds the memory
        // overshoot in time rather than in steps, which can only make the no-room verdict
        // land closer to the budget it names.
        report_every: if every.is_some() { 500 } else { 20_000 },
        check_gap: if every.is_some() { CHECK_GAP } else { std::time::Duration::ZERO },
        // The measurement allowance is far larger than the plugin's, so the machine is the
        // real ceiling here and the guard matters more, not less.
        on_step: None,
        system_reserve: lookahead_engine::core::system_memory::DEFAULT_RESERVE,
        report_gap: every.unwrap_or_default(),
        on_progress: every.map(|_| {
            Box::new(
                move |steps: usize,
                      reached: usize,
                      held: usize,
                      largest: usize,
                      bytes: usize| {
                    // THE LABEL COMES FROM THE ENGINE, and is padded to the width of the
                    // longest so the columns line up. A progress line names itself with
                    // the same word `ENGINES=` takes, so a line watched during a long run
                    // can be typed straight back to reproduce it.
                    println!(
                        "{PROGRESS} {:<6} {:>7}  {steps:>10} steps  {reached:>6} reached  \
                         {held:>11} set nodes  largest {largest:>9}  {} / {}",
                        Engine::Forward.label(),
                        mmss(began.elapsed()),
                        gb(bytes),
                        gb(allowance),
                    );
                },
            ) as Box<dyn Fn(usize, usize, usize, usize, usize)>
        }),
        // The same early exit the forward search has: the question is whether ANY unseen
        // entry is reachable, not what the whole reachable set is.
        halt_on: Some(Box::new(move |id| quarry.contains(&id))),
    };

    let found = Reachability::explore_within(
        graph, start, &seed, &mut compiler, world, COUNTER_CAP as u32, &sym_budget,
    );
    let stats = found.stats();

    let verdict = if stats.halted_at.is_some() {
        "found"
    } else if stats.out_of_system_memory {
        // THE MACHINE, not the budget. de-e33h asked for these to be told apart, and this
        // is the diagram's half of it: the row is not a result and wants running again
        // with the memory free.
        "no-ram"
    } else if stats.out_of_memory {
        "no-room"
    } else if stats.reached_fixed_point {
        "not-there"
    } else {
        "gave-up"
    };

    // TWO DIFFERENT QUANTITIES, and they are both here because neither bounds the other
    // and one of them alone would mislead.
    //
    // `nodes` is what the MANAGER holds: every node allocated for anything - the reachable
    // sets, the compiled guards, the transition relations, the intermediate results of
    // every operation, and whatever has not been reclaimed yet. That is memory in use, so
    // it is what the budget watches.
    //
    // `setsum` is the size of the ANSWER: each entry's set counted separately, so a node
    // shared between two entries is counted twice.
    //
    // On conversation 368 the sum is much the larger (19.3 million against a budget of 8),
    // because a great many entries hold big overlapping sets. On 28 the manager is the
    // larger (562,880 against 255,443), because the answer is small and most of the
    // allocation went on machinery. A column showing only one of them would suggest the
    // budget had failed to fire in the first case and that the search was cheap in the
    // second.
    Cells::of(
        verdict,
        began.elapsed().as_millis(),
        &[vars.node_count(), stats.diagram_nodes],
    )
}

/// The genuine backward search, driven the way the portfolio would drive it.
///
/// ONE CANDIDATE AT A TIME, best novelty class first, stopping at the first candidate
/// proved reachable - which is `novelty_search` and not `Backward` alone. Asking `Backward`
/// about one hand-picked target would measure a question nobody asks: the ordering is what
/// makes the short circuit sound, and the per-candidate cost when the answer is no is the
/// crossover this column exists to price.
///
/// ITS OWN MANAGER, from the same allowance and the same trimmed layout as the symbolic
/// forward column, so the two symbolic columns differ in direction and in nothing else.
fn symbolic_backward(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn lookahead_engine::world::world::ILookAheadWorld,
    symbols: &StateSymbols,
    unseen: &HashSet<DialogueNodeId>,
) -> Cells {
    let began = std::time::Instant::now();

    let layout = DataLayout::for_group(graph, world, COUNTER_CAP);
    let Some(vars) = DataVars::try_new(&layout, symbols, budget()) else {
        return Cells::absent(NOT_MEASURED, Engine::Backward);
    };
    let mut compiler = GuardCompiler::new(&vars)
        .with_world(world)
        .with_constant_clock(DataLayout::group_passes_time(graph));

    let seed = seed_of(graph, world, &vars);
    let novelty = |id: DialogueNodeId| {
        if unseen.contains(&id) { Novelty::UnseenAnyGame } else { Novelty::SeenThisGame }
    };

    // THE GRAPH'S SHAPE ONLY, and deliberately nothing else. Every candidate's pass used
    // to rebuild the parent map from scratch, which on a four-thousand-entry group is a
    // full walk per candidate; sharing it changes no answer at all.
    //
    // WHAT IS NOT SHARED HERE is any forward result. `Known` can carry one, and a pass that
    // meets it stops early having proved the target reachable - but this column exists to
    // say what a backward search costs on its own, and a column quietly answered by another
    // engine's work would be the same mislabelling this file was corrected for. de-cnjw is
    // where that is measured, against a run that pays for the forward half.
    let known = Known::of(graph);
    let every = progress_every();
    let answer = best_novelty(
        graph,
        start,
        StartBranch::Either,
        &seed,
        &mut compiler,
        world,
        COUNTER_CAP as u32,
        &novelty,
        &SearchBudget {
            // NO CANDIDATE CAP, unlike the portfolio's 64. A cap is a product decision
            // about how long a response menu may take, and a row measured under one says
            // where the cap was rather than what the search costs. The row's clock is what
            // stops this, the same as it stops the other two columns.
            targets: usize::MAX,
            time: row_time(),
            each: lookahead_engine::symbolic::backward::Budget {
                steps: usize::MAX,
                // The row's whole allowance, because one candidate is allowed to spend it:
                // on the profiles this file is about there is often only one.
                time: row_time(),
                report_gap: every.unwrap_or_default(),
                on_progress: every.map(|_| {
                    // NODES AGAINST THE CAPACITY, where the other two columns show bytes.
                    // The manager is what fills up here, and its capacity is the number
                    // the budget was turned into - so this is the same question in the
                    // units the answer will arrive in.
                    let capacity = budget().nodes();
                    std::rc::Rc::new(move |steps: usize, known: usize, queued: usize, nodes: usize| {
                        println!(
                            "{PROGRESS} {:<6} {:>7}  {steps:>10} steps  {known:>6} reaching  \
                             {queued:>6} queued  {nodes:>12} / {capacity} nodes",
                            Engine::Backward.label(),
                            mmss(began.elapsed()),
                        );
                    }) as std::rc::Rc<dyn Fn(usize, usize, usize, usize)>
                }),
            },
        },
        Some(&known),
    );

    let verdict = if answer.best != Novelty::SeenThisGame {
        "found"
    } else {
        match answer.stopped_by {
            // Every candidate asked about and every one refused, so the answer is final.
            StoppedBy::Nothing => "not-there",
            StoppedBy::Targets | StoppedBy::Time => "gave-up",
            // A pass that did not settle proves nothing by saying no, and WHY it did not
            // settle is the same distinction the other two columns keep.
            StoppedBy::Incomplete => {
                if answer.out_of_nodes { "no-room" } else { "gave-up" }
            }
        }
    };

    // `asked` AGAINST `cands` IS THE POINT. One fixed point was paid per candidate asked
    // about, where the search beside it pays one walk for every candidate there is - so a
    // row where the two numbers are equal and the verdict is not-there is the worst case
    // for this engine, and a row that asked about one of hundreds is its best.
    Cells::of(
        verdict,
        began.elapsed().as_millis(),
        &[vars.node_count(), answer.targets_asked, answer.candidates],
    )
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
    "conv", "candidates", "unreachable", "undecided", "exact", "ms", "deepest_unreachable",
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
/// `CENSUS_ALL=1` scans every candidate instead. That is a different and much longer run -
/// classification is a bounded backward pass per candidate - so it is asked for rather than
/// assumed.
fn census_wanted() -> usize {
    if std::env::var("CENSUS_ALL").is_ok() {
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
    if std::env::var("NO_HEADER").is_err() {
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
        let exact = if unreachable.len() == wanted { "at-least" } else { "all" };

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
    let Some(path) = common::conversation_index() else { return };
    let engines = engines();

    // Tab separated, so a run pipes straight into a file that something else can read -
    // see the note at the top about keeping the logs. The header names exactly the engines
    // that ran, and is the only place the column names are written down.
    let header: Vec<String> = ROW_COLUMNS
        .iter()
        .map(|name| name.to_string())
        .chain(engines.iter().flat_map(|engine| engine.headers()))
        .collect();

    // ASKED FOR ON ITS OWN by tools/measure-matrix.sh, which needs the column names before
    // it has a row and should not keep a second copy of them to go stale.
    if std::env::var("HEADER_ONLY").is_ok() {
        println!("{}", header.join("\t"));
        return;
    }

    let index = read_index(&path).expect("the index reads");

    // ASKED FOR ON ITS OWN, like the header, and for the same reason: a whole-game run has
    // to know what the rows ARE before it measures any, and a list kept anywhere else can
    // omit a group and never say so.
    if std::env::var("GROUPS_ONLY").is_ok() {
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
    if std::env::var("CENSUS").is_ok() {
        census(&index, &world);
        return;
    }

    // ONCE, BEFORE ANY ROW, and only when a row is going to want it. Reading it per row would
    // be the same file parsed for every group of a whole-game sweep, and asking for it when
    // no unreachable profile was named would make a census a precondition of every run.
    let census = Census::of(&profiles());

    // SUPPRESSIBLE, because the driver script runs one row per process and wants one header
    // in the file rather than one per row.
    if std::env::var("NO_HEADER").is_err() {
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
            // EVERY ENGINE HERE WANTS ONE, since the explicit search went: it was the only
            // column that allocated no manager, and the check used to be skipped for a run
            // of it alone. Now a machine that cannot supply the budget has nothing to
            // measure at all, which is what NOT-MEASURED says.
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

            // EACH ENGINE ON A THREAD OF ITS OWN, which is the one thing this file was not
            // doing and six other measurements were - de-w0rw. The 6 GB run of 2026-09-06
            // lost five of sixty rows to `thread 'main' has overflowed its stack`, and the
            // thread it names is the process's own.
            //
            // WHAT MAKES IT THE FIX rather than a bigger stack, measured on de-fpax: what
            // accumulates is BUILDING A SECOND MANAGER ON A THREAD THAT HAS ALREADY BUILT
            // ONE, and the third overflows - which is why the deaths were always a third
            // row. A run here builds a manager per engine per profile per conversation, all
            // on one thread; a thread per engine means one manager per thread, which is the
            // arrangement that did not fail in twenty runs where the main thread failed in
            // eight of twenty. A half-gigabyte stack only moved the rate.
            //
            // THE HELPER'S CONSTRAINT IS ALREADY MET: each of these builds its layout, its
            // manager, its compiled guards and its seed itself, and hands back `Cells` -
            // strings - so nothing borrowed from a manager crosses the boundary.
            let measured: Vec<String> = engines
                .iter()
                .flat_map(|engine| {
                    isolated::on_its_own_thread(|| match engine {
                        Engine::Forward => {
                            symbolic_forward(&graph, start, &world, &symbols, &unseen).0
                        }
                        Engine::Backward => {
                            symbolic_backward(&graph, start, &world, &symbols, &unseen).0
                        }
                        Engine::InGame | Engine::NoLimit => {
                            forward_backward(&graph, start, &world, &symbols, &unseen, *engine).0
                        }
                    })
                })
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
            if matches!(profile, Profile::DeepestUnreachable(_)) {
                if measured.iter().any(|cell| cell == "found") {
                    eprintln!(
                        "CONTRADICTION: {conversation} {} came back 'found', which cannot \
                         happen - every entry in this set was proved unreachable by the \
                         census. Either the census is wrong or the search is. The likeliest \
                         cause is a CENSUS_FILE taken under a different world from the one \
                         this row was measured under; nothing checks that.",
                        profile.label(),
                    );
                }
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
