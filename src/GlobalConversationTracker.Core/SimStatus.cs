// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker
{
    /// <summary>
    /// The "seen" status the Dialogue System records for a single dialogue entry.
    /// </summary>
    /// <remarks>
    /// Mirrors the <c>SimStatus</c> string in the game's <c>Conversation</c> Lua table
    /// (<c>PixelCrushers.DialogueSystem.DialogueLua</c> defines exactly these three).
    /// The numeric values define the merge ordering and are load bearing:
    /// <c>Untouched &lt; WasOffered &lt; WasDisplayed</c>. Do not renumber.
    /// </remarks>
    public enum SimStatus
    {
        /// <summary>Never offered or shown. The default for anything unrecorded.</summary>
        Untouched = 0,

        /// <summary>Offered as a player response option but never shown.</summary>
        WasOffered = 1,

        /// <summary>Displayed to the player.</summary>
        WasDisplayed = 2,
    }
}
