// SPDX-License-Identifier: MIT
//! The formats a RUNNING GAME reads and writes, defined once.
//!
//! ONE DEFINITION, IN ONE LANGUAGE. Every format here was read by two programs in two
//! languages before, which is the arrangement that has already cost this repository twice:
//! a fact stated twice drifts, and the half that drifts is whichever one no test happens to
//! exercise. C# reaches these through a verb on the engine host rather than through a
//! second implementation of them - except inside the game, which links a library built from
//! this crate instead.
//!
//! ## Why this is a crate, and why it is only half the formats
//!
//! Because of that library. What it drags in is the whole question: built from the engine
//! crate it would be the thirty megabytes the root `Cargo.toml` records the removal of, and
//! the plugin already ships that engine once as the host binary.
//!
//! So the line is WHO READS IT. What the game itself opens while a player is playing is
//! here; how this repository stores a save on disk - the manifest, the diffs, the packer -
//! is `gct_save_files`, which depends on this one. That split is a dependency list as much
//! as a principle: a text diff needs `diffy`, an archive needs `zip`, and a packed save's
//! name needs a clock, and none of the three belongs in what the game links. See de-2p8j.1.
//!
//! Nothing here knows about the search either, and nothing ever did.
//!
//! ## What is in it
//!
//! WHAT A DOCUMENT IS IS WRITTEN AT THE TOP OF IT, and [`header`] is the two fields every
//! file carries and the one rule for reading them. [`runs`] is how all of them - and the
//! wire, and the mod's own state file - spell a dense run of integers.
//!
//! A SAVE'S OWN DATA is three more. [`lua_blob`] is the binary form the game writes,
//! [`lua_manifest`] is what a reader knows about its shape, and [`lua_simx`] is the one
//! place a save repeats itself: the variables that are a second copy of the conversations,
//! left out and rebuilt. [`sparse`] is the ordered, array-less tree those tables are stored
//! as. [`save_statuses`] is the one walk over a blob that answers what a save has shown,
//! which the plugin asks for on every load.
//!
//! AND ONE THAT IS NOT A SAVE'S. [`global_state`] is the mod's own record of what a player
//! has read across every save, which is the only file here that cannot be regenerated.

pub mod global_state;
pub mod header;
pub mod lua_blob;
pub mod lua_manifest;
pub mod lua_simx;
pub mod runs;
pub mod save_statuses;
pub mod sparse;
