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
//! So this binary installs a global allocator with a HEAP CAP. Under it, the code runs
//! normally until the total it holds would pass the cap, and then every further request is
//! refused - which is what running out of memory IS, reproduced in milliseconds on a
//! process holding a few megabytes. The code under test cannot tell the difference: it
//! asked, and it was refused.
//!
//! A cap on the total rather than a threshold on one allocation, because that is how a
//! machine actually runs out: not on one enormous request, but on a search growing in
//! doublings that are individually unremarkable until there is nothing left.
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
//! - THE STATE ITSELF. The search's frontier now GROWS FALLIBLY - see `room_for` - so the
//!   two collections that hold every state it has seen report instead of aborting. What
//!   they hold does not: a `StateKey` keeps its slots on the heap, and cloning one
//!   allocates infallibly like anything else. The collections are where the memory goes and
//!   where the failing ask happens, so this converts the realistic case rather than making
//!   the search allocation-safe.
//! - THE STACK. A deep recursion overflows a guard page and aborts, and that is uncatchable
//!   too - see de-fpax, which is a real occurrence rather than a worry. A heap cap says
//!   nothing about it.
//! - HOW MUCH THE MACHINE ACTUALLY HAS FREE. Reading that needs a platform call this
//!   repository does not make anywhere, and a budget four times what is free is a very
//!   different ask from one larger than the address space: Windows may well accept it
//!   against RAM plus pagefile and then pay for it in thrashing.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use lookahead_engine::symbolic::budget::DiagramBudget;

/// An allocator with a HEAP CAP: it refuses once this binary's live bytes would pass one.
///
/// ## A cap on the total, not a threshold on one allocation
///
/// Because that is how a machine actually runs out. A search does not die on one enormous
/// request; it grows, in doublings that are individually unremarkable, until there is
/// nothing left. A cap reproduces that - and it reproduces it in milliseconds, on a
/// process that never holds more than a few megabytes, so nothing on the machine is
/// disturbed by proving what happens when memory runs out.
///
/// ## Why an allocator and not a real limit
///
/// Rust has no maximum-heap switch, and neither does a Rust binary: the portable way to
/// bound a heap is exactly this, a `#[global_allocator]` that counts and refuses. (The
/// `cap` crate is this, packaged; twenty lines is cheaper than a dependency here.) An OS
/// limit - `ulimit -v`, a job object - would bound the whole PROCESS, which on Windows
/// includes the test harness, and would take the run down with the subject.
///
/// ## Why this is its own test binary
///
/// The allocator is the process's. Cargo gives each integration test its own executable,
/// so the cap here reaches this file's tests and nothing else in the suite.
///
/// Returning null is exactly what an allocator does when it cannot serve a request. What
/// happens next is the whole subject: a fallible reservation turns it into an `Err`, and an
/// infallible one turns it into `handle_alloc_error` and an abort.
struct Capped;

/// What this binary is holding, in bytes.
static LIVE: AtomicUsize = AtomicUsize::new(0);

/// The most it may hold, or `usize::MAX` for no cap.
static CAP: AtomicUsize = AtomicUsize::new(usize::MAX);

/// How many allocations the cap has refused, so a test can prove one was.
static REFUSED: AtomicUsize = AtomicUsize::new(0);

impl Capped {
    /// Takes `size` out of the cap, or refuses.
    fn take(size: usize) -> bool {
        let cap = CAP.load(Ordering::Relaxed);
        if cap == usize::MAX {
            LIVE.fetch_add(size, Ordering::Relaxed);
            return true;
        }

        // A compare-and-swap loop rather than a fetch-add and a check, so two threads
        // cannot both see room for the last byte. libtest allocates on its own threads
        // while a test runs, so this is not hypothetical.
        let mut live = LIVE.load(Ordering::Relaxed);
        loop {
            if live.saturating_add(size) > cap {
                REFUSED.fetch_add(1, Ordering::Relaxed);
                return false;
            }

            match LIVE.compare_exchange_weak(
                live,
                live + size,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(seen) => live = seen,
            }
        }
    }

    fn give_back(size: usize) {
        LIVE.fetch_sub(size, Ordering::Relaxed);
    }
}

unsafe impl GlobalAlloc for Capped {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !Self::take(layout.size()) {
            return std::ptr::null_mut();
        }

        let pointer = unsafe { System.alloc(layout) };
        if pointer.is_null() {
            Self::give_back(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        Self::give_back(layout.size());
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !Self::take(layout.size()) {
            return std::ptr::null_mut();
        }

        let pointer = unsafe { System.alloc_zeroed(layout) };
        if pointer.is_null() {
            Self::give_back(layout.size());
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // The GROWTH is what has to fit; a shrink always does.
        if new_size > layout.size() && !Self::take(new_size - layout.size()) {
            return std::ptr::null_mut();
        }

        let moved = unsafe { System.realloc(pointer, layout, new_size) };
        if moved.is_null() {
            if new_size > layout.size() {
                Self::give_back(new_size - layout.size());
            }
        } else if new_size < layout.size() {
            Self::give_back(layout.size() - new_size);
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Capped = Capped;

/// How much a capped test is allowed to allocate ON TOP of what is already held.
///
/// FOUR MEGABYTES, and the number is a compromise with one job on each side. It has to be
/// big enough that libtest, the panic machinery and the formatting of a failure message all
/// still work while the cap is on - a test that ran out of memory while reporting that
/// something ran out of memory would be a poor joke. It has to be small enough that a search
/// reaches it in a moment rather than after filling the machine.
const HEADROOM: usize = 4 * 1024 * 1024;

/// Arming is process-wide, so the tests in this file take turns.
///
/// EVERY TEST IN THIS FILE TAKES IT, exactly once, at the top. Once, because the guard is a
/// plain `Mutex` and taking it twice on one thread deadlocks - which looks exactly like a
/// slow test, and did. Every test, because the allocator belongs to the process and one
/// test allocating freely while another has the cap on would be refused for a reason that
/// has nothing to do with it.
static ARMED: Mutex<()> = Mutex::new(());

/// Runs `body` under a heap cap of [`HEADROOM`] above what is held now.
///
/// Returns what `body` returned and how many allocations the cap refused while it ran. The
/// count matters: a test that passes because the code never asked for much is not the test
/// that was wanted, and without this it would look identical to one that asked and was
/// refused.
///
/// The caller holds the turn - see [`alone`] - and this does not take it, so a test can do
/// something uncapped as well as capped under one guard.
fn under_a_heap_cap<T>(body: impl FnOnce() -> T) -> (T, usize) {
    let before = REFUSED.load(Ordering::Relaxed);
    CAP.store(LIVE.load(Ordering::Relaxed) + HEADROOM, Ordering::Relaxed);
    let outcome = body();
    CAP.store(usize::MAX, Ordering::Relaxed);

    (outcome, REFUSED.load(Ordering::Relaxed) - before)
}

/// Holds the turn, so no other test in this file has the cap on meanwhile.
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
    let _alone = alone();
    let budget = DiagramBudget::measurement();

    let (supplied, refusals) = under_a_heap_cap(|| budget.can_be_supplied());

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
    let _alone = alone();
    let budget = DiagramBudget::measurement();

    let (manager, refusals) = under_a_heap_cap(|| budget.try_manager());

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

// WHAT IS NOT COVERED HERE: a search that runs out of room mid-flight, as against one
// that is refused the room up front. The manager preallocates, so the abort this file is
// about can only happen at construction - DiagramBudget::can_be_supplied reserves the same
// bytes fallibly first, and the tests above are that claim. A search that spends its
// allowance while running reports it as a verdict instead, which is
// symbolic::reachability's business rather than this file's.
