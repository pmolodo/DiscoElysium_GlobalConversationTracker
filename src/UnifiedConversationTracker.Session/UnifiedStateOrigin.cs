namespace UnifiedConversationTracker.Session
{
    /// <summary>
    /// Where the session's in-memory unified state came from. Reported so a log
    /// reader (and a test) can tell a normal load from a recovery, and a recovery
    /// from a first run.
    /// </summary>
    public enum UnifiedStateOrigin
    {
        /// <summary><see cref="UnifiedStateSession.EnsureInitialized"/> has not run yet.</summary>
        Uninitialized = 0,

        /// <summary>The live file parsed and supplied the state. The ordinary case.</summary>
        LiveFile = 1,

        /// <summary>
        /// The live file was missing or unparseable and the backup generation
        /// supplied the state instead. Everything recorded since the last rotation
        /// is gone; this is always logged as a warning.
        /// </summary>
        BackupFile = 2,

        /// <summary>
        /// Nothing usable was on disk, so the state was copied out of the running
        /// game. A first run, or a run after both generations were lost.
        /// </summary>
        SeededFromGame = 3,

        /// <summary>
        /// Nothing usable was on disk and the game was not ready to be read yet, so
        /// the state is empty and the seed will be retried on the next access. This
        /// is a transient state, not a failure.
        /// </summary>
        AwaitingGame = 4,

        /// <summary>
        /// The live file was written by a newer build of the mod. The file is intact
        /// history, so it is left completely alone: the state is empty and
        /// <see cref="UnifiedStateSession.CanSave"/> is false for the whole session.
        /// </summary>
        RefusedNewerFormat = 5,

        /// <summary>
        /// Nothing usable was on disk and reading the game threw. The state is empty
        /// and no further seed is attempted this session.
        /// </summary>
        SeedFailed = 6,
    }
}
