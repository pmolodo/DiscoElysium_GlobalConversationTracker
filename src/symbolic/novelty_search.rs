// SPDX-License-Identifier: MIT
//! The question the look-ahead actually asks, answered one target at a time.
//!
//! What the mod asks for is the best novelty among the entries a start can reach, and
//! nothing beyond the best there is. So the answer is a MAXIMUM over an ordered enum, and
//! the way to compute a maximum is not to compute the set it is a maximum of.
//!
//! This asks [`Backward`] about one candidate at a time and stops at the first one that
//! can be reached.
//!
//! ## Class before distance
//!
//! The obvious order is nearest first, and it is wrong. Proving a near `UnseenThisGame`
//! entry reachable says nothing about whether a far `UnseenAnyGame` one is, and the far
//! one is the answer if it is - `best` is a maximum, not a first sighting.
//!
//! So candidates are grouped by novelty class, best class first, and only WITHIN a class
//! sorted by how far away they are. The first candidate proved reachable in a class ends
//! the search, because every better class has already been refused entirely. A class every
//! candidate of which is refused drops to the next one down.
//!
//! Distance is the link distance, guards ignored. It is a heuristic about which question
//! is cheap to answer, not a claim about reachability, so it can be as rough as it likes:
//! a near entry has a shorter chain of guards in front of it and a smaller backward
//! fixed point, and that is the whole of the reasoning.
//!
//! ## What it costs when the answer is no
//!
//! One fixed point per candidate. That is what [`Budget`] exists for: a group with a long
//! candidate list, every one of them unreachable, has to pay for every refusal separately,
//! and it is the shape where this costs most. `measurements/performance_matrix.rs`'s
//! percentage profiles are where that is measured rather than assumed.

use std::collections::{HashMap, HashSet, VecDeque};

use oxidd::BooleanFunction;
use oxidd::bdd::BDDFunction;

use crate::core::types::{DialogueNodeId, Novelty, StartBranch};
use crate::graph::graph::LookAheadGraph;
use crate::symbolic::backward::{Backward, SettledPass};
use crate::symbolic::dominators::Dominators;
use crate::symbolic::guard_formula::GuardCompiler;
use crate::symbolic::known::Known;
use crate::symbolic::reachability::{Reachability, never_displays};
use crate::world::world::ILookAheadWorld;

/// Every novelty class better than "seen", best first.
///
/// `SeenThisGame` is the floor `evaluate` starts from and never an improvement on itself,
/// so it is not a candidate class - an entry carrying it is not worth asking about.
const CLASSES: [Novelty; 2] = [Novelty::UnseenAnyGame, Novelty::UnseenThisGame];

/// When to stop asking.
pub struct Budget {
    /// How long to keep asking.
    pub time: std::time::Duration,
    /// The budget each individual backward pass runs under.
    pub each: crate::symbolic::backward::Budget,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            time: std::time::Duration::from_secs(5),
            each: crate::symbolic::backward::Budget::default(),
        }
    }
}

/// Why a search stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoppedBy {
    /// Every candidate was asked about, so the answer is final.
    Nothing,
    /// A census had all the findings it came for, so the answer is a lower bound.
    ///
    /// The cap counts candidates PROVED UNREACHABLE rather than candidates asked about -
    /// see [`Classify::verdict`]. It is the caller's appetite running out rather than the
    /// search failing, and it leaves a caveat behind: an unasked candidate might have
    /// carried a better class.
    Targets,
    /// The time budget ran out.
    Time,
    /// A backward pass could not finish - out of nodes, or out of its own budget.
    Incomplete,
}

/// What the search found.
#[derive(Debug, Clone)]
pub struct NoveltyAnswer {
    /// The best novelty reachable beyond the start.
    ///
    /// A LOWER BOUND when [`Self::stopped_by`] is anything but `Nothing`: the classes
    /// already refused were refused completely, but an unasked candidate might have
    /// carried a better one.
    pub best: Novelty,
    /// The entry that proved it, when something did.
    ///
    /// Worth returning rather than throwing away. A marker with a reason behind it can be
    /// explained, and a disagreement with the search can be investigated from the entry
    /// both engines disagree about rather than from the whole group.
    pub witness: Option<DialogueNodeId>,
    /// How many candidates were asked about.
    pub targets_asked: usize,
    /// How many candidates there were.
    pub candidates: usize,
    pub stopped_by: StoppedBy,
    /// Whether the pass that failed to settle failed by running out of DIAGRAM NODES.
    ///
    /// [`StoppedBy::Incomplete`] covers both ways a pass can fail to settle, and they are
    /// not the same finding: a pass that spent every node it was allowed is a result about
    /// the representation, where one that ran out of steps or seconds is a result about
    /// the clock. A measurement that reported them alike would blame the budget for what
    /// the ration did, which is exactly the mistake de-e33h was raised for.
    pub out_of_nodes: bool,
    /// The entry at which a pass MET what an earlier search already knew, when one did.
    ///
    /// Set only when the meet is what answered the question, so it is the measurement of
    /// whether sharing paid: an answer with this set is one no fixed point had to finish.
    pub met_at: Option<DialogueNodeId>,
    pub elapsed: std::time::Duration,
}

/// The candidates, in the order they should be asked about.
///
/// Non-group entries only: a group is expanded in place and the game never writes its
/// SimStatus, so every group in the database reads as never displayed and treating one as
/// a candidate would make every search succeed instantly on a lie. `evaluate` skips them
/// for the same reason.
///
/// The start is a candidate, at distance zero - see [`link_distances_from`] for why it
/// stopped depending on a link leading back to it.
pub fn candidates<F>(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    novelty: &F,
) -> Vec<DialogueNodeId>
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    candidates_from(graph, &[start], novelty, Nearest::First)
}

/// Which end of a class a search takes first, among candidates that are equally novel.
///
/// THE TWO CALLERS WANT OPPOSITE ENDS, and neither is wrong.
///
/// A look-ahead wants [`Nearest::First`]: it is hunting for ANY reachable unseen entry, a
/// near one is proved soonest, and the first yes ends the search. Starting at the far end
/// would pay for the expensive candidates before the cheap ones on every question.
///
/// A census wants [`Nearest::Last`], because what it records is "the deepest N entries no
/// path can reach" and it stops once it has N. Walked nearest-first it would stop holding
/// the SHALLOWEST N instead - the same count, an entirely different list, and precisely the
/// wrong end of the group for a profile whose whole purpose is the hard case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nearest {
    /// Closest to the start first.
    First,
    /// Furthest from the start first.
    Last,
}

/// The same, from several starts - what one outcome of a rolled check reaches.
pub fn candidates_from<F>(
    graph: &LookAheadGraph,
    starts: &[DialogueNodeId],
    novelty: &F,
    nearest: Nearest,
) -> Vec<DialogueNodeId>
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    let distances = link_distances_from(graph, starts);

    // No filtering of the starts here either: they are recorded at distance zero,
    // so it sorts first within its class - which is where a candidate that needs no walking
    // at all belongs.
    let mut worth: Vec<(usize, usize, i32, i32, DialogueNodeId)> = distances
        .iter()
        .filter_map(|(id, distance)| {
            let node = graph.get(*id)?;
            if node.is_group {
                return None;
            }
            let class = novelty(*id);
            let rank = CLASSES.iter().position(|c| *c == class)?;
            Some((rank, *distance, id.conversation_id, id.entry_id, *id))
        })
        .collect();

    // Class first, distance second, and the identifiers last so the order is total: two
    // candidates at the same distance in the same class must still be asked about in the
    // same order on every run, or a measurement is not repeatable.
    //
    // CLASS ALWAYS OUTRANKS DISTANCE, whichever end `nearest` takes. A more novel candidate
    // is a better answer than a nearer or a deeper one, and reversing the distance must not
    // quietly reverse that too.
    worth.sort_by(|a, b| {
        let (rank, distance, conversation, entry, _) = a;
        let (other_rank, other_distance, other_conversation, other_entry, _) = b;
        rank.cmp(other_rank)
            .then(match nearest {
                Nearest::First => distance.cmp(other_distance),
                Nearest::Last => other_distance.cmp(distance),
            })
            .then(conversation.cmp(other_conversation))
            .then(entry.cmp(other_entry))
    });
    worth.into_iter().map(|(_, _, _, _, id)| id).collect()
}

/// Where a search begins: one or more entries, and the states it holds arriving at them.
///
/// TWO SHAPES, AND THEY ARE THE SAME QUESTION ASKED FROM DIFFERENT PLACES.
///
/// An ordinary search begins at its start, holding the world's seed - what it holds
/// ARRIVING there, before that entry's own guard, cost or actions. One entry, one set.
///
/// A search about one outcome of a rolled start begins at the start's CHILDREN, holding
/// what entering the start by that outcome left. It cannot begin at the check itself: a
/// backward set there answers about either roll, since the pre-image unions both ways in,
/// and this is exactly the question that needs them apart.
pub struct Where {
    at: Vec<DialogueNodeId>,
    holding: BDDFunction,
    /// Whether the manager filled while working out where this search begins.
    ///
    /// A caller that sees it MUST NOT read the rest: `holding` is then the empty set for
    /// want of nodes rather than because the outcome opens nothing, and the two look
    /// alike from here. Asking candidates from an empty position refuses every one of
    /// them on no evidence and settles, which is a wrong answer where the honest one is
    /// "the representation did not fit".
    out_of_nodes: bool,
}

/// A lower bound on choices between this option and each target. Guards can only
/// remove these routes; cut options cannot be entered, even after a loop.
pub fn choice_bounds(
    graph: &LookAheadGraph,
    position: &super::backward::Position,
    cut: &HashSet<DialogueNodeId>,
) -> HashMap<DialogueNodeId, usize> {
    let mut distances = HashMap::new();
    let mut pending = VecDeque::new();
    for &id in &position.entries {
        if !cut.contains(&id) {
            distances.insert(id, 0usize);
            pending.push_back((id, 0usize));
        }
    }
    while let Some((id, distance)) = pending.pop_front() {
        if distances.get(&id) != Some(&distance) {
            continue;
        }
        let Some(node) = graph.get(id) else { continue };
        let cost = usize::from(node.choice && id != position.option);
        for &child in &node.links {
            if cut.contains(&child) || graph.get(child).is_none() {
                continue;
            }
            let candidate = distance + cost;
            if distances
                .get(&child)
                .is_none_or(|previous| candidate < *previous)
            {
                distances.insert(child, candidate);
                if cost == 0 {
                    pending.push_front((child, candidate));
                } else {
                    pending.push_back((child, candidate));
                }
            }
        }
    }
    distances
}

impl Where {
    /// The states and entries from which this outcome is searched.
    pub fn position(&self, option: DialogueNodeId) -> super::backward::Position {
        super::backward::Position {
            option,
            entries: self.at.clone(),
            holding: self.holding.clone(),
        }
    }
    /// The starting position for this outcome of this start.
    #[allow(clippy::too_many_arguments)]
    pub fn of<'a>(
        graph: &LookAheadGraph,
        start: DialogueNodeId,
        branch: StartBranch,
        seed: &BDDFunction,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
    ) -> Self {
        if branch == StartBranch::Either {
            return Self {
                at: vec![start],
                holding: seed.clone(),
                out_of_nodes: false,
            };
        }

        let Some(holding) =
            Reachability::entry_states(graph, start, branch, seed, compiler, world, counter_cap)
        else {
            // NO ENTRIES, so nothing can be asked from here even by a caller that ignores
            // the flag. The set is the seed only because the field needs one; it is not an
            // answer, and `out_of_nodes` is what says so.
            return Self {
                at: Vec::new(),
                holding: seed.clone(),
                out_of_nodes: true,
            };
        };
        let at = graph
            .get(start)
            .map(|node| node.links.clone())
            .unwrap_or_default();

        Self {
            at,
            holding,
            out_of_nodes: false,
        }
    }

    /// Whether working out where this search begins ran the manager out of nodes.
    ///
    /// Nothing else here is worth reading once this is set - the position is empty for
    /// want of nodes, not because the outcome opens nothing.
    pub fn out_of_nodes(&self) -> bool {
        self.out_of_nodes
    }

    /// The entries this search starts at, which are what candidates are measured from.
    fn nodes(&self) -> Vec<DialogueNodeId> {
        self.at.clone()
    }

    /// The entries this outcome actually OPENS, guards and costs considered.
    ///
    /// WHAT THE BASELINE IS MADE OF. A branch's answer is "does this outcome lead anywhere
    /// better than where it LANDS", and where it lands is this: the first non-group entries
    /// that can be entered holding what the outcome left. A group is walked through rather
    /// than to, as everywhere else - it is expanded in place and never scored.
    ///
    /// GUARDS ARE HONOURED HERE, unlike in `LookAheadGraph::best_linked_class`. A cheap
    /// over-approximation is right when the question is whether to spend a search; it is
    /// wrong for a baseline, where naming a destination nothing can reach would raise the
    /// bar a real search has to clear and cost a marker.
    ///
    /// RUNNING OUT OF NODES EMPTIES THE ANSWER AND SETS [`Self::out_of_nodes`], because a
    /// short list here is not a smaller baseline - it is no baseline. A destination the
    /// walk never reached for want of room would lower the bar a real search has to clear,
    /// which is the same cost as naming one nothing can reach, in the other direction.
    pub fn destinations<'a>(
        &mut self,
        graph: &LookAheadGraph,
        compiler: &mut GuardCompiler<'a>,
        world: &dyn ILookAheadWorld,
        counter_cap: u32,
    ) -> Vec<DialogueNodeId> {
        let mut found = Vec::new();
        let mut seen: Vec<DialogueNodeId> = Vec::new();
        let mut pending: VecDeque<(DialogueNodeId, BDDFunction)> = self
            .at
            .iter()
            .map(|id| (*id, self.holding.clone()))
            .collect();

        while let Some((id, arriving)) = pending.pop_front() {
            let Some(node) = graph.get(id) else { continue };
            let Some(entered) = Reachability::entry_states(
                graph,
                id,
                StartBranch::Either,
                &arriving,
                compiler,
                world,
                counter_cap,
            ) else {
                self.out_of_nodes = true;
                return Vec::new();
            };
            if !entered.satisfiable() {
                continue;
            }

            // A CHECK THIS SHEET FAILS IS WALKED THROUGH, exactly as a group is. The
            // outcome does not LAND on a line the player will never read, and naming one
            // here would raise the bar a real search has to clear - the same cost as
            // naming a destination nothing can reach.
            if !node.is_group && !never_displays(node, world) {
                if !found.contains(&id) {
                    found.push(id);
                }
                continue;
            }

            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            for child in &node.links {
                pending.push_back((*child, entered.clone()));
            }
        }

        found
    }

    /// Whether a settled backward pass says the target is reachable from here.
    ///
    /// OVER THE TRAIT, so that a pass this request ran and one `memo` kept from an earlier
    /// one are read by the same line. The sets are the same sets; only who paid for them
    /// differs, and this is the place where that must not matter.
    fn reaches<P: SettledPass + ?Sized>(&self, pass: &P) -> bool {
        self.at
            .iter()
            .any(|id| pass.reachable_from(*id, &self.holding))
    }

    /// What an earlier search may treat as already known, for the meet.
    ///
    /// The pairs are (entry, states arriving there), which is what [`Known::from`] takes.
    /// For an outcome that is its destinations, NOT the check: telling the backward driver
    /// that the check's pre-entry states are known would let a meet there prove a target
    /// reachable by the other roll.
    pub fn known_pairs(&self) -> Vec<(DialogueNodeId, &BDDFunction)> {
        self.at.iter().map(|id| (*id, &self.holding)).collect()
    }
}

/// The best novelty reachable beyond `start`, by asking about candidates in turn.
///
/// ONE OUTCOME OF A ROLLED START IS ASKED ABOUT AT ITS DESTINATIONS, not at the start.
/// A backward set says "arriving HERE, the target is reachable", and a check's set unions
/// both ways in - so asking it about the check answers about either roll, which is not the
/// question. Asked instead about the check's children, holding what entering by this
/// outcome left, it answers about one. See [`Where::of`].
#[allow(clippy::too_many_arguments)]
pub fn best_novelty<'a, F>(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    branch: StartBranch,
    seed: &BDDFunction,
    compiler: &mut GuardCompiler<'a>,
    world: &dyn ILookAheadWorld,
    counter_cap: u32,
    novelty: F,
    budget: &Budget,
    known: Option<&Known>,
) -> NoveltyAnswer
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    search(
        graph,
        start,
        branch,
        seed,
        compiler,
        world,
        counter_cap,
        novelty,
        budget,
        known,
        None,
    )
}

/// The same search, asked WHICH candidates are reachable rather than WHETHER any is.
///
/// ## Why this is an option on the same loop rather than a loop of its own
///
/// A census wants a verdict per entry, and the obvious way to get one is to ask about each
/// candidate separately - which is what `measurements/symbolic_answers.rs` did, and it cost
/// a fresh diagram manager, a fresh guard compiler and a fresh seed PER CANDIDATE. Measured
/// on groups small enough that the pass itself is trivial, that was 19 to 25 milliseconds of
/// rebuilding each time; over the 73,958 candidates of a whole-game census, about
/// twenty-five minutes of it.
///
/// Everything needed to avoid that already lives here. This loop builds `Where::of` once,
/// orders the candidates once, and hands the SAME compiler and `Known` to every
/// `Backward::reaching_knowing` - so a caller that wants every candidate classified should
/// arrive here once with all of them, not once per candidate. de-x8ms.11.
///
/// ## What the option changes, which is three things and not one
///
/// `verdict` is called with each candidate and what its pass established:
///
/// - `Some(true)` - reachable. The ordinary search STOPS here, because the candidates are
///   ordered best-class-first so the first yes is the answer. A census carries on.
/// - `Some(false)` - a settled pass that proved it unreachable.
/// - `None` - the pass did not settle, so nothing is established either way. The ordinary
///   search stops here too, and honestly: it cannot report "nothing reachable" on the
///   strength of a pass that ran out. A census records the one candidate as undecided and
///   goes on to the next, because one unsettled pass says nothing about the others.
///
/// The returned `NoveltyAnswer` still describes the SEARCH - `best` and `witness` are the
/// first and therefore best yes, as always - so a caller gets both readings from one run.
#[allow(clippy::too_many_arguments)]
pub fn classify_candidates<'a, 'c, F>(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    branch: StartBranch,
    seed: &BDDFunction,
    compiler: &mut GuardCompiler<'a>,
    world: &dyn ILookAheadWorld,
    counter_cap: u32,
    novelty: F,
    budget: &Budget,
    known: Option<&Known>,
    census: &mut Classify<'c>,
) -> NoveltyAnswer
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    search(
        graph,
        start,
        branch,
        seed,
        compiler,
        world,
        counter_cap,
        novelty,
        budget,
        known,
        Some(census),
    )
}

/// What a census asks of the search that an ordinary look-ahead does not.
///
/// TWO THINGS, and the second is what makes a long classification survivable. `verdict`
/// collects what each candidate's pass established; `settled` says which candidates a
/// previous run already answered, so that this one does not pay for them again.
///
/// ## Why skipping has to happen HERE and not in the caller
///
/// The expensive part of a classification is one bounded backward pass per candidate, and
/// the candidates are the nodes the search's own walk reaches - not a list the caller hands
/// in. A caller that "asks about fewer" by narrowing its novelty function changes which
/// answers it is told about and nothing else: every candidate still costs its pass. So the
/// only place a resumed run can decline to spend that pass is inside the loop that spends
/// it.
///
/// A SKIPPED CANDIDATE IS NOT AN ANSWERED ONE. Nothing is reported for it and it does not
/// count towards `targets_asked`, because the run that settled it is the run that knows what
/// it settled; this one is being told to keep its hands off. The caller holds those verdicts
/// already, which is how it knew to skip.
///
/// SO THE RETURNED [`NoveltyAnswer`] DESCRIBES THIS RUN'S QUESTIONS, not the classification
/// as a whole: `best` and `witness` are the best yes among the candidates actually asked
/// about, and a resumed run may have skipped a better one. A caller that skips is a caller
/// assembling the whole answer itself, and should read the verdicts rather than this.
pub struct Classify<'a> {
    /// Told about each candidate this run settles, and what it settled - and asked, in
    /// return, whether to go on.
    ///
    /// THE CENSUS'S OWN CAP LIVES IN THAT ANSWER, and it has to, because it counts
    /// FINDINGS where anything this loop could count would be QUESTIONS. A census wants ten
    /// candidates proved unreachable and does not care how many it had to ask about. The two
    /// come apart hardest exactly where it matters: in a group where everything is
    /// reachable, a ceiling of ten questions stops after ten having found nothing, and a
    /// ceiling of ten findings correctly walks the whole group.
    ///
    /// So the count belongs to the caller, which is the only thing that knows what it is
    /// counting, and the loop asks rather than deciding. Answering [`Wants::Enough`] stops
    /// the scan with [`StoppedBy::Targets`] - an allowance on candidates ran out, which is
    /// what happened.
    pub verdict: &'a mut dyn FnMut(DialogueNodeId, Option<bool>) -> Wants,
    /// Whether an earlier run already answered for this candidate.
    pub settled: &'a dyn Fn(DialogueNodeId) -> bool,
}

/// Whether a census wants to go on after the verdict it has just been given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wants {
    /// Keep asking.
    More,
    /// The caller has what it came for, so the scan stops here.
    Enough,
}

/// The loop both entry points share. `every` is the census option - see
/// [`classify_candidates`].
#[allow(clippy::too_many_arguments)]
fn search<'a, 'c, F>(
    graph: &LookAheadGraph,
    start: DialogueNodeId,
    branch: StartBranch,
    seed: &BDDFunction,
    compiler: &mut GuardCompiler<'a>,
    world: &dyn ILookAheadWorld,
    counter_cap: u32,
    novelty: F,
    budget: &Budget,
    known: Option<&Known>,
    mut every: Option<&mut Classify<'c>>,
) -> NoveltyAnswer
where
    F: Fn(DialogueNodeId) -> Novelty,
{
    let began = std::time::Instant::now();
    let from = Where::of(graph, start, branch, seed, compiler, world, counter_cap);
    // A CENSUS WALKS FROM THE FAR END. See [`Nearest`]: it stops once it has the findings it
    // came for, and what it came for is the DEEPEST of them, so taking the near end first
    // would leave it holding the shallowest instead. A look-ahead has no cap on findings and
    // wants its cheapest proof first.
    let nearest = if every.is_some() {
        Nearest::Last
    } else {
        Nearest::First
    };
    let starts = from.nodes();
    let ordered = candidates_from(graph, &starts, &novelty, nearest);

    // WHAT A SETTLED REFUSAL ALREADY ANSWERED, and the tree that turns one refusal into
    // many. See [`crate::symbolic::dominators`] for why the implication holds and why the
    // link graph is the safe one to take it over.
    //
    // BUILT ON THE FIRST REFUSAL RATHER THAN UP FRONT, which is what keeps it free where it
    // cannot pay. Two thirds of the rows of a whole-game run ask about exactly ONE candidate
    // (de-kqgq), and a search whose first candidate is proved REACHABLE stops there - in
    // both cases a tree built at the top would be built and dropped unused. Deferring it to
    // the first settled refusal means it exists exactly when there is something to apply it
    // to, and the `dominance.is_some()` test below is the same condition spelled once.
    let mut refused: HashSet<DialogueNodeId> = HashSet::new();
    let mut dominance: Option<Dominators> = None;

    let mut answer = NoveltyAnswer {
        best: Novelty::SeenThisGame,
        witness: None,
        targets_asked: 0,
        candidates: ordered.len(),
        stopped_by: StoppedBy::Nothing,
        out_of_nodes: false,
        met_at: None,
        elapsed: std::time::Duration::ZERO,
    };

    // THE POSITION ITSELF COULD NOT BE BUILT. Entering the start to find what this outcome
    // hands on filled the manager, so `from` holds the empty set for want of nodes. Asking
    // candidates from there would refuse every one of them without evidence and settle, so
    // the honest answer is the one the pass would give: nothing established, and the reason.
    if from.out_of_nodes() {
        answer.stopped_by = StoppedBy::Incomplete;
        answer.out_of_nodes = true;
        answer.elapsed = began.elapsed();
        return answer;
    }

    for target in ordered {
        // ALREADY ANSWERED BY THE RUN THIS ONE IS CONTINUING, so it is not asked again -
        // before the budget checks, because a candidate that costs nothing should not be
        // able to end the search by exhausting a ration it never spends.
        if every
            .as_ref()
            .is_some_and(|census| (census.settled)(target))
        {
            continue;
        }

        // ALREADY REFUSED BY SOMETHING ABOVE IT, so there is nothing to run. Every path
        // from the start to this target passes through an entry a completed fixed point
        // proved unreachable, so this one is unreachable too - de-kqgq, which measured 87.7
        // per cent of a whole-game run's candidates to be in this position.
        //
        // BEFORE THE CLOCK, for the reason the check above is: a candidate that costs
        // nothing must not be able to end the search by exhausting a ration it never spends.
        //
        // IT IS A FINDING, NOT A SKIP, which is why a census is told. `settled` above means
        // an earlier RUN already answered and the caller has the answer; this means THIS run
        // has just answered it, and dropping it would lose a verdict the census came for.
        if let Some(doms) = &dominance {
            if doms.above(target).any(|above| refused.contains(&above)) {
                if let Some(census) = &mut every {
                    if (census.verdict)(target, Some(false)) == Wants::Enough {
                        answer.stopped_by = StoppedBy::Targets;
                        break;
                    }
                }
                continue;
            }
        }

        if began.elapsed() >= budget.time {
            answer.stopped_by = StoppedBy::Time;
            break;
        }

        answer.targets_asked += 1;

        // THE CANDIDATE MAY NOT OUTLIVE THE ATTEMPT. de-cluo.
        //
        // The check above only tests that the clock has not already run out. Handing the
        // candidate a full `each` on top of that would let a pass beginning a millisecond
        // under `budget.time` return a whole `each` past it - and since the shipped `each`
        // IS the player's number, that would be double what they were promised.
        //
        // `backward::Budget.time` IS CHECKED INSIDE THE FIXED POINT rather than only
        // between passes, so narrowing it to what is left is what turns the wall from
        // advisory into binding. No new checking machinery is needed, only the arithmetic.
        let ran = {
            let left = budget.time.saturating_sub(began.elapsed());
            let mut each = budget.each.clone();
            each.time = each.time.min(left);
            Backward::reaching_knowing(graph, target, compiler, world, counter_cap, &each, known)
        };

        let pass: &dyn SettledPass = &ran;
        let met = ran.stats().met_at;
        let settled = ran.stats().reached_fixed_point;

        // TWO WAYS TO PROVE IT, and the cheap one is asked first. A meet is a proof that
        // stopped the pass early - a state an earlier search can hold at some entry is one
        // this pass has shown reaches the target - so the fixed point is deliberately
        // incomplete and `reachable_from` would be asking the wrong question of it.
        // WHAT THIS CANDIDATE'S PASS ESTABLISHED, decided here and reported once below.
        // `Some(true)` reachable, `Some(false)` proved unreachable, `None` not settled.
        //
        // The ordinary search has no use for the value - it stops on the first two and only
        // carries on past a refusal - so each of its cases breaks out rather than falling
        // through to a report nobody reads.
        let established = if met.is_some() || from.reaches(pass) {
            // The best class is asked about first and exhausted before the next one is
            // begun, so the first candidate that answers yes carries the answer - which is
            // why it is only recorded once even when the census keeps going.
            if answer.witness.is_none() {
                answer.best = novelty(target);
                answer.witness = Some(target);
                answer.met_at = met;
            }
            if every.is_none() {
                break;
            }
            Some(true)
        } else if !settled {
            // A pass that did not settle proves nothing by saying no: it may simply not
            // have got far enough. Say so rather than counting it as a refusal.
            //
            // A CENSUS RECORDS THE ONE AND CARRIES ON. An unsettled pass says nothing about
            // this candidate and nothing about the next, so stopping would throw away every
            // remaining answer for the sake of one it could not give.
            if every.is_none() {
                answer.stopped_by = StoppedBy::Incomplete;
                answer.out_of_nodes = ran.stats().out_of_memory;
                break;
            }
            None
        } else {
            // Settled, and it did not reach: proved unreachable.
            //
            // ONLY THIS BRANCH MAY BE REMEMBERED. A pass that met an earlier search, or ran
            // out of budget, or out of diagram nodes, holds a SUBSET of what it would have
            // held - it proves what it found and nothing about what it did not - so
            // recording one here would turn a budget into an answer, and the entries it
            // then refused for free would be refused on no evidence at all.
            refused.insert(target);
            if dominance.is_none() && answer.candidates > 1 {
                dominance = Some(Dominators::of(graph, &starts));
            }
            Some(false)
        };

        if let Some(census) = &mut every {
            // THE CALLER'S CAP, ASKED FOR RATHER THAN INFERRED. See [`Classify::verdict`]:
            // what a census counts is findings, not questions, and this loop cannot count
            // findings on its behalf without knowing which of them it came for.
            if (census.verdict)(target, established) == Wants::Enough {
                answer.stopped_by = StoppedBy::Targets;
                break;
            }
        }
    }

    answer.elapsed = began.elapsed();
    answer
}

/// How far each entry is from the starts, following links and ignoring guards.
///
/// SEVERAL STARTS, because one outcome of a rolled check has several: the search is about
/// what that outcome opens, so the entries worth asking about are the ones ITS half of the
/// graph reaches. Measuring from the check instead would offer the other outcome's entries
/// as candidates, and every one of them would cost a backward pass to refuse.
fn link_distances_from(
    graph: &LookAheadGraph,
    starts: &[DialogueNodeId],
) -> HashMap<DialogueNodeId, usize> {
    let mut distance: HashMap<DialogueNodeId, usize> = HashMap::new();
    // THE START IS AT DISTANCE ZERO FROM ITSELF, and a candidate like anything else.
    //
    // It used to be recorded only when a link led back to it, on the reading that a search
    // reports what it arrives at rather than where it began. That reading does not survive
    // a rolled check: there the baseline is where an OUTCOME lands, which sits below the
    // check's own class whenever the outcome opens something already read, and the check
    // entry then outranks the baseline without any walking at all. So the start is a
    // result like any other, here and in `LookAheadGraph::best_linked_class` - one rule,
    // and no search with a special case for where it began.
    let mut queue = VecDeque::new();
    for start in starts {
        if distance.insert(*start, 0).is_none() {
            queue.push_back((*start, 0usize));
        }
    }

    while let Some((id, here)) = queue.pop_front() {
        let Some(node) = graph.get(id) else { continue };
        for &child in &node.links {
            if graph.get(child).is_none() || distance.contains_key(&child) {
                continue;
            }
            distance.insert(child, here + 1);
            queue.push_back((child, here + 1));
        }
    }

    distance
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::budget::DiagramBudget;

    use std::collections::HashSet;

    use crate::core::guard_value::GuardValue;
    use crate::core::types::DialogueCheckKind;
    use crate::symbolic::data_layout::DataLayout;
    use crate::symbolic::reachability::seed_of;
    use crate::symbolic::vars::DataVars;
    use crate::test_graph::{Entry, GraphBuilder, node};
    use crate::world::test_world::TestWorld;

    const CAP: i32 = 16;

    /// The novelty function the engine's own tests use: everything named is unseen.
    fn novel(unseen: &[i32], best: Novelty) -> impl Fn(DialogueNodeId) -> Novelty + '_ {
        let set: HashSet<i32> = unseen.iter().copied().collect();
        move |id| {
            if set.contains(&id.entry_id) {
                best
            } else {
                Novelty::SeenThisGame
            }
        }
    }

    fn search<F>(graph: &LookAheadGraph, world: &TestWorld, novelty: F) -> NoveltyAnswer
    where
        F: Fn(DialogueNodeId) -> Novelty,
    {
        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(world);
        let seed = seed_of(graph, world, &vars).expect("room for a seed");

        best_novelty(
            graph,
            node(0),
            StartBranch::Either,
            &seed,
            &mut compiler,
            world,
            CAP as u32,
            novelty,
            &Budget::default(),
            None,
        )
    }

    /// The same search, about ONE OUTCOME of a rolled start.
    fn search_branch<F>(
        graph: &LookAheadGraph,
        world: &TestWorld,
        branch: StartBranch,
        novelty: F,
    ) -> NoveltyAnswer
    where
        F: Fn(DialogueNodeId) -> Novelty,
    {
        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let mut compiler = GuardCompiler::new(&vars).with_world(world);
        let seed = seed_of(graph, world, &vars).expect("room for a seed");

        best_novelty(
            graph,
            node(0),
            branch,
            &seed,
            &mut compiler,
            world,
            CAP as u32,
            novelty,
            &Budget::default(),
            None,
        )
    }

    /// 0 is a white check: passing opens 1 with 2 beyond it, failing opens 3.
    fn rolled_check() -> LookAheadGraph {
        GraphBuilder::new()
            .add(
                Entry::new(0)
                    .kind(DialogueCheckKind::White)
                    .flag("roll")
                    .links(&[1, 3]),
            )
            .add(
                Entry::new(1)
                    .guard(r#"Variable["roll"] == true"#)
                    .links(&[2]),
            )
            .add(Entry::new(2))
            .add(Entry::new(3).guard(r#"Variable["roll"] == false"#))
            .build()
    }

    /// AN OUTCOME IS ASKED ABOUT AT ITS DESTINATIONS, so it answers about its own half.
    ///
    /// The unseen entry lies past what PASSING opens. Failing must not find it, and would
    /// if the question were asked at the check - whose backward set unions both rolls.
    #[test]
    fn only_the_passing_outcome_reaches_what_passing_opens() {
        let graph = rolled_check();
        let world = TestWorld::new().set_variable("roll", GuardValue::from_boolean(false));
        let unseen = novel(&[2], Novelty::UnseenAnyGame);

        let passing = search_branch(&graph, &world, StartBranch::Pass, &unseen);
        assert_eq!(passing.best, Novelty::UnseenAnyGame);
        assert_eq!(passing.witness, Some(node(2)));

        let failing = search_branch(&graph, &world, StartBranch::Fail, &unseen);
        assert_eq!(
            failing.best,
            Novelty::SeenThisGame,
            "2 is behind the pass flag, and failing does not set it",
        );
        assert_eq!(failing.witness, None);
    }

    /// And the other way round, so neither outcome is answering for both.
    #[test]
    fn only_the_failing_outcome_reaches_what_failing_opens() {
        let graph = rolled_check();
        let world = TestWorld::new().set_variable("roll", GuardValue::from_boolean(false));
        let unseen = novel(&[3], Novelty::UnseenAnyGame);

        let failing = search_branch(&graph, &world, StartBranch::Fail, &unseen);
        assert_eq!(failing.best, Novelty::UnseenAnyGame);
        assert_eq!(failing.witness, Some(node(3)));

        let passing = search_branch(&graph, &world, StartBranch::Pass, &unseen);
        assert_eq!(passing.best, Novelty::SeenThisGame);
    }

    /// An outcome does not offer the other outcome's entries as candidates.
    ///
    /// What it costs when it does: a backward pass each, spent to be refused, out of a
    /// budget of sixty-four.
    #[test]
    fn an_outcome_asks_only_about_its_own_half() {
        let graph = rolled_check();
        let unseen = novel(&[2, 3], Novelty::UnseenAnyGame);

        let passing = candidates_from(&graph, &[node(1)], &unseen, Nearest::First);
        assert_eq!(passing, vec![node(2)], "3 is the failing half's business");

        let failing = candidates_from(&graph, &[node(3)], &unseen, Nearest::First);
        assert_eq!(
            failing,
            vec![node(3)],
            "and its own destination is a candidate"
        );
    }

    /// THE START IS A CANDIDATE, at distance zero, whether or not a link leads back to it.
    ///
    /// It used to be one only round a loop. That reading does not survive a rolled check,
    /// where the baseline is where an OUTCOME lands and the check entry can outrank it -
    /// see `LookAheadGraph::best_linked_class`, which now says the same thing.
    #[test]
    fn the_start_is_a_candidate_at_distance_zero() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1))
            .build();

        let novelty = novel(&[0], Novelty::UnseenAnyGame);
        let ordered = candidates(&graph, node(0), &novelty);

        assert_eq!(
            ordered,
            vec![node(0)],
            "the start, and nothing else is unseen"
        );
    }

    /// And it sorts FIRST within its class, being the one that needs no walking at all.
    #[test]
    fn the_start_is_asked_about_before_anything_further_away() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build();

        let novelty = novel(&[0, 2], Novelty::UnseenAnyGame);
        let ordered = candidates(&graph, node(0), &novelty);

        assert_eq!(ordered, vec![node(0), node(2)]);
    }

    /// A group is never a candidate, the start included.
    #[test]
    fn a_start_that_is_a_group_is_not_a_candidate() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).group().links(&[1]))
            .add(Entry::new(1))
            .build();

        let novelty = novel(&[0], Novelty::UnseenAnyGame);

        assert!(candidates(&graph, node(0), &novelty).is_empty());
    }

    #[test]
    fn an_unreachable_candidate_does_not_score() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).guard(r#"Variable["shut"]"#).links(&[2]))
            .add(Entry::new(2))
            .build();
        let world = TestWorld::new().set_variable("shut", GuardValue::from_boolean(false));

        let answer = search(&graph, &world, novel(&[2], Novelty::UnseenAnyGame));
        assert_eq!(answer.best, Novelty::SeenThisGame);
        assert_eq!(answer.witness, None);
        assert_eq!(answer.stopped_by, StoppedBy::Nothing);
    }

    #[test]
    fn a_reachable_candidate_scores_and_names_itself() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2))
            .build();

        let answer = search(
            &graph,
            &TestWorld::new(),
            novel(&[2], Novelty::UnseenAnyGame),
        );
        assert_eq!(answer.best, Novelty::UnseenAnyGame);
        assert_eq!(answer.witness, Some(node(2)));
    }

    /// The case a distance-only order gets wrong, which is why the order is class first.
    ///
    /// A near `UnseenThisGame` entry and a far `UnseenAnyGame` one. Nearest-first would
    /// prove the near one reachable, stop, and report the smaller answer - and it would
    /// look right, because it did find something.
    #[test]
    fn a_far_better_class_beats_a_near_worse_one() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1))
            .add(Entry::new(2).links(&[3]))
            .add(Entry::new(3).links(&[4]))
            .add(Entry::new(4))
            .build();

        let novelty = |id: DialogueNodeId| match id.entry_id {
            1 => Novelty::UnseenThisGame,
            4 => Novelty::UnseenAnyGame,
            _ => Novelty::SeenThisGame,
        };

        let answer = search(&graph, &TestWorld::new(), novelty);
        assert_eq!(answer.best, Novelty::UnseenAnyGame);
        assert_eq!(
            answer.witness,
            Some(node(4)),
            "the near worse candidate won"
        );
        // And it never had to ask about the near one: the better class is exhausted first.
        assert_eq!(answer.targets_asked, 1);
    }

    /// A refused entry answers for everything behind it, without a second fixed point.
    ///
    /// A chain: 0 -> 1 -> 2 -> 3, with 1 shut by a guard. Entries 2 and 3 are unseen and
    /// only reachable through 1, so 1 dominates both. Refusing 1 refuses them, and the
    /// driver should ask ONE question about a list of three.
    #[test]
    fn a_settled_refusal_answers_for_everything_it_dominates() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).guard(r#"Variable["shut"]"#).links(&[2]))
            .add(Entry::new(2).links(&[3]))
            .add(Entry::new(3))
            .build();
        let world = TestWorld::new().set_variable("shut", GuardValue::from_boolean(false));

        let answer = search(&graph, &world, novel(&[1, 2, 3], Novelty::UnseenAnyGame));

        assert_eq!(answer.best, Novelty::SeenThisGame, "nothing is reachable");
        assert_eq!(answer.candidates, 3, "all three were candidates");
        assert_eq!(
            answer.targets_asked, 1,
            "1 dominates 2 and 3, so refusing it should have answered for both",
        );
        assert_eq!(
            answer.stopped_by,
            StoppedBy::Nothing,
            "it ran out of candidates, not budget"
        );
    }

    /// And it must NOT fire where the entry is reachable another way.
    ///
    /// The diamond: 0 -> 1 -> 3 and 0 -> 2 -> 3, with only 1 shut. Nothing dominates 3, so
    /// refusing 1 says nothing about it - and 3 IS reachable, through 2. A rule keyed on
    /// reachability rather than dominance would get this wrong and lose the answer.
    #[test]
    fn a_refusal_says_nothing_about_an_entry_reachable_another_way() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1).guard(r#"Variable["shut"]"#).links(&[3]))
            .add(Entry::new(2).links(&[3]))
            .add(Entry::new(3))
            .build();
        let world = TestWorld::new().set_variable("shut", GuardValue::from_boolean(false));

        let answer = search(&graph, &world, novel(&[1, 3], Novelty::UnseenAnyGame));

        assert_eq!(answer.best, Novelty::UnseenAnyGame);
        assert_eq!(answer.witness, Some(node(3)), "3 is reachable through 2");
    }

    /// An UNSETTLED refusal proves nothing and must not be remembered.
    ///
    /// The ration is cut to nothing, so the pass about the first candidate cannot complete.
    /// The driver stops on an incomplete answer rather than treating it as a refusal and
    /// carrying that into everything the candidate dominates.
    #[test]
    fn an_unsettled_pass_refuses_nothing_on_behalf_of_anything_else() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2).links(&[3]))
            .add(Entry::new(3))
            .build();

        let symbols = graph.symbols().clone();
        let layout = DataLayout::for_graph(&graph, CAP, None, false);
        let vars = DataVars::new(&layout, &symbols, DiagramBudget::modest());
        let world = TestWorld::new();
        let mut compiler = GuardCompiler::new(&vars).with_world(&world);
        let seed = seed_of(&graph, &world, &vars).expect("room for a seed");

        let answer = best_novelty(
            &graph,
            node(0),
            StartBranch::Either,
            &seed,
            &mut compiler,
            &world,
            CAP as u32,
            novel(&[1, 2, 3], Novelty::UnseenAnyGame),
            &Budget {
                time: std::time::Duration::from_secs(5),
                // NO STEPS AT ALL, so no pass can reach a fixed point. That is the state a
                // refusal must not be inferred from.
                each: crate::symbolic::backward::Budget {
                    steps: 0,
                    ..Default::default()
                },
            },
            None,
        );

        assert_eq!(
            answer.stopped_by,
            StoppedBy::Incomplete,
            "an unsettled pass is not a refusal and must stop the search",
        );
        assert_eq!(
            answer.best,
            Novelty::SeenThisGame,
            "and it establishes nothing"
        );
    }

    /// The worse class is only reached once the better one is refused entirely.
    #[test]
    fn a_refused_class_falls_through_to_the_next() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1, 2]))
            .add(Entry::new(1).guard(r#"Variable["shut"]"#).links(&[3]))
            .add(Entry::new(2))
            .add(Entry::new(3))
            .build();
        let world = TestWorld::new().set_variable("shut", GuardValue::from_boolean(false));

        let novelty = |id: DialogueNodeId| match id.entry_id {
            3 => Novelty::UnseenAnyGame,
            2 => Novelty::UnseenThisGame,
            _ => Novelty::SeenThisGame,
        };

        let answer = search(&graph, &world, novelty);
        assert_eq!(answer.best, Novelty::UnseenThisGame);
        assert_eq!(answer.witness, Some(node(2)));
        assert_eq!(
            answer.targets_asked, 2,
            "the shut candidate had to be asked first"
        );
    }

    /// Early exit: a search that settles without asking about everything.
    #[test]
    fn the_search_stops_at_the_first_witness() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).links(&[2]))
            .add(Entry::new(2).links(&[3]))
            .add(Entry::new(3).links(&[4]))
            .add(Entry::new(4))
            .build();

        let answer = search(
            &graph,
            &TestWorld::new(),
            novel(&[1, 2, 3, 4], Novelty::UnseenAnyGame),
        );
        assert_eq!(answer.best, Novelty::UnseenAnyGame);
        assert_eq!(answer.candidates, 4);
        assert_eq!(
            answer.targets_asked, 1,
            "the nearest witness should have ended it"
        );
        assert_eq!(answer.witness, Some(node(1)));
    }

    /// The answer the reference walk gives, on the driver's own shapes.
    ///
    /// The acceptance criterion for the whole driver, and the only one here that is about
    /// the product rather than about the parts. Every other test in this file is checked
    /// against the same machinery that answers it; [`crate::oracle`] walks one state at a
    /// time and shares none of it.
    ///
    /// AT LEAST, not exactly. The symbolic side over-approximates - an undecided guard goes
    /// through, a saturated counter holds together values a walk tells apart - so it may
    /// report a better novelty than the walk finds. Reporting a WORSE one would mean a
    /// marker lost, and that is what this forbids.
    #[test]
    fn the_driver_agrees_with_the_reference_walk_on_its_own_fixtures() {
        let shut = TestWorld::new().set_variable("shut", GuardValue::from_boolean(false));
        let plain = TestWorld::new();

        // Name, graph, world, and which entries are unseen.
        let fixtures: Vec<(&str, LookAheadGraph, &TestWorld, Vec<i32>)> = vec![
            (
                "a false guard blocks",
                GraphBuilder::new()
                    .add(Entry::new(0).links(&[1]))
                    .add(Entry::new(1).guard(r#"Variable["shut"]"#).links(&[2]))
                    .add(Entry::new(2))
                    .build(),
                &shut,
                vec![2],
            ),
            (
                "an unknown guard does not block",
                GraphBuilder::new()
                    .add(Entry::new(0).links(&[1]))
                    .add(Entry::new(1).guard("IsKimHere()").links(&[2]))
                    .add(Entry::new(2))
                    .build(),
                &plain,
                vec![2],
            ),
            (
                "actions unlock their own downstream guards",
                GraphBuilder::new()
                    .add(Entry::new(0).links(&[1]))
                    .add(
                        Entry::new(1)
                            .script(r#"SetVariableValue("opened", true)"#)
                            .links(&[2]),
                    )
                    .add(Entry::new(2).guard(r#"Variable["opened"]"#))
                    .build(),
                &plain,
                vec![2],
            ),
            (
                "groups are traversed but never scored",
                GraphBuilder::new()
                    .add(Entry::new(0).links(&[1]))
                    .add(Entry::new(1).group().links(&[2]))
                    .add(Entry::new(2))
                    .build(),
                &plain,
                vec![1],
            ),
            (
                "cycles terminate",
                GraphBuilder::new()
                    .add(Entry::new(0).links(&[1]))
                    .add(Entry::new(1).links(&[2]))
                    .add(Entry::new(2).links(&[1]))
                    .build(),
                &plain,
                vec![2],
            ),
        ];

        for (name, graph, world, unseen) in fixtures {
            let novelty = novel(&unseen, Novelty::UnseenAnyGame);
            let walk = crate::oracle::walk(&graph, node(0), world, CAP);
            assert!(
                !walk.exhausted(),
                "{name}: the fixture should be exhaustible"
            );
            let expected = walk.best_novelty(&novelty);
            let answer = search(&graph, world, &novelty);

            assert!(
                answer.best >= expected,
                "{name}: the driver said {:?} where the walk found {expected:?}, which is \
                 a marker lost",
                answer.best,
            );
        }
    }

    /// A group is never a candidate, however novel the save says it is.
    #[test]
    fn a_group_entry_is_not_a_candidate() {
        let graph = GraphBuilder::new()
            .add(Entry::new(0).links(&[1]))
            .add(Entry::new(1).group().links(&[2]))
            .add(Entry::new(2))
            .build();

        let answer = search(
            &graph,
            &TestWorld::new(),
            novel(&[1], Novelty::UnseenAnyGame),
        );
        assert_eq!(answer.best, Novelty::SeenThisGame);
        assert_eq!(answer.candidates, 0);
    }
}
