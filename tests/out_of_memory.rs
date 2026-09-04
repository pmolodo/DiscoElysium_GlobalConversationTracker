// SPDX-License-Identifier: MIT
//! Running out of memory should be a RESULT, not a dead process.
//!
//! ## Why this cannot be tested by running out of memory
//!
//! Because a test that genuinely exhausts the machine takes the machine down with it, and
//! on Windows takes it into paging first, where it stops being a test and becomes a wait.
//! What is worth exercising is the REFUSAL PATH - that when the allocator says no, the
//! engine hands back a value instead of dying - and that can be provoked in milliseconds by
//! making the allocator say no.
//!
//! So this installs a global allocator that refuses any single allocation over an armed
//! threshold. Armed, a six-gigabyte reservation fails exactly as it would on a machine with
//! no six gigabytes free, and the code under test cannot tell the difference: it asked, and
//! it was refused.
//!
//! ## What aborting means here, and why it is worth this much trouble
//!
//! Rust does NOT panic when an allocation fails. `std::alloc::handle_alloc_error`, which
//! every infallible allocation reaches, prints to stderr and ABORTS THE PROCESS - it is not
//! a panic, so `catch_unwind` cannot catch it and no care at the call site helps. The engine
//! ships as a native library inside the game's own process, so an abort is not a lost marker
//! but the player's session gone, from a feature whose whole failure budget was supposed to
//! be "the asterisk does not appear".
//!
//! That is why `DiagramBudget` asks before it spends, and why the asking is worth a test
//! that does more than arithmetic.
//!
//! ## What is still not covered, and is not pretended to be
//!
//! - THE FORWARD CRAWL. Its frontier grows a state at a time in a `HashSet`, so it has no
//!   up-front reservation to probe and no fallible growth either. An allocation failure part
//!   way through a crawl aborts, and its budget only stops it at the ceiling the CALLER set,
//!   which says nothing about what the machine has. Nothing here can change that without
//!   making the frontier's growth fallible.
//! - THE STACK. A deep recursion overflows a guard page and aborts, and that is uncatchable
//!   too - see de-fpax, which is a real occurrence rather than a worry. A refusing allocator
//!   says nothing about it.
//! - HOW MUCH THE MACHINE ACTUALLY HAS FREE. Reading that needs a platform call this
//!   repository does not make anywhere, and a budget four times what is free is a very
//!   different ask from one larger than the address space: Windows may well accept it
//!   against RAM plus pagefile and then pay for it in thrashing.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use lookahead_engine::core::action::DialogueAction;
use lookahead_engine::core::guard::GuardExpression;
use lookahead_engine::core::state::StateSymbols;
use lookahead_engine::core::types::{DialogueCheckKind, DialogueNodeId, Novelty};
use lookahead_engine::graph::graph::LookAheadGraph;
use lookahead_engine::graph::node::LookAheadNode;
use lookahead_engine::symbolic::budget::DiagramBudget;

/// An allocator that refuses any single allocation at or above an armed threshold.
///
/// ## Why a threshold and not a total
///
/// Because the test harness has to keep working while this is armed. Printing a failure
/// message, growing the vector that collects them, and everything libtest does between
/// tests are all allocations, and an allocator that refused them would take the process down
/// in the act of proving that the process does not go down. A threshold well above anything
/// ordinary machinery asks for - see [`ORDINARY`] - refuses only the deliberate ask.
///
/// Returning null is exactly what an allocator does when it cannot serve a request. What
/// happens next is the whole subject: a fallible reservation turns it into an `Err`, and an
/// infallible one turns it into `handle_alloc_error` and an abort.
struct Refusing;

/// The size at or above which allocations are refused, or `usize::MAX` for none.
static REFUSE_AT: AtomicUsize = AtomicUsize::new(usize::MAX);

/// How many allocations have actually been refused, so a test can prove one was.
static REFUSED: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() >= REFUSE_AT.load(Ordering::Relaxed) {
            REFUSED.fetch_add(1, Ordering::Relaxed);
            return std::ptr::null_mut();
        }

        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if layout.size() >= REFUSE_AT.load(Ordering::Relaxed) {
            REFUSED.fetch_add(1, Ordering::Relaxed);
            return std::ptr::null_mut();
        }

        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size >= REFUSE_AT.load(Ordering::Relaxed) {
            REFUSED.fetch_add(1, Ordering::Relaxed);
            return std::ptr::null_mut();
        }

        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

/// The largest single allocation the test harness itself is assumed to make.
///
/// Eight megabytes, which libtest and the printing machinery are nowhere near - and which
/// every budget these tests refuse is orders of magnitude above. It is a ceiling on what
/// stays working rather than a measurement, so it is deliberately generous in both
/// directions.
const ORDINARY: usize = 8 * 1024 * 1024;

/// Arming is process-wide, so the tests in this file take turns.
///
/// EVERY TEST HERE HOLDS IT, not only the ones that arm. Cargo runs the tests in a binary in
/// parallel and the allocator belongs to the process, so a test that allocates a
/// thirty-two-megabyte budget while another has the allocator armed is refused for a reason
/// that has nothing to do with it - and would fail, or abort, at random. The lock is what
/// makes "armed" mean "armed for this test".
static ARMED: Mutex<()> = Mutex::new(());

/// Runs `body` with the allocator refusing anything at or above [`ORDINARY`].
///
/// Returns what `body` returned and how many allocations were refused while it ran. The
/// count matters: a test that passes because the code never asked for anything large is not
/// the test that was wanted, and without this it would look identical to one that asked and
/// was refused.
fn while_refusing<T>(body: impl FnOnce() -> T) -> (T, usize) {
    let _held = alone();

    let before = REFUSED.load(Ordering::Relaxed);
    REFUSE_AT.store(ORDINARY, Ordering::Relaxed);
    let outcome = body();
    REFUSE_AT.store(usize::MAX, Ordering::Relaxed);

    (outcome, REFUSED.load(Ordering::Relaxed) - before)
}

/// Holds the turn, so no other test in this file has the allocator armed meanwhile.
///
/// A poisoned lock is taken anyway: it means another test panicked, and its failure is the
/// one worth reading - not a second failure here about the lock.
fn alone() -> std::sync::MutexGuard<'static, ()> {
    ARMED.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The probe reports a refusal rather than dying of one.
///
/// THE POINT OF THE FALLIBLE RESERVATION, tested against a real refusal for the first time.
/// The unit test beside `can_be_supplied` asks for `usize::MAX / 2`, which any allocator
/// turns down on arithmetic without ever reaching the allocator's own no; this reaches it.
#[test]
fn a_budget_the_allocator_refuses_comes_back_as_a_value() {
    let budget = DiagramBudget::measurement();

    let (supplied, refusals) = while_refusing(|| budget.can_be_supplied());

    assert!(!supplied, "the allocator refused and can_be_supplied said yes anyway");
    assert!(
        refusals > 0,
        "nothing was refused, so this proved only that the code asked for nothing large",
    );

    // AND IT IS THE ALLOCATOR TALKING, not the budget being impossible. The same budget on
    // the same machine is supplied a moment later, which is what says the refusal was
    // observed rather than computed.
    assert!(
        budget.can_be_supplied(),
        "six gigabytes cannot be reserved on this machine even unarmed; the refusal above \
         proved nothing, and this test needs a bigger machine or a smaller budget",
    );
}

/// And the guarded constructor hands back None rather than taking the process with it.
///
/// END TO END, which is what was missing: the probe was tested and the DECISION was not.
/// `manager()` on the same budget under the same conditions would abort - oxidd builds its
/// node store with `Vec::with_capacity`, which calls `handle_alloc_error` - so this is the
/// difference between a feature that degrades and a game that closes.
#[test]
fn a_manager_that_cannot_be_afforded_is_refused_rather_than_aborting() {
    let budget = DiagramBudget::measurement();

    let (manager, refusals) = while_refusing(|| budget.try_manager());

    assert!(manager.is_none(), "a manager came back that the allocator would not pay for");
    assert!(refusals > 0, "nothing was refused, so no manager was ever at risk");

    // The process is still here to say so, which is the whole assertion.
    let afforded = budget.try_manager();
    assert!(afforded.is_some(), "unarmed, the same budget builds a manager");
}

/// A small budget is still a budget: the guard does not refuse what the machine can do.
///
/// The other half, and worth its own test. A probe that answered "no" to everything would
/// pass the tests above and disable the feature.
#[test]
fn a_budget_the_machine_can_meet_is_not_refused() {
    let _alone = alone();

    assert!(
        DiagramBudget::modest().try_manager().is_some(),
        "a modest budget was refused on a machine running this suite",
    );
}

/// A graph built to blow up, so the failure can be provoked at a size a test can afford.
///
/// ## Why synthetic
///
/// Every memory measurement in this repository runs on real conversation groups, so the
/// failure is only ever observed where the shipped content happens to reach it - and a test
/// that needs six gigabytes to be interesting is a test nobody runs. A graph built to branch
/// is bounded by what the test asks for rather than by what the database contains.
///
/// ## The shape
///
/// A binary tree of `depth` levels: every node links to two children, and every node is
/// unseen, so nothing prunes and the frontier doubles per level. `2^depth - 1` nodes, and a
/// crawl that walks all of them.
fn branching(depth: u32) -> LookAheadGraph {
    let symbols = StateSymbols::new();
    let mut nodes = Vec::new();

    let count = (1u32 << depth) - 1;
    for id in 0..count {
        // The heap of a binary tree: the children of `id` are `2id + 1` and `2id + 2`, and
        // a node whose children fall past the end is a leaf.
        let links: Vec<DialogueNodeId> = [2 * id + 1, 2 * id + 2]
            .into_iter()
            .filter(|child| *child < count)
            .map(|child| DialogueNodeId::new(1, child as i32))
            .collect();

        nodes.push(LookAheadNode::new(
            DialogueNodeId::new(1, id as i32),
            false,
            DialogueCheckKind::None,
            GuardExpression::always_true(),
            Vec::<DialogueAction>::new(),
            links,
            0,
            false,
            false,
            -1,
            -1,
            false,
            -1,
        ));
    }

    LookAheadGraph::new(nodes, symbols).expect("a binary tree is a graph")
}

/// A crawl over a graph built to branch ends with a verdict, not with a dead process.
///
/// ## What this covers that the tests above do not
///
/// Those are about the SYMBOLIC side, where the whole allowance is reserved up front and
/// can therefore be asked about. This is the forward crawl, which has no such reservation:
/// it grows its frontier a state at a time and is stopped by the budget the caller set. What
/// can be checked of it is that the budget is honoured and reported - that a search too big
/// for its allowance comes back saying so.
///
/// It is NOT a test that the crawl survives the machine running out; see the note at the top
/// of this file. Nothing here makes that true, and pretending otherwise with a passing test
/// would be worse than the gap.
#[test]
fn a_crawl_too_big_for_its_budget_says_so_instead_of_dying() {
    use lookahead_engine::engine::engine::{LookAheadEngine, LookAheadOptions};
    use lookahead_engine::world::test_world::TestWorld;

    // Building 65,535 nodes is itself a large allocation, so this waits its turn like the
    // rest - a tree built while another test has the allocator armed would be refused, and
    // a Vec's growth is not fallible.
    let _alone = alone();

    // Sixteen levels is 65,535 nodes, which builds in well under a second and is far more
    // than the budget below allows the search to hold.
    let graph = branching(16);
    let world = TestWorld::new();

    let engine = LookAheadEngine::new(LookAheadOptions {
        // A budget that stops the search early, so the reported outcome is the budget's.
        memory_budget: 64 * 1024,
        ..Default::default()
    });

    let result = engine.evaluate(
        &graph,
        DialogueNodeId::new(1, 0),
        &world,
        // NOTHING IS SEEN, so nothing prunes and the search has every reason to keep going.
        |_| Novelty::UnseenThisGame,
    );

    assert!(
        result.budget_exhausted(),
        "a 65,535 node tree fitted in 64 KB, so this measured nothing; the budget or the \
         tree needs to move",
    );

    // The assertion that matters is that there is a `result` to read at all - the process
    // is here, the search reported, and the caller can decide what to draw.
    println!(
        "{} states over {} entries, stopped by {:?}",
        result.states_explored, result.nodes_reached, result.stopped_by,
    );
}
