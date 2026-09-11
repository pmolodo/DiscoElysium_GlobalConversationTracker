// SPDX-License-Identifier: MIT
//! How fast does the backward driver answer, and what does sharing buy it?
//!
//! THE ONLY QUESTION THE LOOK-AHEAD ASKS is whether any unseen entry is reachable from
//! here. Not which entries, not the states at them, not a fixed point over the group. So
//! the driver stops at the first candidate it can prove, and this measures the clock from
//! the call to the answer.
//!
//! ## The profiles, and why the unreachable ones are the point
//!
//! `deepest-N` takes the N entries furthest from the start by links. That is the shape the
//! epic is about - a handful of unseen entries, buried - but it is not the hard case on its
//! own, because a deep entry that IS reachable is proved the moment the backward pass meets
//! the seed, and that is usually instant.
//!
//! `deepest-unreachable-N` takes the N deepest entries no path can reach.
//! Those are the expensive ones: a no has to be proved, which means driving the fixed point
//! to completion rather than stumbling on a yes. Where a group has fewer than N unreachable
//! entries the set is topped up from the deepest remaining, and where none can be
//! classified within the budget it falls back to `deepest-N` outright and says so - a
//! profile that silently became a different profile is worse than one that admits it.
//!
//! Classification costs one bounded backward pass per candidate and is done once per group,
//! deepest first, stopping as soon as N unreachable entries are in hand.
//!
//! ## A thread per search, which is not a style choice
//!
//! Something accumulates per-thread inside the diagram manager: twelve identical searches
//! die on the third when they share a thread and all twelve survive on a thread each
//! (de-8hh2.13). This measurement lost whole groups to that before it span each search off -
//! 1030, 368 and 14 all took the process down mid-run. de-fpax is the fix; this is it
//! applied here, and the fat stack is belt and braces for releasing a large diagram.
//!
//! Everything the diagram touches is built INSIDE the thread and dropped there. Only plain
//! numbers come back out.

use std::collections::{HashMap, HashSet, VecDeque};

use lookahead_engine::core::types::{DialogueNodeId, Novelty, StartBranch};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::symbolic::backward::Budget as BackwardBudget;
use lookahead_engine::symbolic::budget::DiagramBudget;
use lookahead_engine::symbolic::data_layout::DataLayout;
use lookahead_engine::symbolic::guard_formula::GuardCompiler;
use lookahead_engine::symbolic::isolated::on_its_own_thread;
use lookahead_engine::symbolic::known::Known;
use lookahead_engine::symbolic::novelty_search::{
    Budget as SearchBudget, Classify, StoppedBy, Wants, best_novelty, classify_candidates,
};
use lookahead_engine::symbolic::reachability::seed_of;
use lookahead_engine::symbolic::vars::DataVars;
use lookahead_engine::world::world::ILookAheadWorld;

#[path = "../tests/common/mod.rs"]
mod common;

const COUNTER_CAP: i32 = 16;

/// The groups every symbolic measurement in this repository is taken on.
const MEASURED: [i32; 5] = [368, 631, 14, 28, 1030];

/// How long the whole driver may run before its answer is called a failure.
const ANSWER_CAP: std::time::Duration = std::time::Duration::from_secs(120);

/// How long ONE candidate may run while a profile is being classified.
///
/// Short on purpose. Classification asks about many candidates and only needs the ones it
/// can settle quickly; anything slower is recorded as unknown rather than waited out.
pub const CLASSIFY_CAP: std::time::Duration = std::time::Duration::from_secs(5);

/// The prefix every progress line carries.
///
/// It must NOT start with a conversation number: `tools/measure-matrix.py` picks the row out
/// of the log with `grep -E "^$conversation\b"` and `tools/measure-census.sh` does the same,
/// so a progress line that matched would be recorded as the row and the real one thrown
/// away.
pub const PROGRESS: &str = "  ~";

/// How often a long run says where it has got to when nothing asks for something else.
///
/// SHORT ENOUGH TO ANSWER "is it stuck", long enough that a run of quick pieces of work does
/// not narrate itself: the ones that need it are spending a cap measured in minutes, and the
/// ones that do not will finish before the first line is due.
const DEFAULT_PROGRESS_SECONDS: u64 = 30;

/// The gap between progress lines, or `None` where they are turned off.
///
/// `PROGRESS_SECONDS` overrides, and `DEGCT_PROGRESS_SECONDS=0` turns them off - which is what the
/// "greater than zero" filter below has always meant and also expresses.
///
/// SHARED BY THE MATRIX AND THE CENSUS. measurements/performance_matrix.rs pulls this file
/// in with `#[path]` and both narrate on this clock; a second copy would be a second thing
/// to keep in step, and the whole question - how often should a long run speak - has one
/// answer rather than one per measurement.
pub fn progress_every() -> Option<std::time::Duration> {
    let seconds = match lookahead_engine::core::env::var("PROGRESS_SECONDS") {
        Ok(named) => named
            .trim()
            .parse::<u64>()
            .unwrap_or(DEFAULT_PROGRESS_SECONDS),
        Err(_) => DEFAULT_PROGRESS_SECONDS,
    };
    (seconds > 0).then(|| std::time::Duration::from_secs(seconds))
}

/// A duration as m:ss, for a line a person reads while waiting.
pub fn mmss(elapsed: std::time::Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{}m{:02}s", seconds / 60, seconds % 60)
}

fn main() {
    let Some(path) = common::conversation_index() else {
        eprintln!("no conversation index; nothing to measure");
        return;
    };
    let index = read_index(&path).expect("the index reads");
    let world = common::measurement_save();

    let asked: Vec<i32> = match lookahead_engine::core::env::var("CONVERSATION") {
        Ok(named) => named
            .split(',')
            .filter_map(|id| id.trim().parse().ok())
            .collect(),
        Err(_) => MEASURED.to_vec(),
    };

    println!(
        "{:>6} {:>24} {:>7}  {:9} {:>9} {:>16} {:>7}",
        "conv", "profile", "unseen", "sharing", "ms", "verdict", "asked",
    );

    for conversation in asked {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let start = DialogueNodeId::new(conversation, 0);
        if graph.get(start).is_none() {
            continue;
        }

        let deepest = deepest_first(&graph, start);
        if deepest.is_empty() {
            continue;
        }

        for wanted in [1usize, 5, 10] {
            for (name, unseen) in profiles(&graph, start, &world, &deepest, wanted) {
                for shared in [false, true] {
                    let answer = answer(&graph, start, &world, &unseen, shared, ANSWER_CAP);
                    println!(
                        "{conversation:>6} {name:>24} {:>7}  {:9} {:>9} {:>16} {:>7}",
                        unseen.len(),
                        if shared { "shared" } else { "alone" },
                        answer.millis,
                        answer.verdict,
                        answer.asked,
                    );
                }
            }
        }
    }
}

/// Every entry a link walk can arrive at, furthest from the start first.
fn deepest_first(graph: &LookAheadGraph, start: DialogueNodeId) -> Vec<DialogueNodeId> {
    let mut depth: HashMap<DialogueNodeId, u32> = HashMap::new();
    let mut queue = VecDeque::from([(start, 0u32)]);
    while let Some((id, here)) = queue.pop_front() {
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if graph.get(child).is_none() || depth.contains_key(&child) {
                continue;
            }
            depth.insert(child, here + 1);
            queue.push_back((child, here + 1));
        }
    }

    let mut all: Vec<DialogueNodeId> = depth
        .keys()
        .copied()
        .filter(|id| *id != start)
        .filter(|id| graph.get(*id).is_some_and(|node| !node.is_group))
        .collect();
    all.sort_by_key(|id| {
        (
            std::cmp::Reverse(depth.get(id).copied().unwrap_or(0)),
            id.conversation_id,
            id.entry_id,
        )
    });
    all
}

/// `deepest-N`, and `deepest-unreachable-N` where enough entries can be classified.
fn profiles(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    deepest: &[DialogueNodeId],
    wanted: usize,
) -> Vec<(String, HashSet<DialogueNodeId>)> {
    let mut out = Vec::new();
    if deepest.len() < wanted {
        return out;
    }
    out.push((
        format!("deepest-{wanted}"),
        deepest.iter().copied().take(wanted).collect(),
    ));

    let (unreachable, unknown) = classify(graph, start, world, deepest, wanted);
    if unreachable.is_empty() {
        // NOTHING TO BUILD IT FROM, and saying so is the point: a profile that quietly
        // becomes `deepest-N` again would be two rows claiming to be different measurements.
        println!(
            "{:>6} {:>24}  no entry classified unreachable ({} undecided); \
             deepest-{wanted} stands alone",
            start.conversation_id,
            format!("deepest-unreachable-{wanted}"),
            unknown.len(),
        );
        return out;
    }

    // TOPPED UP FROM THE DEEPEST REMAINING where too few were classified, because a set of
    // three when ten were asked for is a different profile again. The name says how many of
    // it are genuinely unreachable.
    let mut set: HashSet<DialogueNodeId> = unreachable.iter().copied().take(wanted).collect();
    let short = wanted.saturating_sub(set.len());
    if short > 0 {
        for &id in deepest {
            if set.len() == wanted {
                break;
            }
            set.insert(id);
        }
    }

    let proven = unreachable.len().min(wanted);
    out.push((format!("deepest-unreach-{wanted} ({proven} proved)"), set));
    out
}

/// A classification's verdicts on disk, written as they arrive, and read back to resume.
///
/// ## What it is for
///
/// A census runs one process per group and appends the group's row when that process
/// finishes, so the resume boundary is the whole group. Under the ten-entry cap that cost
/// seconds. Without it every candidate is asked about and an unsettled one costs the whole
/// of [`CLASSIFY_CAP`], so a group with many of them is hours - and interrupting it, or
/// having it crash, threw all of that away. de-e4mu.
///
/// ## Written as they arrive, rather than saved on the way out
///
/// The obvious shape is to catch the terminating signal and write down what has been
/// gathered. It does not survive contact: `tools/stop-measurements.sh` kills the process
/// outright, and a crash is not a signal at all - and a crash is a RESULT here, one this
/// measurement expects often enough to have a word for. Neither gives a handler its turn.
/// Appending each verdict as it is established needs no handler and survives both, because
/// what is on disk was already on disk before the process died.
///
/// The cost is one short line and one flush per candidate, against a backward pass that
/// takes milliseconds at best and five seconds at worst.
///
/// ## Skipping is what makes it a resume rather than a record
///
/// A journal that only recorded would still leave the next run paying for every candidate
/// again. What it is read back for is `novelty_search::Classify::settled`, which is the one
/// place a run can decline to spend a pass: the candidates are the nodes the search's own
/// walk reaches, not a list this could shorten from outside.
struct Journal {
    /// Where verdicts are appended, if anywhere.
    file: Option<std::io::BufWriter<std::fs::File>>,
    /// What an earlier run established, read back from that file.
    before: HashMap<DialogueNodeId, Option<bool>>,
    /// How many candidates there are to settle in all, for the progress line.
    total: usize,
    /// Settled by this run and by the one it continues, which is what a reader wants.
    settled: usize,
    unreachable: usize,
    undecided: usize,
    began: std::time::Instant,
    spoke: std::time::Instant,
    every: Option<std::time::Duration>,
}

/// How a verdict is spelled in the journal, and read back.
///
/// WORDS RATHER THAN A BOOLEAN, because the third case is the one that matters most here and
/// `true`/`false`/absent would spell "the pass ran out of room" as a missing line - which is
/// exactly how a candidate nobody has reached yet is spelled.
const VERDICTS: [(&str, Option<bool>); 3] = [
    ("unreachable", Some(false)),
    ("reachable", Some(true)),
    ("undecided", None),
];

fn spelled(verdict: Option<bool>) -> &'static str {
    VERDICTS
        .iter()
        .find(|(_, v)| *v == verdict)
        .expect("a known verdict")
        .0
}

fn parsed(word: &str) -> Option<Option<bool>> {
    VERDICTS
        .iter()
        .find(|(name, _)| *name == word)
        .map(|(_, verdict)| *verdict)
}

impl Journal {
    /// Nothing kept and nothing said, for a caller that only wants the answer.
    fn quiet() -> Self {
        Self {
            file: None,
            before: HashMap::new(),
            total: 0,
            settled: 0,
            unreachable: 0,
            undecided: 0,
            began: std::time::Instant::now(),
            spoke: std::time::Instant::now(),
            every: None,
        }
    }

    /// Kept at the path `CENSUS_JOURNAL` names, continuing whatever is already there.
    ///
    /// NO PATH IS NOT AN ERROR. A census asked for by hand has nowhere obvious to put one,
    /// and does not want the file; `tools/measure-census.sh` names one per group because it
    /// is the thing that knows where a run's folder is.
    fn of(total: usize) -> Self {
        let mut journal = Self {
            total,
            every: progress_every(),
            ..Self::quiet()
        };
        let Ok(path) = lookahead_engine::core::env::var("CENSUS_JOURNAL") else {
            return journal;
        };
        let path = std::path::PathBuf::from(path);

        if let Ok(text) = std::fs::read_to_string(&path) {
            for line in text.lines() {
                let Some((id, word)) = line.split_once('\t') else {
                    continue;
                };
                let Some((conversation, entry)) = id.split_once(':') else {
                    continue;
                };
                let (Ok(conversation), Ok(entry), Some(verdict)) =
                    (conversation.parse(), entry.parse(), parsed(word))
                else {
                    // A TORN LAST LINE IS EXPECTED rather than a corruption to complain
                    // about: the run that wrote it was killed, which is the case this file
                    // exists for. Dropping it costs the one candidate, asked again.
                    continue;
                };
                journal
                    .before
                    .insert(DialogueNodeId::new(conversation, entry), verdict);
            }
        }

        // COUNTED IN, so the progress line is about the GROUP rather than about this
        // process's share of it. A resumed run that reported only its own settled candidates
        // would appear to be starting over.
        journal.settled = journal.before.len();
        journal.unreachable = journal
            .before
            .values()
            .filter(|v| **v == Some(false))
            .count();
        journal.undecided = journal.before.values().filter(|v| v.is_none()).count();

        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            Ok(file) => journal.file = Some(std::io::BufWriter::new(file)),
            // LOUD, because a run that believes it is resumable and is not would find out
            // hours later, by having lost everything.
            Err(why) => panic!(
                "the census journal {} could not be opened: {why}",
                path.display()
            ),
        }

        if journal.settled > 0 {
            eprintln!(
                "{PROGRESS} resuming: {} of {total} candidate(s) already settled in {}",
                journal.settled,
                path.display(),
            );
        }
        journal
    }

    /// Writes one verdict down, and says where the group has got to if it is time to.
    fn record(&mut self, id: DialogueNodeId, verdict: Option<bool>) {
        use std::io::Write;

        self.settled += 1;
        match verdict {
            Some(false) => self.unreachable += 1,
            None => self.undecided += 1,
            Some(true) => {}
        }

        if let Some(file) = &mut self.file {
            // FLUSHED PER LINE. What is still in this buffer is exactly what a kill would
            // lose, and losing it is what the file is here to prevent.
            let _ = writeln!(
                file,
                "{}:{}\t{}",
                id.conversation_id,
                id.entry_id,
                spelled(verdict)
            );
            let _ = file.flush();
        }

        let Some(every) = self.every else { return };
        if self.spoke.elapsed() < every {
            return;
        }
        self.spoke = std::time::Instant::now();
        eprintln!(
            "{PROGRESS} {}/{} settled  {} unreachable  {} undecided  {}",
            self.settled,
            self.total,
            self.unreachable,
            self.undecided,
            mmss(self.began.elapsed()),
        );
    }
}

/// Which of the deepest entries the search provably cannot reach, and which it could not
/// settle either way.
///
/// Deepest first, stopping once `wanted` are in hand. A candidate whose pass does not settle
/// inside [`CLASSIFY_CAP`] is reported undecided rather than assumed either way - a backward
/// pass that ran out of budget has proved nothing, and treating that as unreachable would
/// build the profile out of the very cases it is meant to exclude.
///
/// BOTH LISTS ARE IDENTITIES, and the undecided one is why. A caller that knows only HOW
/// MANY were undecided cannot recover which candidates are reachable: the reachable ones are
/// everything asked about and not named unreachable, so an unnamed undecided candidate is
/// indistinguishable from a proved one. A count says the subtraction is wrong without saying
/// where, which makes the whole group's reachable set unusable rather than the few entries
/// that are genuinely open. de-x8ms.5.
///
/// RESUMABLE, AND IT NARRATES ITSELF, through [`Journal`] - which keeps every verdict as it
/// arrives, so an interrupted group costs the candidate in flight rather than the hours
/// spent on the ones before it. See there for why that is a file rather than a signal
/// handler, and de-e4mu for what it was like without one.
///
/// PUBLIC BECAUSE THE CENSUS SHARES IT. measurements/performance_matrix.rs pulls this file
/// in with `#[path]` and asks the same question over every group in the game (de-thlz.2), so
/// that the entries a census names unreachable and the entries a `deepest-unreach-N` profile
/// is built from are decided by one piece of code rather than two that could drift.
pub fn classify(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    deepest: &[DialogueNodeId],
    wanted: usize,
) -> (Vec<DialogueNodeId>, Vec<DialogueNodeId>) {
    // ONE APPARATUS FOR THE WHOLE GROUP, not one per candidate. de-x8ms.11.
    //
    // This used to call `reachable` per target, and each of those was a separate `answer`
    // on a thread of its own building a fresh diagram manager, a fresh guard compiler and a
    // fresh seed. On groups small enough that the pass itself is trivial that was 19 to 25
    // ms of rebuilding PER CANDIDATE; group 631 meant 2,845 managers to ask 2,845 questions
    // about one graph in one world.
    //
    // IT GOES THROUGH THE SAME SEARCH THE GAME USES rather than growing a loop of its own.
    // `novelty_search::classify_candidates` is `best_novelty` with one option: report every
    // candidate and do not stop at the first yes. So the guard reuse, the candidate
    // ordering and the `Known` narrowing are inherited rather than reimplemented, and a
    // change to any of them reaches the census automatically.
    let symbols = graph.symbols().clone();
    let layout = DataLayout::for_group(graph, world, COUNTER_CAP);

    // ONE THREAD FOR THE WHOLE GROUP, holding ONE manager, which is the arrangement
    // src/symbolic/isolated.rs measured cleanest of all - 0 deaths in 35 - and the one the
    // per-candidate version could not use.
    on_its_own_thread(|| {
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
        let seed = seed_of(graph, world, &vars).expect("room for a seed");

        // EVERY CANDIDATE IS THE QUARRY, because the question is which of them can be
        // reached rather than whether any can.
        let asking: HashSet<DialogueNodeId> = deepest.iter().copied().collect();
        let novelty = |id: DialogueNodeId| {
            if asking.contains(&id) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        };

        // SHARED, which the per-candidate version could not be: what a run over this group
        // establishes is handed to every pass instead of being rebuilt for each.
        let known = Known::of_from(graph, start).from(start, &seed);

        // WHAT AN EARLIER RUN OVER THIS GROUP ALREADY SETTLED, which this one neither asks
        // about again nor forgets: the verdicts come back in the lists below alongside the
        // ones taken here, so a resumed group's row is the row it would have had.
        let mut journal = Journal::of(deepest.len());
        // TAKEN OUT OF THE JOURNAL, so that the closure recording new verdicts and the one
        // reading old ones do not both need it. The journal's counts were seeded from this
        // when it was read, so it keeps reporting the whole group's progress without it.
        let already = std::mem::take(&mut journal.before);
        let settled_before = |verdict: Option<bool>| -> Vec<DialogueNodeId> {
            already
                .iter()
                .filter(|(id, was)| **was == verdict && asking.contains(id))
                .map(|(id, _)| *id)
                .collect()
        };
        let mut unreachable = settled_before(Some(false));
        let mut undecided = settled_before(None);
        {
            // ONLY WHAT WAS ASKED ABOUT. The search classifies every node its own walk
            // reaches, which is the candidates plus the start itself; a verdict about the
            // start is not an answer to any question here, and letting one through would
            // put an entry in a list whose every other member came from `deepest`.
            let mut record = |target: DialogueNodeId, verdict: Option<bool>| {
                if !asking.contains(&target) {
                    return Wants::More;
                }
                journal.record(target, verdict);
                match verdict {
                    Some(false) => unreachable.push(target),
                    Some(true) => {}
                    None => undecided.push(target),
                }

                // ENOUGH IS COUNTED IN FINDINGS, NOT IN QUESTIONS, which is why the cap
                // belongs to the caller. A group where nothing is unreachable is a group
                // this walks to the end, correctly, having found nothing; a ration on
                // questions would have stopped it after ten and reported the same nothing
                // as though it were an answer. de-nd9o.
                //
                // A RESUMED RUN COUNTS WHAT THE RUN BEFORE IT FOUND, since the journal's
                // verdicts are already in this list - so a group continued twice still
                // stops at `wanted` in total rather than at `wanted` per attempt.
                if unreachable.len() >= wanted {
                    Wants::Enough
                } else {
                    Wants::More
                }
            };
            let settled = |id: DialogueNodeId| already.contains_key(&id);
            let mut census = Classify {
                verdict: &mut record,
                settled: &settled,
            };

            classify_candidates(
                graph,
                start,
                StartBranch::Either,
                &seed,
                &mut compiler,
                world,
                COUNTER_CAP as u32,
                &novelty,
                &SearchBudget {
                    // THE WHOLE GROUP'S ALLOWANCE, not one candidate's: every candidate is
                    // asked about in this single call, and what bounds one of them is
                    // `each` below.
                    time: std::time::Duration::MAX,
                    each: BackwardBudget {
                        steps: usize::MAX,
                        time: CLASSIFY_CAP,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                Some(&known),
                &mut census,
            );
        }

        // BACK INTO DEEPEST-FIRST ORDER, which the search now shares but does not promise.
        //
        // A census asks for `Nearest::Last`, so its candidates arrive deepest-first within a
        // class and the cap above stops it holding the deepest `wanted` rather than the
        // shallowest. This sort is what the ORDER of the recorded list rests on: the search
        // groups by novelty class before distance, and a resumed run's journal verdicts are
        // merged in from a HashMap, so neither arrives in the order `candidates()` defines.
        //
        // Worth keeping rather than trusting the walk. Comparing group 436 against an
        // earlier census caught exactly this once already: same count, entirely different
        // list.
        let rank: std::collections::HashMap<DialogueNodeId, usize> = deepest
            .iter()
            .enumerate()
            .map(|(at, id)| (*id, at))
            .collect();
        unreachable.sort_by_key(|id| rank.get(id).copied().unwrap_or(usize::MAX));
        unreachable.truncate(wanted);

        // THE UNDECIDED LIST IS NOT TRUNCATED, and the asymmetry is deliberate. `wanted` is
        // an appetite for unreachable entries, applied to the REPORT rather than to the work
        // - every candidate is asked about either way - and the undecided ones are what
        // stands between the rest of the group and a status. Cutting that list to the same
        // length would hide open questions behind a number chosen for a different purpose.
        undecided.sort_by_key(|id| rank.get(id).copied().unwrap_or(usize::MAX));
        (unreachable, undecided)
    })
}

/// What the driver answered, and how long it took.
struct Answer {
    verdict: String,
    millis: u128,
    asked: String,
}

/// The backward driver over one profile, on its own thread.
///
/// `shared` decides whether the passes are told what earlier work over this group
/// established. Without it each candidate rebuilds the parent map and the iteration order
/// and recompiles every guard it touches; with it they are built once and handed on.
fn answer(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    world: &dyn ILookAheadWorld,
    unseen: &HashSet<DialogueNodeId>,
    shared: bool,
    cap: std::time::Duration,
) -> Answer {
    let symbols = graph.symbols().clone();

    // THE LAYOUT THE GAME USES, which this did not use until de-x8ms.4.
    //
    // It was `for_graph(graph, COUNTER_CAP, None, false).keeping_only_read(..)`, and the
    // `None` is the whole story: that argument is the MONEY CEILING, and for_group takes it
    // from the world (see the note at the top of src/workspace.rs). Without one, money is
    // unbounded, so a guard asking whether the player can afford something is satisfiable,
    // so entries behind it look REACHABLE when the world says they are not.
    //
    // MEASURED, on group 436 - money-gated content, which is why it showed there first.
    // Same graph, same world, same start, changing only this line:
    //
    //     for_graph(.., None, false).keeping_only_read(..)   4 of 30 candidates unreachable
    //     for_group(graph, world, ..)                        10+ of 30, including 436:14
    //
    // and 436:14 is the entry de-x8ms.4 was filed about, where this measurement said
    // reachable and all three of the matrix's engines said not-there. They were right.
    //
    // SO THE CENSUS WAS OVER-ESTIMATING REACHABILITY, and therefore UNDER-counting what no
    // path can reach. The entries it did name were sound - a for_graph unreachable is a
    // stronger claim and stays unreachable here - but it found too few of them.
    let layout = DataLayout::for_group(graph, world, COUNTER_CAP);

    // A THREAD PER SEARCH. See the note at the top: this is de-fpax's remedy, and without it
    // this measurement loses whole groups partway through.
    on_its_own_thread(|| {
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::over_a_group());
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(world)
            .with_constant_clock(DataLayout::group_passes_time(graph));
        let seed = seed_of(graph, world, &vars).expect("room for a seed");

        let novelty = |id: DialogueNodeId| {
            if unseen.contains(&id) {
                Novelty::UnseenAnyGame
            } else {
                Novelty::SeenThisGame
            }
        };

        let known = shared.then(|| Known::of_from(graph, start).from(start, &seed));

        let began = std::time::Instant::now();
        let found = best_novelty(
            graph,
            start,
            StartBranch::Either,
            &seed,
            &mut compiler,
            world,
            COUNTER_CAP as u32,
            &novelty,
            &SearchBudget {
                time: cap,
                each: BackwardBudget {
                    steps: usize::MAX,
                    time: cap,
                    ..Default::default()
                },
                ..Default::default()
            },
            known.as_ref(),
        );

        Answer {
            verdict: match found.stopped_by {
                StoppedBy::Nothing => format!("{:?}", found.best),
                _ => "Incomplete".to_string(),
            },
            millis: began.elapsed().as_millis(),
            asked: found.targets_asked.to_string(),
        }
    })
}
