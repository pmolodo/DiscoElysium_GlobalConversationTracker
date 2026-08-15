using System;

namespace UnifiedConversationTracker.Session
{
    /// <summary>
    /// The minimal logging surface the initialization path needs.
    /// </summary>
    /// <remarks>
    /// This exists so <see cref="UnifiedStateSession"/> can be loud about recovery
    /// without referencing BepInEx. The plugin adapts a
    /// <c>BepInEx.Logging.ManualLogSource</c> onto it; tests capture the lines and
    /// assert on them, which is the only way a "log loudly" requirement can be
    /// tested at all.
    /// </remarks>
    public interface IUnifiedStateLog
    {
        /// <summary>Ordinary progress: what was loaded, from where, how much.</summary>
        void Info(string message);

        /// <summary>
        /// Something recoverable that the player should know happened, such as the
        /// live file being unusable and the backup generation taking over.
        /// </summary>
        void Warning(string message);

        /// <summary>Something that lost data or disabled part of the mod.</summary>
        void Error(string message);
    }

    /// <summary>A log that discards everything. Only for callers that genuinely want silence.</summary>
    public sealed class NullUnifiedStateLog : IUnifiedStateLog
    {
        /// <summary>The shared instance.</summary>
        public static readonly NullUnifiedStateLog Instance = new NullUnifiedStateLog();

        private NullUnifiedStateLog()
        {
        }

        /// <inheritdoc />
        public void Info(string message)
        {
        }

        /// <inheritdoc />
        public void Warning(string message)
        {
        }

        /// <inheritdoc />
        public void Error(string message)
        {
        }
    }

    /// <summary>
    /// Adapts three delegates onto <see cref="IUnifiedStateLog"/>, for hosts whose
    /// logger is a set of methods rather than an object.
    /// </summary>
    public sealed class DelegateUnifiedStateLog : IUnifiedStateLog
    {
        private readonly Action<string> _info;
        private readonly Action<string> _warning;
        private readonly Action<string> _error;

        /// <summary>Creates a log that forwards to the given delegates.</summary>
        /// <exception cref="ArgumentNullException">Any delegate is null.</exception>
        public DelegateUnifiedStateLog(Action<string> info, Action<string> warning, Action<string> error)
        {
            _info = info ?? throw new ArgumentNullException(nameof(info));
            _warning = warning ?? throw new ArgumentNullException(nameof(warning));
            _error = error ?? throw new ArgumentNullException(nameof(error));
        }

        /// <inheritdoc />
        public void Info(string message) => _info(message);

        /// <inheritdoc />
        public void Warning(string message) => _warning(message);

        /// <inheritdoc />
        public void Error(string message) => _error(message);
    }
}
