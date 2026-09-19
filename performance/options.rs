// SPDX-License-Identifier: MIT
//! The options the drivers share, each carrying the default of whichever driver embeds it.
//!
//! ## Why a module and not a helper per driver
//!
//! `from_env` and `numbers` were copy-pasted into fifteen driver files, identical bodies every
//! time, because each driver read its options out of the environment and every one of them needed
//! the same two readers. An option that reaches a driver on its command line needs no reader at
//! all - `clap` parses it - so what is left to share is the option's NAME, its help and how its
//! value is spelled, and those are worth writing once for a different reason: `--starts` should
//! mean the same thing and read the same way in every driver that offers it.
//!
//! ## One group per thing, not one group for everything
//!
//! A driver embeds only the pieces it actually takes:
//!
//! ```ignore
//! #[derive(Parser)]
//! struct Options {
//!     #[command(flatten)]
//!     groups: options::Groups,
//!     #[command(flatten)]
//!     starts: options::Starts<8>,
//! }
//! ```
//!
//! so its `--help` lists what it reads and nothing else, and a reader of the struct can see what
//! the driver depends on. A single struct carrying every option in the project would put
//! `--unseen` in the help of a driver that never looks at it, which is a lie the compiler cannot
//! catch.
//!
//! ## The default comes from the driver, through a const parameter
//!
//! The drivers do NOT agree on these numbers - `--starts` defaults to 8 in one driver and 24 in
//! another, `--unseen` to 10 in most and 1 in `nodes_repeat` - so a shared group with the number
//! written into it would be wrong everywhere it was not right. The default is a const parameter
//! instead, which `clap` carries into the generated help, so `Starts<24>` prints
//! `[default: 24]` and the number still lives in the driver that chose it.
//!
//! WHERE A DEFAULT IS NOT A CONSTANT the driver declares the option itself rather than bending
//! this: `menu_matrix`'s memory budget depends on whether `--nolimit` was passed, which is not
//! known until the arguments are parsed.
//!
//! ## What is here is what something embeds
//!
//! A group is added when the first driver that needs it arrives, not in anticipation of one. Nine
//! drivers take a `--budget-mb` whose default IS a constant, so that group belongs here and will
//! be here as soon as one of them is converted; writing it before then would be a struct with no
//! caller, which is the thing that had to be deleted from the C# side of this same epic.

// EACH DRIVER COMPILES THIS FILE OF ITS OWN, through `#[path]`, so a group one driver does not
// embed - or a helper it does not call - is dead code in that driver and live in the next.
// `tests/common/mod.rs`, `prepared.rs` and `save_world.rs` are shared the same way and say the
// same thing.
#![allow(dead_code)]

use clap::Args;

/// WHICH GROUPS a run measures.
///
/// The default is a LIST and differs per driver, and a const parameter can only be a number, so
/// this one resolves through [`Groups::or`] rather than through `clap`. The driver's own list is
/// the fallback, and the help says so because `clap` cannot print it.
#[derive(Args, Debug, Clone)]
pub struct Groups {
    // WHAT NOTHING NAMED MEANS IS THE DRIVER'S, so the help here does not claim it: one driver
    // falls back to a list of its own, another sweeps every group in the game. Each says which in
    // its own `about`.
    /// Which groups to measure; repeat the flag or comma-separate the ids
    #[arg(long = "conversation", value_name = "ID", value_delimiter = ',')]
    pub conversations: Vec<i32>,
}

impl Groups {
    /// What was asked for, or `fallback` where nothing was.
    pub fn or(&self, fallback: &[i32]) -> Vec<i32> {
        if self.conversations.is_empty() {
            fallback.to_vec()
        } else {
            self.conversations.clone()
        }
    }
}

/// HOW WIDE A MENU IS: how many options the run asks it with.
#[derive(Args, Debug, Clone, Copy)]
pub struct Starts<const DEFAULT: usize> {
    /// How many starts the menu is asked with.
    #[arg(long, value_name = "N", default_value_t = DEFAULT)]
    pub starts: usize,
}

/// HOW MUCH IS LEFT TO FIND: how many of the deepest entries count as never read.
#[derive(Args, Debug, Clone, Copy)]
pub struct Unseen<const DEFAULT: usize> {
    /// How many of the deepest entries the profile treats as unseen.
    #[arg(long, value_name = "N", default_value_t = DEFAULT)]
    pub unseen: usize,
}
