// SPDX-License-Identifier: MIT
//! The C# `ClockTimeTests`, ported.
//!
//! Porting these found two bugs, both since verified against the game's own source rather
//! than against the C#: the daytime buckets were shifted an hour, and `IsHourBetween`
//! excluded the wrong half of a range that wraps past midnight.

use crate::core::clock::{ClockTime, Daytime};
use crate::core::guard_value::{GuardValue, GuardValueKind};

/// Asks a clock question at a given hour, as the engine would.
fn ask(name: &str, hour: i32, arguments: &[f64]) -> bool {
    let values: Vec<GuardValue> = arguments
        .iter()
        .map(|a| GuardValue::from_number(*a))
        .collect();
    let result = ClockTime::answer(name, &values, hour * 60, 1);
    assert_eq!(
        result.kind(),
        GuardValueKind::Boolean,
        "{name} should answer a boolean"
    );
    result.boolean()
}

/// The buckets the game divides a day into.
///
/// Ported from the C# `DaytimeOf_MatchesTheGamesBuckets`, whose name is the point: these
/// boundaries were taken from the game, not chosen. The port had 18 as Dusk and 19 as
/// Evening - each an hour early - which made `IsEvening` true and `IsAfternoon` false at
/// six in the evening, where the game says the opposite.
#[test]
fn daytime_of_matches_the_games_buckets() {
    let cases = [
        (0, Daytime::Midnight),
        (1, Daytime::Night),
        (6, Daytime::Night),
        (7, Daytime::Dawn),
        (8, Daytime::Morning),
        (11, Daytime::Morning),
        (12, Daytime::Noon),
        (13, Daytime::Afternoon),
        (18, Daytime::Afternoon),
        (19, Daytime::Dusk),
        (20, Daytime::Evening),
        (23, Daytime::Evening),
    ];

    for (hour, expected) in cases {
        assert_eq!(ClockTime::daytime_of(hour), expected, "at hour {hour}");
    }
}

#[test]
fn hours_of_divides_and_wraps() {
    assert_eq!(ClockTime::hours_of(0), 0);
    assert_eq!(ClockTime::hours_of(59), 0);
    assert_eq!(ClockTime::hours_of(60), 1);
    assert_eq!(ClockTime::hours_of(1439), 23);
    assert_eq!(ClockTime::hours_of(1440), 0);
}

/// PassTime is fifteen minutes, so it takes four to move the hour at all - which is why a
/// single use rarely changes what a guard answers.
#[test]
fn advance_moves_fifteen_minutes() {
    assert_eq!(ClockTime::PASS_TIME_MINUTES, 15);

    let mut minutes = 11 * 60;
    assert_eq!(ClockTime::hours_of(minutes), 11);
    for _ in 0..3 {
        minutes = ClockTime::advance(minutes);
        assert_eq!(ClockTime::hours_of(minutes), 11);
    }

    minutes = ClockTime::advance(minutes);
    assert_eq!(ClockTime::hours_of(minutes), 12);
}

#[test]
fn advance_wraps_past_midnight() {
    let minutes = ClockTime::advance(23 * 60 + 55);
    assert_eq!(minutes, 10);
    assert_eq!(ClockTime::hours_of(minutes), 0);
}

/// The predicates are not a partition, and the two night-ish ones are different sets:
/// IsNighttime is the clock's own night property (dusk, evening, night), while IsNight the
/// Lua function is night or midnight.
#[test]
fn night_predicates_are_different_sets() {
    let cases = [
        (0, false, true),
        (3, true, true),
        (19, true, false),
        (21, true, false),
        (10, false, false),
    ];

    for (hour, is_nighttime, is_night) in cases {
        assert_eq!(
            ask("IsNighttime", hour, &[]),
            is_nighttime,
            "IsNighttime at {hour}"
        );
        assert_eq!(ask("IsNight", hour, &[]), is_night, "IsNight at {hour}");
    }
}

/// Midnight satisfies neither.
#[test]
fn midnight_is_neither_day_nor_night() {
    assert!(!ask("IsDaytime", 0, &[]));
    assert!(!ask("IsNighttime", 0, &[]));
}

#[test]
fn is_morning_includes_dawn() {
    assert!(ask("IsMorning", 7, &[]));
    assert!(ask("IsMorning", 10, &[]));
    assert!(!ask("IsMorning", 12, &[]));
}

#[test]
fn is_afternoon_includes_noon() {
    assert!(ask("IsAfternoon", 12, &[]));
    assert!(ask("IsAfternoon", 15, &[]));
    assert!(!ask("IsAfternoon", 19, &[]));
}

#[test]
fn is_evening_includes_dusk() {
    assert!(ask("IsEvening", 19, &[]));
    assert!(ask("IsEvening", 22, &[]));
    assert!(!ask("IsEvening", 18, &[]));
}

#[test]
fn is_hour_between_is_inclusive_at_both_ends() {
    assert!(ask("IsHourBetween", 8, &[8.0, 12.0]));
    assert!(ask("IsHourBetween", 12, &[8.0, 12.0]));
    assert!(!ask("IsHourBetween", 7, &[8.0, 12.0]));
    assert!(!ask("IsHourBetween", 13, &[8.0, 12.0]));
}

/// A range written backwards wraps past midnight rather than being empty.
#[test]
fn is_hour_between_wraps_when_first_exceeds_second() {
    for (hour, expected) in [(23, true), (2, true), (4, true), (5, false), (12, false)] {
        assert_eq!(
            ask("IsHourBetween", hour, &[22.0, 4.0]),
            expected,
            "at {hour}"
        );
    }
}

#[test]
fn hour_count_and_total_hour_count() {
    let hours = ClockTime::answer("HourCount", &[], 15 * 60, 3);
    assert_eq!(hours.number(), 15.0);

    // 24 * (day - 1) + hours
    let total = ClockTime::answer("TotalHourCount", &[], 15 * 60, 3);
    assert_eq!(total.number(), 63.0);
}

/// The day counter is not the clock's to move: PassTime advances a real-day counter while
/// DayCount reads another, so these stay the host's queries however tempting the names
/// look.
#[test]
fn owns_disclaims_queries_the_clock_cannot_answer() {
    for name in [
        "DayCount",
        "IsDayFrom",
        "IsDayUntil",
        "IsKimHere",
        "CheckItem",
    ] {
        assert!(!ClockTime::owns(name), "{name} should not be the clock's");
        assert_eq!(
            ClockTime::answer(name, &[], 0, 1).kind(),
            GuardValueKind::Unknown,
            "{name} should answer unknown",
        );
    }
}

#[test]
fn owns_claims_the_hour_queries() {
    for name in [
        "IsMorning",
        "IsHourBetween",
        "HourCount",
        "TotalHourCount",
        "IsMidnight",
    ] {
        assert!(ClockTime::owns(name), "{name} should be the clock's");
    }
}

/// The day questions are held apart from the hour ones, and the separation is the point.
///
/// `owns` marks what a search's own `PassTime` can change, which is what makes holding the
/// clock still an approximation. The day cannot change within a conversation at all, so
/// these are exact - answered, but never through `owns`.
#[test]
fn the_day_questions_are_owned_separately_from_the_hour_ones() {
    for name in ["DayCount", "IsDayFrom", "IsDayUntil"] {
        assert!(ClockTime::owns_day(name), "{name} reads the day counter");
        assert!(!ClockTime::owns(name), "{name} is not the hour clock's");
    }

    for name in ["IsMorning", "HourCount", "IsKimHere", "CheckItem"] {
        assert!(
            !ClockTime::owns_day(name),
            "{name} does not read the day counter"
        );
    }
}

/// `IsDayFrom(d)` is `DayCounter >= d` and `IsDayUntil(d)` is `DayCounter < d`, which is
/// how `DaytimeLuaFunctions` defines them. The boundary is where the two must not agree.
#[test]
fn the_day_questions_answer_from_the_day_counter() {
    let day = |value: f64| [GuardValue::from_number(value)];

    assert_eq!(ClockTime::day_answer("DayCount", &[], 3).number(), 3.0);

    // On day 2 exactly: `from 2` holds, `until 2` does not.
    assert!(ClockTime::day_answer("IsDayFrom", &day(2.0), 2).boolean());
    assert!(!ClockTime::day_answer("IsDayUntil", &day(2.0), 2).boolean());

    // On day 1, the case conversation 631 actually asks.
    assert!(!ClockTime::day_answer("IsDayFrom", &day(2.0), 1).boolean());
    assert!(ClockTime::day_answer("IsDayUntil", &day(2.0), 1).boolean());
}

/// A day question with no day to compare against is unknown, not a guess.
#[test]
fn a_day_question_without_its_argument_is_unknown() {
    assert_eq!(
        ClockTime::day_answer("IsDayFrom", &[], 1).kind(),
        GuardValueKind::Unknown,
    );
}
