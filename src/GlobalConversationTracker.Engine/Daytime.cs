// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// The part of the day an hour falls in, as <c>SunshineClockTime.GetDaytime</c>
    /// buckets it.
    /// </summary>
    /// <remarks>
    /// Nine buckets over twenty-four hours, unevenly sized: four of them are a single
    /// hour. Reproduced rather than referenced so the look-ahead can ask what time it
    /// would be after a path advances the clock, which the game has no way to answer.
    /// </remarks>
    public enum Daytime
    {
        /// <summary>Hour 0.</summary>
        Midnight = 0,

        /// <summary>Hours 1 to 6.</summary>
        Night = 1,

        /// <summary>Hour 7.</summary>
        Dawn = 2,

        /// <summary>Hours 8 to 11.</summary>
        Morning = 3,

        /// <summary>Hour 12.</summary>
        Noon = 4,

        /// <summary>Hours 13 to 18.</summary>
        Afternoon = 5,

        /// <summary>Hour 19.</summary>
        Dusk = 6,

        /// <summary>Hours 20 to 23.</summary>
        Evening = 7,
    }
}
