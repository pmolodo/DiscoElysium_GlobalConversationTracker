// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker
{
    /// <summary>
    /// The "seen" status the Dialogue System records for a single dialogue entry.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Mirrors the <c>SimStatus</c> string stored in the game's <c>Conversation</c>
    /// Lua table (see <c>PixelCrushers.DialogueSystem.DialogueLua</c>, which defines
    /// exactly the three constants "Untouched", "WasOffered" and "WasDisplayed").
    /// </para>
    /// <para>
    /// The numeric values define the merge ordering and are load bearing:
    /// <c>Untouched &lt; WasOffered &lt; WasDisplayed</c>. Do not renumber them.
    /// </para>
    /// </remarks>
    public enum SimStatus
    {
        /// <summary>The entry has never been offered or shown. The default for anything unrecorded.</summary>
        Untouched = 0,

        /// <summary>The entry was presented as a player response option but never actually shown.</summary>
        WasOffered = 1,

        /// <summary>The entry was actually displayed to the player.</summary>
        WasDisplayed = 2,
    }
}
