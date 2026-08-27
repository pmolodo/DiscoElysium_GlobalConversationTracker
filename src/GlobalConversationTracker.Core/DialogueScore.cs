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
    /// <para>A score rather than a count because the two statuses above Untouched are
    /// not the same achievement: <see cref="SimStatus.WasDisplayed"/> means the line
    /// was shown, <see cref="SimStatus.WasOffered"/> only that it was listed as a
    /// response option - a line half reached.</para>
    ///
    /// <para>Every score is a whole number of halves, which are exactly representable
    /// in binary floating point, so totals add up without drift and
    /// <see cref="HasHalf"/> needs no tolerance. A double carries halves exactly up
    /// to 2^52; a playthrough reaches five figures.</para>
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
        /// What one seen orb is worth. Same as <see cref="Displayed"/>: clicking an orb
        /// is text actually reached. The game records a single <c>OrbSeen=1</c>, so
        /// there is no half-measure for an orb.
        /// </summary>
        public const double Orb = 1.0;

        /// <summary>
        /// Group separators, no decimals: the all-saves total runs into five figures,
        /// and the game's own money display beside it is grouped the same way.
        /// </summary>
        private const string WholeFormat = "N0";

        /// <summary>The same, plus the one decimal place a half needs.</summary>
        private const string HalfFormat = "N1";

        /// <summary>What one entry at this status is worth.</summary>
        /// <exception cref="ArgumentOutOfRangeException">
        /// <paramref name="status"/> is not a defined <see cref="SimStatus"/> value.
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

        /// <summary>The same, plus seen orbs.</summary>
        public static double Total(int offeredCount, int displayedCount, int orbCount) =>
            (offeredCount * Offered) + (displayedCount * Displayed) + (orbCount * Orb);

        /// <summary>True when the total ends in .5 - the only fraction a total can have.</summary>
        public static bool HasHalf(double score) => score != Math.Floor(score);

        /// <summary>
        /// The total as shown to the player: the decimal place appears only when there
        /// is a half to show.
        /// </summary>
        public static string Format(double score) =>
            score.ToString(HasHalf(score) ? HalfFormat : WholeFormat, CultureInfo.InvariantCulture);
    }
}
