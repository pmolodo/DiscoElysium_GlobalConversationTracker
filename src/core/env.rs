// SPDX-License-Identifier: MIT
//! Reading this project's environment variables, with the prefix supplied rather than typed.
//!
//! ## Why a helper rather than a convention
//!
//! Every environment variable this project defines is prefixed `DEGCT_` - see CLAUDE.md for
//! the rule and `docs/environment.md` for the list. A convention a person has to remember is
//! a convention that grows exceptions: the whole reason the rule exists is that `GROUPS` is a
//! bash built-in array and assigning to it looks like it works, which cost time three separate
//! times before anybody traced it.
//!
//! So the prefix is applied HERE, by a function, and the only way to read one of ours is to
//! ask for it by its bare name. A new variable is named right because there is no other way
//! to name it.
//!
//! ## The escape hatch, and when it is right
//!
//! [`foreign`] reads a variable somebody else owns - `PATH`, `CARGO_TARGET_DIR`,
//! `NUMBER_OF_PROCESSORS` - and does not touch the name. It is spelled differently on purpose:
//! a reader can see at a glance which names this project invented and which it merely
//! consumes, and neither can be mistaken for the other.

/// The prefix on every environment variable this project defines.
pub const PREFIX: &str = "DEGCT_";

/// One of ours, by its bare name: `var("CONVERSATION")` reads `DEGCT_CONVERSATION`.
pub fn var(name: &str) -> Result<String, std::env::VarError> {
    std::env::var(qualified(name))
}

/// Whether one of ours is set at all, whatever it is set to.
///
/// The shape a flag takes here: several measurements switch on PRESENCE rather than value, so
/// that `DEGCT_NOLIMIT=1` and `DEGCT_NOLIMIT=` mean the same thing and neither has to be parsed.
pub fn is_set(name: &str) -> bool {
    std::env::var_os(qualified(name)).is_some()
}

/// One of ours as a number, or `fallback` where it is unset, empty or unparseable.
pub fn number<T: std::str::FromStr>(name: &str, fallback: T) -> T {
    var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(fallback)
}

/// Sets one of ours, for a child process built with [`std::process::Command`].
///
/// TAKES THE COMMAND rather than calling `set_var`, because setting a variable in THIS process
/// is a different and far more dangerous thing - it is global, it is not thread-safe, and
/// nothing that reads it later can tell it was set by us.
pub fn pass<'a>(
    command: &'a mut std::process::Command,
    name: &str,
    value: &str,
) -> &'a mut std::process::Command {
    command.env(qualified(name), value)
}

/// A variable somebody else owns, read under its own name.
///
/// `PATH`, `CARGO_TARGET_DIR`, `NUMBER_OF_PROCESSORS` and the like. Spelled differently from
/// [`var`] so that which of the two a call means is visible at the call site.
pub fn foreign(name: &str) -> Result<String, std::env::VarError> {
    std::env::var(name)
}

/// The full name of one of ours.
///
/// Public because a message about a variable should name it as the person would have to type
/// it, and because the drivers check for the OLD unprefixed spellings to refuse them.
pub fn qualified(name: &str) -> String {
    if name.starts_with(PREFIX) {
        name.to_string()
    } else {
        format!("{PREFIX}{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_name_gains_the_prefix() {
        assert_eq!(qualified("CONVERSATION"), "DEGCT_CONVERSATION");
    }

    /// Asking for a name that already carries the prefix does not double it.
    ///
    /// Worth a test rather than a comment: the drivers build names from both halves - a
    /// bare one they were given and a full one they read back out of a message - and
    /// `DEGCT_DEGCT_CONVERSATION` would be unset, silently, and read as a default.
    #[test]
    fn a_qualified_name_is_left_alone() {
        assert_eq!(qualified("DEGCT_CONVERSATION"), "DEGCT_CONVERSATION");
    }

    #[test]
    fn a_number_falls_back_where_there_is_nothing_to_read() {
        assert_eq!(number::<usize>("A_NAME_NOTHING_SETS", 7), 7);
    }
}
