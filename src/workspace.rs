// SPDX-License-Identifier: MIT
//! A diagram manager that outlives the query, on a thread that outlives it too.
//!
//! ## What this is for
//!
//! [`crate::bridge::answer`] builds everything from scratch per request: the group graph,
//! the layout, the manager, the compiled guards and the seed. `measurements/repeat_question.rs`
//! prices that at eighteen to twenty-seven milliseconds a request, and
//! `measurements/manager_reuse.rs` found a second cost nobody had looked for - the FIRST
//! request against a fresh manager takes ninety-eight milliseconds where later ones against
//! the same manager take fifty-three, because an empty apply cache and an untouched node
//! store have to be warmed. Today every request pays both.
//!
//! This keeps the expensive half alive between requests. de-2wtl.
//!
//! WHAT IT IS WORTH THROUGH THE SHIPPED CALL, rather than through the search underneath:
//! `measurements/workspace_menus.rs` drives [`crate::service::Service::look_ahead`] twelve
//! times over one group and finds a served request costs 53 to 60 milliseconds where an
//! unserved one costs 116 to 126 - about a halving, on every menu after the first in a
//! group. That is the measurement to re-run if this is ever suspected of not paying; the
//! two above say where the saving comes from, and it alone says what a player gets.
//!
//! ## What is kept and what is rebuilt, which is not what the issue assumed
//!
//! KEPT: the graph, the layout, the manager ([`DataVars`]) and the [`GroupShape`].
//! `DataLayout::for_group` reads the world through `money()` alone - the money ceiling - so
//! the layout, and the manager sized from it, survives everything else the world does.
//!
//! NOTHING ELSE IS KEPT ACROSS REQUESTS. Settled backward passes used to be, on a key of
//! their own - de-znov.3 - and they halved a served request: twelve requests of eight starts
//! over conversation 28 went from an average of 38 ms to 18. That memo was retired in
//! de-0jsf.24 after the marking changed shape underneath it. It held one exhaustive pass per
//! target, and the marking that replaced the per-option search stops its passes at the first
//! MEET - which is the optimisation - so every pass worth caching was one the memo refused to
//! keep. It had been unreachable for some time before anyone noticed, because a marking that
//! silently redoes work still draws the right markers.
//!
//! REBUILT PER REQUEST, inside the thread, at one to five milliseconds: the
//! [`GuardCompiler`] and the seed. They depend on the world in two different ways, and the
//! difference matters to anything built on top of this:
//!
//! - THE COMPILER folds in the clock, the variables, the items, the tasks, the thoughts and
//!   the world queries. Those move when the player acts on the world.
//! - THE SEED carries what has been SEEN - `core::state` seeds a node's seen-slot from
//!   `world.is_seen` - and that moves on every line the player reads. The compiler never
//!   asks `is_seen` at all; a seen-slot a guard reads is a tracked VARIABLE, and its
//!   starting value is the seed's business.
//!
//! So between two menus in one conversation, usually only the seed has changed. That is what
//! keeps the manager and the layout worth holding: neither depends on what has been read.
//!
//! THAT ASYMMETRY IS THE WHOLE DESIGN. de-2wtl originally proposed a workspace "valid for
//! ONE world snapshot", which would have been thrown away almost every menu and bought
//! nothing. Keying on the money ceiling instead keeps the nine or ten milliseconds that
//! matter and pays the one to five that do not.
//!
//! ## Why a thread rather than a struct
//!
//! Because a manager may not move. [`crate::symbolic::isolated`] records the fault: what
//! kills a process is BUILDING A SECOND MANAGER ON A THREAD THAT HAS ALREADY BUILT ONE, and
//! the arrangement measured safe in thirty-five runs is one manager, many searches, one
//! thread. So the manager stays on the thread that built it and requests are sent to it.
//!
//! It also avoids the self-referential struct that made design A expensive: `DataVars<'a>`
//! borrows the layout and the symbols, `GuardCompiler<'a>` borrows the `DataVars`, and here
//! every one of those borrows lives on the owner thread's own stack with the request loop
//! inside their scope. Nothing crosses a struct boundary, so none of the hundred and forty
//! construction sites in the tests has to change.
//!
//! ## And why invalidation RESPAWNS rather than rebuilds
//!
//! Same fault, read the other way. When the key moves - a different group, a different money
//! ceiling, a different budget - the manager has to be rebuilt, and rebuilding it on the
//! thread that already holds one is the failing arrangement exactly. So the thread is shut
//! down and a fresh one takes its place.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;

use crate::bridge::{
    COUNTER_CAP, LookAheadAnswer, LookAheadRequest, SnapshotWorld, WorldSnapshot, answer_starts,
};
use crate::core::types::{DialogueNodeId, Novelty};
use crate::graph::graph::LookAheadGraph;
use crate::index::VariableTable;
use crate::symbolic::budget::DiagramBudget;
use crate::symbolic::data_layout::DataLayout;
use crate::symbolic::guard_formula::GuardCompiler;
use crate::symbolic::isolated;
use crate::symbolic::known::GroupShape;
use crate::symbolic::reachability::seed_of;
use crate::symbolic::vars::DataVars;
use crate::world::world::ILookAheadWorld;

/// What a workspace is valid FOR. A request whose key differs needs a new one.
///
/// ## Every field is here because something is baked to it
///
/// - `group`: the conversations the graph covers. A different group is a different graph.
/// - `money_ceiling`: the ONLY way the world reaches the layout, through
///   `DataLayout::for_group`. Held rather than the money itself, so spending change that
///   does not move the ceiling does not throw the manager away.
/// - `memory` and `cache_split`: the manager is preallocated from both, so neither can move
///   under it.
///
/// WHAT IS DELIBERATELY ABSENT is the rest of the world. That is the correction de-2wtl
/// needed: keying on the world snapshot would invalidate on every line the player reads.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    group: Vec<i32>,
    /// The conversation the request entered at, which the LAYOUT is narrowed to.
    ///
    /// de-3x76.8. A layout built from what one conversation can structurally reach is
    /// smaller than one built from the whole group - 241 variables to 124 on conversation
    /// 368 - so it has to be part of what a workspace is valid for.
    ///
    /// THIS IS WHY THE NARROWING IS PER CONVERSATION AND NOT PER MENU. The plugin sends one
    /// request per conversation, so consecutive menus in the conversation the player is
    /// standing in share this key and share the manager; only walking into another
    /// conversation of the group rebuilds it. A per-menu layout would rebuild it every
    /// menu and hand back more than the smaller layout saves.
    entered_at: Vec<i32>,
    money_ceiling: Option<u32>,
    memory: usize,
    cache_split: usize,
}

/// One request for the owner thread, and where to send the answers.
enum Job {
    Answer {
        /// Boxed, so the channel does not carry a request's size in every job it holds.
        request: Box<LookAheadRequest>,
        /// The answers, or WHY THERE ARE NONE.
        ///
        /// A refusal has to travel as a refusal. It used to come back as an empty answer
        /// list, which the caller could only read as "nothing was established" - so a world
        /// answering the wrong questions, the one failure a positional answer list makes
        /// possible, arrived at the plugin indistinguishable from a search that found
        /// nothing. de-r4e0.
        answers: Sender<Result<Vec<LookAheadAnswer>, String>>,
    },
    /// How many diagram nodes the manager is holding right now.
    ///
    /// ON THE OWNER THREAD, because that is where the manager lives and nothing else may
    /// touch it - one manager per thread is the invariant the whole arrangement rests on.
    /// It queues behind the requests ahead of it like any other job, so what it reports is
    /// the store as of the last request answered rather than a racing snapshot.
    Held(Sender<usize>),
}

/// A live manager for one group, and the thread that owns it.
pub struct Workspace {
    key: Key,
    /// SHARED WITH THE OWNER THREAD, not moved into it, so a hit does not have to rebuild
    /// the graph to find out that it is a hit.
    ///
    /// [`Self::serves`] needs the money ceiling, which is a function of the graph, and
    /// rebuilding the graph to ask would give back the three to nine milliseconds this is
    /// partly here to save. The graph is plain data - parsed guards, actions, links, a
    /// symbol table - and the thread only ever reads it.
    graph: Arc<LookAheadGraph>,
    jobs: Sender<Job>,
    /// Joined on drop, so a replaced workspace's thread and manager are gone before the
    /// next one allocates its own - which matters at six gigabytes.
    thread: Option<JoinHandle<()>>,
}

impl Workspace {
    /// Starts an owner thread holding a manager for `graph`, or `None` where the machine
    /// could not supply one.
    ///
    /// `world` is used ONLY for the money ceiling that sizes the layout; every request
    /// carries its own world for the compiler and the seed.
    ///
    /// `entered_at` is the conversation the requests will start in, which narrows the
    /// layout - see [`Key::entered_at`].
    pub fn open(
        graph: LookAheadGraph,
        group: Vec<i32>,
        entered_at: Vec<i32>,
        world: WorldSnapshot,
        declared: Option<Arc<VariableTable>>,
        budget: DiagramBudget,
    ) -> Option<Self> {
        let graph = Arc::new(graph);
        let key = Key {
            group: group.clone(),
            entered_at: entered_at.clone(),
            money_ceiling: ceiling_of(&graph, &world, declared.clone()),
            memory: budget.memory(),
            cache_split: budget.cache_split(),
        };

        let (jobs, inbox) = std::sync::mpsc::channel::<Job>();
        let (ready, started) = std::sync::mpsc::channel::<bool>();

        // NOT `isolated::on_its_own_thread`, which is scoped and ends with the call. This
        // thread has to outlive it, so everything it touches is owned or shared - the graph
        // by handle, because the caller needs it too.
        let owned = Arc::clone(&graph);
        let thread = std::thread::Builder::new()
            .stack_size(isolated::STACK)
            .spawn(move || {
                own(
                    Opening {
                        graph: owned,
                        group,
                        entered_at,
                        layout_world: world,
                        declared,
                        budget,
                    },
                    inbox,
                    ready,
                )
            })
            .ok()?;

        // WAITED FOR, because the manager is what can fail and the caller has to be told
        // now rather than on the first request. A thread that could not allocate says so
        // and ends; `None` here means fall back to the per-request path.
        match started.recv() {
            Ok(true) => Some(Self {
                key,
                graph,
                jobs,
                thread: Some(thread),
            }),
            _ => {
                let _ = thread.join();
                None
            }
        }
    }

    /// Whether this workspace can serve a request over `group` at `budget` and `world`.
    ///
    /// Asked WITHOUT building a graph, which is the point of holding one: the only thing
    /// the world contributes to the key is the money ceiling, and that is a function of the
    /// graph this already has.
    pub fn serves(
        &self,
        group: &[i32],
        entered_at: &[i32],
        world: &WorldSnapshot,
        declared: Option<Arc<VariableTable>>,
        budget: DiagramBudget,
    ) -> bool {
        self.key.group == group
            && self.key.entered_at == entered_at
            && self.key.memory == budget.memory()
            && self.key.cache_split == budget.cache_split()
            && self.key.money_ceiling == ceiling_of(&self.graph, world, declared)
    }

    /// Answers one request on the owner thread, or `None` if that thread is gone.
    ///
    /// The inner `Err` is a request the engine REFUSED and the reason it gave, which is a
    /// different thing from the `None`: a refusal is an answer about this request, and a
    /// missing thread is a reason to ask somewhere else.
    pub fn answer(
        &self,
        request: LookAheadRequest,
    ) -> Option<Result<Vec<LookAheadAnswer>, String>> {
        let (answers, waiting) = std::sync::mpsc::channel();
        self.jobs
            .send(Job::Answer {
                request: Box::new(request),
                answers,
            })
            .ok()?;
        waiting.recv().ok()
    }

    /// How many diagram nodes this workspace's manager is holding, or `None` if its thread
    /// is gone.
    ///
    /// THE OCCUPANCY OF THE STORE, not what is still referenced - `oxidd`'s
    /// `num_inner_nodes`. Every search this workspace has served dropped its sets as it
    /// went, so this says how far the store GREW rather than how much any one menu needs.
    /// That is the right currency for asking whether the budget is enough, since the budget
    /// is the store's capacity, and the wrong one for asking what a menu costs.
    ///
    /// Here so that what a session accumulates can be measured rather than reasoned about -
    /// see `measurements/workspace_menus.rs` and the note on [`memo_cap`], whose quarter is
    /// argued from the other three quarters rather than from any measurement of this.
    pub fn held(&self) -> Option<usize> {
        let (held, waiting) = std::sync::mpsc::channel();
        self.jobs.send(Job::Held(held)).ok()?;
        waiting.recv().ok()
    }
}

impl Drop for Workspace {
    /// Ends the thread and waits for it, so the manager is released before anything else
    /// asks the machine for one.
    ///
    /// Dropping `jobs` closes the channel, which is what the loop reads as "no more work".
    fn drop(&mut self) {
        // The sender has to go before the join, or the loop is still waiting on it.
        let (dead, _) = std::sync::mpsc::channel();
        let _ = std::mem::replace(&mut self.jobs, dead);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// What a workspace is opened for, handed to its owner thread whole.
///
/// Everything [`Workspace::open`] was given, moved rather than borrowed, because the thread
/// outlives the call that starts it.
struct Opening {
    graph: Arc<LookAheadGraph>,
    group: Vec<i32>,
    entered_at: Vec<i32>,
    /// The world the layout is sized from, and nothing else - every request brings its own.
    layout_world: WorldSnapshot,
    declared: Option<Arc<VariableTable>>,
    budget: DiagramBudget,
}

/// The owner thread's whole life: build once, then answer until the channel closes.
///
/// Every borrow below lives on this stack, with the request loop inside their scope. That
/// is what lets `DataVars` keep borrowing the layout exactly as it does everywhere else.
fn own(opening: Opening, inbox: Receiver<Job>, ready: Sender<bool>) {
    let Opening {
        graph,
        group,
        entered_at,
        layout_world,
        declared,
        budget,
    } = opening;
    let symbols = graph.symbols().clone();
    // THE LAYOUT IS BUILT FROM THE WORLD THAT OPENED THIS, and the key above records which
    // ceiling that produced - so a later request whose money moves the ceiling is refused
    // by `serves` rather than answered against a layout that does not fit it.
    let opening = SnapshotWorld::declaring(layout_world, declared.clone());
    let layout = DataLayout::for_group_entered_at(&graph, &opening, COUNTER_CAP, Some(&entered_at));

    // FALLIBLY, and reported before any request is accepted: the node store is one big
    // preallocation and asking for it infallibly aborts rather than fails - de-0a3a.
    let Some(vars) = DataVars::try_new(&layout, &symbols, budget) else {
        let _ = ready.send(false);
        return;
    };
    let shape = GroupShape::of(&graph);
    // ONCE, like everything else here: the questions a group can ask depend on the graph
    // and on nothing a request carries.
    let questions = crate::bridge::questions_of(&graph, group);
    if ready.send(true).is_err() {
        return;
    }

    while let Ok(job) = inbox.recv() {
        // THE STORE IS READ AND NOTHING ELSE HAPPENS, which is why it is a job at all: the
        // manager belongs to this thread and may not be touched from another.
        let Job::Answer {
            request: job_request,
            answers: job_answers,
        } = job
        else {
            if let Job::Held(held) = job {
                let _ = held.send(vars.node_count());
            }
            continue;
        };

        // RESOLVED FIRST, exactly as `bridge::answer` does: the plugin answers the engine's
        // questions positionally, and `resolve` puts those answers back onto their names.
        // A request whose answers do not line up is refused rather than guessed at, AND THE
        // REASON TRAVELS WITH THE REFUSAL - see `Job::answers`. Sending an empty answer
        // list instead, which is what this did, made a misaligned world look exactly like a
        // search that found nothing (de-r4e0).
        let mut snapshot = job_request.world.clone();
        if let Err(reason) = snapshot.resolve(&questions) {
            let _ = job_answers.send(Err(reason));
            continue;
        }
        let world = SnapshotWorld::declaring(snapshot, declared.clone());

        // PER REQUEST, because these are what the world is baked into. One to five
        // milliseconds against the nine or ten the manager cost once, and unlike the
        // manager they cannot be kept: the compiler holds the clock, the variables, the
        // items, the tasks and the queries, and the seed holds what has been read.
        let mut compiler = GuardCompiler::new(&vars)
            .with_world(&world)
            .with_constant_clock(DataLayout::group_passes_time(&graph));
        // A SEED THE MANAGER HAS NO ROOM FOR is a search that cannot start, and it is
        // answerable rather than fatal: every start is nothing established, exactly as
        // `bridge::answer` answers a manager the machine would not give it. The workspace
        // stays up, because the next request may carry a world that seeds more cheaply.
        let request = &job_request;
        let Some(seed) = seed_of(&graph, &world, &vars) else {
            let _ = job_answers.send(Ok(crate::bridge::all_unanswered(request, "no-ram")));
            continue;
        };

        let novelty = |id: DialogueNodeId| {
            let node = crate::bridge::NodeRef::from(id);
            if request.unseen_any_game.contains(&node) {
                Novelty::UnseenAnyGame
            } else if request.unseen_this_game.contains(&node) {
                Novelty::UnseenThisGame
            } else {
                Novelty::SeenThisGame
            }
        };

        let answers = answer_starts(
            &graph,
            &world,
            request,
            &novelty,
            &mut compiler,
            &seed,
            &shape,
        );

        // A caller that has gone away is not an error - it means the request was abandoned,
        // and the next one is already waiting.
        let _ = job_answers.send(Ok(answers));
    }
}

/// The money ceiling a world produces for a graph, which is the whole of the key's
/// dependence on the world.
///
/// Through the same `DataLayout::money_ceiling` the layout uses, rather than comparing the
/// money itself: a purchase that does not move the ceiling must not throw a manager away,
/// and the ceiling saturates well below the range money actually takes.
fn ceiling_of(
    graph: &LookAheadGraph,
    world: &WorldSnapshot,
    declared: Option<Arc<VariableTable>>,
) -> Option<u32> {
    let world = SnapshotWorld::declaring(world.clone(), declared);
    DataLayout::money_ceiling(graph, world.money())
}
