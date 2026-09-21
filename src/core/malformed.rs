// SPDX-License-Identifier: MIT
//! Saying that a guard calls something the guard language cannot mean.
//!
//! ## What these are
//!
//! A handful of questions take an argument and can only be answered when it has the right
//! shape: `IsHour` wants a number, `SubstanceUsedOnce` wants the substance's name as text,
//! `IsHighestPolitical` wants a reputation's name. A call that gives something else is not a
//! hard question - it is a line in the database that says something the language does not
//! define, which is a CONTENT BUG rather than a state of the world.
//!
//! ## Why it is worth a line rather than a silent Unknown
//!
//! Such a call answers Unknown, and Unknown is what a symbolic search cannot prune on - so
//! both branches of the guard stay in the crawl and the entry behind it can never be marked.
//! The symptom in the game is a marker no play can clear, on an entry nobody can reach, and
//! nothing connects that back to the guard that caused it.
//!
//! The shipped database contains none of these as far as anything has measured - see
//! de-m11s.6 - so a line here is a thing to investigate rather than noise to filter. That is
//! exactly why it should be said: the case is rare enough that its absence is the expectation.
//!
//! ON STDERR, which is this process's only channel that is not the wire, and ONCE per call,
//! because a search asks the same guard thousands of times as it fans out. The same reasoning
//! and the same shape as `index::undeclared_variable_warning`.

use std::collections::HashSet;
use std::sync::Mutex;

/// Says, once per call, that `name` was given an argument it cannot be answered from.
///
/// `wanted` says what the shape should have been, in the words a reader of the database would
/// use - "a number", "the substance's name as text".
pub(crate) fn call_warning(name: &str, wanted: &str) {
    static NAMED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

    let Ok(mut named) = NAMED.lock() else {
        return;
    };
    if !named
        .get_or_insert_with(HashSet::new)
        .insert(format!("{name}\u{1}{wanted}"))
    {
        return;
    }

    eprintln!(
        "look-ahead: a guard calls {name} with an argument that is not {wanted}, so it cannot \
         be answered and the guard is left undecided. The entry behind it can never be shown. \
         This is a line in the database rather than a state of the world - see de-m11s.6."
    );
}
