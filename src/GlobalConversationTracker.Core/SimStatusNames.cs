using System;

namespace GlobalConversationTracker
{
    /// <summary>
    /// Conversion between <see cref="SimStatus"/> and the exact status strings the
    /// game uses in its Lua state.
    /// </summary>
    /// <remarks>
    /// Matching is ordinal and case sensitive: the game only ever writes these three
    /// literals (they are <c>public const string</c> fields on
    /// <c>PixelCrushers.DialogueSystem.DialogueLua</c>), so anything else is a signal
    /// that an assumption is wrong rather than something to silently coerce.
    /// </remarks>
    public static class SimStatusNames
    {
        /// <summary>The game's string for <see cref="SimStatus.Untouched"/>.</summary>
        public const string Untouched = "Untouched";

        /// <summary>The game's string for <see cref="SimStatus.WasOffered"/>.</summary>
        public const string WasOffered = "WasOffered";

        /// <summary>The game's string for <see cref="SimStatus.WasDisplayed"/>.</summary>
        public const string WasDisplayed = "WasDisplayed";

        /// <summary>
        /// Returns the exact string the game uses for <paramref name="status"/>.
        /// </summary>
        /// <exception cref="ArgumentOutOfRangeException">
        /// <paramref name="status"/> is not one of the three defined values.
        /// </exception>
        public static string ToGameString(SimStatus status)
        {
            switch (status)
            {
                case SimStatus.Untouched:
                    return Untouched;
                case SimStatus.WasOffered:
                    return WasOffered;
                case SimStatus.WasDisplayed:
                    return WasDisplayed;
                default:
                    throw new ArgumentOutOfRangeException(
                        nameof(status),
                        status,
                        "Not a defined SimStatus value.");
            }
        }

        /// <summary>
        /// Parses a status string coming from the game.
        /// </summary>
        /// <returns><c>true</c> if <paramref name="name"/> is one of the three known
        /// strings; otherwise <c>false</c>, with <paramref name="status"/> set to
        /// <see cref="SimStatus.Untouched"/>.</returns>
        public static bool TryParse(string? name, out SimStatus status)
        {
            switch (name)
            {
                case Untouched:
                    status = SimStatus.Untouched;
                    return true;
                case WasOffered:
                    status = SimStatus.WasOffered;
                    return true;
                case WasDisplayed:
                    status = SimStatus.WasDisplayed;
                    return true;
                default:
                    status = SimStatus.Untouched;
                    return false;
            }
        }

        /// <summary>
        /// Parses a status string coming from the game, failing loudly on anything
        /// unrecognized.
        /// </summary>
        /// <exception cref="ArgumentException">
        /// <paramref name="name"/> is not one of the three known strings.
        /// </exception>
        public static SimStatus Parse(string? name)
        {
            if (!TryParse(name, out SimStatus status))
            {
                throw new ArgumentException(
                    $"Unrecognized SimStatus '{name ?? "<null>"}'. Expected one of " +
                    $"'{Untouched}', '{WasOffered}', '{WasDisplayed}'.",
                    nameof(name));
            }

            return status;
        }
    }
}
