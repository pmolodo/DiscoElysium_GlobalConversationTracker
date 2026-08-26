// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.Session
{
    /// <summary>
    /// Where the session's in-memory global state came from. Reported so a log
    /// reader (and a test) can tell a normal load from a recovery, and a recovery
    /// from a first run.
    /// </summary>
    /// <remarks>
    /// This describes the DISK read only, which happens once per session. It is not
    /// updated by <see cref="GlobalStateSession.ResyncFromSaveRawBytes"/>: a resync merges
    /// into whatever was loaded, and overwriting the origin would erase the one
    /// signal that says a recovery happened.
    /// </remarks>
    public enum GlobalStateOrigin
    {
        /// <summary><see cref="GlobalStateSession.EnsureInitialized"/> has not run yet.</summary>
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
        /// Nothing usable was on disk, so the state started empty. A first run, or a
        /// run after both generations were lost. Not a failure and not transient:
        /// the state fills up from the write-through hook as the game is played and
        /// from <see cref="GlobalStateSession.ResyncFromSaveRawBytes"/> on every savegame
        /// load, which is what recovers history the lost file used to hold.
        /// </summary>
        NoStateOnDisk = 3,

        /// <summary>
        /// The live file was written by a newer build of the mod. The file is intact
        /// history, so it is left completely alone: the state is empty and
        /// <see cref="GlobalStateSession.CanSave"/> is false for the whole session.
        /// </summary>
        RefusedNewerFormat = 4,
    }
}
