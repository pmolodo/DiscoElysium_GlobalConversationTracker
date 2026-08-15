using System.Collections.Generic;

namespace UnifiedConversationTracker.Session
{
    /// <summary>
    /// Supplies the SimStatus values the running game currently holds, used to seed
    /// the unified state the first time there is nothing on disk to load.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The plugin implements this over <c>DialogueLua.GetSimStatus</c> and the
    /// dialogue database. It is an interface so the initialization logic can be
    /// tested without the game, and so the (potentially expensive) walk of the whole
    /// dialogue database only happens when a seed is actually needed.
    /// </para>
    /// <para>
    /// <b>Readiness is the whole point of <see cref="IsReady"/>.</b> The Lua
    /// <c>Conversation</c> table is rebuilt wholesale by
    /// <c>PersistentDataManager</c> when a savegame loads, so seeding before that
    /// has happened would copy an all-Untouched table and record nothing. A source
    /// that is not ready is not an error: the seed is deferred and retried on the
    /// next access.
    /// </para>
    /// </remarks>
    public interface ISimStatusSource
    {
        /// <summary>
        /// A short human-readable name for this source, used in log lines so a
        /// player can tell where a seed came from.
        /// </summary>
        string Description { get; }

        /// <summary>
        /// True when the game is far enough along that its SimStatus values are
        /// worth reading. Must be cheap: it is consulted on every access until it
        /// returns true.
        /// </summary>
        bool IsReady { get; }

        /// <summary>
        /// Every SimStatus the game currently holds. Only called when
        /// <see cref="IsReady"/> is true, and at most once per session.
        /// </summary>
        /// <remarks>
        /// Rows whose status string is not recognized are skipped and counted by the
        /// caller, so an implementation should pass the game's value through rather
        /// than filtering or normalizing it. Returning Untouched rows is fine and
        /// costs nothing: the merge rule stores nothing for them.
        /// </remarks>
        IEnumerable<SimStatusRow> EnumerateSimStatuses();
    }
}
