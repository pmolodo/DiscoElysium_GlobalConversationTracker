// SPDX-License-Identifier: MIT
use crate::core::guard_value::{GuardValue, GuardValueKind};

/// Time of day buckets matching the game's SunshineClockTime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Daytime {
    Midnight = 0,
    Night = 1,
    Dawn = 2,
    Morning = 3,
    Noon = 4,
    Afternoon = 5,
    Dusk = 6,
    Evening = 7,
}

/// The game's clock functions.
pub struct ClockTime;

impl ClockTime {
    pub const MINUTES_IN_DAY: i32 = 1440;
    pub const PASS_TIME_MINUTES: i32 = 15;

    pub fn hours_of(day_minutes: i32) -> i32 {
        let m = day_minutes % Self::MINUTES_IN_DAY;
        let m = if m < 0 { m + Self::MINUTES_IN_DAY } else { m };
        m / 60
    }

    pub fn advance(day_minutes: i32) -> i32 {
        (day_minutes + Self::PASS_TIME_MINUTES) % Self::MINUTES_IN_DAY
    }

    pub fn daytime_of(hours: i32) -> Daytime {
        match hours {
            0 => Daytime::Midnight,
            1..=6 => Daytime::Night,
            7 => Daytime::Dawn,
            8..=11 => Daytime::Morning,
            12 => Daytime::Noon,
            // Afternoon runs to the END of the eighteenth hour and dusk is the single
            // hour of nineteen. Taken from the game's own
            // SunshineClockTime.GetDaytime(): case 13..18 AFTERNOON, case 19 DUSK, case
            // 20..23 EVENING. This port previously had dusk at 18 and evening from 19,
            // each an hour early, which made IsEvening true and IsAfternoon false at six
            // in the evening - the opposite of what the game answers.
            13..=18 => Daytime::Afternoon,
            19 => Daytime::Dusk,
            20..=23 => Daytime::Evening,
            _ => Daytime::Dawn,
        }
    }

    /// `IsHourBetween(first, second)`, inclusive at both ends.
    ///
    /// A range written backwards wraps past midnight rather than being empty, so
    /// `IsHourBetween(22, 4)` is the small hours. The game writes that as "if the hour is
    /// below the start, it must be at or below the end; otherwise it is in range", which
    /// is `hours >= first || hours <= second`.
    ///
    /// This port had `hours < first` for that first half, which is the negation of the
    /// condition it should have been - so a wrapping range excluded everything from the
    /// start hour to midnight, exactly the half the range is usually written for.
    /// `IsHourBetween(22, 4)` answered false at 23:00.
    pub fn is_hour_between(hours: i32, first: i32, second: i32) -> bool {
        if first > second {
            hours >= first || hours <= second
        } else {
            hours >= first && hours <= second
        }
    }

    pub fn owns(name: &str) -> bool {
        matches!(name,
            "IsDaytime" | "IsNighttime" | "IsNight" | "IsDawn" | "IsMorning" |
            "IsNoon" | "IsAfternoon" | "IsDusk" | "IsEvening" | "IsMidnight" |
            "IsHour" | "IsHourBetween" | "HourCount" | "TotalHourCount"
        )
    }

    pub fn answer(
        name: &str,
        arguments: &[GuardValue],
        day_minutes: i32,
        day_counter: i32,
    ) -> GuardValue {
        let hours = Self::hours_of(day_minutes);
        let daytime = Self::daytime_of(hours);

        match name {
            "IsDaytime" => GuardValue::from_boolean(matches!(daytime, Daytime::Dawn | Daytime::Morning | Daytime::Noon | Daytime::Afternoon)),
            "IsNighttime" => GuardValue::from_boolean(matches!(daytime, Daytime::Dusk | Daytime::Evening | Daytime::Night)),
            "IsNight" => GuardValue::from_boolean(matches!(daytime, Daytime::Night | Daytime::Midnight)),
            "IsDawn" => GuardValue::from_boolean(daytime == Daytime::Dawn),
            "IsMorning" => GuardValue::from_boolean(matches!(daytime, Daytime::Dawn | Daytime::Morning)),
            "IsNoon" => GuardValue::from_boolean(daytime == Daytime::Noon),
            "IsAfternoon" => GuardValue::from_boolean(matches!(daytime, Daytime::Noon | Daytime::Afternoon)),
            "IsDusk" => GuardValue::from_boolean(daytime == Daytime::Dusk),
            "IsEvening" => GuardValue::from_boolean(matches!(daytime, Daytime::Dusk | Daytime::Evening)),
            "IsMidnight" => GuardValue::from_boolean(daytime == Daytime::Midnight),
            "IsHour" => {
                if let Some(h) = arguments.get(0)
                    .filter(|v| v.kind() == GuardValueKind::Number)
                    .map(|v| v.number())
                {
                    GuardValue::from_boolean(hours == h as i32)
                } else {
                    GuardValue::unknown()
                }
            }
            "IsHourBetween" => {
                if arguments.len() >= 2 {
                    let f = arguments[0].try_as_number();
                    let s = arguments[1].try_as_number();
                    if let (Some(f), Some(s)) = (f, s) {
                        return GuardValue::from_boolean(Self::is_hour_between(hours, f as i32, s as i32));
                    }
                }
                GuardValue::unknown()
            }
            "HourCount" => GuardValue::from_number(hours as f64),
            "TotalHourCount" => GuardValue::from_number((24 * (day_counter - 1) + hours) as f64),
            _ => GuardValue::unknown(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asks a clock question at a given hour, as the engine would.
    fn ask(name: &str, hour: i32, arguments: &[f64]) -> bool {
        let values: Vec<GuardValue> =
            arguments.iter().map(|a| GuardValue::from_number(*a)).collect();
        let result = ClockTime::answer(name, &values, hour * 60, 1);
        assert_eq!(result.kind(), GuardValueKind::Boolean, "{name} should answer a boolean");
        result.boolean()
    }

    /// The buckets the game divides a day into.
    ///
    /// Ported from the C# `DaytimeOf_MatchesTheGamesBuckets`, whose name is the point:
    /// these boundaries were taken from the game, not chosen. The port had 18 as Dusk and
    /// 19 as Evening - each an hour early - which made `IsEvening` true and `IsAfternoon`
    /// false at six in the evening, where the game says the opposite.
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

    /// PassTime is fifteen minutes, so it takes four to move the hour at all - which is
    /// why a single use rarely changes what a guard answers.
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
    /// IsNighttime is the clock's own night property (dusk, evening, night), while IsNight
    /// the Lua function is night or midnight.
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
            assert_eq!(ask("IsNighttime", hour, &[]), is_nighttime, "IsNighttime at {hour}");
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
            assert_eq!(ask("IsHourBetween", hour, &[22.0, 4.0]), expected, "at {hour}");
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

    /// The day counter is not the clock's to move: PassTime advances a real-day counter
    /// while DayCount reads another, so these stay the host's queries however tempting
    /// the names look.
    #[test]
    fn owns_disclaims_queries_the_clock_cannot_answer() {
        for name in ["DayCount", "IsDayFrom", "IsDayUntil", "IsKimHere", "CheckItem"] {
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
        for name in ["IsMorning", "IsHourBetween", "HourCount", "TotalHourCount", "IsMidnight"] {
            assert!(ClockTime::owns(name), "{name} should be the clock's");
        }
    }
}
