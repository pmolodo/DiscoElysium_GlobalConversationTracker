// SPDX-License-Identifier: MIT
//! How every file in this repository spells a dense run of integers.
//!
//! `3,5,7-25`. Comma between groups, hyphen inside one, and a group of one is just the
//! number. It is a string rather than a list because an indented writer would otherwise
//! spread a few hundred ids over a few hundred lines.
//!
//! ## The hyphen, and the separator past the first character
//!
//! A hyphen looks ambiguous the first time an id is negative, and it is not: the separator
//! is looked for PAST THE FIRST CHARACTER, so a leading `-` is a sign. `-5--3` is the run
//! from -5 to -3, and one condition is the whole of the reasoning. That is what let the
//! wire, the save tables, their diffs and the mod's own state file settle on one spelling
//! instead of two that agree until they do not.
//!
//! ## A run may count down
//!
//! [`pack`] writes a descending run where it finds one, because the saves store their
//! dialogue variables newest first and a backwards run costs exactly what a forwards one
//! does. Callers that have their own reason to refuse one - an entry set, whose ids come
//! out of a sorted collection and where a backwards run means a file that is not what it
//! claims to be - read [`bounds`] and say so themselves.

/// Between one group and the next.
const GROUP_SEPARATOR: char = ',';

/// Between the two ends of one group.
const RUN_SEPARATOR: char = '-';

/// A group of a run list that is not a number or a pair of them.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{context} has a malformed key range '{part}'")]
pub struct RunFault {
    /// What was being read, so a caller reading forty of them knows which.
    pub context: String,
    /// The group that could not be read, as it was written.
    pub part: String,
}

/// Renders a run of numbers, e.g. `0-11,14-26`.
///
/// A group counts DOWN when the numbers do. Nothing is sorted and nothing is
/// de-duplicated: what comes out is what went in, grouped where it was consecutive.
#[must_use]
pub fn pack(numbers: &[i64]) -> String {
    let mut text = String::new();
    let mut at = 0;
    while at < numbers.len() {
        let mut last = at;
        if let Some(step) = step_at(numbers, at) {
            while last + 1 < numbers.len() && numbers[last + 1] - numbers[last] == step {
                last += 1;
            }
        }

        if !text.is_empty() {
            text.push(GROUP_SEPARATOR);
        }
        text.push_str(&numbers[at].to_string());
        if last > at {
            text.push(RUN_SEPARATOR);
            text.push_str(&numbers[last].to_string());
        }

        at = last + 1;
    }

    text
}

/// The step a run starting at `at` moves by, or nothing where there is no run.
fn step_at(numbers: &[i64], at: usize) -> Option<i64> {
    let next = numbers.get(at + 1)?;
    match next - numbers[at] {
        step @ (1 | -1) => Some(step),
        _ => None,
    }
}

/// The ends of each group, in the order they were written, without expanding them.
///
/// The two ends of a group of one are the same number. Reading the ends rather than the
/// numbers is what lets a caller judge a group before it becomes a million entries.
///
/// # Errors
///
/// Where a group is not a number or a pair of them.
pub fn bounds(text: &str, context: &str) -> Result<Vec<(i64, i64)>, RunFault> {
    let mut found = Vec::new();
    for part in text.split(GROUP_SEPARATOR) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // PAST THE FIRST CHARACTER, so a leading '-' reads as a sign rather than as a
        // separator. See the module note: that one condition is the whole of it.
        let (first, last) = match part
            .char_indices()
            .skip(1)
            .find(|(_, character)| *character == RUN_SEPARATOR)
        {
            Some((at, _)) => (&part[..at], &part[at + RUN_SEPARATOR.len_utf8()..]),
            None => (part, part),
        };

        let malformed = || RunFault {
            context: context.to_string(),
            part: part.to_string(),
        };
        found.push((
            first.trim().parse().map_err(|_| malformed())?,
            last.trim().parse().map_err(|_| malformed())?,
        ));
    }

    Ok(found)
}

/// Expands what [`pack`] wrote.
///
/// # Errors
///
/// As [`bounds`].
pub fn unpack(text: &str, context: &str) -> Result<Vec<i64>, RunFault> {
    let mut numbers = Vec::new();
    for (first, last) in bounds(text, context)? {
        expand(first, last, &mut numbers);
    }

    Ok(numbers)
}

/// One group's numbers, counting either way, both ends included.
pub fn expand(first: i64, last: i64, out: &mut Vec<i64>) {
    if first <= last {
        out.extend(first..=last);
    } else {
        out.extend((last..=first).rev());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unpack_ok(text: &str) -> Vec<i64> {
        unpack(text, "a test run list").expect("it reads")
    }

    #[test]
    fn consecutive_numbers_become_one_group() {
        assert_eq!(pack(&[0, 1, 2, 3, 5]), "0-3,5");
        assert_eq!(pack(&[7]), "7");
        assert_eq!(pack(&[]), "");
        assert_eq!(pack(&[3, 5, 7, 8, 9]), "3,5,7-9");
    }

    /// The saves store their dialogue variables newest first, so this is the common case.
    #[test]
    fn a_run_that_counts_down_is_as_short_as_one_that_counts_up() {
        assert_eq!(pack(&[26, 25, 24, 23]), "26-23");
        assert_eq!(unpack_ok("26-23"), vec![26, 25, 24, 23]);
    }

    /// A leading hyphen is a sign, which is the whole reason the separator can BE a hyphen.
    #[test]
    fn a_negative_bound_survives_at_either_end_of_a_group() {
        assert_eq!(pack(&[-5, -4, -3, -1, 2, 3]), "-5--3,-1,2-3");
        assert_eq!(unpack_ok("-5--3,-1,2-3"), vec![-5, -4, -3, -1, 2, 3]);
    }

    #[test]
    fn what_was_packed_comes_back() {
        for numbers in [
            vec![],
            vec![0],
            vec![-1_000_000, 0, 1_000_000],
            vec![1, 2, 3, 7, 8, 20, 19, 18],
        ] {
            assert_eq!(unpack_ok(&pack(&numbers)), numbers, "{}", pack(&numbers));
        }
    }

    #[test]
    fn an_empty_list_reads_as_nothing_rather_than_as_a_fault() {
        assert_eq!(unpack_ok(""), Vec::<i64>::new());
        assert_eq!(unpack_ok(" , "), Vec::<i64>::new());
    }

    #[test]
    fn the_ends_come_back_without_the_million_numbers_between_them() {
        assert_eq!(
            bounds("0-1000000,4", "a test run list").expect("it reads"),
            vec![(0, 1_000_000), (4, 4)],
        );
    }

    #[test]
    fn a_group_that_is_not_a_number_is_refused_and_quoted() {
        let refused = unpack("0-3,x-9", "Dialog").expect_err("refused");

        assert_eq!(refused.part, "x-9");
        assert!(refused.to_string().contains("Dialog"), "{refused}");
    }
}
