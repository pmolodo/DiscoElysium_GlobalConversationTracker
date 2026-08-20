using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Threading;
using Il2CppInterop.Runtime.InteropTypes.Arrays;
using UnifiedConversationTracker.Core;
using UnifiedConversationTracker.Persistence;
using UnifiedConversationTracker.Persistence.Interop;

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
    /// being played. The load-time path covers the writer that never goes through it -
    /// a savegame load rebuilding the whole SimStatus table at once, without ever
    /// calling <c>MarkDialogueEntry</c>. Between them they see every SimStatus the
    /// game ever holds; there is no third writer.</para>
    ///
    /// <para><b>The load-time path is <see cref="ResyncFromSaveRawBytes"/>, which
    /// reads the dialogue data straight from the save bytes.</b></para>
    ///
    /// <para><b>Nothing writes the file on the caller's thread.</b>
    /// <see cref="Record"/> and <see cref="ResyncFromSaveRawBytes"/> merge and then mark the
    /// state dirty; a single background thread does the writing. A save costs
    /// 6.7-7.4 ms for a realistic Day-1 save and ~40 ms at the ceiling, of which ~85%
    /// is the flush and the two renames - a whole dropped frame or more, inside a
    /// dialogue hook, mid-conversation. See <see cref="TrySave"/> for the
    /// synchronous path that remains, and <see cref="Shutdown"/> for the shutdown
    /// flush that stops a clean exit losing the tail - which says in the log what it
    /// did, including when it did nothing, because a silent success is
    /// indistinguishable from a handler that never fired at all.</para>
    ///
    /// <para>Not thread safe in the sense of being lock-free, but every public
    /// method takes the same lock and C# locks are reentrant, so a hook that
    /// re-enters during initialization gets the current state back instead of
    /// deadlocking or recursing. That one lock is also the writer thread's condition
    /// variable, so there is exactly one lock in the whole design and therefore no
    /// lock ordering to get wrong. The writer holds it only long enough to copy the
    /// state, and serializes and writes the copy outside it, so the longest
    /// anything can make a mark wait is that copy.</para>
    /// </remarks>
    public sealed class UnifiedStateSession : IDisposable
    {
        /// <summary>
        /// How many skipped-row descriptions a resync logs before it stops listing
        /// them. Deliberately the file loader's own cap, so a skipped save row and a
        /// skipped file row read the same way in a log.
        /// </summary>
        public const int MaxGameWalkWarnings = UnifiedStateJson.MaxWarnings;

        /// <summary>What a null status string is called in the log and in the set of
        /// already-warned-about statuses, since null cannot go in the set itself.</summary>
        private const string NullStatusName = "null";

        /// <summary>
        /// How long a caller waiting for a write to land - <see cref="TrySave"/>,
        /// <see cref="Flush"/>, the flush inside <see cref="Dispose"/> - waits before
        /// giving up and saying so. Generous by two orders of magnitude against the
        /// 40 ms a save costs at the largest file that can be constructed, because
        /// the only thing a shorter one buys is giving up on a write that was about
        /// to succeed. It exists so a writer wedged on a locked file cannot hang the
        /// game's shutdown for ever.
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
        private const string WriterThreadName = "UnifiedConversationTracker writer";

        /// <summary>
        /// What <see cref="Dispose"/> calls the trigger when nobody named one. The
        /// plugin always names one; this covers tests, tools and <c>using</c> blocks.
        /// </summary>
        private const string DefaultShutdownTrigger = "Dispose";

        /// <summary>
        /// The prefix every shutdown line shares, so one search finds all of them
        /// whatever the outcome was.
        /// </summary>
        private const string ShutdownLinePrefix = "Unified state shutdown flush";

        private readonly object _gate = new object();
        private readonly UnifiedStateStore _store;
        private readonly IUnifiedStateLog _log;

        private readonly UnifiedConversationState _state = new UnifiedConversationState();

        /// <summary>Status strings from the game that have already been warned about.</summary>
        private readonly HashSet<string> _unrecognizedStatuses = new HashSet<string>(StringComparer.Ordinal);

        private bool _diskLoadDone;
        private bool _resyncGivenUp;

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
        /// shutdown for the same reason as everything else on that line: a zero here
        /// separates "the marking hook fired and the game marked nothing new" from
        /// "the marking hook never fired", which no other line in the log distinguishes
        /// (the resync reports its own raises separately, and a write can come from
        /// either path).
        /// </summary>
        private long _statusesRecorded;

        /// <summary>Creates a session.</summary>
        /// <param name="store">The store over the SaveGames directory.</param>
        /// <param name="log">Where recovery and resyncing are reported.</param>
        /// <exception cref="ArgumentNullException">Any argument is null.</exception>
        public UnifiedStateSession(
            UnifiedStateStore store,
            IUnifiedStateLog log)
        {
            _store = store ?? throw new ArgumentNullException(nameof(store));
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
        /// The unified state. Prefer <see cref="EnsureInitialized"/>; this property
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
        /// <para><b>Nothing is written unless something changed.</b> The game marks
        /// the same entry repeatedly - every time a line is offered again, and
        /// "Untouched" over entries that already have history - and the merge rule
        /// turns all of those into no-ops. Rewriting the whole file for a no-op would
        /// put the file's entire cost on the common case, so the write is driven by
        /// the merge's own changed flag.</para>
        ///
        /// <para><b>This never writes the file.</b> It merges, marks the state dirty
        /// and returns; the background writer does the rest. This is the
        /// <c>DialogueLua.MarkDialogueEntry</c> postfix, so it runs on the Unity main
        /// thread mid-conversation, where the 6.7-40 ms a save costs is a dropped
        /// frame. Deferring it is safe for the same reason the whole design is: the
        /// unified state is write-only, so nothing reads back what has not landed yet,
        /// and a mark lost to a hard crash is re-marked the next time the line is
        /// reached.</para>
        ///
        /// <para>This is the write-through half of a write-only design: the unified
        /// state never flows back into the game, so a failure here loses tracking and
        /// nothing else.</para>
        /// </remarks>
        /// <returns>
        /// True if this call raised a status in the unified state. The write that
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
        /// <para><b>Synchronous, and deliberately the exception.</b> Everything the
        /// game drives goes through the dirty flag instead; this is for the callers
        /// that genuinely need the bytes on disk before they carry on -
        /// shutdown, and tests. It still writes on the background thread, because that
        /// thread is the only thing allowed to touch the three files, and simply waits
        /// for it. Nothing is skipped when the state is clean: an explicit "save now"
        /// means save now.</para>
        ///
        /// <para>IO failures are logged and swallowed rather than thrown: a save file
        /// being locked must cost tracking and nothing else. The next write attempt
        /// retries from scratch, and <see cref="UnifiedStateStore.Save"/> never leaves
        /// a partial file behind.</para>
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
        /// <para>This is the "never lose the tail on a clean exit" half of the
        /// deferred write, and the reason that write needs no durability machinery of
        /// its own. It is idempotent; after it, the session still merges but no longer
        /// writes, which is the right behaviour for a mark that arrives while the game
        /// is tearing down.</para>
        /// <para>BepInEx does not call plugin <c>Unload</c> on game exit, so the
        /// plugin drives this from process- and application-level shutdown events
        /// instead. Use <see cref="Shutdown"/> to say which one; this overload exists
        /// for <c>using</c> blocks, tests and tools, which have only one.</para>
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
        /// <para><b>Same work as <see cref="Dispose"/>; the difference is the log.</b>
        /// A shutdown that said nothing on success would make "the handler fired and
        /// there was nothing to flush" and "the handler never fired at all" identical
        /// in a session log - and since BepInEx's IL2CPP chainloader calls neither
        /// <c>Unload</c> nor anything else on the way out, which of the two registered
        /// events actually fires is exactly the open question. So the shutdown logs
        /// two lines: one when the trigger arrives, before the gate is taken, so a
        /// shutdown that then blocks behind a resync still proves it fired; one when
        /// the drain is over, carrying the trigger, what was pending, whether it
        /// landed, how long it took, how much the session recorded and wrote in total,
        /// and whether the writer thread stopped.</para>
        ///
        /// <para><b>Why the closing line is logged outside the gate.</b> The gate is
        /// the writer's condition variable, and the writer logs its own failures from
        /// its own thread, so a log sink with a lock of its own is reachable from two
        /// threads. Nothing in the writer ever holds a log's lock while it wants the
        /// gate - <see cref="TryWritePayload"/> logs after releasing it and before
        /// re-taking it, and <see cref="WriterLoop"/>'s catch logs after its lock
        /// block - so there is no cycle either way round. The closing line is logged
        /// after the gate is released anyway, which leaves the one line that has to be
        /// early (the trigger line, logged before the gate is taken at all) touching no
        /// session state whatsoever.</para>
        ///
        /// <para><b>Nothing here can hang.</b> The drain is
        /// <see cref="WaitForWrite"/>, already bounded by
        /// <see cref="WriteWaitTimeoutMilliseconds"/> and already loud when it expires;
        /// the join is bounded by <see cref="WriterExitTimeoutMilliseconds"/>; the two
        /// log calls are the same sink the rest of the class already writes to under
        /// the gate.</para>
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
            UnifiedStateOrigin origin = UnifiedStateOrigin.Uninitialized;
            bool canSave = false;
            bool writerExisted = false;
            bool flushed = false;
            long pending = 0;
            long writesLanded = 0;
            long recorded = 0;

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
                + $"{recorded} status(es) recorded during play and "
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
        /// Says what the shutdown flush actually did, including when the answer is
        /// "nothing" - which is a result, not a reason to stay quiet.
        /// </summary>
        private string DescribeShutdownFlush(
            UnifiedStateOrigin origin, bool canSave, bool writerExisted, long pending, bool flushed)
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
                    $"Not saving the unified state: saving is disabled for this session ({Origin}). "
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
        /// <para><b>Why the lock is held for the copy and nothing else.</b> Reading the
        /// live state cannot happen concurrently with a merge, so something has to be
        /// under the lock; everything after the copy touches only the copy and the
        /// filesystem, so nothing else has to be. Holding the lock for the whole
        /// serialize instead would make a mark arriving mid-serialize wait for it on
        /// the Unity main thread - up to 16 ms at 30,000 entries and 39 ms at the
        /// 112,940-entry ceiling, a dropped frame either way.
        /// Copying first shrinks what is under the lock by 24-25x at every size the
        /// benchmark sweeps (0.008 ms against 0.21 ms at a realistic 1,473 entries,
        /// 0.9 ms against 22 ms at the ceiling), at the price of one transient copy of
        /// the state per write.</para>
        ///
        /// <para><b>It is still one lock.</b> The copy is taken under the same gate
        /// everything else uses, so there is still no second lock and no ordering to get
        /// wrong; the gate is simply held for less time. And it is still one writer: this
        /// thread is the only thing that ever calls
        /// <see cref="UnifiedStateStore.SavePayload"/>.</para>
        ///
        /// <para><b>Bursts coalesce for free.</b> A response menu marks every offered
        /// response, so several raises land in one frame. Each of those bumps
        /// <see cref="_dirtyVersion"/> while this thread is busy with the previous
        /// snapshot, and one further pass then covers all of them - so a burst of any
        /// size costs at most two writes rather than one per mark. That is why there
        /// is no debounce delay here: the delay would buy the difference between two
        /// writes and one, at the price of a tunable and a wider window in which a
        /// hard crash loses something.</para>
        ///
        /// <para><b>It never touches the game.</b> Only the unified state and the
        /// filesystem, never the Dialogue System, <c>DialogueLua</c>, the Lua
        /// environment or any IL2CPP object. Reading the game off the Unity thread is
        /// not established as safe and this design does not need it.</para>
        /// </remarks>
        private void WriterLoop()
        {
            try
            {
                while (true)
                {
                    long version;
                    UnifiedConversationState snapshot;

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
                    byte[] payload = UnifiedStateJson.SerializeToUtf8Bytes(snapshot);
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
                    $"The unified state writer stopped unexpectedly: {ex}. Nothing further will be "
                    + $"written to '{_store.LivePath}' this session; the in-memory state is intact "
                    + "and the game is unaffected.");
            }
        }

        /// <summary>
        /// Writes one serialized snapshot. Must be called without
        /// <see cref="_gate"/> held, and only ever from the writer thread - it is the
        /// single-writer guarantee <see cref="UnifiedStateStore"/> relies on.
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
                    $"Failed to write the unified state to '{_store.LivePath}': {ex.Message}. "
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
                        $"Timed out after {WriteWaitTimeoutMilliseconds} ms waiting for the unified "
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

            // Nothing on disk is not a dead end: the next savegame load reads the
            // save's whole SimStatus table back in (see ResyncFromSaveRawBytes),
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
        /// Resyncs from the raw bytes of the ntwtf.lua file in the save.
        /// </summary>
        public int ResyncFromSaveRawBytes(Il2CppStructArray<byte> bytes)
        {
            string byteInfo = bytes.Length == 0 ? "<empty> bytes" : $"{bytes.Length} bytes, first byte: {bytes[0]}";
            _log.Info($"ResyncFromSaveRawBytes callback fired - {byteInfo}");
            // Aliases the IL2CPP array's elements in place rather than copying them.
            Span<byte> data = bytes.AsSpan();
            _log.Info($"raw data ready? {data.Length > 0}");

            lock (_gate)
            {
                if (!CanResync())
                {
                    return 0;
                }

                if (data.Length == 0)
                {
                    _log.Warning(
                        "Not resyncing the unified state: received a null or empty byte string for raw save bytes"
                    );
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
        /// Whether a resync may run at all, logging the reason when it may not. Caller
        /// must hold <see cref="_gate"/>.
        /// </summary>
        private bool CanResync()
        {
            if (_resyncGivenUp)
            {
                return false;
            }

            EnsureInitialized();

            if (!CanSave)
            {
                _log.Warning(
                    $"Not resyncing the unified state from the loaded save data: "
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
                    $"Failed to resync the unified state from the loaded save game "
                    + $"after {rowCount} rows: {ex}. No further resync will be attempted this session, "
                    + "so statuses restored by loading a savegame will only be recorded if they are "
                    + "marked again during play.");
                return 0;
            }

            string outcome =
                $"Resynced the unified state after a savegame load: ";
            _log.Info(
                raisedCount == 0
                    ? outcome
                        + $"nothing new in {rowCount} rows, so no file was written."
                    : outcome
                        + $"{raisedCount} statuses raised from {rowCount} rows; "
                        + $"now {_state.ConversationCount} "
                        + $"conversations, {_state.EntryCount} entries.");

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
