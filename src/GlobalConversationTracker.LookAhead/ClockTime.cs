// SPDX-License-Identifier: MIT
using System.Collections.Generic;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>
    /// The game's clock, as far as a dialogue guard can see it.
    /// </summary>
    /// <remarks>
    /// <para>Reproduces <c>SunshineClockTime</c> and <c>DaytimeLuaFunctions</c> so the
    /// look-ahead can answer "what time would it be here" for a path that advances the
    /// clock. The game can only report the time it is now.</para>
    ///
    /// <para>Two facts shrink this to almost nothing. <c>PassTime()</c> takes no
    /// arguments and moves the clock exactly fifteen minutes - all 207 uses in the
    /// database are the bare call - and it does NOT advance the day: the
    /// <c>DayMinutes</c> setter rolls <c>RealDayCounter</c> past midnight, while
    /// <c>DayCount()</c> reads <c>dayCounter</c>, which only moves when the story
    /// assigns it. So a conversation can change the hour and never the day, which leaves
    /// the 158 <c>DayCount</c> / <c>IsDayFrom</c> / <c>IsDayUntil</c> guards fixed for
    /// the whole crawl.</para>
    ///
    /// <para>The predicates are not the tidy partition their names suggest, and the
    /// differences are load bearing: <c>IsNighttime()</c> is dusk, evening or night,
    /// while <c>IsNight()</c> is night or midnight, and midnight satisfies neither
    /// <c>IsDaytime()</c> nor <c>IsNighttime()</c>.</para>
    /// </remarks>
    public static class ClockTime
    {
        /// <summary>How long a day is, in minutes.</summary>
        public const int MinutesInADay = 1440;

        /// <summary>
        /// How far one <c>PassTime()</c> moves the clock. <c>Clang()</c> is
        /// <c>NormalTimeForward(15, wasSleeping: false)</c>, so it takes four of them to
        /// cross an hour boundary at all.
        /// </summary>
        public const int PassTimeMinutes = 15;

        /// <summary>The hour of the day a minute count falls in, 0 to 23.</summary>
        /// <param name="dayMinutes">Minutes since midnight.</param>
        public static int HoursOf(int dayMinutes)
        {
            int wrapped = dayMinutes % MinutesInADay;
            if (wrapped < 0)
            {
                wrapped += MinutesInADay;
            }

            return wrapped / 60;
        }

        /// <summary>Advances a minute count by one <c>PassTime()</c>, wrapping at midnight.</summary>
        /// <param name="dayMinutes">Minutes since midnight.</param>
        public static int Advance(int dayMinutes)
        {
            return (dayMinutes + PassTimeMinutes) % MinutesInADay;
        }

        /// <summary>Which bucket an hour falls in.</summary>
        /// <param name="hours">The hour, 0 to 23.</param>
        public static Daytime DaytimeOf(int hours)
        {
            switch (hours)
            {
                case 0:
                    return Daytime.Midnight;
                case 1:
                case 2:
                case 3:
                case 4:
                case 5:
                case 6:
                    return Daytime.Night;
                case 7:
                    return Daytime.Dawn;
                case 8:
                case 9:
                case 10:
                case 11:
                    return Daytime.Morning;
                case 12:
                    return Daytime.Noon;
                case 13:
                case 14:
                case 15:
                case 16:
                case 17:
                case 18:
                    return Daytime.Afternoon;
                case 19:
                    return Daytime.Dusk;
                case 20:
                case 21:
                case 22:
                case 23:
                    return Daytime.Evening;
                default:
                    return Daytime.Dawn;
            }
        }

        /// <summary>
        /// <c>IsHourBetween(first, second)</c>, including a range that wraps past
        /// midnight - <c>IsHourBetween(22, 4)</c> is the small hours, not the empty set.
        /// Both ends are inclusive.
        /// </summary>
        /// <param name="hours">The current hour.</param>
        /// <param name="first">The range's start hour.</param>
        /// <param name="second">The range's end hour.</param>
        public static bool IsHourBetween(int hours, int first, int second)
        {
            if (first > second)
            {
                return hours < first ? hours <= second : true;
            }

            return hours >= first && hours <= second;
        }

        /// <summary>
        /// Answers a clock query, or <see cref="GuardValue.Unknown"/> if it is not one.
        /// </summary>
        /// <remarks>
        /// The one place the game's predicate names are mapped onto buckets. Callers pass
        /// every query through here and fall back to the host for anything it declines,
        /// so a query the clock does not own cannot be silently answered from the clock.
        /// </remarks>
        /// <param name="name">The function's name.</param>
        /// <param name="arguments">Its evaluated arguments.</param>
        /// <param name="dayMinutes">Minutes since midnight, as the crawl has them.</param>
        /// <param name="dayCounter">The day number, which a crawl cannot change.</param>
        public static GuardValue Answer(
            string name, IReadOnlyList<GuardValue> arguments, int dayMinutes, int dayCounter)
        {
            int hours = HoursOf(dayMinutes);
            Daytime daytime = DaytimeOf(hours);

            switch (name)
            {
                case "IsDaytime":
                    return GuardValue.FromBoolean(
                        daytime == Daytime.Dawn || daytime == Daytime.Morning
                        || daytime == Daytime.Noon || daytime == Daytime.Afternoon);

                case "IsNighttime":
                    // The SunshineClockTime.IsNight property: dusk, evening or night.
                    return GuardValue.FromBoolean(
                        daytime == Daytime.Dusk || daytime == Daytime.Evening
                        || daytime == Daytime.Night);

                case "IsNight":
                    // The Lua function, which is a different set: night or midnight.
                    return GuardValue.FromBoolean(
                        daytime == Daytime.Night || daytime == Daytime.Midnight);

                case "IsDawn":
                    return GuardValue.FromBoolean(daytime == Daytime.Dawn);

                case "IsMorning":
                    return GuardValue.FromBoolean(
                        daytime == Daytime.Dawn || daytime == Daytime.Morning);

                case "IsNoon":
                    return GuardValue.FromBoolean(daytime == Daytime.Noon);

                case "IsAfternoon":
                    return GuardValue.FromBoolean(
                        daytime == Daytime.Noon || daytime == Daytime.Afternoon);

                case "IsDusk":
                    return GuardValue.FromBoolean(daytime == Daytime.Dusk);

                case "IsEvening":
                    return GuardValue.FromBoolean(
                        daytime == Daytime.Dusk || daytime == Daytime.Evening);

                case "IsMidnight":
                    return GuardValue.FromBoolean(daytime == Daytime.Midnight);

                case "IsHour":
                    return arguments.Count >= 1 && arguments[0].TryAsNumber(out double hour)
                        ? GuardValue.FromBoolean(hours == (int)hour)
                        : GuardValue.Unknown;

                case "IsHourBetween":
                    if (arguments.Count >= 2
                        && arguments[0].TryAsNumber(out double first)
                        && arguments[1].TryAsNumber(out double second))
                    {
                        return GuardValue.FromBoolean(
                            IsHourBetween(hours, (int)first, (int)second));
                    }

                    return GuardValue.Unknown;

                case "HourCount":
                    return GuardValue.FromNumber(hours);

                case "TotalHourCount":
                    return GuardValue.FromNumber((24 * (dayCounter - 1)) + hours);

                default:
                    return GuardValue.Unknown;
            }
        }

        /// <summary>Whether a query's answer depends on the clock.</summary>
        /// <remarks>
        /// Deliberately excludes <c>DayCount</c>, <c>IsDayFrom</c> and
        /// <c>IsDayUntil</c>. They read the story's day counter, which no amount of
        /// <c>PassTime</c> moves, so they are constants for a crawl and belong to the
        /// host.
        /// </remarks>
        /// <param name="name">The function's name.</param>
        public static bool Owns(string name)
        {
            switch (name)
            {
                case "IsDaytime":
                case "IsNighttime":
                case "IsNight":
                case "IsDawn":
                case "IsMorning":
                case "IsNoon":
                case "IsAfternoon":
                case "IsDusk":
                case "IsEvening":
                case "IsMidnight":
                case "IsHour":
                case "IsHourBetween":
                case "HourCount":
                case "TotalHourCount":
                    return true;
                default:
                    return false;
            }
        }
    }
}
