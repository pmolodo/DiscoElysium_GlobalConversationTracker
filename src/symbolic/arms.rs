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

    /// Whether a round of the exact marking is searched by one pool rather than by branch and
    /// bound.
    ///
    /// OFF UNLESS ASKED, and the pool is the SLOWER of the two on the menu it was built for.
    /// Measured 2026-09-19 at ff1db64, conversation 761 at link-deepest-10 with the limits off,
    /// medians of three runs with a cold pass discarded: branch and bound answers in 4,810,836
    /// diagram nodes and 5.2 seconds, the pool in 6,444,119 and 8.9. Every run of each arm held
    /// its arm's node count to the node, so the nodes are the arm and the seconds are the
    /// machine. What the pool once won by - tens of millions of nodes - was mostly a
    /// redundant counter's doing, and dropping those took branch and bound from 93,353,567 nodes
    /// to the 4.8 million above without changing a mark. So a run measuring the pool is asking
    /// whether anything is left of its advantage, rather than turning on a known win.
    ///
    /// The two do not always mark the same options, and that is not a defect in either - see the
    /// rule in CLAUDE.md. On this menu they star four each, agreeing on three.
    ///
    /// It is an arm at all because the exact marking runs on 133 menus of 389. See de-y04p, and
    /// de-t329 for the ceiling: step 2 costs 451 ms across the whole game, over the 48 menus
    /// that reach it.
    pub pooled_rounds: bool,
}

impl Arms {
    /// The shipped algorithm, named rather than spelled `default()` at a call site that means
    /// it deliberately.
    pub fn shipped() -> Self {
        Self::default()
    }
}
