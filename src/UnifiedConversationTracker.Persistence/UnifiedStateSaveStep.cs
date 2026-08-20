namespace UnifiedConversationTracker.Persistence
{
    /// <summary>
    /// The points during <see cref="UnifiedStateStore.Save"/> at which the process
    /// could die. Internal: this exists so tests can crash the save exactly here,
    /// in-process by throwing and out-of-process by killing the process, and prove
    /// the on-disk result is recoverable. Not part of the public API.
    /// </summary>
    internal enum UnifiedStateSaveStep
    {
        /// <summary>
        /// Inside the temp-file write, after a chunk has been pushed to the OS but
        /// before the rest of the payload. Crashing here leaves a truncated temp file
        /// and an untouched live file.
        /// </summary>
        DuringTempWrite,

        /// <summary>
        /// The temp file is complete and flushed to disk; the live file is still the
        /// old generation. Crashing here leaves a stale temp file that the next save
        /// truncates.
        /// </summary>
        AfterTempFlushed,

        /// <summary>
        /// The old live file has been renamed over the backup slot. This is the only
        /// window in which no live file exists, and it is exactly why a backup
        /// generation is kept: the previous state is complete in the backup slot the
        /// whole time.
        /// </summary>
        AfterLiveRotatedToBackup,

        /// <summary>The temp file has been renamed over the live path. The save is done.</summary>
        AfterTempPromoted,
    }
}
