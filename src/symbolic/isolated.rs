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
//! ## What actually accumulates: a SECOND MANAGER on a thread, not a second search
//!
//! Measured 2026-09-06 with `measurements/search_residue.rs`, conversation 28, five searches
//! at the matrix's six-gigabyte budget, twenty-five runs of each arrangement. The searches
//! are identical in all of them; only how many threads and how many MANAGERS differ.
//!
//! ```text
//!   arrangement                                                  died
//!   a manager per search, the process's own thread              8 / 20
//!   a manager per search, all on one 512 MB spawned thread     13 / 45
//!   a manager per search, a fresh spawned thread each           0 / 20
//!   ONE manager, many searches, one spawned thread              0 / 35
//! ```
//!
//! So it is not the number of searches and it is not the stack: a thread may run as many
//! searches as it likes against ONE manager and never fail, and giving a thread more stack
//! only leaves the rate where it was. What costs is BUILDING A MANAGER ON A THREAD THAT HAS
//! ALREADY BUILT ONE - the third is the one that overflows, which is exactly the row the
//! matrix has always died on.
//!
//! THE INVARIANT IS THEREFORE ONE MANAGER PER THREAD, and this helper is how it is kept: a
//! caller that builds its world inside the closure gets a manager that is the only one its
//! thread will ever see. A caller that built one outside and searched inside would satisfy
//! the letter of "everything inside" and not this.
//!
//! It also says `bridge::answer` is right as it stands, which was an open question: it takes
//! one of these threads PER REQUEST and runs a whole menu's starts on it, against one manager
//! built inside. `measurements/menu_residue.rs` puts that arrangement to
//! `bridge::answer` directly - forty-five runs, budgets to six gigabytes, up to
//! twenty-four starts, and once at three hundred and eighty-four - and none of them died.
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
/// exactly as it would have without the thread. A caller that would rather lose the ANSWER
/// than the process wants [`on_its_own_thread_caught`]; the tests and the measurements want
/// this one, because a panic there is the finding.
pub fn on_its_own_thread<T, F>(search: F) -> T
where
    F: FnOnce() -> T + Send,
    T: Send,
{
    on_its_own_thread_caught(search).unwrap_or_else(|panicked| std::panic::resume_unwind(panicked))
}

/// The same thread, but a panic inside comes back as an error instead of being re-raised.
///
/// ## What it is for
///
/// A search that panics should cost its answer, not the process. The engine runs as a child
/// process the mod talks to over a pipe, so a panic here is not a stack trace somebody reads
/// - it is the engine vanishing mid-menu, and the mod reporting that it "stopped answering
/// while reading a frame length". Every question in flight is lost, and what replaces the
/// answer is a dead engine rather than an unmarked option.
///
/// The caller gets to decide what an unfinished search looks like, which is why this returns
/// the payload rather than some answer of its own: only the caller knows the shape of the
/// thing it was asking for. [`crate::bridge`] turns it into the same "nothing established"
/// every start gets when the machine cannot supply a manager.
///
/// ## WHAT IT DOES NOT CATCH, and this is the important half
///
/// A STACK OVERFLOW IS NOT A PANIC. It is STATUS_STACK_OVERFLOW and it takes the process
/// down with no unwinding, so there is nothing here to catch - see the module note, which is
/// the fault this whole module was written for. This makes an ORDINARY panic survivable and
/// changes nothing about the one that made the thread necessary. Do not read the existence
/// of this function as cover for that.
///
/// A panic while a large diagram is being DROPPED is the same story: unwinding out of a
/// destructor aborts, and dropping is where the recursion the stack is sized for happens.
///
/// ## Why nothing poisoned escapes
///
/// The closure builds its manager inside the thread and owns it, which the module note
/// requires for a different reason. So a panic unwinds that thread and drops the manager on
/// the way out; there is no half-finished diagram left for anything else to pick up, because
/// nothing outside ever had a handle to it.
pub fn on_its_own_thread_caught<T, F>(search: F) -> Result<T, Box<dyn std::any::Any + Send>>
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
    })
}

/// What a caught panic said, for a log line.
///
/// Rust hands a panic's payload back as `Any`, and the two shapes it takes in practice are
/// the ones `panic!` produces: a `&'static str` for a literal and a `String` for a format.
/// Anything else is a payload somebody chose deliberately and there is nothing useful to say
/// about it.
pub fn panic_message(panicked: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = panicked.downcast_ref::<&'static str>() {
        (*text).to_string()
    } else if let Some(text) = panicked.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic with no message".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_that_finishes_hands_back_what_it_returned() {
        assert_eq!(on_its_own_thread_caught(|| 6 * 7).ok(), Some(42));
    }

    /// THE POINT OF THE WHOLE FUNCTION, pinned with an explicit panic rather than by
    /// starving a real search.
    ///
    /// A test that provoked the panic through a tiny memory budget would be testing
    /// de-x8ms.9's bug as much as this containment, and would start failing the moment that
    /// bug is fixed - which would be a fix breaking a test that exists to protect it.
    #[test]
    fn a_search_that_panics_comes_back_as_an_error_rather_than_unwinding() {
        let caught = on_its_own_thread_caught(|| -> i32 { panic!("the diagram ran out") });

        let Err(panicked) = caught else {
            panic!("the panic was not caught");
        };
        assert_eq!(panic_message(panicked.as_ref()), "the diagram ran out");
    }

    #[test]
    fn a_formatted_panic_keeps_its_message_too() {
        let nodes = 37_081_132;
        let caught = on_its_own_thread_caught(|| -> i32 { panic!("out of room at {nodes}") });

        let Err(panicked) = caught else {
            panic!("the panic was not caught");
        };
        assert_eq!(panic_message(panicked.as_ref()), "out of room at 37081132");
    }
}
