// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.LookAhead
{
    /// <summary>
    /// How new a dialogue entry is to the player, ordered so that a plain
    /// <c>&gt;</c> comparison finds the more interesting of two.
    /// </summary>
    /// <remarks>
    /// The three states the mod already distinguishes when colouring an option, in
    /// the same order: seen in this save is the least interesting, never seen in any
    /// save the most. The numeric values are load bearing - the whole look-ahead is a
    /// maximum over this enum - so do not renumber.
    /// </remarks>
    public enum Novelty
    {
        /// <summary>Displayed in the current save. The player has read this.</summary>
        SeenThisGame = 0,

        /// <summary>Displayed in some other save, but not this one.</summary>
        UnseenThisGame = 1,

        /// <summary>Never displayed in any save.</summary>
        UnseenAnyGame = 2,
    }
}
