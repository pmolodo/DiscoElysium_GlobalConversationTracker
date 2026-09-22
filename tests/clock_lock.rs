// SPDX-License-Identifier: MIT
//! Whether a save's clock is locked, which the save does not record and the reader derives.
//!
//! The game recomputes `isTimeLocked` on load - the loader assigns through the `DayMinutes`
//! setter and that setter calls `LockTimeIfNeed` - so the flag is never persisted and a
//! fixture that wants the LOADED world has to work it out. See
//! `fixtures::clock_locked_in_save`, and CLAUDE.md on simulating what the game changes on
//! the way in.
//!
//! ## Why the counters are passed in rather than read
//!
//! The rule turns on `realDayCounter > dayCounter`, and no committed save satisfies it - the
//! player is never staged having stayed up past midnight. Handing the function a day counter
//! reaches the locked half of the rule without editing a save the game wrote, which is the
//! one thing this repository never does.

use gct_measure::common;

use common::fixtures;

/// A save whose clock is a long way from the small hours: day 3, ten in the morning.
const MORNING: &str = "at-trashcan";

/// What that save's clock actually says, which the rule is exercised around.
const MORNING_HOUR: i32 = 10;
const MORNING_DAY: i32 = 3;

/// Minutes in an hour, for writing an hour as the clock holds it.
const MINUTES_PER_HOUR: i32 = 60;

/// The hour the game stops the clock at, per `SunshineClockTime.stopTickingHour`.
const STOP_TICKING_HOUR: i32 = 2;

#[test]
fn every_committed_save_has_a_running_clock() {
    let locked: Vec<String> = fixtures::committed_saves()
        .into_iter()
        .filter(|save| fixtures::holdings_in_save(save).clock_locked)
        .collect();

    assert!(
        locked.is_empty(),
        "these saves stage a player who stayed up past midnight: {}\n\
         that is a world the game can be in, so this is a fact about the fixtures rather \
         than a defect - but it is worth knowing, since a locked clock is one no PassTime \
         moves and every hour question in such a save is answered where it stands",
        locked.join(", ")
    );
}

/// The day the save was actually written on leaves the clock running.
#[test]
fn a_day_that_has_not_rolled_over_leaves_the_clock_running() {
    assert!(!fixtures::clock_locked_in_save(
        MORNING,
        MORNING_HOUR * MINUTES_PER_HOUR,
        MORNING_DAY,
    ));
}

/// A story day behind the midnights counted locks it, once the hour is late enough.
///
/// The save is untouched; what moves is the day counter handed to the rule, which is how the
/// locked half is reached at all - see the note at the top.
#[test]
fn a_story_day_behind_the_midnights_locks_the_clock_after_two() {
    let unslept = MORNING_DAY - 1;

    assert!(
        fixtures::clock_locked_in_save(MORNING, STOP_TICKING_HOUR * MINUTES_PER_HOUR, unslept),
        "a day behind, at two in the morning, is exactly the must-sleep case",
    );
    assert!(
        fixtures::clock_locked_in_save(MORNING, MORNING_HOUR * MINUTES_PER_HOUR, unslept),
        "and it stays locked for the rest of the day, however late it gets",
    );
}

/// Before two in the morning the day may have rolled over and the clock still runs.
///
/// The hour is the half of the rule that keeps the player's night from ending at midnight:
/// they are given until two before the game stops time on them.
#[test]
fn the_small_hours_before_two_are_not_locked() {
    let unslept = MORNING_DAY - 1;

    for hour in 0..STOP_TICKING_HOUR {
        assert!(
            !fixtures::clock_locked_in_save(MORNING, hour * MINUTES_PER_HOUR, unslept),
            "{hour}:00 is before the hour the game stops the clock at",
        );
    }
}
