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

    /// Whether a query reads the STORY's day counter rather than the time of day.
    ///
    /// Held apart from [`Self::owns`] deliberately, and the distinction is not cosmetic.
    /// `owns` marks the questions a crawl's own `PassTime` can change the answer to,
    /// which is exactly what makes holding the clock still an APPROXIMATION. THE DAY
    /// CANNOT CHANGE WITHIN A CONVERSATION - only the time of day can - so these three
    /// are exact constants for any crawl, and answering them costs nothing and risks
    /// nothing.
    ///
    /// Answered here rather than left to each world, even though they are the host's
    /// facts, because they are a pure function of `day_counter`, which
    /// [`crate::world::world::ILookAheadWorld`] already exposes. Left to the worlds, all
    /// three would be reimplemented in every one of them and unanswered in most - and
    /// unanswered is what they were: 2 guards in conversation 631's group, 2 in 14's and
    /// 14 `DayCount` calls in 28's fell back for want of a comparison the crawl could
    /// have made itself.
    pub fn owns_day(name: &str) -> bool {
        matches!(name, "DayCount" | "IsDayFrom" | "IsDayUntil")
    }

    /// What a day question answers, given the story's day counter.
    ///
    /// `IsDayFrom(d)` is `DayCounter >= d` and `IsDayUntil(d)` is `DayCounter < d`, which
    /// is what `DaytimeLuaFunctions` defines them as.
    pub fn day_answer(name: &str, arguments: &[GuardValue], day_counter: i32) -> GuardValue {
        if name == "DayCount" {
            return GuardValue::from_number(day_counter as f64);
        }

        let Some(day) = arguments.first().and_then(|v| v.try_as_number()) else {
            return GuardValue::unknown();
        };

        match name {
            "IsDayFrom" => GuardValue::from_boolean((day_counter as f64) >= day),
            "IsDayUntil" => GuardValue::from_boolean((day_counter as f64) < day),
            _ => GuardValue::unknown(),
        }
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
