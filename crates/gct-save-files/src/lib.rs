// SPDX-License-Identifier: MIT
//! How a save is stored in this repository, which is not how the game writes one.
//!
//! The game writes an archive. What sits beside it here is the EXPANDED form: a manifest
//! naming the save it is a change to, the Lua tables split across a directory, the diffs
//! between one save and the next, and a packer that puts an archive back together. A test,
//! a fixture or a tool reads all of it; a running game reads none of it, which is why it is
//! apart from [`gct_formats`] rather than beside it. See de-2p8j.1, and that crate's own
//! notes for the dependency list the split is really about.
//!
//! ## What is in it
//!
//! WHAT ONE DOCUMENT CHANGES IN ANOTHER: [`json_diff`] for the JSON members, [`text_diff`]
//! for the ones git wrote, and [`sparse_diff`] for the Lua tables. [`resolve`] follows a
//! diff to what it is a diff of, and to what that is a diff of, until it reaches a whole
//! document. [`cycle_refs`] is the one thing in a save's JSON that a round trip through a
//! diff can break, put back before the game is asked to read it.
//!
//! HOW A SAVE IS ASSEMBLED: [`expanded_save`] is a save written as a change to another,
//! with the members that come of resolving one; [`lua_sparse`] turns the game's blob into
//! the tree it is stored as and back; [`lua_parts`] is the directory those trees are split
//! across, read back as one blob; [`packed_save`] puts the two halves together as the
//! archive the game loads, and [`expand`] takes one apart again.
//!
//! AND TWO ABOUT THE FORMATS THEMSELVES: [`registry`] is every format this repository
//! defines and the version of it this build writes, and [`convert`] brings a document
//! written by an older build up to it.
//!
//! ## Why it re-exports the other crate
//!
//! ONE PATH FOR EVERYTHING, for the callers that want everything - the tools, the tests, the
//! engine host. They are outside the game and the split is not about them, so making each
//! of them name two crates would be churn that teaches nobody anything. The modules below
//! reach their own dependencies through these re-exports for the same reason.

pub use gct_formats::{
    global_state, header, lua_blob, lua_manifest, lua_simx, runs, save_statuses, sparse,
};

pub mod convert;
pub mod cycle_refs;
pub mod expand;
pub mod expanded_save;
pub mod json_diff;
pub mod lua_parts;
pub mod lua_sparse;
pub mod packed_save;
pub mod registry;
pub mod resolve;
pub mod sparse_diff;
pub mod text_diff;
