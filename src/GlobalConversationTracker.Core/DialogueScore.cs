// SPDX-License-Identifier: MIT
using System;
using System.Globalization;

namespace GlobalConversationTracker
{
    /// <summary>
    /// What a dialogue entry is worth towards a displayed count, and how such a
    /// count is written out.
    /// </summary>
    /// <remarks>
    /// <para><b>Why a score and not a count.</b> The two statuses above Untouched are
    /// not the same achievement. <see cref="SimStatus.WasDisplayed"/> means the line
    /// was actually shown; <see cref="SimStatus.WasOffered"/> means it was only ever
    /// listed as a response option, which is a line half reached. Counting both as
    /// one flattened that difference away, so an entry that was merely offered is
    /// worth <see cref="Offered"/> and one that was displayed is worth
    /// <see cref="Displayed"/>.</para>
    ///
    /// <para><b>Why the total is exact.</b> Every score is a whole number of halves,
    /// and halves are exactly representable in binary floating point, so totals add
    /// up without drift and <see cref="HasHalf"/> needs no tolerance. That holds far
    /// past any total the game can produce - a playthrough reaches five figures, and
    /// a double carries halves exactly up to 2^52.</para>
    /// </remarks>
    public static class DialogueScore
    {
        /// <summary>What an entry at <see cref="SimStatus.Untouched"/> is worth.</summary>
        public const double Untouched = 0.0;

        /// <summary>What an entry at <see cref="SimStatus.WasOffered"/> is worth.</summary>
        public const double Offered = 0.5;

        /// <summary>What an entry at <see cref="SimStatus.WasDisplayed"/> is worth.</summary>
        public const double Displayed = 1.0;

        /// <summary>
        /// What one seen orb is worth. The same as <see cref="Displayed"/>, and for the
        /// same reason: clicking an orb is text actually reached, not text merely
        /// offered. There is no half-measure for an orb - the game records a single
        /// <c>OrbSeen=1</c> and nothing between.
        /// </summary>
        public const double Orb = 1.0;

        /// <summary>
        /// Group separators, no decimals: the all-saves total runs into five figures,
        /// and the game's own money display beside it is grouped the same way.
        /// </summary>
        private const string WholeFormat = "N0";

        /// <summary>
        /// The same, plus the one decimal place a half needs. One place and not more:
        /// ".5" is the only fraction a total can ever end in.
        /// </summary>
        private const string HalfFormat = "N1";

        /// <summary>What one entry at this status is worth.</summary>
        /// <exception cref="ArgumentOutOfRangeException">
        /// <paramref name="status"/> is not one of the three defined
        /// <see cref="SimStatus"/> values.
        /// </exception>
        public static double Of(SimStatus status)
        {
            switch (status)
            {
                case SimStatus.Untouched:
                    return Untouched;
                case SimStatus.WasOffered:
                    return Offered;
                case SimStatus.WasDisplayed:
                    return Displayed;
                default:
                    throw new ArgumentOutOfRangeException(
                        nameof(status),
                        status,
                        "Not a defined SimStatus value.");
            }
        }

        /// <summary>
        /// The total for a population of entries: so many only ever offered, so many
        /// actually displayed.
        /// </summary>
        public static double Total(int offeredCount, int displayedCount) =>
            Total(offeredCount, displayedCount, 0);

        /// <summary>
        /// The same, plus seen orbs. Orbs are whole numbers, so they cannot introduce a
        /// fraction that <see cref="HasHalf"/> and <see cref="Format"/> do not already
        /// handle.
        /// </summary>
        public static double Total(int offeredCount, int displayedCount, int orbCount) =>
            (offeredCount * Offered) + (displayedCount * Displayed) + (orbCount * Orb);

        /// <summary>
        /// True when the total ends in .5, which is the only case where a total is
        /// not a whole number.
        /// </summary>
        public static bool HasHalf(double score) => score != Math.Floor(score);

        /// <summary>
        /// The total as it is shown to the player: the decimal place appears only when
        /// there is a half to show, so a whole total reads as the plain count it is.
        /// </summary>
        public static string Format(double score) =>
            score.ToString(HasHalf(score) ? HalfFormat : WholeFormat, CultureInfo.InvariantCulture);
    }
}
