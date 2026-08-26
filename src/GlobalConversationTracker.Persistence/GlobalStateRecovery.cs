using System;

namespace GlobalConversationTracker.Persistence
{
    /// <summary>
    /// The result of trying the live file and, if that failed, the backup
    /// generation. Both attempts are kept so the caller can log exactly what it
    /// found and decide what to do next.
    /// </summary>
    public sealed class GlobalStateRecovery
    {
        internal GlobalStateRecovery(GlobalStateLoadResult live, GlobalStateLoadResult? backup)
        {
            Live = live ?? throw new ArgumentNullException(nameof(live));
            Backup = backup;
        }

        /// <summary>What happened when the live file was read. Always present.</summary>
        public GlobalStateLoadResult Live { get; }

        /// <summary>
        /// What happened when the backup file was read, or null if the backup was
        /// never consulted because the live file loaded.
        /// </summary>
        public GlobalStateLoadResult? Backup { get; }

        /// <summary>
        /// The attempt whose outcome the caller should act on: the backup when it
        /// rescued a failed live load, the live attempt otherwise.
        /// </summary>
        public GlobalStateLoadResult Effective =>
            Backup != null && Backup.IsLoaded ? Backup : Live;

        /// <summary>
        /// True when the live file failed and the backup generation supplied the
        /// state instead. Worth logging loudly: it means the last save did not land.
        /// </summary>
        public bool RecoveredFromBackup => Backup != null && Backup.IsLoaded;

        /// <summary>Shorthand for <c>Effective.Outcome</c>.</summary>
        public GlobalStateLoadOutcome Outcome => Effective.Outcome;

        /// <summary>Shorthand for <c>Effective.State</c>.</summary>
        public GlobalConversationState? State => Effective.State;

        /// <summary>Shorthand for <c>Effective.IsLoaded</c>.</summary>
        public bool IsLoaded => Effective.IsLoaded;

        /// <inheritdoc />
        public override string ToString()
        {
            string backup = Backup == null ? "not consulted" : Backup.Outcome.ToString();
            return $"GlobalStateRecovery(live={Live.Outcome}, backup={backup}, "
                + $"recovered={RecoveredFromBackup})";
        }
    }
}
