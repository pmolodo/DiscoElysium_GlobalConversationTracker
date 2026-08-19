using System;
using System.Collections.Generic;
using UnifiedConversationTracker.Core;

namespace UnifiedConversationTracker.Session
{
    /// <summary>
    /// One attempt to read a savegame's SimStatus out of the compressed blobs it
    /// arrived in, rather than out of the game's expanded tables afterwards: either
    /// the rows, or the reason there are none.
    /// </summary>
    /// <remarks>
    /// There is no half-usable outcome on purpose. Interception has to be all-or-
    /// nothing because the alternative is the walk, and merging a partial
    /// interception and then walking anyway would cost both.
    /// </remarks>
    public readonly struct SimStatusInterception
    {
        private readonly IReadOnlyList<SimStatusRow>? _rows;

        private SimStatusInterception(
            IReadOnlyList<SimStatusRow>? rows,
            string? reason,
            SimStatusInterceptionMeasurement measurement)
        {
            _rows = rows;
            Reason = reason;
            Measurement = measurement;
        }

        /// <summary>
        /// Says that nothing could be intercepted, and why - in a form fit to be
        /// logged verbatim, because it is what explains the walk that follows.
        /// </summary>
        /// <param name="reason">Why interception did not happen.</param>
        /// <exception cref="ArgumentException"><paramref name="reason"/> is null or empty.</exception>
        public static SimStatusInterception Unavailable(string reason)
        {
            if (string.IsNullOrEmpty(reason))
            {
                throw new ArgumentException(
                    "An unavailable interception must say why, because that is what explains "
                    + "the fallback walk in the log.",
                    nameof(reason));
            }

            return new SimStatusInterception(null, reason, default);
        }

        /// <summary>Says that the blobs were read and understood.</summary>
        /// <param name="rows">The non-Untouched rows to merge; may be empty.</param>
        /// <param name="measurement">Where the time went, for the log.</param>
        /// <exception cref="ArgumentNullException"><paramref name="rows"/> is null.</exception>
        public static SimStatusInterception Succeeded(
            IReadOnlyList<SimStatusRow> rows, SimStatusInterceptionMeasurement measurement)
        {
            if (rows == null)
            {
                throw new ArgumentNullException(nameof(rows));
            }

            return new SimStatusInterception(rows, null, measurement);
        }

        /// <summary>True when <see cref="Rows"/> may be merged.</summary>
        public bool IsUsable => _rows != null;

        /// <summary>Why there are no rows, or null when there are.</summary>
        public string? Reason { get; }

        /// <summary>Where the time went. Meaningless unless <see cref="IsUsable"/>.</summary>
        public SimStatusInterceptionMeasurement Measurement { get; }

        /// <summary>The rows to merge.</summary>
        /// <exception cref="InvalidOperationException">Interception was not usable.</exception>
        public IReadOnlyList<SimStatusRow> Rows =>
            _rows ?? throw new InvalidOperationException(
                $"This interception produced no rows: {Reason}");

        /// <inheritdoc />
        public override string ToString() =>
            IsUsable
                ? $"SimStatusInterception({_rows!.Count} rows)"
                : $"SimStatusInterception(unavailable: {Reason})";
    }

    /// <summary>
    /// Reads the SimStatus a savegame is carrying straight out of its compressed form,
    /// before the game expands it - the alternative to
    /// <see cref="ISimStatusSource"/>'s walk of the whole master database afterwards.
    /// </summary>
    /// <remarks>
    /// <para>An interface for the same reason <see cref="ISimStatusSource"/> is one:
    /// the session decides WHEN a resync happens and what it means, and can be tested
    /// without the game. The plugin implements this over the Lua <c>Variable</c> table
    /// and an articy id map.</para>
    ///
    /// <para><b>Implementations must never half-succeed.</b> Anything that does not
    /// look right - no map, no blobs, a pair that cannot be read, a count that does
    /// not add up - must come back as
    /// <see cref="SimStatusInterception.Unavailable"/> with a reason, so the session
    /// falls back to the walk and the log says why. Until a prefix has actually run
    /// in a real game, the walk is the path that is known to work.</para>
    /// </remarks>
    public interface ISimStatusInterceptor
    {
        /// <summary>
        /// A short human-readable name for this route, used in log lines so a player
        /// can tell which of the two paths a resync took.
        /// </summary>
        string Description { get; }

        /// <summary>
        /// Attempts one interception. Called from the prefix of the game method that
        /// destroys the compressed form, so it must not modify game state.
        /// </summary>
        SimStatusInterception Intercept();
    }
}
