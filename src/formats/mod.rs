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

pub mod expanded_save;
pub mod header;
pub mod json_diff;
pub mod lua_blob;
pub mod resolve;
pub mod sparse;
pub mod sparse_diff;
pub mod text_diff;
