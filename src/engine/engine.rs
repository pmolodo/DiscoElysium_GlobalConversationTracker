use crate::core::types::{DialogueNodeId, Novelty, DialogueCheckKind, Ternary, LookAheadLimit};
use crate::core::state::LookAheadState;
use crate::core::action::{CounterCaps, DialogueAction};
use crate::graph::graph::LookAheadGraph;
use crate::graph::node::LookAheadNode;
use crate::world::world::{ILookAheadWorld, CrawlContext};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};
use std::fmt;

/// Options for a look-ahead crawl.
pub struct LookAheadOptions {
    /// The most search states one crawl may hold, or `usize::MAX` for no such limit.
    ///
    /// A TEST-ONLY KNOB SINCE de-7z0f, and no longer reachable from outside this crate: it
    /// is not on the wire and there is no setting for it. What a player can set is memory
    /// and time, which are quantities anybody can reason about; a count of search states is
    /// not, because the same 200,000 of them cost 136 MB in one conversation and 455 MB in
    /// another.
    ///
    /// Kept for the tests because it is the one limit that can be set to an exact small
    /// number and give an exactly reproducible give-up, which a memory budget cannot
    /// promise, and because holding the state count FIXED is what makes two versions of the
    /// crawl comparable - see tests/slot_width_cost.rs.
    pub state_budget: usize,

    /// The most memory the search frontier may occupy, in bytes, or 0 for no such limit.
    ///
    /// THE DEFAULT LIMIT. Counted rather than asked of the allocator: every state the crawl
    /// keeps costs its slots on the heap plus its own inline size plus the set's overhead,
    /// and that is arithmetic the crawl can do as it goes for the price of an add. Asking
    /// the operating system what the process is using would measure the game as well.
    ///
    /// An estimate, deliberately. It is a budget rather than an accounting: it needs to be
    /// the right size and to move the right way, not to be exact.
    pub memory_budget: usize,
    pub time_budget: Duration,
    pub time_check_interval: usize,
    /// Called every [`Self::progress_interval`] with where the crawl is: the entry it is
    /// standing on, the states held, the entries reached, the BYTES those states occupy,
    /// and how long it has been going.
    ///
    /// THE BYTES ARE THE ONE TO WATCH, and the reason they are passed rather than left to
    /// be worked out: [`Self::memory_budget`] is what stops this crawl in practice - on the
    /// six-gigabyte matrix every heavy row ended by spending all of it - so a progress line
    /// without them cannot say how close the end is. The state count cannot stand in for
    /// them either, because the same 200,000 states cost 136 MB in one conversation and
    /// 455 MB in another.
    pub on_progress: Option<Box<dyn Fn(DialogueNodeId, usize, usize, usize, Duration) + Send + Sync>>,
    pub progress_interval: Duration,
    pub on_state_reached: Option<Box<dyn Fn(DialogueNodeId, &LookAheadState, usize) + Send + Sync>>,
    pub state_sample_interval: usize,
    pub counter_cap: i32,
    /// A per-slot override for [`Self::counter_cap`]; `None` from it means the default.
    ///
    /// For an experimental crawl that has PROVEN a particular variable saturates lower
    /// than the blanket cap - the offline crawler's `counterCaps` block supplies these.
    #[allow(clippy::type_complexity)]
    pub counter_cap_for_slot: Option<Box<dyn Fn(usize) -> Option<i32> + Send + Sync>>,
    pub failed_checks_pass_through: bool,
    pub collect_trace: bool,
    pub trace_node_limit: usize,

    /// How much of the MACHINE a crawl must leave alone, as a fraction of the total.
    ///
    /// A DIFFERENT KIND OF LIMIT FROM THE OTHERS, and the reason it is here rather than in
    /// a caller's hands. [`Self::memory_budget`] is a promise about this search; this is a
    /// promise about the box, and no budget can make it - 256 MB is honoured perfectly on
    /// a machine with 100 MB free, right up until the allocation that ends the process.
    ///
    /// Worse than ending it, first: a search that takes a machine to its last page makes
    /// everything on it wait on a disk, this game included, and an operating system in
    /// that state can be hard to get back. A twentieth left alone costs a crawl almost
    /// nothing where there is room, and is the difference where there is not.
    ///
    /// Zero turns it off. [`crate::engine::system_memory`] says what happens on a platform
    /// that cannot be asked: nothing, and it says so rather than assuming the best.
    pub system_reserve: f64,

    /// How many states pass between readings of the machine's memory.
    ///
    /// A reading is a system call, and the loop this sits in runs once per state, so the
    /// truth is read on a cadence and estimated in between - see
    /// [`crate::engine::system_memory::Runway`]. The estimate can bring a reading forward
    /// and can never end a crawl by itself.
    pub system_check_interval: usize,
}

impl Default for LookAheadOptions {
    fn default() -> Self {
        Self {
            state_budget: usize::MAX,
            memory_budget: DEFAULT_MEMORY_BUDGET,
            time_budget: Duration::from_secs(1),
            time_check_interval: 512,
            on_progress: None,
            progress_interval: Duration::from_secs(1),
            on_state_reached: None,
            state_sample_interval: 0,
            counter_cap: 16,
            counter_cap_for_slot: None,
            failed_checks_pass_through: true,
            collect_trace: false,
            trace_node_limit: 15,
            system_reserve: crate::engine::system_memory::DEFAULT_RESERVE,
            system_check_interval: crate::engine::system_memory::Runway::READINGS_EVERY,
        }
    }
}

impl LookAheadOptions {
    pub fn state_budget(mut self, budget: usize) -> Self { self.state_budget = budget; self }
    pub fn memory_budget(mut self, bytes: usize) -> Self { self.memory_budget = bytes; self }
    pub fn time_budget(mut self, budget: Duration) -> Self { self.time_budget = budget; self }
    pub fn time_check_interval(mut self, interval: usize) -> Self { self.time_check_interval = interval; self }
    pub fn on_progress<F>(mut self, f: F) -> Self where F: Fn(DialogueNodeId, usize, usize, usize, Duration) + Send + Sync + 'static { self.on_progress = Some(Box::new(f)); self }
    pub fn progress_interval(mut self, interval: Duration) -> Self { self.progress_interval = interval; self }
    pub fn on_state_reached<F>(mut self, f: F) -> Self where F: Fn(DialogueNodeId, &LookAheadState, usize) + Send + Sync + 'static { self.on_state_reached = Some(Box::new(f)); self }
    pub fn state_sample_interval(mut self, interval: usize) -> Self { self.state_sample_interval = interval; self }
    pub fn counter_cap(mut self, cap: i32) -> Self { self.counter_cap = cap; self }
    pub fn counter_cap_for_slot<F>(mut self, f: F) -> Self
    where
        F: Fn(usize) -> Option<i32> + Send + Sync + 'static,
    {
        self.counter_cap_for_slot = Some(Box::new(f));
        self
    }
    pub fn failed_checks_pass_through(mut self, v: bool) -> Self { self.failed_checks_pass_through = v; self }
    pub fn collect_trace(mut self, v: bool) -> Self { self.collect_trace = v; self }
    pub fn trace_node_limit(mut self, limit: usize) -> Self { self.trace_node_limit = limit; self }
    pub fn system_reserve(mut self, fraction: f64) -> Self { self.system_reserve = fraction; self }
    pub fn system_check_interval(mut self, states: usize) -> Self { self.system_check_interval = states; self }
}

/// Result of a look-ahead crawl.
#[derive(Debug, Clone)]
pub struct LookAheadResult {
    pub best: Novelty,
    pub states_explored: usize,
    pub nodes_reached: usize,
    pub stopped_by: LookAheadLimit,
    pub trace: Option<LookAheadTrace>,
}

impl LookAheadResult {
    pub fn budget_exhausted(&self) -> bool {
        self.stopped_by != LookAheadLimit::None
    }
}

impl fmt::Display for LookAheadResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} after {} states over {} nodes", self.best, self.states_explored, self.nodes_reached)?;
        match self.stopped_by {
            LookAheadLimit::States => write!(f, " (state budget exhausted)"),
            LookAheadLimit::Memory => write!(f, " (out of memory budget)"),
            LookAheadLimit::NoMemory => write!(f, " (the machine had no memory to give)"),
            LookAheadLimit::Time => write!(f, " (out of time)"),
            LookAheadLimit::None => Ok(()),
        }
    }
}

/// What one state in the search frontier costs, in bytes.
///
/// The key the crawl keeps is an entry id and a state; the state is its slots on the heap
/// plus its money, clock and cached hash inline. The extra eighth is the hash set's own
/// overhead - a control byte per element, at around seven-eighths load.
///
/// AN ESTIMATE, AND ONLY EVER USED AS ONE. It backs a budget, which needs to be the right
/// size and to move the right way when a group's states get wider. It is not an accounting
/// of the process, and nothing should read it as one.
fn state_bytes(slot_count: usize) -> usize {
    let key = std::mem::size_of::<StateKey>() + slot_count * std::mem::size_of::<i32>();
    key + key / 8 + 1
}

/// Whether the frontier can take `more` states, asked of the allocator before it is told.
///
/// ## Why the question is asked at all
///
/// Because the telling has no failure path. `HashSet::insert` and `VecDeque::push_back`
/// grow by doubling, and when the allocator refuses they reach
/// `std::alloc::handle_alloc_error`, which prints to stderr and ABORTS THE PROCESS - it is
/// not a panic, so nothing can catch it. This engine ships as a native library inside the
/// game's own process, where that is the player's session rather than a lost marker.
///
/// `try_reserve` asks the same allocator for the same room and returns an error instead, so
/// running out becomes a verdict: [`LookAheadLimit::NoMemory`], which the caller can tell
/// apart from the budget it set itself.
///
/// ## What it does NOT cover, and is not pretended to
///
/// The STATE ITSELF. A `StateKey` holds its slots on the heap, so cloning one allocates,
/// and that allocation is infallible like any other. What this covers is the two
/// collections, which is where the memory actually goes - they hold every state the crawl
/// has seen, and they grow in doublings, so the ask that fails on a machine near its limit
/// is theirs and not a ninety-six byte clone's. It converts the realistic case; it does not
/// make the crawl allocation-safe.
///
/// ## What it costs when there is room
///
/// TWO SUBTRACTIONS AND TWO COMPARES, on the overwhelming majority of states. The allocator
/// is only asked when a collection is actually full, which after each doubling is one state
/// in an ever-growing number of them; every other state takes the spare-capacity path and
/// never enters `try_reserve` at all.
///
/// Measured, over a quarter of a million states of pure frontier growth - the worst case
/// for this, since a real group spends most of its time in guard evaluation: 907 ns a state
/// before this existed, 909 after. Calling `try_reserve` unconditionally instead cost 941,
/// which is where the spare-capacity check earns its place.
fn room_for(
    seen: &mut HashSet<StateKey>,
    queue: &mut VecDeque<StateKey>,
    more: usize,
) -> bool {
    (seen.capacity() - seen.len() >= more || seen.try_reserve(more).is_ok())
        && (queue.capacity() - queue.len() >= more || queue.try_reserve(more).is_ok())
}

/// What one crawl's search frontier may hold by default, in bytes.
///
/// 256 MB. Chosen against the measurement rather than picked: at the old budget of 200,000
/// states the worst group in the game held 455 MB and the best 136, so this cuts the worst
/// case roughly in half while leaving every group the same allowance as every other. See
/// tests/crawl_memory.rs for the table it comes from.
///
/// In states, that is about 112,000 in the group with the widest states and about 377,000
/// in the narrowest - which is the point. The old number spent three and a half times as
/// much memory on one conversation as another for no reason anybody chose.
pub const DEFAULT_MEMORY_BUDGET: usize = 256 * 1024 * 1024;

/// Which outcome of a rolled start a crawl explores.
///
/// A white or red check is the one node that can be entered in two ways: the roll passes
/// and its success flag is set, or it fails and - for a red check - its failure flag is.
/// [`LookAheadEngine::enter_rolled`] builds both, in that order, and this picks between
/// them.
///
/// WHY THE ORDER IS LOAD BEARING: `Pass` and `Fail` are positions in that vector, not a
/// re-derivation of the roll. Anything that reorders `enter_rolled` has to reorder these
/// with it, which is why they are defined next to each other in the same file.
///
/// A start that does not roll leaves exactly one state, and that state is its `Pass`; its
/// `Fail` is empty, because there is no failure to explore. The bridge asks for branches
/// only where the start is a white or red check, so that case is a definition rather than
/// a situation - but it is the definition that keeps `Fail` from quietly meaning `Pass`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartBranch {
    /// Both, when the start rolls. What an ordinary crawl wants: the answer is the best
    /// anything reachable can offer, and which side of a roll it lay on does not change it.
    Either,

    /// The roll passed.
    Pass,

    /// The roll failed. EMPTY WHERE THERE IS NO SUCH BRANCH - a red check with no failure
    /// flag has nowhere to fail to, and the honest answer is that the crawl found nothing
    /// rather than that it explored the pass branch twice.
    Fail,
}

impl StartBranch {
    /// The states this branch keeps, out of everything entering the start produced.
    fn take(self, entered: Vec<LookAheadState>) -> Vec<LookAheadState> {
        match self {
            StartBranch::Either => entered,
            // A single state means the start does not roll, and one branch is all there
            // is - so naming a branch of it names that state rather than nothing.
            StartBranch::Pass => entered.into_iter().take(1).collect(),
            StartBranch::Fail => {
                if entered.len() < 2 {
                    Vec::new()
                } else {
                    entered.into_iter().skip(1).collect()
                }
            }
        }
    }
}

/// Trace of a crawl for diagnostics.
#[derive(Debug, Clone)]
pub struct LookAheadTrace {
    pub start: DialogueNodeId,
    pub graph_node_count: usize,
    pub tracked_slots: usize,
    pub money: i32,
    pub day_minutes: i32,
    pub day_counter: i32,
    pub clock_locked: bool,
    pub hottest_nodes: Vec<NodeStateCount>,
}

#[derive(Debug, Clone)]
pub struct NodeStateCount {
    pub node: DialogueNodeId,
    pub states: usize,
}

impl fmt::Display for NodeStateCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} x{}", self.node, self.states)
    }
}

/// A visited (node, state) pair.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct StateKey {
    node: DialogueNodeId,
    state: LookAheadState,
}

/// The look-ahead engine.
pub struct LookAheadEngine {
    options: LookAheadOptions,
}

impl LookAheadEngine {
    pub fn new(options: LookAheadOptions) -> Self {
        Self { options }
    }

    pub fn default() -> Self {
        Self::new(LookAheadOptions::default())
    }

    /// Whether this conversation group holds any scoreable entry that could improve an
    /// option already showing `own_novelty`.
    ///
    /// A structural upper bound: it does not ask whether such an entry is REACHABLE in
    /// the current world, only whether one exists. That makes its positive answer weak
    /// and its negative answer exact - if nothing in the whole group outranks the
    /// option, no walk can produce a marker, so the caller can skip building any crawl
    /// state at all.
    ///
    /// Free-standing rather than folded into [`Self::evaluate`], matching the C#, because
    /// the caller is what decides whether to crawl: the plugin asks this before composing
    /// a marker, and the offline crawler asks it before reporting a cost. Answering it
    /// inside `evaluate` would hide from callers the fact that no crawl happened.
    pub fn has_potential_improvement<F>(
        graph: &LookAheadGraph,
        own_novelty: Novelty,
        novelty: F,
    ) -> bool
    where
        F: Fn(DialogueNodeId) -> Novelty,
    {
        graph.nodes().any(|node| {
            // Groups are expanded in place and the game never writes their SimStatus, so
            // treating one as an unseen candidate would make this check useless - 36.6%
            // of the database is groups and every one of them reads as never displayed.
            !node.is_group && novelty(node.id) > own_novelty
        })
    }

    /// The same question, asked only of what this option CAN REACH.
    ///
    /// [`Self::has_potential_improvement`] scans the whole loaded group, which includes
    /// everything no path from this option leads to. This follows links from `start` and
    /// asks only about entries it can actually arrive at, so it refuses strictly more
    /// crawls - and never one that could have found something, because a guard can only
    /// refuse a link, never create one.
    ///
    /// ## Why it is affordable, when the same idea was measured and reverted in C#
    ///
    /// A stateless prefilter of this shape was built, measured and reverted there (see
    /// de-asw.3): it pruned only 0.5 to 4 per cent in the hub-connected case and cost
    /// more than it saved once wired per option. Two things are different here.
    ///
    /// The walk is FUSED WITH THE QUESTION rather than run before it. It stops the moment
    /// it meets an entry worth crawling for, so the case where the crawl is going to
    /// happen anyway costs almost nothing; the only case that pays for a full traversal
    /// is the one where a whole crawl is then skipped. That is the right way round.
    ///
    /// And the walk is small. Measured over the shipped database: a start reaches 1,144
    /// entries on average and 4,473 at the very widest, against a loaded group that runs
    /// to 16,558 - so this is a few thousand pointer-follows against a crawl that budgets
    /// 200,000 states.
    ///
    /// ## What it does not do
    ///
    /// It ignores guards entirely, so its answer is an upper bound: everything it calls
    /// reachable may still be shut. Refusing on it is safe; believing it is not.
    pub fn reaches_potential_improvement<F>(
        graph: &LookAheadGraph,
        start: DialogueNodeId,
        own_novelty: Novelty,
        novelty: F,
    ) -> bool
    where
        F: Fn(DialogueNodeId) -> Novelty,
    {
        let mut seen = HashSet::new();
        let mut pending = VecDeque::new();
        seen.insert(start);
        pending.push_back(start);

        while let Some(id) = pending.pop_front() {
            let Some(node) = graph.get(id) else { continue };
            for &child_id in &node.links {
                if !seen.insert(child_id) {
                    continue;
                }

                let Some(child) = graph.get(child_id) else { continue };
                // The start is not a candidate - `own_novelty` is what it already scores,
                // and `evaluate` scores children rather than where it began.
                if !child.is_group && novelty(child_id) > own_novelty {
                    return true;
                }

                pending.push_back(child_id);
            }
        }

        false
    }

    /// Find the most novel entry reachable beyond `start`.
    pub fn evaluate<F>(
        &self,
        graph: &LookAheadGraph,
        start: DialogueNodeId,
        world: &dyn ILookAheadWorld,
        novelty: F,
    ) -> LookAheadResult
    where
        F: Fn(DialogueNodeId) -> Novelty,
    {
        self.evaluate_from(graph, start, world, novelty, StartBranch::Either)
    }

    /// The same crawl, told which outcome of a rolled start to explore.
    ///
    /// A white or red check is two options wearing one line of text, and the interesting
    /// thing about it is often which of the two leads somewhere. [`StartBranch`] picks one
    /// of them, so the caller can ask twice and report the answers apart.
    ///
    /// A start that does not roll has one way in, and every branch names it.
    pub fn evaluate_from<F>(
        &self,
        graph: &LookAheadGraph,
        start: DialogueNodeId,
        world: &dyn ILookAheadWorld,
        novelty: F,
        branch: StartBranch,
    ) -> LookAheadResult
    where
        F: Fn(DialogueNodeId) -> Novelty,
    {
        let start_node = graph.get(start).expect("start node not in graph");
        let context = CrawlContext::new(graph.symbols(), world);
        let initial = Self::seed(graph, world);

        // Enter the start node (pay cost, apply actions), keeping EVERY state that entering
        // it can leave the crawl in. A rolled check leaves two - it is the one node type
        // that does - and taking only the first of them was how a check option's failure
        // branch went unexplored for as long as this function took `.next()`.
        let entered = branch.take(self.enter(start_node, &initial, &context));
        if entered.is_empty() {
            return LookAheadResult {
                best: Novelty::SeenThisGame,
                states_explored: 0,
                nodes_reached: 0,
                stopped_by: LookAheadLimit::None,
                trace: self.build_trace(graph, start, world, None),
            };
        }

        let mut seen = HashSet::new();
        let mut queue = VecDeque::new();
        let mut reached = HashSet::new();
        let mut tally = if self.options.collect_trace { Some(HashMap::new()) } else { None };

        // WHAT ONE KEPT STATE COSTS, in bytes, for the memory budget. Every state in the
        // group has the same slot count, so this is worked out once rather than per state.
        let bytes_per_state = state_bytes(graph.symbols().count());
        let mut frontier_bytes = 0usize;

        // THE MACHINE'S OWN LIMIT, beside the caller's. None where this build cannot ask,
        // or where the caller turned it off, and then the reserve is simply not enforced.
        let mut runway = if self.options.system_reserve > 0.0 {
            crate::engine::system_memory::Runway::every(
                self.options.system_reserve,
                self.options.system_check_interval,
            )
        } else {
            None
        };

        // ROOM FOR THE SEED FIRST. A crawl that cannot even hold what entering the start
        // produced has not been stopped by its budget, and saying so is the difference
        // between a verdict and a dead process - see `room_for`.
        let mut out_of_memory = !room_for(&mut seen, &mut queue, entered.len());

        if !out_of_memory {
            for state in &entered {
                let key = StateKey { node: start, state: state.clone() };
                if seen.insert(key.clone()) {
                    frontier_bytes += bytes_per_state;
                    queue.push_back(key);
                }
            }
        }

        reached.insert(start);
        if let Some(t) = &mut tally { t.insert(start, entered.len()); }

        let sample_fn = self.options.on_state_reached.as_ref();
        let sample_every = self.options.state_sample_interval;
        let sampling = sample_fn.is_some() && sample_every > 0;
        if sampling {
            for state in &entered {
                sample_fn.unwrap()(start, state, seen.len());
            }
        }

        let mut best = Novelty::SeenThisGame;
        let mut stopped_by = LookAheadLimit::None;

        let timed = !self.options.time_budget.is_zero();
        let reporting = self.options.on_progress.is_some() && !self.options.progress_interval.is_zero();
        let clocked = timed || reporting;

        let start_time = if clocked { Some(Instant::now()) } else { None };
        let deadline = if timed { Some(start_time.unwrap() + self.options.time_budget) } else { None };
        let mut next_report = if reporting { Some(start_time.unwrap() + self.options.progress_interval) } else { None };
        let mut until_clock_check = self.options.time_check_interval;

        if out_of_memory {
            stopped_by = LookAheadLimit::NoMemory;
        }

        while !out_of_memory {
            let Some(current) = queue.pop_front() else { break };
            if self.options.memory_budget > 0 && frontier_bytes >= self.options.memory_budget {
                stopped_by = LookAheadLimit::Memory;
                break;
            }

            if seen.len() >= self.options.state_budget {
                stopped_by = LookAheadLimit::States;
                break;
            }

            if clocked && until_clock_check > 0 {
                until_clock_check -= 1;
                if until_clock_check == 0 {
                    until_clock_check = self.options.time_check_interval;
                    let now = Instant::now();
                    if let Some(d) = deadline {
                        if now >= d {
                            stopped_by = LookAheadLimit::Time;
                            break;
                        }
                    }
                    if let Some(nr) = next_report {
                        if now >= nr {
                            if let Some(report) = &self.options.on_progress {
                                report(
                                    current.node,
                                    seen.len(),
                                    reached.len(),
                                    frontier_bytes,
                                    now - start_time.unwrap(),
                                );
                            }
                            next_report = Some(now + self.options.progress_interval);
                        }
                    }
                }
            }

            let node = graph.get(current.node).unwrap();

            for &child_id in &node.links {
                let Some(child) = graph.get(child_id) else { continue; };

                for next_state in self.enter(child, &current.state, &context) {
                    if !child.is_group {
                        let score = novelty(child_id);
                        if score > best {
                            best = score;
                            if best == Novelty::UnseenAnyGame {
                                reached.insert(child_id);
                                return LookAheadResult {
                                    best,
                                    states_explored: seen.len(),
                                    nodes_reached: reached.len(),
                                    stopped_by: LookAheadLimit::None,
                                    trace: self.build_trace(graph, start, world, tally),
                                };
                            }
                        }
                    }

                    // BEFORE THE STATE IS BUILT INTO THEM. Both collections grow by
                    // doubling, so the ask that fails on a machine that has run out is a
                    // large one, and it is infallible: `insert` and `push_back` reach
                    // `handle_alloc_error`, which ABORTS. Asking for the room first turns
                    // that into a verdict a caller can draw nothing for.
                    if !room_for(&mut seen, &mut queue, 1) {
                        out_of_memory = true;
                        stopped_by = LookAheadLimit::NoMemory;
                        break;
                    }

                    // AND WHETHER THE MACHINE STILL HAS ROOM TO SPARE. The allocator has
                    // not refused anything yet; this is the crawl declining to be the
                    // process that takes the last of it. Same verdict, because a caller
                    // can do nothing different about either: the budget is not the
                    // problem and no marker is worth a wedged machine.
                    if let Some(runway) = runway.as_mut() {
                        if runway.is_low(bytes_per_state as u64) {
                            out_of_memory = true;
                            stopped_by = LookAheadLimit::NoMemory;
                            break;
                        }
                    }

                    let key = StateKey { node: child_id, state: next_state.clone() };
                    if seen.insert(key.clone()) {
                        frontier_bytes += bytes_per_state;
                        reached.insert(child_id);
                        if let Some(t) = &mut tally { *t.entry(child_id).or_insert(0) += 1; }
                        queue.push_back(key);
                        if sampling && seen.len() % sample_every == 0 {
                            sample_fn.unwrap()(child_id, &next_state, seen.len());
                        }
                    }
                }
            }
        }

        LookAheadResult {
            best,
            states_explored: seen.len(),
            nodes_reached: reached.len(),
            stopped_by,
            trace: self.build_trace(graph, start, world, tally),
        }
    }

    /// The state a crawl starts in: what the world says about every slot the graph has.
    ///
    /// Public so the symbolic search can start from the same place rather than from its
    /// own idea of one. Seeding is not a detail - a symbolic run seeded with every data
    /// state explores paths that need an item the player does not have, and reports
    /// entries the crawl cannot reach. Two searches only agree if they start together.
    pub fn seed(graph: &LookAheadGraph, world: &dyn ILookAheadWorld) -> LookAheadState {
        let symbols = graph.symbols();
        let mut state = LookAheadState::empty(symbols.count(), world.money(), world.day_minutes());

        for slot in 0..symbols.count() {
            if let Some(name) = symbols.name_of(slot) {
                if let Some(stripped) = name.strip_prefix("item:") {
                    if world.initially_has_item(stripped) {
                        state = state.with(slot, 1);
                    }
                } else if let Some(stripped) = name.strip_prefix("task:") {
                    if world.initially_task_active(stripped) {
                        state = state.with(slot, 1);
                    }
                } else if let Some(stripped) = name.strip_prefix("thought:") {
                    if world.initially_has_thought(stripped) {
                        state = state.with(slot, 1);
                    }
                } else if !name.starts_with("once:") && !name.starts_with("seen:") {
                    let val = world.get_variable(name);
                    if val.kind() == crate::core::guard_value::GuardValueKind::Boolean && val.boolean() {
                        state = state.with(slot, 1);
                    } else if val.kind() == crate::core::guard_value::GuardValueKind::Number && val.number() != 0.0 {
                        state = state.with(slot, val.number() as i32);
                    }
                }
            }
        }

        // Seed seen slots from save
        for node in graph.nodes() {
            if node.seen_slot >= 0 && world.is_seen(node.id) {
                state = state.with(node.seen_slot as usize, 1);
            }
        }

        state
    }


    /// The entries one branch of a start leads to DIRECTLY.
    ///
    /// What the mod colours the word "Pass" or "Fail" by: the outcome entry the player
    /// would actually be shown, as against [`Self::evaluate_from`]'s answer, which is the
    /// best anything further down that branch can offer.
    ///
    /// GROUPS ARE WALKED THROUGH rather than reported. A group is a container the game
    /// expands in place and never displays, so a branch that leads to one leads, as far as
    /// a player can tell, to whatever the group holds. Better than a third of the database
    /// is groups, so stopping at one would name a destination the player never sees.
    ///
    /// More than one is normal - a branch can open several entries at once - and the caller
    /// takes the best novelty among them, on the same reasoning the crawl itself uses: the
    /// interesting thing about a set of destinations is the most novel one in it.
    pub fn branch_destinations(
        &self,
        graph: &LookAheadGraph,
        start: DialogueNodeId,
        world: &dyn ILookAheadWorld,
        branch: StartBranch,
    ) -> Vec<DialogueNodeId> {
        let Some(start_node) = graph.get(start) else { return Vec::new() };
        let context = CrawlContext::new(graph.symbols(), world);
        let initial = Self::seed(graph, world);
        let entered = branch.take(self.enter(start_node, &initial, &context));

        let mut found = Vec::new();
        let mut seen = HashSet::new();
        let mut pending: VecDeque<(DialogueNodeId, LookAheadState)> = VecDeque::new();

        for state in entered {
            pending.push_back((start, state));
        }

        while let Some((id, state)) = pending.pop_front() {
            let Some(node) = graph.get(id) else { continue };

            for &child_id in &node.links {
                let Some(child) = graph.get(child_id) else { continue };

                for next in self.enter(child, &state, &context) {
                    if child.is_group {
                        // Through it, not to it - and only once per state, since a group
                        // reached twice the same way holds the same entries.
                        if seen.insert(StateKey { node: child_id, state: next.clone() }) {
                            pending.push_back((child_id, next));
                        }
                        continue;
                    }

                    if !found.contains(&child_id) {
                        found.push(child_id);
                    }
                    break;
                }
            }
        }

        found
    }

    fn can_afford(&self, node: &LookAheadNode, state: &LookAheadState) -> bool {
        if !node.is_cost_option() { return true; }
        if node.cost_once && node.once_slot >= 0 && state.is_set(node.once_slot as usize) {
            return true;
        }
        node.cost <= state.money()
    }

    /// The states entering this node can leave the crawl in: none if it is closed,
    /// one for an ordinary entry, two where a rolled check can go either way.
    ///
    /// Returns an owned `Vec` rather than an iterator borrowing the caller's state. The
    /// caller enqueues these and moves on, so nothing is gained by streaming them, and
    /// an iterator would tie the results' lifetime to a state the search wants to drop.
    fn enter(
        &self,
        node: &LookAheadNode,
        state: &LookAheadState,
        context: &CrawlContext,
    ) -> Vec<LookAheadState> {
        let mut results = Vec::new();

        if !ternary_logic::can_pass(node.guard.test(&context.bound(state))) {
            return results;
        }
        if !self.can_afford(node, state) {
            return results;
        }

        match node.kind {
            DialogueCheckKind::Test => {}
            DialogueCheckKind::Fake => {
                if !self.has_been_seen(node, state) {
                    results.push(self.charge(node, state, context.world.is_clock_locked()));
                }
            }
            DialogueCheckKind::KimSwitch => {
                if node.boolean_only || !self.has_been_seen(node, state) {
                    results.push(self.charge(node, state, context.world.is_clock_locked()));
                }
            }
            DialogueCheckKind::Red | DialogueCheckKind::White => {
                for s in self.enter_rolled(node, state, context.world.is_clock_locked()) {
                    results.push(s);
                }
            }
            DialogueCheckKind::Passive => {
                let passes = context.world.check_passes(node.id);
                if passes != Ternary::False {
                    results.push(self.charge(node, state, context.world.is_clock_locked()));
                }
                if passes != Ternary::True && self.options.failed_checks_pass_through {
                    results.push(state.clone());
                }
            }
            _ => {
                results.push(self.charge(node, state, context.world.is_clock_locked()));
            }
        }

        results
    }

    fn enter_rolled(
        &self,
        node: &LookAheadNode,
        state: &LookAheadState,
        clock_locked: bool,
    ) -> Vec<LookAheadState> {
        let mut results = Vec::new();

        let passed = node.flag_slot >= 0 && state.is_set(node.flag_slot as usize);
        let failed = node.failed_flag_slot >= 0 && state.is_set(node.failed_flag_slot as usize);

        if passed || (node.kind == DialogueCheckKind::Red && failed) {
            return results;
        }

        let entered = self.charge(node, state, clock_locked);

        // Both branches start from the same charged state, so the success branch takes a
        // copy and leaves the original for the failure branch below. Moving it into the
        // success branch would leave nothing to build the failure from.
        let success = if node.flag_slot >= 0 {
            entered.with(node.flag_slot as usize, 1)
        } else {
            entered.clone()
        };
        results.push(success);

        // Failure branch
        if node.kind == DialogueCheckKind::Red && node.failed_flag_slot >= 0 {
            results.push(entered.with(node.failed_flag_slot as usize, 1));
        } else if node.kind == DialogueCheckKind::White {
            // Failed white check: state unchanged, can retry
            results.push(entered);
        }

        results
    }

    fn has_been_seen(&self, node: &LookAheadNode, state: &LookAheadState) -> bool {
        node.seen_slot >= 0 && state.is_set(node.seen_slot as usize)
    }

    fn charge(
        &self,
        node: &LookAheadNode,
        state: &LookAheadState,
        clock_locked: bool,
    ) -> LookAheadState {
        let mut paid = state.clone();
        if node.is_cost_option() {
            let already_paid = node.cost_once
                && node.once_slot >= 0
                && state.is_set(node.once_slot as usize);
            if !already_paid {
                paid = paid.with_money(paid.money() - node.cost);
                if node.cost_once && node.once_slot >= 0 {
                    paid = paid.with(node.once_slot as usize, 1);
                }
            }
        }

        if node.seen_slot >= 0 {
            paid = paid.with(node.seen_slot as usize, 1);
        }

        DialogueAction::apply(&node.actions, &paid, node.once_slot, &self.counter_caps(), clock_locked)
    }

    /// The counter caps this engine's options describe.
    ///
    /// Built per call rather than stored, because it borrows the options' override
    /// closure and so cannot outlive them; it is two pointers, and the alternative is a
    /// self-referential struct to save nothing.
    fn counter_caps(&self) -> CounterCaps<'_> {
        match &self.options.counter_cap_for_slot {
            Some(f) => CounterCaps::with_overrides(self.options.counter_cap, f.as_ref()),
            None => CounterCaps::flat(self.options.counter_cap),
        }
    }

    fn build_trace(
        &self,
        graph: &LookAheadGraph,
        start: DialogueNodeId,
        world: &dyn ILookAheadWorld,
        tally: Option<HashMap<DialogueNodeId, usize>>,
    ) -> Option<LookAheadTrace> {
        let tally = tally?;
        let mut hottest: Vec<NodeStateCount> = tally.into_iter()
            .map(|(node, states)| NodeStateCount { node, states })
            .collect();
        hottest.sort_by_key(|n| std::cmp::Reverse(n.states));
        if hottest.len() > self.options.trace_node_limit {
            hottest.truncate(self.options.trace_node_limit);
        }

        Some(LookAheadTrace {
            start,
            graph_node_count: graph.count(),
            tracked_slots: graph.symbols().count(),
            money: world.money(),
            day_minutes: world.day_minutes(),
            day_counter: world.day_counter(),
            clock_locked: world.is_clock_locked(),
            hottest_nodes: hottest,
        })
    }
}

mod ternary_logic {
    use crate::core::types::Ternary;
    pub fn can_pass(value: Ternary) -> bool {
        value != Ternary::False
    }
}
