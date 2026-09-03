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
            13..=17 => Daytime::Afternoon,
            18 => Daytime::Dusk,
            19..=22 => Daytime::Evening,
            23 => Daytime::Evening,
            _ => Daytime::Dawn,
        }
    }

    pub fn is_hour_between(hours: i32, first: i32, second: i32) -> bool {
        if first > second {
            hours < first || hours <= second
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
                if let Some(&GuardValue { kind: GuardValueKind::Number, number: h, .. }) = arguments.get(0) {
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
