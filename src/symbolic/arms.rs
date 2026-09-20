// SPDX-License-Identifier: MIT
//! The engine options a MEASUREMENT varies, carried rather than read where they are wanted.
//!
//! ## Why a struct, and why one struct
//!
//! These are read a long way from any `main`: the variable ordering inside
//! `DataLayout::for_group` is three calls below wherever a search is set up. An option read at
//! the point of use is one decided somewhere no caller can see - and a measurement can then be
//! taken under an arm nobody asked for, because nothing between the command line and the read
//! says which arm is in force.
//!
//! DEPTH IS NOT A REASON TO LEAVE THEM THERE. The objection to threading them - that it would
//! change signatures having nothing else to do with the option - is an objection to threading
//! options ONE AT A TIME. This is one parameter, and the next arm to arrive costs no signature
//! at all. See de-3dx9.
//!
//! ONE STRUCT EVEN AT ONE ARM, because what it carries is a KIND of thing - an arm a measurement
//! turns on - rather than one particular option, and the next one costs no signature. A callable
//! that carries an arm it does not read is the price, and it is smaller than threading options
//! one at a time again.
//!
//! ## The default is what ships, and that is the whole point
//!
//! `Arms::default()` is the shipped algorithm: the interning variable order. Every path the game
//! takes builds one, so a default measurement, a default in-game
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
}

impl Arms {
    /// The shipped algorithm, named rather than spelled `default()` at a call site that means
    /// it deliberately.
    pub fn shipped() -> Self {
        Self::default()
    }
}
