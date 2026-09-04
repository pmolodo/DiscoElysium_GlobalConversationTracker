// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using GlobalConversationTracker.Engine;
using Xunit;

namespace GlobalConversationTracker.LookAhead.Tests
{
    public class ClockTimeTests
    {
        private static readonly GuardValue[] NoArguments = Array.Empty<GuardValue>();

        private static bool Ask(string name, int hour, params double[] arguments)
        {
            var values = new List<GuardValue>();
            foreach (double argument in arguments)
            {
                values.Add(GuardValue.FromNumber(argument));
            }

            GuardValue result = ClockTime.Answer(name, values, hour * 60, dayCounter: 1);
            Assert.Equal(GuardValueKind.Boolean, result.Kind);
            return result.Boolean;
        }

        [Theory]
        [InlineData(0, Daytime.Midnight)]
        [InlineData(1, Daytime.Night)]
        [InlineData(6, Daytime.Night)]
        [InlineData(7, Daytime.Dawn)]
        [InlineData(8, Daytime.Morning)]
        [InlineData(11, Daytime.Morning)]
        [InlineData(12, Daytime.Noon)]
        [InlineData(13, Daytime.Afternoon)]
        [InlineData(18, Daytime.Afternoon)]
        [InlineData(19, Daytime.Dusk)]
        [InlineData(20, Daytime.Evening)]
        [InlineData(23, Daytime.Evening)]
        public void DaytimeOf_MatchesTheGamesBuckets(int hour, Daytime expected)
        {
            Assert.Equal(expected, ClockTime.DaytimeOf(hour));
        }

        [Theory]
        [InlineData(0, 0)]
        [InlineData(59, 0)]
        [InlineData(60, 1)]
        [InlineData(1439, 23)]
        [InlineData(1440, 0)]
        public void HoursOf_DividesAndWraps(int dayMinutes, int expected)
        {
            Assert.Equal(expected, ClockTime.HoursOf(dayMinutes));
        }

        /// <summary>
        /// PassTime is fifteen minutes, so it takes four to move the hour at all - which
        /// is why most single uses cannot change a guard's answer.
        /// </summary>
        [Fact]
        public void Advance_MovesFifteenMinutes()
        {
            Assert.Equal(15, ClockTime.PassTimeMinutes);

            int minutes = 11 * 60;
            Assert.Equal(11, ClockTime.HoursOf(minutes));
            for (int i = 0; i < 3; i++)
            {
                minutes = ClockTime.Advance(minutes);
                Assert.Equal(11, ClockTime.HoursOf(minutes));
            }

            minutes = ClockTime.Advance(minutes);
            Assert.Equal(12, ClockTime.HoursOf(minutes));
        }

        [Fact]
        public void Advance_WrapsPastMidnight()
        {
            int minutes = ClockTime.Advance((23 * 60) + 55);
            Assert.Equal(10, minutes);
            Assert.Equal(0, ClockTime.HoursOf(minutes));
        }

        /// <summary>
        /// The predicates are not a partition, and the two night-ish ones are different
        /// sets: IsNighttime is the clock's IsNight property (dusk, evening, night),
        /// while IsNight the Lua function is night or midnight.
        /// </summary>
        [Theory]
        [InlineData(0, false, true)]
        [InlineData(3, true, true)]
        [InlineData(19, true, false)]
        [InlineData(21, true, false)]
        [InlineData(10, false, false)]
        public void NightPredicates_AreDifferentSets(
            int hour, bool isNighttime, bool isNight)
        {
            Assert.Equal(isNighttime, Ask("IsNighttime", hour));
            Assert.Equal(isNight, Ask("IsNight", hour));
        }

        /// <summary>Midnight satisfies neither IsDaytime nor IsNighttime.</summary>
        [Fact]
        public void Midnight_IsNeitherDayNorNight()
        {
            Assert.False(Ask("IsDaytime", 0));
            Assert.False(Ask("IsNighttime", 0));
        }

        [Theory]
        [InlineData(7, true)]
        [InlineData(10, true)]
        [InlineData(12, false)]
        public void IsMorning_IncludesDawn(int hour, bool expected)
        {
            Assert.Equal(expected, Ask("IsMorning", hour));
        }

        [Theory]
        [InlineData(12, true)]
        [InlineData(15, true)]
        [InlineData(19, false)]
        public void IsAfternoon_IncludesNoon(int hour, bool expected)
        {
            Assert.Equal(expected, Ask("IsAfternoon", hour));
        }

        [Theory]
        [InlineData(19, true)]
        [InlineData(22, true)]
        [InlineData(18, false)]
        public void IsEvening_IncludesDusk(int hour, bool expected)
        {
            Assert.Equal(expected, Ask("IsEvening", hour));
        }

        [Theory]
        [InlineData(8, 8, 12, true)]
        [InlineData(12, 8, 12, true)]
        [InlineData(7, 8, 12, false)]
        [InlineData(13, 8, 12, false)]
        public void IsHourBetween_IsInclusiveAtBothEnds(
            int hour, int first, int second, bool expected)
        {
            Assert.Equal(expected, Ask("IsHourBetween", hour, first, second));
        }

        /// <summary>A range written backwards wraps past midnight rather than being empty.</summary>
        [Theory]
        [InlineData(23, true)]
        [InlineData(2, true)]
        [InlineData(4, true)]
        [InlineData(5, false)]
        [InlineData(12, false)]
        public void IsHourBetween_WrapsWhenFirstExceedsSecond(int hour, bool expected)
        {
            Assert.Equal(expected, Ask("IsHourBetween", hour, 22, 4));
        }

        [Fact]
        public void HourCount_AndTotalHourCount()
        {
            GuardValue hours = ClockTime.Answer("HourCount", NoArguments, 15 * 60, 3);
            Assert.Equal(15d, hours.Number);

            // 24 * (day - 1) + hours
            GuardValue total = ClockTime.Answer("TotalHourCount", NoArguments, 15 * 60, 3);
            Assert.Equal(63d, total.Number);
        }

        /// <summary>
        /// The day counter is not the clock's to move: PassTime advances RealDayCounter,
        /// while DayCount reads dayCounter. So these stay the host's queries, and letting
        /// the clock answer them would be wrong however tempting the name.
        /// </summary>
        [Theory]
        [InlineData("DayCount")]
        [InlineData("IsDayFrom")]
        [InlineData("IsDayUntil")]
        [InlineData("IsKimHere")]
        [InlineData("CheckItem")]
        public void Owns_DisclaimsQueriesTheClockCannotAnswer(string name)
        {
            Assert.False(ClockTime.Owns(name));
            Assert.Equal(
                GuardValueKind.Unknown,
                ClockTime.Answer(name, NoArguments, 0, 1).Kind);
        }

        [Theory]
        [InlineData("IsMorning")]
        [InlineData("IsHourBetween")]
        [InlineData("HourCount")]
        [InlineData("TotalHourCount")]
        [InlineData("IsMidnight")]
        public void Owns_ClaimsTheHourQueries(string name)
        {
            Assert.True(ClockTime.Owns(name));
        }
    }
}
