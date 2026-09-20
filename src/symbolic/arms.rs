// SPDX-License-Identifier: MIT
//! The engine options a MEASUREMENT varies, carried rather than read where they are wanted.
//!
//! ## Why a struct, and why one struct
//!
//! These are read a long way from any `main`: the variable ordering inside
//! `DataLayout::for_group`, which is three calls below wherever a search is set up, and the
//! pooled scheduler inside `menu::mark_menu_blocking`, which is inside the marking itself. Both
//! used to read the environment at the point of use, which is how an option comes to be decided
//! somewhere no caller can see - and how a measurement can be taken under an arm nobody asked
//! for, because nothing between the command line and the read says which arm is in force.
//!
//! DEPTH IS NOT A REASON TO LEAVE THEM THERE. The objection to threading them - that it would
//! change signatures having nothing else to do with the option - is an objection to threading
//! options ONE AT A TIME. This is one parameter, and the next arm to arrive costs no signature
//! at all. See de-3dx9.
//!
//! ONE STRUCT RATHER THAN ONE PER CONSUMER, because the two consumers are the same KIND of
//! thing - an arm a measurement turns on - and they are set together, from one command line, by
//! whoever is taking the measurement. A callable that carries an arm it does not read is the
//! price, and it is smaller than two structs travelling the same route.
//!
//! ## The default is what ships, and that is the whole point
//!
//! `Arms::default()` is the shipped algorithm: the interning variable order, and no pooled
//! rounds. Every path the game takes builds one, so a default measurement, a default in-game
//! run and a default offline test all do what the product does - which is the rule in CLAUDE.md
//! that a green test or a measured number must describe code the game runs.

use crate::symbolic::var_order::Ordering;

/// Which arms a run is taken under. See the module note.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Arms {
    /// How the layout orders its variables.
    ///
    /// SPAN IS A PROXY AND ON THIS DIALOGUE SET IT MISLEADS - see `var_order` for the numbers -
    /// so an ordering is worth MEASURING rather than worth assuming, which is why this is an
    /// arm at all rather than a decision taken once.
    pub var_order: Ordering,

    /// How a round of the exact marking searches for the nearest target.
    ///
    /// THREE SEARCHES FOR ONE QUESTION, and they disagree about cost rather than about the
    /// answer. See [`Rounds`] for what each one does and what it measured.
    pub rounds: Rounds,
}

/// How a round of the exact marking searches, and what each way costs.
///
/// ## The measured comparison
///
/// Conversation 761 at link-deepest-10 with the limits off - the group the arms were built for -
/// as medians of three runs with a cold pass discarded. Each arm holds its node count to the
/// node across its runs, so the nodes are the arm and the seconds are the machine.
///
/// ```text
///                        carried (bea707f)      dropped (35fccbc)
///   per-target backward  131,399 ms  93.4M      5,198 ms  4.81M     25.3x
///   pooled meeting        20,593 ms  14.7M      8,910 ms  6.44M      2.3x
/// ```
///
/// THE RANKING IS THE REDUNDANT COUNTERS' DOING. They were worth an order of magnitude more to
/// the per-target arm than to the pool, so the pool wins by 6.4x with them and loses by 1.7x
/// without. The reading that fits, though nothing measures it on its own: one pooled race pays
/// for a wide variable once a round where a per-target search pays for it again per target, so a
/// redundant variable is worth most to the arm that revisits it.
///
/// ## All three, at two depths
///
/// Same group and same three-run medians, with the counters dropped:
///
/// ```text
///                        unseen 5                 unseen 10
///   per-target backward    4,262 ms  4.41M          5,193 ms  4.81M
///   pooled meeting         6,249 ms  6.34M          8,874 ms  6.44M
///   per-target meeting     6,715 ms  6.34M        454,450 ms   131M   settled 3 of 8
/// ```
///
/// THE LAST FIGURE IS NOT A TIMING. At link-deepest-10 the per-target meeting does not answer
/// the menu at all: it runs past a five-minute wall, holds 27x the nodes the default holds and
/// settles three options of eight, so its milliseconds are a floor rather than a cost.
///
/// A forward front is rebuilt every round, and with the counters gone the backward half it
/// replaces is cheap - so the meeting arms now pay for the expensive half to save a cheap one.
/// The pool at least stops a whole round at the first meeting; the per-target arm grows the
/// shared front deeper for every target the bound does not skip. Weighing fronts by entry count
/// instead, which is what this arm was measured with when it was first evaluated, makes it worse
/// again: 21,874 ms and 16.8M nodes at unseen 5.
///
/// The arms do not always mark the same options, and that is not a defect in any of them - see
/// the rule in CLAUDE.md. `tests/menu_oracle.rs` runs every case under every arm, which is what
/// holds each one to the distances exhaustive search finds.
///
/// It is an arm at all because the exact marking runs on 133 menus of 389. See de-y04p, and
/// de-t329 for the ceiling: step 2 costs 451 ms across the whole game, over the 48 menus that
/// reach it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Rounds {
    /// ONE BACKWARD CRAWL PER TARGET, in bound order, stopping at the first target whose bound
    /// cannot beat the best distance proven this round. What the product does.
    #[default]
    PerTargetBackward,

    /// PER TARGET, MEETING IN THE MIDDLE. The bound order, the attribution and what a round
    /// claims are the default's; only the direction each pass walks differs, so a comparison
    /// against the default says what meeting is worth on its own. The forward walk is the same
    /// for every target in a round - the same options, the same cut - so it is built once and
    /// grown as far as each target asks.
    PerTargetMeeting,

    /// ONE POOL FOR THE WHOLE ROUND, forward and backward. The shared walk of the meeting arm,
    /// plus deciding the round's winner by whose crawl meets first - which proves nothing per
    /// target, so the bound and the unreachable set stay empty and the next round starts from
    /// nothing.
    PooledMeeting,
}

impl Arms {
    /// The shipped algorithm, named rather than spelled `default()` at a call site that means
    /// it deliberately.
    pub fn shipped() -> Self {
        Self::default()
    }
}
