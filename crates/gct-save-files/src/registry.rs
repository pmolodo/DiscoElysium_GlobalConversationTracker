// SPDX-License-Identifier: MIT
//! Every format this repository writes, in one list.
//!
//! ## Why a list, when each format already declares its own
//!
//! Because more than one thing has to know the WHOLE SET rather than one member of it, and
//! each of those things had its own copy. [`super::convert`] decides what a file is by
//! looking its name up here; the test that every committed document is at the current
//! version walks the same rows. Before this, those were two hand-written tables in two
//! languages, and a format present in one and missing from the other fails in the quietest
//! possible way: a file it does not recognise reads as "not one of ours" rather than as an
//! error.
//!
//! THE ROWS POINT AT THE MODULES rather than restating them. A format's name and version
//! are declared once, beside the code that reads it, and this names that constant - so the
//! only thing that can go stale here is a format left out entirely, which is what
//! `every_format_this_build_writes_is_registered` is for.
//!
//! ## The ones with no module of their own
//!
//! Four of them describe a TEST FIXTURE rather than a player's or a save's data - the
//! scenario suites, the branch shapes, the table of which scenes are outdoors, and the table
//! of which weather each preset is. They are read by the tests and written by hand or by a
//! script, so there is no reader module to hang them off; their `Expected` is declared here
//! instead. They are still formats, they are still stamped, and a converter that did not
//! know them would call a committed fixture foreign.

use super::header::Expected;
use super::{expanded_save, global_state, json_diff, lua_sparse, sparse_diff};

/// The look-ahead scenarios, as suites of saves and the markers they draw.
pub const SCENARIO_SUITES: Expected = Expected {
    format: "scenario-suites",
    version: 1,
};

/// The shapes a check's Pass and Fail line can take.
pub const BRANCH_SHAPES: Expected = Expected {
    format: "branch-shapes",
    version: 1,
};

/// Which of the game's scenes are outdoors, which is what `IsExterior()` answers from.
pub const SCENES: Expected = Expected {
    format: "scenes",
    version: 1,
};

/// Which weather each preset index is, which is what `IsRaining()` ends up answering from.
///
/// A save records the weather as an index into the controller's ordered list of presets, and
/// the name at that index is not the answer - `RainClear_0` is CLEAR and `SnowClear_0` is
/// SNOW - so the type each one carries is written down.
pub const WEATHER_PRESETS: Expected = Expected {
    format: "weather-presets",
    version: 1,
};

/// Every format, and the version of it this build writes.
pub const EVERY_FORMAT: [Expected; 9] = [
    global_state::FORMAT,
    json_diff::FORMAT,
    sparse_diff::FORMAT,
    expanded_save::FORMAT,
    lua_sparse::FORMAT,
    SCENARIO_SUITES,
    BRANCH_SHAPES,
    SCENES,
    WEATHER_PRESETS,
];

/// What version of a named format this build writes, or nothing where it knows none.
#[must_use]
pub fn current_version(format: &str) -> Option<u32> {
    EVERY_FORMAT
        .iter()
        .find(|known| known.format == format)
        .map(|known| known.version)
}

/// Every format's name, in the order a message should list them.
#[must_use]
pub fn every_name() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = EVERY_FORMAT.iter().map(|known| known.format).collect();
    names.sort_unstable();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn no_format_is_registered_twice() {
        let mut seen = HashSet::new();
        for known in EVERY_FORMAT {
            assert!(
                seen.insert(known.format),
                "'{}' is registered more than once",
                known.format,
            );
        }
    }

    #[test]
    fn every_registered_format_says_what_version_it_is_at() {
        for known in EVERY_FORMAT {
            assert_eq!(current_version(known.format), Some(known.version));
        }
    }

    #[test]
    fn a_format_this_build_never_wrote_is_not_current_at_any_version() {
        assert_eq!(current_version("lua-tables-in-yaml"), None);
    }

    /// The names come back sorted, since a message that lists them is read by a person.
    #[test]
    fn the_names_are_listed_in_an_order_somebody_can_scan() {
        let names = every_name();

        assert_eq!(names.len(), EVERY_FORMAT.len());
        assert!(names.windows(2).all(|pair| pair[0] < pair[1]), "{names:?}");
    }
}
