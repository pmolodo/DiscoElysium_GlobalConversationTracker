// SPDX-License-Identifier: MIT
//! What a measurement or a test needs besides the engine.
//!
//! ## Why these are a crate rather than modules each caller includes
//!
//! They were included by path - `#[path = "../tests/common/mod.rs"] mod common;` and the same
//! for the rest - which makes each one a module of the INCLUDING crate. That was deliberate:
//! [`save_world`] asks [`prepared`] for a `Shipped`, so two copies of `prepared` would be two
//! distinct types and a world built with one could not be asked for with the other. Every
//! includer therefore had to declare the whole set together.
//!
//! The cost was compiling them again in every caller - 31 examples and 37 integration tests,
//! over 2,566 lines in `common` alone. A crate answers the same need and removes the cost: one
//! `prepared`, one `Shipped`, one compilation. See de-07kd for the measurements that prompted
//! it.
//!
//! ## The names did not change
//!
//! A caller writes `use gct_measure::common;` where it wrote `mod common;`, and everything
//! after that - `common::shipped_index()`, `prepared::Shipped` - reads as it did.

#![allow(dead_code)] // Each caller uses a different part of this.

pub mod common;
/// The counting allocator two measurements install. It only PROVIDES the type - the
/// `#[global_allocator]` attribute stays in the binary, where it must be.
pub mod counting_allocator;
pub mod kept;
pub mod menu_profile;
pub mod options;
pub mod plugin_defaults;
pub mod prepared;
pub mod save_world;
pub mod seen_profile;
