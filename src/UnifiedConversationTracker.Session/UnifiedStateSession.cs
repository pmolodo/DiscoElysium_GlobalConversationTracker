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
    /// once, seeds it from the running game when there is nothing to read, and is
    /// the only thing that decides whether saving is allowed.
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
    /// raises a status, so a later load or seed cannot undo it.</para>
    ///
    /// <para><b>When to call it.</b> It is called from the write-through hook on
    /// <c>DialogueLua.MarkDialogueEntry</c> - by way of <see cref="Record"/>, which
    /// is what that hook actually calls - and not from plugin <c>Load()</c>. The
    /// difference matters because of the seeding path:
    /// <c>PersistentDataManager</c> rewrites the whole Lua SimStatus table when a
    /// savegame is loaded, without going through <c>MarkDialogueEntry</c> (see
    /// de-0s5). Seeding at plugin load, or at any point before a save has been
    /// loaded, would copy an all-Untouched table and record nothing. A mark can
    /// only happen inside a running conversation, which can only happen after a
    /// game is in play, so the first mark is the earliest moment at which the
    /// game's SimStatus values are real. <see cref="ISimStatusSource.IsReady"/> is
    /// the belt to that braces: while it is false the seed is deferred and retried,
    /// never written off.</para>
    ///
    /// <para><b>An empty seed is never persisted.</b> A brand new game legitimately
    /// has nothing above Untouched. Writing a file for that would be worse than
    /// writing nothing, because the file's existence is exactly what stops a later
    /// session from seeding: the pre-existing history of a save made before the mod
    /// was installed would then be unreachable forever.</para>
    ///
    /// <para>Not thread safe in the sense of being lock-free, but every public
    /// method takes the same lock and C# locks are reentrant, so a hook that
    /// re-enters during initialization gets the current state back instead of
    /// deadlocking or recursing.</para>
    /// </remarks>
    public sealed class UnifiedStateSession
    {
        /// <summary>
        /// How many skipped-row descriptions a seed logs before it stops listing
        /// them. Deliberately the file loader's own cap, so a skipped game row and a
        /// skipped file row read the same way in a log.
        /// </summary>
        public const int MaxSeedWarnings = UnifiedStateJson.MaxWarnings;

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
        private bool _seedPending;
        private bool _deferralLogged;

        /// <summary>Creates a session. Nothing is read, written or logged yet.</summary>
        /// <param name="store">The store over the SaveGames directory.</param>
        /// <param name="simStatusSource">The running game, for the seeding path.</param>
        /// <param name="log">Where recovery and seeding are reported.</param>
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

        /// <summary>True once the disk read has happened. Seeding may still be pending.</summary>
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
        /// True while a seed is still owed because the game was not ready. Purely
        /// informational; <see cref="EnsureInitialized"/> retries on its own.
        /// </summary>
        public bool IsSeedPending
        {
            get
            {
                lock (_gate)
                {
                    return _seedPending;
                }
            }
        }

        /// <summary>How many times the seeding path has been entered this session.</summary>
        public int SeedAttemptCount { get; private set; }

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
        /// once per session, and after a successful seed this is a lock and a
        /// couple of field reads.
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

                if (_seedPending)
                {
                    TrySeedFromGame();
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

            if (recovery.Live.Outcome == UnifiedStateLoadOutcome.Missing
                && (recovery.Backup == null || recovery.Backup.Outcome == UnifiedStateLoadOutcome.Missing))
            {
                _log.Info(
                    $"No unified state file at '{recovery.Live.SourcePath}'. "
                    + $"Seeding from the running game ({_source.Description}).");
            }
            else
            {
                _log.Error(
                    "No usable unified state on disk: "
                    + $"live file {DescribeFailure(recovery.Live)}, "
                    + $"backup {(recovery.Backup == null ? "not consulted" : DescribeFailure(recovery.Backup))}. "
                    + $"Seeding from the running game ({_source.Description}); "
                    + "cross-save history recorded before this point is gone.");
            }

            Origin = UnifiedStateOrigin.AwaitingGame;
            _seedPending = true;
        }

        private void RefuseNewerFormat(UnifiedStateLoadResult result)
        {
            Origin = UnifiedStateOrigin.RefusedNewerFormat;
            CanSave = false;
            _seedPending = false;
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
            _seedPending = false;
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
        // Step 2: seeding. Retried until the game is ready, then done for good.
        // -------------------------------------------------------------------

        private void TrySeedFromGame()
        {
            if (!_source.IsReady)
            {
                if (!_deferralLogged)
                {
                    _deferralLogged = true;
                    _log.Info(
                        $"The game ({_source.Description}) is not ready to be read yet; "
                        + "seeding the unified state is deferred until it is.");
                }

                return;
            }

            SeedAttemptCount++;
            _seedPending = false;

            var stopwatch = Stopwatch.StartNew();
            int rowCount = 0;
            int skippedCount = 0;
            var warnings = new List<string>();

            try
            {
                foreach (SimStatusRow row in _source.EnumerateSimStatuses())
                {
                    rowCount++;
                    if (_state.TryMerge(row.ConversationId, row.DialogueEntryId, row.StatusName, out _))
                    {
                        continue;
                    }

                    skippedCount++;
                    if (warnings.Count < MaxSeedWarnings)
                    {
                        warnings.Add(
                            $"conversation {row.ConversationId} entry {row.DialogueEntryId}: "
                            + $"unrecognized status '{row.StatusName ?? NullStatusName}'");
                    }
                }
            }
            catch (Exception ex)
            {
                // A throwing game walk is not retried: the same call would throw again
                // on every dialogue line for the rest of the session.
                Origin = UnifiedStateOrigin.SeedFailed;
                _log.Error(
                    $"Failed to seed the unified state from the running game ({_source.Description}) "
                    + $"after {rowCount} rows: {ex}. The unified state will only contain what is "
                    + "recorded from here on.");
                return;
            }

            stopwatch.Stop();
            Origin = UnifiedStateOrigin.SeededFromGame;

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

            if (_state.IsEmpty)
            {
                // Deliberately no save. An empty file on disk is indistinguishable from
                // a real one on the next run, and its presence is what would stop that
                // run from seeding a save made before the mod was installed.
                _log.Info(
                    $"Seeded the unified state from the running game ({_source.Description}): "
                    + $"nothing above Untouched in {rowCount} entries, so no file was written.");
                return;
            }

            _log.Info(
                $"Seeded the unified state from the running game ({_source.Description}): "
                + $"{_state.ConversationCount} conversations, {_state.EntryCount} entries "
                + $"from {rowCount} rows in {stopwatch.ElapsedMilliseconds} ms.");

            if (TrySave())
            {
                _log.Info($"Wrote the seeded unified state to '{_store.LivePath}'.");
            }
        }

        /// <inheritdoc />
        public override string ToString() =>
            $"UnifiedStateSession({Origin}, initialized={IsInitialized}, canSave={CanSave}, "
            + $"seedPending={IsSeedPending})";
    }
}
