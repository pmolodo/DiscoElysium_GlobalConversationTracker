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
    pub state_budget: usize,
    pub time_budget: Duration,
    pub time_check_interval: usize,
    pub on_progress: Option<Box<dyn Fn(DialogueNodeId, usize, usize, Duration) + Send + Sync>>,
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
}

impl Default for LookAheadOptions {
    fn default() -> Self {
        Self {
            state_budget: 200_000,
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
        }
    }
}

impl LookAheadOptions {
    pub fn state_budget(mut self, budget: usize) -> Self { self.state_budget = budget; self }
    pub fn time_budget(mut self, budget: Duration) -> Self { self.time_budget = budget; self }
    pub fn time_check_interval(mut self, interval: usize) -> Self { self.time_check_interval = interval; self }
    pub fn on_progress<F>(mut self, f: F) -> Self where F: Fn(DialogueNodeId, usize, usize, Duration) + Send + Sync + 'static { self.on_progress = Some(Box::new(f)); self }
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
            LookAheadLimit::Time => write!(f, " (out of time)"),
            LookAheadLimit::None => Ok(()),
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
        let start_node = graph.get(start).expect("start node not in graph");
        let context = CrawlContext::new(graph.symbols(), world);
        let initial = Self::seed(graph, world);

        // Enter the start node (pay cost, apply actions)
        let entered = match self.try_enter(start_node, &initial, &context) {
            Some(s) => s,
            None => {
                return LookAheadResult {
                    best: Novelty::SeenThisGame,
                    states_explored: 0,
                    nodes_reached: 0,
                    stopped_by: LookAheadLimit::None,
                    trace: self.build_trace(graph, start, world, None),
                };
            }
        };

        let mut seen = HashSet::new();
        let mut queue = VecDeque::new();
        let mut reached = HashSet::new();
        let mut tally = if self.options.collect_trace { Some(HashMap::new()) } else { None };

        let first_key = StateKey { node: start, state: entered.clone() };
        seen.insert(first_key.clone());
        queue.push_back(first_key);
        reached.insert(start);
        if let Some(t) = &mut tally { t.insert(start, 1); }

        let sample_fn = self.options.on_state_reached.as_ref();
        let sample_every = self.options.state_sample_interval;
        let sampling = sample_fn.is_some() && sample_every > 0;
        if sampling {
            sample_fn.unwrap()(start, &entered, seen.len());
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

        while let Some(current) = queue.pop_front() {
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
                                report(current.node, seen.len(), reached.len(), now - start_time.unwrap());
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

                    let key = StateKey { node: child_id, state: next_state.clone() };
                    if seen.insert(key.clone()) {
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

    fn seed(graph: &LookAheadGraph, world: &dyn ILookAheadWorld) -> LookAheadState {
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

    fn try_enter(
        &self,
        node: &LookAheadNode,
        state: &LookAheadState,
        context: &CrawlContext,
    ) -> Option<LookAheadState> {
        if !ternary_logic::can_pass(node.guard.test(&context.bound(state))) {
            return None;
        }
        if !self.can_afford(node, state) {
            return None;
        }
        self.enter(node, state, context).into_iter().next()
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
