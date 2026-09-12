// SPDX-License-Identifier: MIT
//! The file formats this repository defines, read and written in one place.
//!
//! ONE DEFINITION, IN ONE LANGUAGE. Every format here was read by two programs in two
//! languages before, which is the arrangement that has already cost this repository twice:
//! a fact stated twice drifts, and the half that drifts is whichever one no test happens to
//! exercise. C# reaches these through a verb on the engine host rather than through a
//! second implementation of them.
//!
//! WHAT A DOCUMENT IS IS WRITTEN AT THE TOP OF IT. [`header`] is the two fields every file
//! carries and the one rule for reading them; [`json_diff`] is what one document changes in
//! another; [`resolve`] follows a diff to what it is a diff of, and to what that is a diff
//! of, until it reaches a whole document; [`sparse`] is the ordered, array-less tree a
//! save's Lua tables are stored as, and [`sparse_diff`] is what one of those changes in
//! another; [`text_diff`] applies what git wrote for the members that are not JSON; and
//! [`expanded_save`] is a save written as a change to another save, with the members that
//! come of resolving one.
//!
//! A SAVE'S OWN DATA lives in five more. [`lua_blob`] is the binary form the game writes,
//! [`lua_sparse`] turns that into the tree it is stored as on disk and back, [`lua_manifest`]
//! is what the second knows about the shape of the first, [`lua_simx`] is the one place
//! a save repeats itself - the variables that are a second copy of the conversations, left
//! out and rebuilt - and [`lua_parts`] is the directory those trees are split across, read
//! back as one blob. [`runs`] is how all of them - and the wire, and the mod's own state
//! file - spell a dense run of integers. [`packed_save`] puts the two halves of a save back
//! together as the archive the game loads.
//!
//! AND ONE THAT IS NOT A SAVE'S. [`global_state`] is the mod's own record of what a player
//! has read across every save, which is the only file here that cannot be regenerated.

pub mod expanded_save;
pub mod global_state;
pub mod header;
pub mod json_diff;
pub mod lua_blob;
pub mod lua_manifest;
pub mod lua_parts;
pub mod lua_simx;
pub mod lua_sparse;
pub mod packed_save;
pub mod resolve;
pub mod runs;
pub mod sparse;
pub mod sparse_diff;
pub mod text_diff;
