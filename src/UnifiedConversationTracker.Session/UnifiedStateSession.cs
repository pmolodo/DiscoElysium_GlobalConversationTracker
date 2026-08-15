using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using UnifiedConversationTracker.Persistence;

namespace UnifiedConversationTracker.Session
{
    /// <summary>
    /// Owns the one unified state a game session has: reads it from disk exactly
    /// once, resyncs it from the running game whenever a savegame load rewrites the
    /// game's own tables, and is the only thing that decides whether saving is
    /// allowed.
    /// </summary>
    /// <remarks>
    /// <para><b>The trigger is explicit and idempotent.</b> Nothing happens in the
    /// constructor, so building a session during BepInEx chainload touches neither
    /// the disk nor the game. Every access goes through
    /// <see cref="EnsureInitialized"/>, which does the disk read once per session
    /// and returns the same state object every time afterwards. The state object
    /// exists and is valid from construction (empty, never null), so a hook that
    /// fires while initialization is in flight sees an empty state rather than a
    /// half-built one, and anything it merges survives: the merge rule only ever
    /// raises a status, so a later load or resync cannot undo it.</para>
    ///
    /// <para><b>Two ways in, and they cover different writers.</b>
    /// <see cref="Record"/> is the write-through path, driven by the hook on
    /// <c>DialogueLua.MarkDialogueEntry</c>: everything the game marks while it is
    /// being played. <see cref="ResyncFromGame"/> is the load-time path, driven by
    /// the hook on <c>PersistentDataManager.ExpandCompressedSimStatusData</c>: that
    /// method rebuilds the whole Lua SimStatus table from a savegame without ever
    /// calling <c>MarkDialogueEntry</c> (de-0s5), so nothing else would see it.
    /// Between them they see every SimStatus the game ever holds - there is no
    /// third writer (de-0s5 audited the whole build for one).</para>
    ///
    /// <para><b>There is no first-mark seed any more</b> (de-omm.23). There used to
    /// be one, reading the whole game the first time a line was marked in a session
    /// with no state file, and it cost 649 ms on the main thread at the moment a
    /// conversation opened. It was a proxy for "a save has been loaded, so the Lua
    /// table is real"; the load hook is that condition directly, so the proxy - and
    /// its deferral, its retry, and the rule that an empty seed must never be
    /// persisted lest the file suppress a later one - is gone. The resync strictly
    /// subsumes it: it reads the same thing, it runs on every load rather than once
    /// per playthrough-ever, and it runs inside a loading screen. Starting a brand
    /// new game triggers neither, which is correct: a new game's table is
    /// all-Untouched, so there is nothing to read, and everything from there on is
    /// a mark.</para>
    ///
    /// <para>Not thread safe in the sense of being lock-free, but every public
    /// method takes the same lock and C# locks are reentrant, so a hook that
    /// re-enters during initialization gets the current state back instead of
    /// deadlocking or recursing.</para>
    /// </remarks>
    public sealed class UnifiedStateSession
    {
        /// <summary>
        /// How many skipped-row descriptions a walk of the game logs before it stops
        /// listing them. Deliberately the file loader's own cap, so a skipped game row
        /// and a skipped file row read the same way in a log.
        /// </summary>
        public const int MaxGameWalkWarnings = UnifiedStateJson.MaxWarnings;

        /// <summary>What a null status string is called in the log and in the set of
        /// already-warned-about statuses, since null cannot go in the set itself.</summary>
        private const string NullStatusName = "null";

        private readonly object _gate = new object();
        private readonly UnifiedStateStore _store;
        private readonly ISimStatusSource _source;
        private readonly IUnifiedStateLog _log;

        private readonly UnifiedConversationState _state = new UnifiedConversationState();

        /// <summary>Status strings from the game that have already been warned about.</summary>
        private readonly HashSet<string> _unrecognizedStatuses = new HashSet<string>(StringComparer.Ordinal);

        private bool _diskLoadDone;
        private bool _resyncGivenUp;

        /// <summary>Creates a session. Nothing is read, written or logged yet.</summary>
        /// <param name="store">The store over the SaveGames directory.</param>
        /// <param name="simStatusSource">The running game, for the resync path.</param>
        /// <param name="log">Where recovery and resyncing are reported.</param>
        /// <exception cref="ArgumentNullException">Any argument is null.</exception>
        public UnifiedStateSession(
            UnifiedStateStore store,
            ISimStatusSource simStatusSource,
            IUnifiedStateLog log)
        {
            _store = store ?? throw new ArgumentNullException(nameof(store));
            _source = simStatusSource ?? throw new ArgumentNullException(nameof(simStatusSource));
            _log = log ?? throw new ArgumentNullException(nameof(log));
        }

        /// <summary>The store this session reads and writes.</summary>
        public UnifiedStateStore Store => _store;

        /// <summary>Where the in-memory state came from.</summary>
        public UnifiedStateOrigin Origin { get; private set; } = UnifiedStateOrigin.Uninitialized;

        /// <summary>True once the disk read has happened.</summary>
        public bool IsInitialized
        {
            get
            {
                lock (_gate)
                {
                    return _diskLoadDone;
                }
            }
        }

        /// <summary>
        /// How many times <see cref="ResyncFromGame"/> has actually walked the game
        /// this session. Calls that were skipped - because the game was not readable,
        /// because a previous walk threw, or because saving is disabled - are not
        /// counted.
        /// </summary>
        public int ResyncCount { get; private set; }

        /// <summary>
        /// False when writing would destroy something we cannot replace: currently
        /// only when a file on disk was written by a newer build of the mod. Checked
        /// by <see cref="TrySave"/>, and worth checking before doing work whose only
        /// purpose is to be saved.
        /// </summary>
        public bool CanSave { get; private set; } = true;

        /// <summary>
        /// The unified state. Prefer <see cref="EnsureInitialized"/>; this property
        /// exists for callers that have already initialized and want to read without
        /// re-checking. It never returns null and never returns a different object.
        /// </summary>
        /// <exception cref="InvalidOperationException">
        /// <see cref="EnsureInitialized"/> has not run yet. Reading the state before
        /// the disk copy has been consulted would silently start from empty, which
        /// is the exact bug the explicit trigger exists to prevent.
        /// </exception>
        public UnifiedConversationState State
        {
            get
            {
                lock (_gate)
                {
                    if (!_diskLoadDone)
                    {
                        throw new InvalidOperationException(
                            "The unified state has not been initialized. Call EnsureInitialized() first.");
                    }

                    return _state;
                }
            }
        }

        /// <summary>
        /// Initializes the unified state if that has not happened yet, and returns
        /// it. Safe and cheap to call on every access: the disk is read at most
        /// once per session, and after that this is a lock and a couple of field
        /// reads.
        /// </summary>
        /// <returns>The session's one state object, never null.</returns>
        public UnifiedConversationState EnsureInitialized()
        {
            lock (_gate)
            {
                if (!_diskLoadDone)
                {
                    // Set before loading, not after: LoadFromDisk can reach code that
                    // calls back in here, and a second pass over the same files would
                    // double-log the recovery it found.
                    _diskLoadDone = true;
                    LoadFromDisk();
                }

                return _state;
            }
        }

        /// <summary>
        /// Records one status change from the running game: initializes the state if
        /// this is the first access, merges the status, and rewrites the file if that
        /// actually raised something.
        /// </summary>
        /// <param name="conversationId">The conversation's integer ID.</param>
        /// <param name="dialogueEntryId">The dialogue entry's integer ID.</param>
        /// <param name="statusName">
        /// The status string the game passed, one of "Untouched", "WasOffered" or
        /// "WasDisplayed". Anything else is warned about once and dropped.
        /// </param>
        /// <remarks>
        /// <para><b>Nothing is written unless something changed.</b> The game marks
        /// the same entry repeatedly - every time a line is offered again, and
        /// "Untouched" over entries that already have history - and the merge rule
        /// turns all of those into no-ops. Rewriting the whole file for a no-op would
        /// put the file's entire cost on the common case (see de-omm.11), so the
        /// write is driven by the merge's own changed flag.</para>
        ///
        /// <para>This is the write-through half of a write-only design: the unified
        /// state never flows back into the game, so a failure here loses tracking and
        /// nothing else.</para>
        /// </remarks>
        /// <returns>
        /// True if this call raised a status in the unified state. The write that
        /// follows is best effort and reports its own failures through the log, so a
        /// true return does not by itself mean the file was updated.
        /// </returns>
        public bool Record(int conversationId, int dialogueEntryId, string? statusName)
        {
            lock (_gate)
            {
                EnsureInitialized();

                if (!_state.TryMerge(conversationId, dialogueEntryId, statusName, out bool changed))
                {
                    WarnAboutUnrecognizedStatus(conversationId, dialogueEntryId, statusName);
                    return false;
                }

                if (!changed)
                {
                    return false;
                }

                TrySave();
                return true;
            }
        }

        /// <summary>
        /// Reports a status string the game passed that is not one of the three the
        /// mod knows, once per distinct string. Once per string rather than once per
        /// call: this runs on every line of dialogue, and a repeating warning would
        /// bury everything else in the log without adding anything.
        /// </summary>
        private void WarnAboutUnrecognizedStatus(int conversationId, int dialogueEntryId, string? statusName)
        {
            if (!_unrecognizedStatuses.Add(statusName ?? NullStatusName))
            {
                return;
            }

            _log.Warning(
                $"Ignoring an unrecognized SimStatus '{statusName ?? NullStatusName}' "
                + $"(conversation {conversationId} entry {dialogueEntryId}). "
                + "Further entries with this status will be dropped silently.");
        }

        /// <summary>
        /// Writes the current state, unless <see cref="CanSave"/> says the file on
        /// disk must not be touched.
        /// </summary>
        /// <remarks>
        /// IO failures are logged and swallowed rather than thrown: this runs inside
        /// a game hook, and taking the frame down because a save file was locked
        /// would be worse than losing one write. The next write attempt retries from
        /// scratch, and <see cref="UnifiedStateStore.Save"/> never leaves a partial
        /// file behind.
        /// </remarks>
        /// <returns>True if the state reached disk.</returns>
        public bool TrySave()
        {
            lock (_gate)
            {
                if (!CanSave)
                {
                    _log.Warning(
                        $"Not saving the unified state: saving is disabled for this session ({Origin}). "
                        + $"See the earlier log lines about '{_store.LivePath}'.");
                    return false;
                }

                try
                {
                    _store.Save(_state);
                    return true;
                }
                catch (Exception ex) when (ex is IOException || ex is UnauthorizedAccessException)
                {
                    _log.Error(
                        $"Failed to write the unified state to '{_store.LivePath}': {ex.Message}. "
                        + "The in-memory state is intact and the next change will try again.");
                    return false;
                }
            }
        }

        // -------------------------------------------------------------------
        // Step 1: the disk copy. Runs exactly once per session.
        // -------------------------------------------------------------------

        private void LoadFromDisk()
        {
            UnifiedStateRecovery recovery = _store.LoadWithBackupFallback();

            // A newer format version is intact history this build cannot read.
            // LoadWithBackupFallback deliberately does not consult the backup for it,
            // so there is nothing else to try: leave both files alone.
            if (recovery.Live.Outcome == UnifiedStateLoadOutcome.UnsupportedVersion)
            {
                RefuseNewerFormat(recovery.Live);
                return;
            }

            // A corrupt live file is data loss whatever happens next, so preserve the
            // bytes before anything can rotate them away, and say so out loud.
            if (recovery.Live.Outcome == UnifiedStateLoadOutcome.Corrupt)
            {
                _log.Error(
                    $"The unified state file '{recovery.Live.SourcePath}' could not be read: "
                    + $"{recovery.Live.ErrorMessage ?? "no detail"}");
                QuarantineCorruptFile(recovery.Live.SourcePath);
            }

            if (recovery.RecoveredFromBackup)
            {
                AdoptLoadedState(recovery.Backup!, UnifiedStateOrigin.BackupFile);
                _log.Warning(
                    $"Recovered the unified state from the backup generation '{recovery.Backup!.SourcePath}' "
                    + $"because the live file was {DescribeFailure(recovery.Live)}. "
                    + "Anything recorded since the last save has been lost.");
                return;
            }

            if (recovery.Live.IsLoaded)
            {
                AdoptLoadedState(recovery.Live, UnifiedStateOrigin.LiveFile);
                _log.Info(
                    $"Loaded the unified state from '{recovery.Live.SourcePath}': "
                    + $"{_state.ConversationCount} conversations, {_state.EntryCount} entries.");
                return;
            }

            // Nothing usable. Either a genuine first run, or both generations are gone.
            if (recovery.Backup != null
                && recovery.Backup.Outcome == UnifiedStateLoadOutcome.UnsupportedVersion)
            {
                // The live file is unusable and the backup was written by a newer
                // build. Saving would rotate the unusable live file over that backup
                // and destroy the only intact history left.
                RefuseNewerFormat(recovery.Backup);
                return;
            }

            if (recovery.Backup != null && recovery.Backup.Outcome == UnifiedStateLoadOutcome.Corrupt)
            {
                _log.Error(
                    $"The backup generation '{recovery.Backup.SourcePath}' could not be read either: "
                    + $"{recovery.Backup.ErrorMessage ?? "no detail"}");
                QuarantineCorruptFile(recovery.Backup.SourcePath);
            }

            // Nothing on disk is not a dead end: the next savegame load resyncs the
            // whole of the game's own SimStatus table back in (see ResyncFromGame),
            // and play is recorded as it happens. Say so, so an empty start does not
            // read as data loss when it is a first run.
            const string RecoveryHint =
                "Starting empty; whatever the game itself still holds will be read back in "
                + "the next time a savegame is loaded, and play is recorded as it happens.";

            if (recovery.Live.Outcome == UnifiedStateLoadOutcome.Missing
                && (recovery.Backup == null || recovery.Backup.Outcome == UnifiedStateLoadOutcome.Missing))
            {
                _log.Info($"No unified state file at '{recovery.Live.SourcePath}'. {RecoveryHint}");
            }
            else
            {
                _log.Error(
                    "No usable unified state on disk: "
                    + $"live file {DescribeFailure(recovery.Live)}, "
                    + $"backup {(recovery.Backup == null ? "not consulted" : DescribeFailure(recovery.Backup))}. "
                    + $"{RecoveryHint} Cross-save history for conversations this playthrough's "
                    + "savegame does not itself hold is gone.");
            }

            Origin = UnifiedStateOrigin.NoStateOnDisk;
        }

        private void RefuseNewerFormat(UnifiedStateLoadResult result)
        {
            Origin = UnifiedStateOrigin.RefusedNewerFormat;
            CanSave = false;
            _log.Error(
                $"The unified state file '{result.SourcePath}' was written by a newer version of this mod "
                + $"({result.ErrorMessage ?? "unsupported format version"}). It holds history this build "
                + "cannot read, so it is being left completely alone: nothing will be saved for the rest of "
                + "this session. Update the mod, or move that file aside if you want to start over.");
        }

        private void AdoptLoadedState(UnifiedStateLoadResult result, UnifiedStateOrigin origin)
        {
            // MergeAll rather than replacing the field: anything a hook already
            // merged in while this was running keeps its (higher) status, and the
            // caller's reference to the state object stays valid.
            _state.MergeAll(result.RequireState());
            Origin = origin;
            ReportSkippedRows(result);
        }

        private void ReportSkippedRows(UnifiedStateLoadResult result)
        {
            if (result.SkippedRowCount == 0)
            {
                return;
            }

            _log.Warning(
                $"{result.SkippedRowCount} unreadable rows were skipped while loading "
                + $"'{result.SourcePath}'; those entries are not in the unified state.");
            foreach (string warning in result.Warnings)
            {
                _log.Warning($"  {warning}");
            }
        }

        private static string DescribeFailure(UnifiedStateLoadResult result)
        {
            return result.Outcome switch
            {
                UnifiedStateLoadOutcome.Missing => "was missing",
                UnifiedStateLoadOutcome.Corrupt => $"was unreadable ({result.ErrorMessage ?? "no detail"})",
                UnifiedStateLoadOutcome.UnsupportedVersion =>
                    $"is a newer format ({result.ErrorMessage ?? "no detail"})",
                _ => "loaded",
            };
        }

        /// <summary>
        /// Copies an unreadable file aside under a timestamped name so it can be
        /// looked at later. Best effort: failing to keep a copy is worth a warning,
        /// not worth stopping over.
        /// </summary>
        private void QuarantineCorruptFile(string path)
        {
            string stamp = DateTime.Now.ToString("yyyyMMdd-HHmmss", CultureInfo.InvariantCulture);
            string quarantinePath = $"{path}.corrupt-{stamp}";
            try
            {
                if (!File.Exists(path))
                {
                    // Corrupt without a file present means it was unreadable rather than
                    // unparseable (locked, permissions). There is nothing to copy.
                    return;
                }

                File.Copy(path, quarantinePath, overwrite: true);
                _log.Warning($"Kept a copy of the unreadable file at '{quarantinePath}'.");
            }
            catch (Exception ex) when (ex is IOException || ex is UnauthorizedAccessException)
            {
                _log.Warning(
                    $"Could not keep a copy of the unreadable file at '{quarantinePath}': {ex.Message}");
            }
        }

        // -------------------------------------------------------------------
        // Step 2: resyncing. Runs once per savegame load, for the whole session.
        // -------------------------------------------------------------------

        /// <summary>
        /// Re-reads every SimStatus the game holds and merges it in, for the case the
        /// write-through hook cannot see: <c>PersistentDataManager</c> rebuilding the
        /// Lua SimStatus table wholesale when a savegame is loaded, without ever
        /// calling <c>MarkDialogueEntry</c> (de-0s5).
        /// </summary>
        /// <remarks>
        /// <para><b>Call it after the load has finished rewriting the table</b>, not
        /// before: the plugin drives it from a postfix on
        /// <c>PersistentDataManager.ExpandCompressedSimStatusData</c>, which is the
        /// method that does the rewriting. Called too early it would read the
        /// pre-load table, which is harmless but pointless.</para>
        ///
        /// <para><b>It cannot lose anything.</b> The state is merged into, never
        /// replaced, and the merge rule only ever raises a status, so a resync can
        /// only add. That is what makes it safe to run unconditionally on every
        /// load, and however many times one load happens to trigger it (de-cvq).</para>
        ///
        /// <para><b>This is also the only bulk read of the game there is.</b> It
        /// replaced the first-mark seed outright (de-omm.23), so the case the seed
        /// existed for - a savegame whose history the unified state has never seen,
        /// because the file was lost or the save predates the mod - is now covered
        /// here, on every load rather than once per playthrough-ever.</para>
        ///
        /// <para><b>Skips, because the walk is not free</b> (de-omm.23 measured
        /// 112,940 rows at 649 ms before the per-row lookup was hoisted). The walk
        /// itself is skipped when the game is not readable, when a previous walk
        /// threw, and when saving is disabled for the session - that last one because
        /// the walk's only purpose is to be saved. The <em>write</em> is skipped when
        /// nothing was raised, which de-omm.26 measured as the common case: replaying
        /// a save the mod already tracked walked 89 rows to gain 0 entries, and paying
        /// a whole-file write for that on every load would be pure cost.</para>
        /// </remarks>
        /// <returns>
        /// How many statuses this resync raised. Zero both when the game had nothing
        /// the unified state was missing and when the walk was skipped; the log line
        /// says which.
        /// </returns>
        public int ResyncFromGame()
        {
            lock (_gate)
            {
                if (_resyncGivenUp)
                {
                    return 0;
                }

                EnsureInitialized();

                if (!CanSave)
                {
                    _log.Warning(
                        $"Not resyncing the unified state from the running game ({_source.Description}): "
                        + $"saving is disabled for this session ({Origin}), so the walk could not be kept. "
                        + $"See the earlier log lines about '{_store.LivePath}'.");
                    return 0;
                }

                if (!_source.IsReady)
                {
                    _log.Warning(
                        $"Not resyncing the unified state: the game ({_source.Description}) is not "
                        + "readable yet. Statuses restored by this savegame load will only be recorded "
                        + "if they are marked again during play.");
                    return 0;
                }

                ResyncCount++;

                var stopwatch = Stopwatch.StartNew();
                int rowCount = 0;
                int raisedCount;

                try
                {
                    raisedCount = MergeEverythingFromGame(ref rowCount);
                }
                catch (Exception ex)
                {
                    // Not retried: the next load would call the same thing and fail
                    // the same way, once per load, for the rest of the session.
                    _resyncGivenUp = true;
                    _log.Error(
                        $"Failed to resync the unified state from the running game ({_source.Description}) "
                        + $"after {rowCount} rows: {ex}. No further resync will be attempted this session, "
                        + "so statuses restored by loading a savegame will only be recorded if they are "
                        + "marked again during play.");
                    return 0;
                }

                stopwatch.Stop();

                if (raisedCount == 0)
                {
                    _log.Info(
                        $"Resynced the unified state from the running game ({_source.Description}) after a "
                        + $"savegame load: nothing new in {rowCount} rows in {stopwatch.ElapsedMilliseconds} ms, "
                        + "so no file was written.");
                    return 0;
                }

                _log.Info(
                    $"Resynced the unified state from the running game ({_source.Description}) after a "
                    + $"savegame load: {raisedCount} statuses raised from {rowCount} rows in "
                    + $"{stopwatch.ElapsedMilliseconds} ms; now {_state.ConversationCount} conversations, "
                    + $"{_state.EntryCount} entries.");

                TrySave();
                return raisedCount;
            }
        }

        /// <summary>
        /// Walks every SimStatus the game currently holds and merges it into the
        /// state, reporting the rows whose status string was not recognized.
        /// </summary>
        /// <param name="rowCount">
        /// Counted up as the walk goes, so a caller that catches a throwing walk can
        /// still say how far it got.
        /// </param>
        /// <returns>How many statuses this walk actually raised.</returns>
        private int MergeEverythingFromGame(ref int rowCount)
        {
            int raisedCount = 0;
            int skippedCount = 0;
            var warnings = new List<string>();

            foreach (SimStatusRow row in _source.EnumerateSimStatuses())
            {
                rowCount++;
                if (_state.TryMerge(row.ConversationId, row.DialogueEntryId, row.StatusName, out bool changed))
                {
                    if (changed)
                    {
                        raisedCount++;
                    }

                    continue;
                }

                skippedCount++;
                if (warnings.Count < MaxGameWalkWarnings)
                {
                    warnings.Add(
                        $"conversation {row.ConversationId} entry {row.DialogueEntryId}: "
                        + $"unrecognized status '{row.StatusName ?? NullStatusName}'");
                }
            }

            if (skippedCount > 0)
            {
                _log.Warning(
                    $"{skippedCount} of {rowCount} SimStatus values read from the game were not "
                    + "recognized and were skipped.");
                foreach (string warning in warnings)
                {
                    _log.Warning($"  {warning}");
                }
            }

            return raisedCount;
        }

        /// <inheritdoc />
        public override string ToString() =>
            $"UnifiedStateSession({Origin}, initialized={IsInitialized}, canSave={CanSave}, "
            + $"resyncs={ResyncCount})";
    }
}
