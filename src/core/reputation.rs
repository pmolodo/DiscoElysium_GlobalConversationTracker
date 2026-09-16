// SPDX-License-Identifier: MIT
//! Which reputation is winning, which is what `IsHighestCopotype` and `IsHighestPolitical`
//! ask.
//!
//! ## Why this is not a maximum
//!
//! It reads like one and is not. The game's `ReputationAlterant.GetHighestReputationString`
//! is:
//!
//! ```text
//! int num = -1;
//! int num2 = 0;
//! for (int i = startIndex; i < endIndex; i++)
//! {
//!     if (GetReputationAmount((Reputation)i) == num2)
//!     {
//!         num = -1;
//!     }
//!     else if (GetReputationAmount((Reputation)i) > num2)
//!     {
//!         num = i;
//!         num2 = GetReputationAmount((Reputation)i);
//!     }
//! }
//! if (num == -1) return "";
//! return copotypeThought[num];
//! ```
//!
//! Two things fall out of that which a maximum would get wrong, and both are reachable with
//! ordinary values:
//!
//! - A TIE CLEARS THE WINNER rather than keeping the earlier one, and the running best
//!   starts at ZERO - so any reputation still at zero ties with it and clears whoever was
//!   ahead. `[2, 2, 0, 0]` answers nothing at all.
//! - A LATER ENTRY WINS IT BACK, because the clear only sets the index aside and leaves the
//!   amount standing. `[2, 2, 3, 0]` answers the third one.
//!
//! So it is order-dependent, and the order is the `Reputation` enum's.
//!
//! ## Two tables that are not the same table
//!
//! The amount is read as `Variable["reputation.{reputation}"]`, which interpolates the
//! `Reputation` ENUM's name, while the answer returned is `copotypeThought[index]`, a
//! separate array. They agree for indices 0 to 7, which is everything these two questions
//! look at, and they DIVERGE at 14, where the enum says `suicide_cop` and the array says
//! `suicide_is_painless`. Nothing asks about 14 today. Anyone widening a range past 7 has to
//! carry both tables rather than this one.

/// The reputations in the order the game's `Reputation` enum declares them, which is the
/// order the loop walks and therefore part of the answer.
///
/// Only as far as the two questions reach. See the note above before extending it.
pub const IN_ENUM_ORDER: [&str; 8] = [
    "apocalypse_cop",
    "boring_cop",
    "superstar_cop",
    "sorry_cop",
    "communist",
    "revacholian_nationhood",
    "ultraliberal",
    "moralist",
];

/// What `IsHighestCopotype` looks at: `GetHighestReputationString(0, 4)`.
pub const COPOTYPE: std::ops::Range<usize> = 0..4;

/// What `IsHighestPolitical` looks at: `GetHighestReputationString(4, 8)`.
pub const POLITICAL: std::ops::Range<usize> = 4..8;

/// The dialogue variable a reputation's amount is kept in.
pub fn variable_of(name: &str) -> String {
    format!("reputation.{name}")
}

/// The range a query looks at, or `None` for a query that is not about reputation.
pub fn range_of(query: &str) -> Option<std::ops::Range<usize>> {
    match query {
        "IsHighestCopotype" => Some(COPOTYPE),
        "IsHighestPolitical" => Some(POLITICAL),
        _ => None,
    }
}

/// Every reputation variable a query reads, so the group can declare them.
///
/// ALL OF THE RANGE, not only what a conversation writes: the answer is decided by comparing
/// the whole range, so a reputation left out is one the comparison cannot see and the answer
/// is then not the game's.
pub fn variables_read_by(query: &str) -> Vec<String> {
    range_of(query)
        .map(|range| {
            IN_ENUM_ORDER[range]
                .iter()
                .map(|n| variable_of(n))
                .collect()
        })
        .unwrap_or_default()
}

/// Which reputation is winning in `range`, by the game's own loop.
///
/// `amount_of` gives a reputation's value by name; `None` where the world cannot say, which
/// makes the whole answer unknown rather than treating the gap as a zero - a zero is a
/// PARTICIPANT here, not an absence, and one invented in the wrong place clears a winner.
pub fn highest(
    range: std::ops::Range<usize>,
    amount_of: impl Fn(&str) -> Option<i32>,
) -> Option<Option<&'static str>> {
    let mut best: Option<usize> = None;
    let mut best_amount = 0;

    for index in range {
        let amount = amount_of(IN_ENUM_ORDER[index])?;
        if amount == best_amount {
            best = None;
        } else if amount > best_amount {
            best = Some(index);
            best_amount = amount;
        }
    }

    Some(best.map(|index| IN_ENUM_ORDER[index]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads a fixed set of amounts, positionally over the whole table.
    fn amounts(values: [i32; 8]) -> impl Fn(&str) -> Option<i32> {
        move |name| {
            IN_ENUM_ORDER
                .iter()
                .position(|known| *known == name)
                .map(|index| values[index])
        }
    }

    #[test]
    fn nothing_is_winning_when_everything_is_zero() {
        assert_eq!(highest(COPOTYPE, amounts([0; 8])), Some(None));
    }

    #[test]
    fn the_only_one_above_zero_wins() {
        let values = amounts([2, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(highest(COPOTYPE, values), Some(Some("apocalypse_cop")));
    }

    /// A zero EARLIER in the range does not stop a later one winning.
    #[test]
    fn a_leading_zero_does_not_prevent_a_winner() {
        let values = amounts([0, 2, 0, 0, 0, 0, 0, 0]);
        assert_eq!(highest(COPOTYPE, values), Some(Some("boring_cop")));
    }

    /// THE TIE RULE: two equal leaders answer nothing, not the first of them.
    #[test]
    fn a_tie_leaves_nothing_winning() {
        let values = amounts([2, 2, 0, 0, 0, 0, 0, 0]);
        assert_eq!(highest(COPOTYPE, values), Some(None));
    }

    /// And a later, higher one wins it back - the clear sets the index aside and leaves the
    /// amount standing.
    #[test]
    fn a_later_higher_one_wins_back_a_cleared_tie() {
        let values = amounts([2, 2, 3, 0, 0, 0, 0, 0]);
        assert_eq!(highest(COPOTYPE, values), Some(Some("superstar_cop")));
    }

    /// The political range is the same loop over a different four.
    #[test]
    fn the_political_range_is_its_own_four() {
        let values = amounts([9, 0, 0, 0, 0, 3, 0, 0]);
        assert_eq!(
            highest(POLITICAL, values),
            Some(Some("revacholian_nationhood")),
            "the copotype range's nine is not in this one"
        );
    }

    /// A reputation the world cannot answer makes the whole question unknown, because a
    /// missing value cannot be read as a zero without changing who wins.
    #[test]
    fn one_unreadable_reputation_makes_the_answer_unknown() {
        let answer = highest(COPOTYPE, |name| (name != "superstar_cop").then_some(1));
        assert_eq!(answer, None);
    }

    #[test]
    fn a_query_names_every_variable_its_range_reads() {
        assert_eq!(
            variables_read_by("IsHighestPolitical"),
            vec![
                "reputation.communist".to_string(),
                "reputation.revacholian_nationhood".to_string(),
                "reputation.ultraliberal".to_string(),
                "reputation.moralist".to_string(),
            ]
        );
        assert!(variables_read_by("IsKimHere").is_empty());
    }
}
