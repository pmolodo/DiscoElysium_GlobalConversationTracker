// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Threading;
using Il2CppInterop.Runtime.InteropTypes.Arrays;
using GlobalConversationTracker.Core;
using GlobalConversationTracker.Persistence;
using GlobalConversationTracker.Persistence.Interop;

namespace GlobalConversationTracker.Session
{
    /// <summary>
    /// Owns the one global state a game session has: reads it from disk exactly
    /// once, resyncs it from the running game whenever a savegame load rewrites the
    /// game's own tables, and is the only thing that decides whether saving is
    /// allowed.
    /// </summary>
    /// <remarks>
    /// <para>The trigger is explicit and idempotent. Nothing happens in the constructor,
    /// so building a session during BepInEx chainload touches neither the disk nor the
    /// game. Every access goes through <see cref="EnsureInitialized"/>, which reads the
    /// disk once per session and returns the same state object thereafter. That object
    /// is valid from construction (empty, never null), so a hook firing while
    /// initialization is in flight sees an empty state rather than a half-built one, and
    /// anything it merges survives - the merge rule only raises a status.</para>
    ///
    /// <para>Two ways in, covering different writers. <see cref="Record"/> is
    /// write-through, driven by the hook on <c>DialogueLua.MarkDialogueEntry</c>:
    /// everything the game marks while being played.
    /// <see cref="ResyncFromSaveRawBytes"/> covers the writer that never goes through it
    /// - a savegame load rebuilding the whole SimStatus table at once. Between them they
    /// see every SimStatus the game holds.</para>
    ///
    /// <para>Nothing writes the file on the caller's thread. Both paths merge and mark
    /// the state dirty; a single background thread does the writing. A save costs
    /// 6.7-7.4 ms for a realistic Day-1 save and ~40 ms at the ceiling, of which ~85% is
    /// the flush and the two renames - a dropped frame or more, inside a dialogue hook.
    /// <see cref="TrySave"/> is the synchronous path that remains;
    /// <see cref="Shutdown"/> is the flush that stops a clean exit losing the
    /// tail.</para>
    ///
    /// <para>Not lock-free, but every public method takes the same lock, and C# locks
    /// are reentrant, so a hook that re-enters during initialization gets the current
    /// state back instead of deadlocking. That one lock is also the writer thread's
    /// condition variable, so there is no lock ordering to get wrong. The writer holds
    /// it only long enough to copy the state, and serializes and writes the copy outside
    /// it.</para>
    /// </remarks>
    public sealed class GlobalStateSession : IDisposable
    {
        /// <summary>
        /// How many skipped-row descriptions a resync logs before it stops listing
        /// them. Deliberately the file loader's own cap, so a skipped save row and a
        /// skipped file row read the same way in a log.
        /// </summary>
        public const int MaxGameWalkWarnings = GlobalStateJson.MaxWarnings;

        /// <summary>What a null status string is called in the log and in the set of
        /// already-warned-about statuses, since null cannot go in the set itself.</summary>
        private const string NullStatusName = "null";

        /// <summary>
        /// How long a caller waiting for a write to land - <see cref="TrySave"/>,
        /// <see cref="Flush"/>, the flush inside <see cref="Dispose"/> - waits before
        /// giving up and saying so. Generous by two orders of magnitude against the 40 ms
        /// a save costs at the largest constructible file: a shorter one only buys giving
        /// up on a write that was about to succeed. It exists so a writer wedged on a
        /// locked file cannot hang the game's shutdown for ever.
        /// </summary>
        private const int WriteWaitTimeoutMilliseconds = 30_000;

        /// <summary>
        /// How long <see cref="Dispose"/> waits for the writer thread to notice it
        /// should exit, after its work has already been waited for. Short, because by
        /// then there is nothing left to lose by not waiting: the thread is a
        /// background thread, so leaving it behind cannot hold the process open.
        /// </summary>
        private const int WriterExitTimeoutMilliseconds = 1_000;

        /// <summary>Name of the background writer thread, as a debugger shows it.</summary>
        private const string WriterThreadName = "GlobalConversationTracker writer";

        /// <summary>
        /// What <see cref="Dispose"/> calls the trigger when nobody named one. The
        /// plugin always names one; this covers tests, tools and <c>using</c> blocks.
        /// </summary>
        private const string DefaultShutdownTrigger = "Dispose";

        /// <summary>
        /// The prefix every shutdown line shares, so one search finds all of them
        /// whatever the outcome was.
        /// </summary>
        private const string ShutdownLinePrefix = "Global state shutdown flush";

        private readonly object _gate = new object();
        private readonly GlobalStateStore _store;
        private readonly IGlobalStateLog _log;

        private readonly GlobalConversationState _state = new GlobalConversationState();

        /// <summary>
        /// The count for the save being played, as opposed to the across-all-saves
        /// state beside it. Filled from the save's own rows at every resync and kept
        /// live off <see cref="Record"/> in between, because a count that is only
        /// right as of the last savegame load is not what "this save" means while
        /// somebody is playing it.
        /// </summary>
        private readonly CurrentSaveTally _currentSave = new CurrentSaveTally();

        /// <summary>Status strings from the game that have already been warned about.</summary>
        private readonly HashSet<string> _unrecognizedStatuses = new HashSet<string>(StringComparer.Ordinal);

        private bool _diskLoadDone;
        private bool _resyncGivenUp;
        private bool _orbResyncGivenUp;

        /// <summary>
        /// When the last resync refilled the current save's tally, or null if none
        /// has. Only ever read for <see cref="ResetCurrentSave"/>'s log line, which
        /// uses it to show whether a reset landed suspiciously close to a load.
        /// </summary>
        private DateTime? _lastResyncUtc;

        // ---- The deferred write. All guarded by _gate. ----

        /// <summary>
        /// Bumped every time the state changes into something the file does not yet
        /// reflect. A counter rather than a bool so a caller can say "wait until
        /// everything I had recorded by now is on disk" without caring what has
        /// happened since.
        /// </summary>
        private long _dirtyVersion;

        /// <summary>
        /// The highest <see cref="_dirtyVersion"/> a write has been <em>attempted</em>
        /// for, successfully or not. Attempted rather than achieved, so a failing
        /// write cannot leave a flush waiting for ever; <see cref="_lastWriteSucceeded"/>
        /// carries the outcome.
        /// </summary>
        private long _writtenVersion;

        /// <summary>Whether the most recent write attempt reached disk.</summary>
        private bool _lastWriteSucceeded = true;

        private Thread? _writer;

        /// <summary>Set by <see cref="Dispose"/> to let the writer thread finish and exit.</summary>
        private bool _writerStopping;

        /// <summary>
        /// Set when the writer thread died of something other than a write failure,
        /// which it is not built to do. Nothing waits on a dead writer, and no
        /// replacement is started: whatever broke would break the next one too.
        /// </summary>
        private bool _writerFaulted;

        /// <summary>
        /// What triggered the shutdown that stopped the writer. Kept so a second
        /// trigger arriving afterwards can say which one got there first, rather than
        /// returning silently and leaving the log looking as though it never fired.
        /// </summary>
        private string? _shutdownTrigger;

        /// <summary>
        /// How many writes have reached disk this session. Only ever read for the
        /// shutdown line, where it separates "the writer ran all session and had
        /// nothing left to do" from "the writer never wrote anything at all".
        /// </summary>
        private long _writesLanded;

        /// <summary>
        /// How many statuses <see cref="Record"/> has raised this session. Reported at
        /// shutdown, where a zero separates "the marking hook fired and the game marked
        /// nothing new" from "the marking hook never fired".
        /// </summary>
        private long _statusesRecorded;

        /// <summary>
        /// How many orbs <see cref="RecordOrb"/> has added this session. Reported at
        /// shutdown beside <see cref="_statusesRecorded"/>, for the same reason.
        /// </summary>
        private long _orbsRecorded;

        /// <summary>Creates a session.</summary>
        /// <param name="store">The store over the SaveGames directory.</param>
        /// <param name="log">Where recovery and resyncing are reported.</param>
        /// <exception cref="ArgumentNullException">Any argument is null.</exception>
        public GlobalStateSession(
            GlobalStateStore store,
            IGlobalStateLog log)
        {
            _store = store ?? throw new ArgumentNullException(nameof(store));
            _log = log ?? throw new ArgumentNullException(nameof(log));
        }

        /// <summary>The store this session reads and writes.</summary>
        public GlobalStateStore Store => _store;

        /// <summary>Where the in-memory state came from.</summary>
        public GlobalStateOrigin Origin { get; private set; } = GlobalStateOrigin.Uninitialized;

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
        /// How many dialogue entries are above Untouched in the save currently being
        /// played.
        /// </summary>
        /// <remarks>
        /// <para>Zero is a real answer: a new game has reached nothing yet. What it is
        /// never allowed to be is the previous save's figure, which is why both the
        /// resync and <see cref="ResetCurrentSave"/> empty the tally before anything
        /// refills it.</para>
        ///
        /// <para>Unlike <see cref="GlobalConversationState.EntryCount"/> this can go
        /// down: the game marks entries Untouched, which in this save is a real loss
        /// rather than history to preserve.</para>
        /// </remarks>
        public int CurrentSaveEntryCount
        {
            get
            {
                lock (_gate)
                {
                    return _currentSave.Count;
                }
            }
        }

        /// <summary>
        /// What the save currently being played is worth: an entry that was only ever
        /// offered counts half, one that was displayed counts whole. This is what the
        /// display shows, rather than <see cref="CurrentSaveEntryCount"/>, which
        /// flattens the two together.
        /// </summary>
        /// <remarks>
        /// Everything said about <see cref="CurrentSaveEntryCount"/> applies here too:
        /// zero is a real answer, and this can go down.
        /// </remarks>
        public double CurrentSaveScore
        {
            get
            {
                lock (_gate)
                {
                    return _currentSave.Score;
                }
            }
        }

        /// <summary>How many distinct orbs have been opened in the save being played.</summary>
        public int CurrentSaveOrbCount
        {
            get
            {
                lock (_gate)
                {
                    return _currentSave.OrbCount;
                }
            }
        }

        /// <summary>
        /// How many resyncs actually read the save this session. Calls that were
        /// skipped - because the save bytes were not readable, because a previous
        /// resync threw, or because saving is disabled - are not counted.
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
        /// The global state. Prefer <see cref="EnsureInitialized"/>; this property
        /// exists for callers that have already initialized and want to read without
        /// re-checking. It never returns null and never returns a different object.
        /// </summary>
        /// <remarks>
        /// The lock covers getting the object, not using it. There is a second thread
        /// that reads this object - the writer, copying it - so reading it here while
        /// marks are still arriving is a concurrent read and write of a plain
        /// <c>Dictionary</c>. Nothing in the mod does that; it is for tests and tools,
        /// which read it once the marking has stopped.
        /// </remarks>
        /// <exception cref="InvalidOperationException">
        /// <see cref="EnsureInitialized"/> has not run yet. Reading the state before
        /// the disk copy has been consulted would silently start from empty, which
        /// is the exact bug the explicit trigger exists to prevent.
        /// </exception>
        public GlobalConversationState State
        {
            get
            {
                lock (_gate)
                {
                    if (!_diskLoadDone)
                    {
                        throw new InvalidOperationException(
                            "The global state has not been initialized. Call EnsureInitialized() first.");
                    }

                    return _state;
                }
            }
        }

        /// <summary>
        /// Initializes the global state if that has not happened yet, and returns
        /// it. Safe and cheap to call on every access: the disk is read at most
        /// once per session, and after that this is a lock and a couple of field
        /// reads.
        /// </summary>
        /// <returns>The session's one state object, never null.</returns>
        public GlobalConversationState EnsureInitialized()
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
        /// this is the first access, merges the status, and marks the file out of date
        /// if that actually raised something.
        /// </summary>
        /// <param name="conversationId">The conversation's integer ID.</param>
        /// <param name="dialogueEntryId">The dialogue entry's integer ID.</param>
        /// <param name="statusName">
        /// The status string the game passed, one of "Untouched", "WasOffered" or
        /// "WasDisplayed". Anything else is warned about once and dropped.
        /// </param>
        /// <remarks>
        /// <para>Nothing is written unless something changed. The game marks the same
        /// entry repeatedly - every time a line is offered again, and "Untouched" over
        /// entries that already have history - and the merge rule turns all of those into
        /// no-ops, so the write is driven by the merge's own changed flag.</para>
        ///
        /// <para>This never writes the file: it merges, marks the state dirty and
        /// returns. It runs on the Unity main thread mid-conversation, where the 6.7-40
        /// ms a save costs is a dropped frame. Deferring is safe because the global state
        /// is write-only - nothing reads back what has not landed, and a mark lost to a
        /// hard crash is re-marked the next time the line is reached.</para>
        /// </remarks>
        /// <returns>
        /// True if this call raised a status in the global state. The write that
        /// follows is deferred and best effort and reports its own failures through
        /// the log, so a true return does not mean the file has been updated - only
        /// that it will be. Use <see cref="Flush"/> to wait for that.
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

                // Before the early return below, and not gated on it: a mark that the
                // global state ignores can still move this save's count. Marking an
                // entry Untouched is the case that matters - history keeps it, this
                // save loses it - and re-marking an entry the global state already
                // has at a higher status is the common one.
                _currentSave.TrySet(conversationId, dialogueEntryId, statusName, out _);

                if (!changed)
                {
                    return false;
                }

                _statusesRecorded++;
                MarkDirty();
                return true;
            }
        }

        /// <summary>
        /// Records that an orb has been opened, in both the global state and the current
        /// save's tally.
        /// </summary>
        /// <remarks>
        /// <para>The orb counterpart of <see cref="Record"/>, driven by the hook on
        /// <c>SenseOrb.SetShown</c>.</para>
        ///
        /// <para>No merge rule to apply: an orb has one state and the game never unsets
        /// it, so recording is set insertion in both directions. The current save cannot
        /// lose an orb the way it loses an entry marked Untouched, so unlike
        /// <see cref="Record"/> nothing has to happen before the early return.</para>
        ///
        /// <para>Called far more often than it changes anything - the game calls
        /// <c>SetShown</c> on every click, not only the first - and the sets absorb the
        /// repeats.</para>
        /// </remarks>
        /// <param name="conversationTitle">
        /// The orb's conversation title, exactly as the game keys <c>ShownOrbs</c>.
        /// </param>
        /// <returns>
        /// True if this call added an orb the global state had never seen. As with
        /// <see cref="Record"/>, the write that follows is deferred, so a true return
        /// means the file will be updated rather than that it has been.
        /// </returns>
        /// <exception cref="ArgumentException">
        /// <paramref name="conversationTitle"/> is null or empty. Callers must filter
        /// those out rather than pass them on: an orb with no conversation - a thought
        /// orb, instantiated from the template - is one the game itself declines to
        /// record, not an error worth reporting.
        /// </exception>
        public bool RecordOrb(string conversationTitle)
        {
            if (string.IsNullOrEmpty(conversationTitle))
            {
                throw new ArgumentException(
                    "An orb's conversation title must not be null or empty.",
                    nameof(conversationTitle));
            }

            lock (_gate)
            {
                EnsureInitialized();

                _currentSave.SetOrb(conversationTitle);

                if (!_state.MergeOrb(conversationTitle))
                {
                    return false;
                }

                _orbsRecorded++;
                MarkDirty();
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
        /// Writes the current state and waits for the write to finish, unless
        /// <see cref="CanSave"/> says the file on disk must not be touched.
        /// </summary>
        /// <remarks>
        /// <para>Synchronous, and deliberately the exception - everything the game drives
        /// goes through the dirty flag instead. This is for callers that need the bytes
        /// on disk before carrying on: shutdown, and tests. It still writes on the
        /// background thread, the only one allowed to touch the three files, and waits
        /// for it. Nothing is skipped when the state is clean: an explicit "save now"
        /// means save now.</para>
        ///
        /// <para>IO failures are logged and swallowed rather than thrown: a locked save
        /// file must cost tracking and nothing else. The next attempt retries from
        /// scratch, and <see cref="GlobalStateStore.Save"/> never leaves a partial file
        /// behind.</para>
        /// </remarks>
        /// <returns>True if the state reached disk.</returns>
        public bool TrySave()
        {
            lock (_gate)
            {
                if (!RequestWrite())
                {
                    return false;
                }

                return WaitForWrite(_dirtyVersion);
            }
        }

        /// <summary>
        /// Waits for everything recorded so far to reach disk, without asking for a
        /// write that is not already needed.
        /// </summary>
        /// <remarks>
        /// The counterpart to the deferred write: <see cref="Record"/> and
        /// <see cref="ResyncFromSaveRawBytes"/> return before the file has been rewritten, and
        /// this is how a caller that has to see the result waits for it. Returns
        /// immediately when the file is already up to date.
        /// </remarks>
        /// <returns>
        /// True if everything recorded before this call is on disk - including the
        /// vacuous case where nothing needed writing. False if the write failed, timed
        /// out, or saving is disabled for this session.
        /// </returns>
        public bool Flush()
        {
            lock (_gate)
            {
                return CanSave && WaitForWrite(_dirtyVersion);
            }
        }

        /// <summary>
        /// Flushes anything still pending and shuts the background writer down.
        /// </summary>
        /// <remarks>
        /// <para>The "never lose the tail on a clean exit" half of the deferred write,
        /// and the reason that write needs no durability machinery of its own.
        /// Idempotent; afterwards the session still merges but no longer writes, which is
        /// right for a mark arriving while the game tears down.</para>
        /// <para>BepInEx does not call plugin <c>Unload</c> on game exit, so the plugin
        /// drives this from process- and application-level shutdown events. Use
        /// <see cref="Shutdown"/> to say which one; this overload is for <c>using</c>
        /// blocks, tests and tools, which have only one.</para>
        /// </remarks>
        public void Dispose() => Shutdown(DefaultShutdownTrigger);

        /// <summary>
        /// Flushes anything still pending, shuts the background writer down, and says
        /// so in the log under the name of whatever triggered it.
        /// </summary>
        /// <param name="trigger">
        /// What is shutting the session down, as it should read in the log -
        /// "Application.quitting", "AppDomain.ProcessExit", "BasePlugin.Unload".
        /// Null or empty is reported as "Dispose".
        /// </param>
        /// <remarks>
        /// <para>Same work as <see cref="Dispose"/>; the difference is the log. Two
        /// lines: one when the trigger arrives, before the gate is taken, so a shutdown
        /// that then blocks behind a resync still proves it fired; one when the drain is
        /// over, carrying the trigger, what was pending, whether it landed, how long it
        /// took, what the session recorded and wrote, and whether the writer
        /// stopped.</para>
        ///
        /// <para>The closing line is logged outside the gate. The gate is the writer's
        /// condition variable and the writer logs from its own thread, so a log sink
        /// with a lock of its own is reachable from two threads. Nothing in the writer
        /// holds a log's lock while it wants the gate - <see cref="TryWritePayload"/>
        /// logs after releasing it, <see cref="WriterLoop"/>'s catch logs after its lock
        /// block - so there is no cycle either way.</para>
        ///
        /// <para>Nothing here can hang: the drain is bounded by
        /// <see cref="WriteWaitTimeoutMilliseconds"/> and the join by
        /// <see cref="WriterExitTimeoutMilliseconds"/>.</para>
        /// </remarks>
        public void Shutdown(string? trigger)
        {
            string triggerName = string.IsNullOrEmpty(trigger) ? DefaultShutdownTrigger : trigger;
            var elapsed = Stopwatch.StartNew();

            // Before the gate, and touching nothing but the argument: this is the line
            // that proves the trigger fired even if everything after it wedges.
            _log.Info($"{ShutdownLinePrefix} triggered by {triggerName}.");

            Thread? writer = null;
            string? alreadyShutDownBy = null;
            GlobalStateOrigin origin = GlobalStateOrigin.Uninitialized;
            bool canSave = false;
            bool writerExisted = false;
            bool flushed = false;
            long pending = 0;
            long writesLanded = 0;
            long recorded = 0;
            long orbsRecorded = 0;

            lock (_gate)
            {
                if (_writerStopping)
                {
                    alreadyShutDownBy = _shutdownTrigger ?? DefaultShutdownTrigger;
                }
                else
                {
                    _shutdownTrigger = triggerName;
                    origin = Origin;
                    canSave = CanSave;
                    writerExisted = _writer != null;
                    pending = _dirtyVersion - _writtenVersion;

                    // Flush before stopping, not after: the writer thread is what does
                    // the writing, so it has to still be running for the tail to land.
                    flushed = WaitForWrite(_dirtyVersion);
                    writesLanded = _writesLanded;
                    recorded = _statusesRecorded;
                    orbsRecorded = _orbsRecorded;

                    _writerStopping = true;
                    writer = _writer;
                    Monitor.PulseAll(_gate);
                }
            }

            if (alreadyShutDownBy != null)
            {
                _log.Info(
                    $"{ShutdownLinePrefix} already ran on {alreadyShutDownBy}, so {triggerName} had "
                    + "nothing left to do. Firing more than once is expected and harmless.");
                return;
            }

            bool writerStopped = writer == null || writer.Join(WriterExitTimeoutMilliseconds);
            elapsed.Stop();

            string outcome =
                $"{ShutdownLinePrefix} finished on {triggerName} in {elapsed.ElapsedMilliseconds} ms: "
                + $"{DescribeShutdownFlush(origin, canSave, writerExisted, pending, flushed)}. "
                + $"{recorded} status(es) and {orbsRecorded} orb(s) recorded during play and "
                + $"{writesLanded} write(s) reached disk this session; "
                + $"{DescribeWriterExit(writerExisted, writerStopped)}.";

            if (flushed && canSave && writerStopped)
            {
                _log.Info(outcome);
            }
            else
            {
                _log.Warning(outcome);
            }
        }

        /// <summary>
        /// Says what the shutdown flush did, including when the answer is "nothing".
        /// </summary>
        private string DescribeShutdownFlush(
            GlobalStateOrigin origin, bool canSave, bool writerExisted, long pending, bool flushed)
        {
            if (!canSave)
            {
                return $"saving was disabled for this session ({origin}), so nothing was written";
            }

            if (!writerExisted)
            {
                return "nothing was ever recorded this session, so there was nothing to write";
            }

            if (pending <= 0)
            {
                return flushed
                    ? "nothing was pending; everything recorded this session was already on disk"
                    : "nothing was pending, but the session's last write had already failed "
                        + "(see the error above)";
            }

            return flushed
                ? $"wrote the last {pending} pending change(s) to '{_store.LivePath}'"
                : $"the last {pending} pending change(s) did NOT reach '{_store.LivePath}' "
                    + "(see the error above)";
        }

        /// <summary>Says what became of the writer thread, for the shutdown line.</summary>
        private static string DescribeWriterExit(bool writerExisted, bool writerStopped)
        {
            if (!writerExisted)
            {
                return "no writer thread was ever started";
            }

            return writerStopped
                ? "the writer thread stopped"
                : $"the writer thread had not stopped {WriterExitTimeoutMilliseconds} ms later "
                    + "(it is a background thread, so it cannot hold the process open)";
        }

        // -------------------------------------------------------------------
        // The deferred write: a dirty flag, and one thread that acts on it.
        // -------------------------------------------------------------------

        /// <summary>
        /// Marks the state as something the file does not reflect yet, and wakes the
        /// writer. Caller must hold <see cref="_gate"/>.
        /// </summary>
        private void MarkDirty() => RequestWrite();

        /// <summary>
        /// Asks for a write of the current state. Caller must hold
        /// <see cref="_gate"/>.
        /// </summary>
        /// <returns>
        /// False if no write will happen, in which case the reason has been logged.
        /// </returns>
        private bool RequestWrite()
        {
            if (!CanSave)
            {
                _log.Warning(
                    $"Not saving the global state: saving is disabled for this session ({Origin}). "
                    + $"See the earlier log lines about '{_store.LivePath}'.");
                return false;
            }

            _dirtyVersion++;

            if (_writerStopping || _writerFaulted)
            {
                // Shut down, or dead of something already reported. The merge still
                // happened and the version is still bumped, so a later Flush reports
                // honestly that the file is behind; there is simply nothing left that
                // could catch it up.
                return false;
            }

            EnsureWriterStarted();
            Monitor.PulseAll(_gate);
            return true;
        }

        /// <summary>
        /// Starts the writer thread on first use. Caller must hold
        /// <see cref="_gate"/>.
        /// </summary>
        /// <remarks>
        /// Lazily, so a session that is built and never written to - the BepInEx
        /// chainload case, and most of the test suite - never has a thread at all. A
        /// background thread, so a writer that somehow wedges cannot keep the game's
        /// process alive after the window has closed; <see cref="Dispose"/> is what
        /// makes the tail land, not the thread outliving the process.
        /// </remarks>
        private void EnsureWriterStarted()
        {
            if (_writer != null)
            {
                return;
            }

            _writer = new Thread(WriterLoop)
            {
                IsBackground = true,
                Name = WriterThreadName,
            };
            _writer.Start();
        }

        /// <summary>
        /// The background writer. Sleeps on <see cref="_gate"/> until the state is
        /// dirty, copies the state under the lock, and serializes and writes the copy
        /// outside it.
        /// </summary>
        /// <remarks>
        /// <para>The lock covers the copy and nothing else. Reading the live state cannot
        /// happen concurrently with a merge, so the copy has to be under the lock;
        /// everything after it touches only the copy and the filesystem. Holding the lock
        /// for the whole serialize would make a mark arriving mid-serialize wait on the
        /// Unity main thread - up to 16 ms at 30,000 entries and 39 ms at the
        /// 112,940-entry ceiling. Copying first shrinks the locked region by 24-25x at
        /// every size the benchmark sweeps, at the price of one transient copy per
        /// write.</para>
        ///
        /// <para>Still one lock, and still one writer: this thread is the only caller of
        /// <see cref="GlobalStateStore.SavePayload"/>.</para>
        ///
        /// <para>Bursts coalesce for free. A response menu marks every offered response,
        /// so several raises land in one frame; each bumps <see cref="_dirtyVersion"/>
        /// while this thread is busy with the previous snapshot, and one further pass
        /// covers all of them. Hence no debounce delay: it would buy the difference
        /// between two writes and one, at the price of a tunable and a wider window for a
        /// hard crash to lose something.</para>
        ///
        /// <para>It never touches the game - only the global state and the filesystem,
        /// never the Dialogue System, <c>DialogueLua</c>, the Lua environment or any
        /// IL2CPP object. Reading the game off the Unity thread is not established as
        /// safe, and this design does not need it.</para>
        /// </remarks>
        private void WriterLoop()
        {
            try
            {
                while (true)
                {
                    long version;
                    GlobalConversationState snapshot;

                    lock (_gate)
                    {
                        while (!_writerStopping && _dirtyVersion == _writtenVersion)
                        {
                            Monitor.Wait(_gate);
                        }

                        if (_writerStopping)
                        {
                            // Dispose drains before it sets this, under this same lock,
                            // so nothing that was pending at that moment is lost here.
                            // Anything recorded after it is deliberately not written:
                            // the game is going away, and a mark that arrives during
                            // teardown must not be able to restart the writing.
                            return;
                        }

                        version = _dirtyVersion;
                        snapshot = _state.Snapshot();
                    }

                    // Everything from here on works on the copy, so nothing below this
                    // line can make a mark on the main thread wait.
                    byte[] payload = GlobalStateJson.SerializeToUtf8Bytes(snapshot);
                    bool succeeded = TryWritePayload(payload);

                    lock (_gate)
                    {
                        _writtenVersion = version;
                        _lastWriteSucceeded = succeeded;
                        if (succeeded)
                        {
                            _writesLanded++;
                        }

                        Monitor.PulseAll(_gate);
                    }
                }
            }
            catch (Exception ex)
            {
                // Not reachable by design - the write's own failures are caught in
                // TryWritePayload - so getting here means something unforeseen. Say so,
                // and release anything waiting rather than leaving it on a dead thread.
                lock (_gate)
                {
                    _writerFaulted = true;
                    _writtenVersion = _dirtyVersion;
                    _lastWriteSucceeded = false;
                    Monitor.PulseAll(_gate);
                }

                _log.Error(
                    $"The global state writer stopped unexpectedly: {ex}. Nothing further will be "
                    + $"written to '{_store.LivePath}' this session; the in-memory state is intact "
                    + "and the game is unaffected.");
            }
        }

        /// <summary>
        /// Writes one serialized snapshot. Must be called without
        /// <see cref="_gate"/> held, and only ever from the writer thread - it is the
        /// single-writer guarantee <see cref="GlobalStateStore"/> relies on.
        /// </summary>
        private bool TryWritePayload(byte[] payload)
        {
            try
            {
                _store.SavePayload(payload);
                return true;
            }
            catch (Exception ex) when (ex is IOException || ex is UnauthorizedAccessException)
            {
                _log.Error(
                    $"Failed to write the global state to '{_store.LivePath}': {ex.Message}. "
                    + "The in-memory state is intact and the next change will try again.");
                return false;
            }
        }

        /// <summary>
        /// Blocks until a write has been attempted for <paramref name="target"/>.
        /// Caller must hold <see cref="_gate"/>.
        /// </summary>
        /// <remarks>
        /// <see cref="Monitor.Wait(object, int)"/> releases the lock fully, however
        /// many times the calling thread has entered it, and restores the recursion
        /// count on the way back - so this is safe to reach from inside another
        /// method that already holds the gate.
        /// </remarks>
        private bool WaitForWrite(long target)
        {
            if (_writtenVersion >= target)
            {
                return _lastWriteSucceeded;
            }

            if (_writer == null || _writerStopping || _writerFaulted)
            {
                return false;
            }

            var elapsed = Stopwatch.StartNew();
            while (_writtenVersion < target)
            {
                long remaining = WriteWaitTimeoutMilliseconds - elapsed.ElapsedMilliseconds;
                if (remaining <= 0 || !Monitor.Wait(_gate, (int)remaining))
                {
                    if (_writtenVersion >= target)
                    {
                        break;
                    }

                    _log.Error(
                        $"Timed out after {WriteWaitTimeoutMilliseconds} ms waiting for the global "
                        + $"state to be written to '{_store.LivePath}'. The in-memory state is intact.");
                    return false;
                }

                if (_writtenVersion < target && (_writerFaulted || _writerStopping))
                {
                    // Nothing is going to write it now. Never reached by a wait that
                    // started before Dispose - Dispose waits for a version at least as
                    // high as any outstanding one before it stops the writer - but a
                    // faulted writer can land here.
                    return false;
                }
            }

            return _lastWriteSucceeded;
        }

        // -------------------------------------------------------------------
        // Step 1: the disk copy. Runs exactly once per session.
        // -------------------------------------------------------------------

        private void LoadFromDisk()
        {
            GlobalStateRecovery recovery = _store.LoadWithBackupFallback();

            // A newer format version is intact history this build cannot read.
            // LoadWithBackupFallback deliberately does not consult the backup for it,
            // so there is nothing else to try: leave both files alone.
            if (recovery.Live.Outcome == GlobalStateLoadOutcome.UnsupportedVersion)
            {
                RefuseNewerFormat(recovery.Live);
                return;
            }

            // A corrupt live file is data loss whatever happens next, so preserve the
            // bytes before anything can rotate them away, and say so out loud.
            if (recovery.Live.Outcome == GlobalStateLoadOutcome.Corrupt)
            {
                _log.Error(
                    $"The global state file '{recovery.Live.SourcePath}' could not be read: "
                    + $"{recovery.Live.ErrorMessage ?? "no detail"}");
                QuarantineCorruptFile(recovery.Live.SourcePath);
            }

            if (recovery.RecoveredFromBackup)
            {
                AdoptLoadedState(recovery.Backup!, GlobalStateOrigin.BackupFile);
                _log.Warning(
                    $"Recovered the global state from the backup generation '{recovery.Backup!.SourcePath}' "
                    + $"because the live file was {DescribeFailure(recovery.Live)}. "
                    + "Anything recorded since the last save has been lost.");
                return;
            }

            if (recovery.Live.IsLoaded)
            {
                AdoptLoadedState(recovery.Live, GlobalStateOrigin.LiveFile);
                _log.Info(
                    $"Loaded the global state from '{recovery.Live.SourcePath}': "
                    + $"{_state.ConversationCount} conversations, {_state.EntryCount} entries.");
                return;
            }

            // Nothing usable. Either a genuine first run, or both generations are gone.
            if (recovery.Backup != null
                && recovery.Backup.Outcome == GlobalStateLoadOutcome.UnsupportedVersion)
            {
                // The live file is unusable and the backup was written by a newer
                // build. Saving would rotate the unusable live file over that backup
                // and destroy the only intact history left.
                RefuseNewerFormat(recovery.Backup);
                return;
            }

            if (recovery.Backup != null && recovery.Backup.Outcome == GlobalStateLoadOutcome.Corrupt)
            {
                _log.Error(
                    $"The backup generation '{recovery.Backup.SourcePath}' could not be read either: "
                    + $"{recovery.Backup.ErrorMessage ?? "no detail"}");
                QuarantineCorruptFile(recovery.Backup.SourcePath);
            }

            // Nothing on disk is not a dead end: the next savegame load reads the
            // save's whole SimStatus table back in (see ResyncFromSaveRawBytes),
            // and play is recorded as it happens. Say so, so an empty start does not
            // read as data loss when it is a first run.
            const string RecoveryHint =
                "Starting empty; whatever the game itself still holds will be read back in "
                + "the next time a savegame is loaded, and play is recorded as it happens.";

            if (recovery.Live.Outcome == GlobalStateLoadOutcome.Missing
                && (recovery.Backup == null || recovery.Backup.Outcome == GlobalStateLoadOutcome.Missing))
            {
                _log.Info($"No global state file at '{recovery.Live.SourcePath}'. {RecoveryHint}");
            }
            else
            {
                _log.Error(
                    "No usable global state on disk: "
                    + $"live file {DescribeFailure(recovery.Live)}, "
                    + $"backup {(recovery.Backup == null ? "not consulted" : DescribeFailure(recovery.Backup))}. "
                    + $"{RecoveryHint} Cross-save history for conversations this playthrough's "
                    + "savegame does not itself hold is gone.");
            }

            Origin = GlobalStateOrigin.NoStateOnDisk;
        }

        private void RefuseNewerFormat(GlobalStateLoadResult result)
        {
            Origin = GlobalStateOrigin.RefusedNewerFormat;
            CanSave = false;
            _log.Error(
                $"The global state file '{result.SourcePath}' was written by a newer version of this mod "
                + $"({result.ErrorMessage ?? "unsupported format version"}). It holds history this build "
                + "cannot read, so it is being left completely alone: nothing will be saved for the rest of "
                + "this session. Update the mod, or move that file aside if you want to start over.");
        }

        private void AdoptLoadedState(GlobalStateLoadResult result, GlobalStateOrigin origin)
        {
            // MergeAll rather than replacing the field: anything a hook already
            // merged in while this was running keeps its (higher) status, and the
            // caller's reference to the state object stays valid.
            _state.MergeAll(result.RequireState());
            Origin = origin;
            ReportSkippedRows(result);
        }

        private void ReportSkippedRows(GlobalStateLoadResult result)
        {
            if (result.SkippedRowCount == 0)
            {
                return;
            }

            _log.Warning(
                $"{result.SkippedRowCount} unreadable rows were skipped while loading "
                + $"'{result.SourcePath}'; those entries are not in the global state.");
            foreach (string warning in result.Warnings)
            {
                _log.Warning($"  {warning}");
            }
        }

        private static string DescribeFailure(GlobalStateLoadResult result)
        {
            return result.Outcome switch
            {
                GlobalStateLoadOutcome.Missing => "was missing",
                GlobalStateLoadOutcome.Corrupt => $"was unreadable ({result.ErrorMessage ?? "no detail"})",
                GlobalStateLoadOutcome.UnsupportedVersion =>
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
        /// Resyncs from the raw bytes of the ntwtf.lua file in the save.
        /// </summary>
        public int ResyncFromSaveRawBytes(Il2CppStructArray<byte> bytes)
        {
            // Aliases the IL2CPP array's elements in place rather than copying them.
            Span<byte> data = bytes.AsSpan();

            lock (_gate)
            {
                if (!CanResync())
                {
                    return 0;
                }

                if (data.Length == 0)
                {
                    _log.Warning(
                        "Not resyncing the global state: the raw save bytes were null or empty.");
                    return 0;
                }

                List<SimStatusRow> rows = RawDataParser.GetSimStatuses(
                    data, out SimStatusParseCounts parseCounts);

                // data aliases the IL2CPP array in place, and bytes is dead from here on, so
                // without this the wrapper could be finalized - freeing its GCHandle, and with
                // it the array - mid-read.
                GC.KeepAlive(bytes);

                return Resync(rows);
            }
        }

        /// <summary>
        /// Refills the orb half of the current save from a loaded savegame, and merges
        /// what it finds into the global state.
        /// </summary>
        /// <remarks>
        /// <para>A second resync rather than part of the first, because the two halves of
        /// a save live in different files. Dialogue SimStatus is in
        /// <c>{save}.ntwtf.lua</c>, which <see cref="ResyncFromSaveRawBytes"/> reads as
        /// raw bytes on their way into <c>PersistentDataManager.ApplyRawData</c>. Orbs
        /// are in <c>{save}.states.lua</c>, a plain Lua script the game executes, so
        /// there are no bytes to intercept.</para>
        ///
        /// <para>Replacement, not merge, for the current save: these titles are the
        /// loaded save's <c>ShownOrbs</c> in full, so the previous save's orbs must not
        /// survive into it. The global state is merged into as always.</para>
        ///
        /// <para>An empty table is a real answer - a save from the start of a playthrough
        /// genuinely has no orbs - but it is also what reading the table too early would
        /// give, so the count is logged every time. Loads that all report zero orbs on
        /// saves that should have them is what a wrongly ordered hook looks like.</para>
        /// </remarks>
        /// <param name="conversationTitles">
        /// The keys of the loaded save's <c>ShownOrbs</c> table. Null or empty titles are
        /// skipped and counted rather than throwing: this is data read back out of the
        /// game, and one bad key should cost that key alone.
        /// </param>
        /// <returns>How many orbs the global state had never seen before.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="conversationTitles"/> is null.</exception>
        public int ResyncOrbs(IEnumerable<string?> conversationTitles)
        {
            if (conversationTitles == null)
            {
                throw new ArgumentNullException(nameof(conversationTitles));
            }

            lock (_gate)
            {
                if (!CanResyncOrbs())
                {
                    return 0;
                }

                int dropped = _currentSave.ClearOrbs();
                int seen = 0;
                int skipped = 0;
                int raised = 0;

                try
                {
                    foreach (string? title in conversationTitles)
                    {
                        if (string.IsNullOrEmpty(title))
                        {
                            skipped++;
                            continue;
                        }

                        seen++;
                        _currentSave.SetOrb(title);
                        if (_state.MergeOrb(title))
                        {
                            raised++;
                        }
                    }
                }
                catch (Exception ex)
                {
                    // Not retried, matching the entry resync: whatever broke reading the
                    // table would break the next load the same way, once per load, for
                    // the rest of the session. The orbs already taken from this table
                    // stay - they were real - and the tally is left holding them rather
                    // than being emptied on the way out.
                    _orbResyncGivenUp = true;
                    _log.Error(
                        $"Failed to resync orbs from the loaded save game after {seen} title(s): {ex}. "
                        + "No further orb resync will be attempted this session, so orbs already in a "
                        + "loaded save will only be counted if they are opened again.");
                    return 0;
                }

                _log.Info(
                    $"Resynced orbs after a savegame load: the save has {seen} orb(s) "
                    + $"(was showing {dropped}), of which {raised} were new to the global state, "
                    + $"which now has {_state.OrbCount}."
                    + (skipped == 0 ? string.Empty : $" {skipped} empty title(s) skipped."));

                if (raised == 0)
                {
                    return 0;
                }

                MarkDirty();
                return raised;
            }
        }

        /// <summary>
        /// Throws away the current save's tally, for a new game that has reset the
        /// game's own SimStatus table behind the mod's back.
        /// </summary>
        /// <remarks>
        /// <para>Only the current save's tally. The global state is not touched and must
        /// never be: a new game is precisely the event the across-all-saves history
        /// exists to survive.</para>
        ///
        /// <para>There is no other way for the mod to see this. A new game rebuilds the
        /// whole Lua Conversation table rather than marking entries one at a time, so
        /// <c>MarkDialogueEntry</c> never fires and there is no savegame load to resync
        /// from.</para>
        ///
        /// <para>The log line reports how long it has been since the last resync: were
        /// the game to reset world state <em>during</em> a load, after that load's
        /// resync, this would empty a tally that was just filled, and the timing is what
        /// makes that visible instead of quietly wrong.</para>
        /// </remarks>
        /// <param name="trigger">What detected the new game, for the log.</param>
        /// <returns>How many entries the tally was holding.</returns>
        public int ResetCurrentSave(string? trigger)
        {
            lock (_gate)
            {
                // Counted before clearing so the two halves can be reported apart; a new
                // game does reset ShownOrbs (GenericLuaFunctions.InitTables assigns it a
                // fresh empty table), so unlike a savegame load this really does drop
                // both.
                int droppedEntries = _currentSave.Count;
                int droppedOrbs = _currentSave.OrbCount;
                int dropped = _currentSave.Clear();
                string sinceResync = _lastResyncUtc is null
                    ? "no savegame has been resynced this session"
                    : $"{(DateTime.UtcNow - _lastResyncUtc.Value).TotalSeconds:F1} s since the last resync";

                _log.Info(
                    $"Current-save dialogue count reset by {trigger ?? "an unnamed trigger"}: "
                    + $"dropped {droppedEntries} entries and {droppedOrbs} orbs ({sinceResync}). "
                    + "The across-all-saves state is untouched and still has "
                    + $"{(_diskLoadDone ? _state.EntryCount : 0)} entries and "
                    + $"{(_diskLoadDone ? _state.OrbCount : 0)} orbs.");
                return dropped;
            }
        }

        /// <summary>
        /// Whether a resync may run at all, logging the reason when it may not. Caller
        /// must hold <see cref="_gate"/>.
        /// </summary>
        private bool CanResync() => CanResyncCore(_resyncGivenUp, "the loaded save data");

        /// <summary>
        /// The same gate for the orb half of a load. Separate given-up flag on purpose:
        /// the two resyncs read different files through different code, so one failing
        /// permanently says nothing about the other.
        /// </summary>
        private bool CanResyncOrbs() => CanResyncCore(_orbResyncGivenUp, "the loaded save's orbs");

        private bool CanResyncCore(bool givenUp, string what)
        {
            if (givenUp)
            {
                return false;
            }

            EnsureInitialized();

            if (!CanSave)
            {
                _log.Warning(
                    $"Not resyncing the global state from {what}: "
                    + $"saving is disabled for this session ({Origin}), so what it read could not be "
                    + $"kept. See the earlier log lines about '{_store.LivePath}'.");
                return false;
            }

            return true;
        }

        /// <summary>
        /// Merges one resync's rows and logs the line that describes the outcome.
        /// Caller must hold <see cref="_gate"/>, and must already have established
        /// through <see cref="CanResync"/> that a resync may run.
        /// </summary>
        /// <param name="rows">Latest rows from save.</param>
        /// <returns>How many statuses were raised.</returns>
        private int Resync(
            List<SimStatusRow> rows)
        {
            ResyncCount++;

            int rowCount = 0;
            int raisedCount;

            // The rows are the whole SimStatus table of the save being loaded, so they
            // are the current save's count in full. Emptying first is what stops the
            // previous save's entries from surviving into this one: this is a
            // replacement, not a merge, and it is the only difference between the two
            // states either side of this loop.
            //
            // ClearEntries and not Clear: these bytes are the ntwtf.lua dialogue data
            // and carry no orbs at all - ShownOrbs lives in the save's states.lua - so
            // clearing the orbs here would drop them with nothing in this method able to
            // put them back.
            _currentSave.ClearEntries();
            _lastResyncUtc = DateTime.UtcNow;

            try
            {
                raisedCount = MergeEverythingFromGame(rows, ref rowCount);
            }
            catch (Exception ex)
            {
                // Not retried: the next load would call the same thing and fail
                // the same way, once per load, for the rest of the session.
                _resyncGivenUp = true;
                _log.Error(
                    $"Failed to resync the global state from the loaded save game "
                    + $"after {rowCount} rows: {ex}. No further resync will be attempted this session, "
                    + "so statuses restored by loading a savegame will only be recorded if they are "
                    + "marked again during play.");
                return 0;
            }

            string raised = raisedCount == 0
                ? $"nothing new in {rowCount} rows, so no file was written."
                : $"{raisedCount} statuses raised from {rowCount} rows; now "
                    + $"{_state.ConversationCount} conversations, {_state.EntryCount} entries.";
            _log.Info(
                $"Resynced the global state after a savegame load: {raised} The loaded save itself "
                + $"has {_currentSave.DisplayedCount} displayed and {_currentSave.OfferedCount} "
                + $"offered of {rowCount} entries, scoring "
                + $"{DialogueScore.Format(_currentSave.Score)}.");

            if (raisedCount == 0)
            {
                return 0;
            }

            MarkDirty();
            return raisedCount;
        }

        /// <summary>
        /// Merges every SimStatus a resync read into the state, reporting the rows
        /// whose status string was not recognized.
        /// </summary>
        /// <param name="rows">
        /// The rows to merge, as parsed out of the loaded save's bytes.
        /// </param>
        /// <param name="rowCount">
        /// Counted up as the merge goes, so a caller that catches a throwing read can
        /// still say how far it got.
        /// </param>
        /// <returns>How many statuses this resync actually raised.</returns>
        private int MergeEverythingFromGame(IEnumerable<SimStatusRow> rows, ref int rowCount)
        {
            int raisedCount = 0;
            int skippedCount = 0;
            var warnings = new List<string>();

            foreach (SimStatusRow row in rows)
            {
                rowCount++;
                if (_state.TryMerge(row.ConversationId, row.DialogueEntryId, row.StatusName, out bool changed))
                {
                    // The same row, counted for this save. Untouched rows are the bulk
                    // of the table and are exactly the ones that must not count, which
                    // the tally handles by storing nothing for them.
                    _currentSave.TrySet(row.ConversationId, row.DialogueEntryId, row.StatusName, out _);

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
            $"GlobalStateSession({Origin}, initialized={IsInitialized}, canSave={CanSave}, "
            + $"resyncs={ResyncCount})";
    }
}
