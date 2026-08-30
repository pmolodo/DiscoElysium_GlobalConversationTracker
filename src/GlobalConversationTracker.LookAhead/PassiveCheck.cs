// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.LookAhead
{
    /// <summary>
    /// Whether a passive skill check fires, given the numbers.
    /// </summary>
    /// <remarks>
    /// <para>The arithmetic half of <c>PassiveNode.CheckSuccess</c>, kept here rather than
    /// beside the game code that gathers its inputs so that it can be tested. Reading a
    /// skill value out of the character sheet needs the game running; deciding what the
    /// number means does not, and the inversion is the part worth a test.</para>
    ///
    /// <para>No dice. A passive check is a comparison, which is what lets a look-ahead
    /// from the player's current state answer definitely instead of carrying both
    /// branches through the 10,500 entries in the database that carry one.</para>
    /// </remarks>
    public static class PassiveCheck
    {
        /// <summary>
        /// The flat bonus every passive check gets: the game tests
        /// <c>skillValue + 6 >= minSkillValue</c>.
        /// </summary>
        public const int SkillBonus = 6;

        /// <summary>Whether the check clears its threshold, before any inversion.</summary>
        /// <param name="skillValue">The character's value in the relevant skill.</param>
        /// <param name="threshold">
        /// The difficulty threshold, already converted from the entry's difficulty id and
        /// already adjusted for thoughts.
        /// </param>
        public static bool Clears(int skillValue, int threshold)
        {
            return skillValue + SkillBonus >= threshold;
        }

        /// <summary>Whether the entry fires.</summary>
        /// <remarks>
        /// An antipassive entry is the mirror of an ordinary one: it is the line that
        /// shows when you are NOT sharp enough, so it fires exactly when the check fails.
        /// </remarks>
        /// <param name="skillValue">The character's value in the relevant skill.</param>
        /// <param name="threshold">The adjusted difficulty threshold.</param>
        /// <param name="antipassive">Whether the entry carries the Antipassive field.</param>
        public static Ternary Outcome(int skillValue, int threshold, bool antipassive)
        {
            return Clears(skillValue, threshold) != antipassive
                ? Ternary.True
                : Ternary.False;
        }
    }
}
