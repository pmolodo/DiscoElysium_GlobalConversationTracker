using System;
using UnifiedConversationTracker.Session;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// Reports failures inside a game hook, and switches that hook off once they stop
    /// looking like one-offs.
    /// </summary>
    /// <remarks>
    /// A hook runs on the game's own frame, so an error that repeats produces one log
    /// line per occurrence for the rest of the session and buries everything else. A
    /// mod that corrupts a playthrough is worse than a mod that stops tracking, so
    /// every hook swallows its failures; this is what keeps that from being silent
    /// and from being endless.
    /// </remarks>
    internal sealed class HookFailureLimiter
    {
        /// <summary>
        /// How many failures are reported before the hook gives up. The first is the
        /// one that matters; a handful more in case the first was a one-off.
        /// </summary>
        private const int MaxFailures = 10;

        private readonly string _activity;
        private readonly IUnifiedStateLog _log;

        private int _failureCount;

        /// <summary>Creates a limiter for one hook.</summary>
        /// <param name="activity">
        /// What the hook does, as a gerund phrase that reads inside "Giving up on
        /// ..." - for instance "recording dialogue statuses into the unified state".
        /// </param>
        /// <param name="log">Where failures are reported.</param>
        /// <exception cref="ArgumentNullException">Any argument is null.</exception>
        internal HookFailureLimiter(string activity, IUnifiedStateLog log)
        {
            _activity = activity ?? throw new ArgumentNullException(nameof(activity));
            _log = log ?? throw new ArgumentNullException(nameof(log));
        }

        /// <summary>True once the hook has failed often enough to be switched off.</summary>
        internal bool HasGivenUp => _failureCount >= MaxFailures;

        /// <summary>Reports one failure, and says so if it was the last one allowed.</summary>
        internal void Report(Exception ex)
        {
            _failureCount++;
            _log.Error($"Error while {_activity} ({_failureCount} of {MaxFailures} allowed): {ex}");

            if (HasGivenUp)
            {
                _log.Error(
                    $"Giving up on {_activity} for the rest of this session; the game itself is "
                    + "unaffected. Restart the game to try again.");
            }
        }
    }
}
