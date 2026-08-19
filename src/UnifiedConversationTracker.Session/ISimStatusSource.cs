using System.Collections.Generic;
using UnifiedConversationTracker.Core;

namespace UnifiedConversationTracker.Session
{
    /// <summary>
    /// Supplies the SimStatus values the running game currently holds, for
    /// <see cref="UnifiedStateSession.ResyncFromGame"/> to merge in after a savegame
    /// load has rewritten them behind the write-through hook's back.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The plugin implements this over the Lua <c>Conversation</c> table and the
    /// dialogue database. It is an interface so the session logic can be tested
    /// without the game, and so the (not free) walk of the whole dialogue database
    /// only happens when a resync actually runs.
    /// </para>
    /// <para>
    /// <b><see cref="IsReady"/> is a precondition, not a scheduler.</b> It says
    /// whether the tables the walk reads exist yet. The session does not defer or
    /// retry on it - the caller's trigger point, a postfix on the very method that
    /// rewrites the SimStatus table, already guarantees the timing (de-0s5). It is
    /// checked so that a walk which cannot work is skipped with a clear log line,
    /// rather than throwing and disabling the resync for the whole session.
    /// </para>
    /// </remarks>
    public interface ISimStatusSource
    {
        /// <summary>
        /// A short human-readable name for this source, used in log lines so a
        /// player can tell where a reading came from.
        /// </summary>
        string Description { get; }

        /// <summary>
        /// True when the tables <see cref="EnumerateSimStatuses"/> reads are present.
        /// Must be cheap: it is consulted before every walk.
        /// </summary>
        bool IsReady { get; }

        /// <summary>
        /// Every SimStatus the game currently holds. Only called when
        /// <see cref="IsReady"/> is true.
        /// </summary>
        /// <remarks>
        /// Rows whose status string is not recognized are skipped and counted by the
        /// caller, so an implementation should pass the game's value through rather
        /// than filtering or normalizing it. Returning Untouched rows is fine and
        /// costs nothing: the merge rule stores nothing for them.
        /// </remarks>
        IEnumerable<SimStatusRow> EnumerateSimStatuses();

        /// <summary>
        /// A one-line breakdown of where the most recent walk spent its time, or null
        /// from a source that does not measure itself (which is every source but the
        /// real one). Logged verbatim next to the resync's own total.
        /// </summary>
        /// <remarks>
        /// This exists because the resync's total is one number for a walk, a merge
        /// and a decision, and nothing in a BepInEx log said which of them was slow -
        /// the first in-game measurement was 1744 ms and could not be attributed
        /// (de-p1h). Called once per walk, after the walk, so it may format freely.
        /// </remarks>
        string? DescribeLastWalk() => null;
    }
}
