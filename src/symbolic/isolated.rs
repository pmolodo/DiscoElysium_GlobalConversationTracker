// SPDX-License-Identifier: MIT
//! One search, one thread.
//!
//! ## The fault this exists for
//!
//! Something accumulates PER THREAD inside the diagram manager. Twelve rounds of an
//! identical search over conversation 28 die on the third when they share a thread, and all
//! twelve survive when each gets its own; running all twelve on one SPAWNED thread also dies
//! on the third, which rules out stack size as the cause. The node count is identical every
//! round, so nothing is growing in the sets themselves. That is de-8hh2.13's finding, and
//! de-fpax is this: give each search a thread and the accumulation has nowhere to build up.
//!
//! The symptom is a STATUS_STACK_OVERFLOW inside a recursive diagram operation, which takes
//! the whole process with it - no panic to catch, no row printed, nothing after it runs. It
//! has cost this repository entire measurement runs: conversations 1030, 368 and 14 all
//! died partway through a sweep, repeatedly, and the workaround until now was one process
//! per conversation, which only limits the damage rather than avoiding it.
//!
//! ## What has to happen inside the thread, and why the seam is here
//!
//! EVERYTHING THE DIAGRAM TOUCHES. The manager, the variable table, the compiled guards and
//! the sets are all built inside and dropped inside; only plain numbers come back out. That
//! is not tidiness - it is the configuration that was actually verified. Building a manager
//! on one thread and running the search on another is a different arrangement and is not
//! known to help, quite apart from whether the diagram handles may cross a thread at all.
//!
//! So this takes a closure that builds its own world from scratch, rather than a search to
//! run against something prepared outside. A caller that wants to share work across searches
//! shares the things that are plain data - the parent map, the iteration order, the graph -
//! and rebuilds the diagram side per thread. See [`crate::symbolic::known::Known`], all of
//! whose contents are either plain data or formulas belonging to one manager.
//!
//! ## The stack size is belt and braces
//!
//! de-8hh2.13 ruled out stack size as the CAUSE - one big spawned thread dies just the same -
//! but releasing a large diagram still walks it recursively, so a thread that is going to
//! hold one wants room. [`STACK`] is what the measurements had already settled on
//! independently.

/// A search thread's stack.
///
/// Not the fix - see the module note - but the size the measurements arrived at before this
/// was factored out, and there is no reason to make a search thread thrifty.
pub const STACK: usize = 512 * 1024 * 1024;

/// Runs `search` on a thread of its own and hands back what it returned.
///
/// SCOPED, so the closure may borrow the graph, the world and the layout from the caller.
/// What it must NOT borrow is anything belonging to a diagram manager, because it should be
/// building that itself - see the module note.
///
/// Panics from inside are re-raised here rather than swallowed, so a search that fails looks
/// exactly as it would have without the thread.
pub fn on_its_own_thread<T, F>(search: F) -> T
where
    F: FnOnce() -> T + Send,
    T: Send,
{
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(STACK)
            .spawn_scoped(scope, search)
            .expect("a thread for the search")
            .join()
            .unwrap_or_else(|panicked| std::panic::resume_unwind(panicked))
    })
}
