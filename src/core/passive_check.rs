// SPDX-License-Identifier: MIT
//! Whether a passive skill check fires, given the numbers.
//!
//! The arithmetic half of the game's `PassiveNode.CheckSuccess`, kept apart from the code
//! that gathers its inputs so that it can be tested. Reading a skill value out of the
//! character sheet needs the game running; deciding what the number means does not, and
//! the inversion is the part worth a test.
//!
//! No dice. A passive check is a comparison, which is what lets a look-ahead from the
//! player's current state answer definitely instead of carrying both branches through the
//! 10,500 entries in the database that have one.

use crate::core::types::Ternary;

/// The flat bonus every passive check gets: the game tests
/// `skill_value + SKILL_BONUS >= min_skill_value`.
pub const SKILL_BONUS: i32 = 6;

/// Whether the check clears its threshold, before any inversion.
///
/// `threshold` is the difficulty, already converted from the entry's difficulty id and
/// already adjusted for thoughts.
pub fn clears(skill_value: i32, threshold: i32) -> bool {
    skill_value + SKILL_BONUS >= threshold
}

/// Whether the entry fires.
///
/// An antipassive entry is the mirror of an ordinary one: it is the line that shows when
/// you are NOT sharp enough, so it fires exactly when the check fails.
pub fn outcome(skill_value: i32, threshold: i32, antipassive: bool) -> Ternary {
    if clears(skill_value, threshold) != antipassive {
        Ternary::True
    } else {
        Ternary::False
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bonus_is_added_to_the_skill_not_the_threshold() {
        // Six under is exactly enough, seven under is not.
        assert!(clears(4, 10));
        assert!(!clears(3, 10));
    }

    #[test]
    fn clearing_is_inclusive() {
        assert!(clears(10, 16));
        assert!(!clears(10, 17));
    }

    #[test]
    fn an_ordinary_entry_fires_when_it_clears() {
        assert_eq!(outcome(4, 10, false), Ternary::True);
        assert_eq!(outcome(3, 10, false), Ternary::False);
    }

    #[test]
    fn an_antipassive_entry_fires_when_it_does_not() {
        assert_eq!(outcome(4, 10, true), Ternary::False);
        assert_eq!(outcome(3, 10, true), Ternary::True);
    }

    #[test]
    fn the_answer_is_never_unknown() {
        // The point of the type: a passive check is a comparison, so a look-ahead never
        // has to carry both branches for one.
        for skill in -2..20 {
            for threshold in -2..20 {
                for antipassive in [false, true] {
                    assert_ne!(outcome(skill, threshold, antipassive), Ternary::Unknown);
                }
            }
        }
    }
}
