using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using UnifiedConversationTracker.Persistence;
using Xunit;

namespace UnifiedConversationTracker.Session.Tests
{
    /// <summary>
    /// The deferred write: that recording a status does not write the file on the
    /// caller's thread, that bursts collapse into a couple of writes instead of one
    /// per mark, that shutdown lands the tail, and that all of it survives being
    /// hammered from several threads at once.
    /// </summary>
    /// <remarks>
    /// <para>These tests drive the writer through
    /// <see cref="UnifiedStateStore.SaveStepHook"/>, which exists for the crash-point
    /// tests but works just as well as a way to hold a write open at a known point.
    /// Holding it open is what makes "the hook thread is not the thread writing"
    /// something that can be asserted rather than timed: with a write stuck
    /// mid-flight, anything that still returns promptly demonstrably did not do that
    /// write.</para>
    ///
    /// <para>Nothing here waits without a bound. Every wait has
    /// <see cref="WaitTimeoutMilliseconds"/> on it and every "this should not block"
    /// check runs on a task that is joined with a timeout, so a regression that
    /// reintroduces the synchronous write fails the suite instead of hanging it.</para>
    /// </remarks>
    public class DeferredWriteTests
    {
        /// <summary>
        /// How long any wait in these tests is willing to last. Long enough that a
        /// loaded CI machine cannot fail it by being slow, short enough that a real
        /// deadlock is reported rather than waited out.
        /// </summary>
        private const int WaitTimeoutMilliseconds = 30_000;

        /// <summary>
        /// A write held open at a chosen point, so a test can observe what the rest of
        /// the session does while the filesystem half of a save is in flight.
        /// </summary>
        private sealed class HeldWrite : IDisposable
        {
            private readonly ManualResetEventSlim _started = new ManualResetEventSlim(false);
            private readonly ManualResetEventSlim _release = new ManualResetEventSlim(false);
            private int _completedWrites;

            public HeldWrite(UnifiedStateStore store)
            {
                store.SaveStepHook = step =>
                {
                    switch (step)
                    {
                        case UnifiedStateSaveStep.AfterTempFlushed:
                            _started.Set();

                            // Deliberately not asserted: this runs on the writer thread,
                            // and an assertion failure there would surface as the writer
                            // faulting rather than as the real failure.
                            _release.Wait(WaitTimeoutMilliseconds);
                            break;
                        case UnifiedStateSaveStep.AfterTempPromoted:
                            Interlocked.Increment(ref _completedWrites);
                            break;
                    }
                };
            }

            /// <summary>How many saves have run to completion.</summary>
            public int CompletedWrites => Volatile.Read(ref _completedWrites);

            /// <summary>Blocks until a write is in flight and stuck.</summary>
            public void WaitUntilInFlight() =>
                Assert.True(_started.Wait(WaitTimeoutMilliseconds), "No write ever started.");

            /// <summary>Lets the held write, and every write after it, finish.</summary>
            public void Release() => _release.Set();

            public void Dispose()
            {
                // Never leave a writer parked on this, whatever the test did.
                _release.Set();
                _started.Dispose();
                _release.Dispose();
            }
        }

        /// <summary>
        /// Waits for work running on another thread, failing rather than hanging if it
        /// never finishes.
        /// </summary>
        /// <remarks>
        /// Blocking on a task is exactly the point here - these tests are about what
        /// happens when one thread is stuck and another is not - so the waits live in
        /// helpers rather than in the test methods themselves, where xUnit1031 would
        /// (rightly, in general) object to them.
        /// </remarks>
        private static void AssertCompletes(string what, Task task)
        {
            Assert.True(task.Wait(WaitTimeoutMilliseconds), $"{what} did not finish.");

            // Surfaces whatever it threw, rather than an aggregate wrapping it.
            task.GetAwaiter().GetResult();
        }

        /// <summary>
        /// Runs an action that must not block, failing rather than hanging if it does.
        /// </summary>
        private static void AssertDoesNotBlock(string what, Action action) =>
            AssertCompletes($"{what} blocked; it", Task.Run(action));

        // -------------------------------------------------------------------
        // The write is off the caller's thread.
        // -------------------------------------------------------------------

        [Fact]
        public void Record_WhileAWriteIsInFlight_DoesNotWaitForIt()
        {
            // MarkDialogueEntry's postfix runs on the Unity main thread, where a
            // save costs a dropped frame. With a save deliberately stuck, a Record
            // that still returns cannot have been the thing doing it.
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();
            using var held = new HeldWrite(store);

            var log = new RecordingLog();
            using var session = new UnifiedStateSession(store, log);

            Assert.True(session.Record(3, 17, "WasDisplayed"));
            held.WaitUntilInFlight();

            AssertDoesNotBlock(
                "Record",
                () =>
                {
                    for (int entryId = 0; entryId < 100; entryId++)
                    {
                        Assert.True(session.Record(4, entryId, "WasDisplayed"));
                    }
                });

            held.Release();
            Assert.True(session.Flush());
            Assert.Equal(101, store.Load().RequireState().EntryCount);
            Assert.Empty(log.Errors);
        }

        // -------------------------------------------------------------------
        // Bursts coalesce.
        // -------------------------------------------------------------------

        [Fact]
        public void Record_InABurst_CollapsesIntoAHandfulOfWritesRatherThanOnePerMark()
        {
            // A response menu marks every offered response, so several raises land in
            // one frame, and rewriting the whole file for each would be pure cost.
            // Marks that arrive while the writer is busy are all picked up by its
            // next pass, so the cost of a burst is bounded by the writer's speed
            // rather than by its length.
            const int burstSize = 200;

            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();
            using var held = new HeldWrite(store);

            using var session = new UnifiedStateSession(store, new RecordingLog());

            Assert.True(session.Record(3, 0, "WasDisplayed"));
            held.WaitUntilInFlight();

            for (int entryId = 1; entryId < burstSize; entryId++)
            {
                Assert.True(session.Record(3, entryId, "WasDisplayed"));
            }

            held.Release();
            Assert.True(session.Flush());

            // The number that matters is that it is nowhere near burstSize.
            Assert.InRange(held.CompletedWrites, 1, 3);
            Assert.Equal(burstSize, store.Load().RequireState().EntryCount);
        }

        // -------------------------------------------------------------------
        // Shutdown lands the tail.
        // -------------------------------------------------------------------

        [Fact]
        public void Dispose_LandsAMarkThatArrivedWhileAWriteWasAlreadyInFlight()
        {
            // The tail the shutdown flush exists for: recorded after the snapshot
            // currently being written was taken, so no write in flight covers it.
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();
            using var held = new HeldWrite(store);

            var log = new RecordingLog();
            var session = new UnifiedStateSession(store, log);

            Assert.True(session.Record(3, 17, "WasDisplayed"));
            held.WaitUntilInFlight();
            Assert.True(session.Record(3, 18, "WasOffered"));

            held.Release();
            session.Dispose();

            UnifiedConversationState saved = store.Load().RequireState();
            Assert.Equal(SimStatus.WasDisplayed, saved.GetStatus(3, 17));
            Assert.Equal(SimStatus.WasOffered, saved.GetStatus(3, 18));
            Assert.Empty(log.Errors);
        }

        [Fact]
        public void Dispose_ImmediatelyAfterABurst_LandsEveryMarkOfIt()
        {
            // Quitting the game the moment a conversation ends. Nothing is held open
            // here, so the writer is genuinely behind rather than artificially so.
            const int markCount = 500;

            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            var session = new UnifiedStateSession(store, log);

            for (int entryId = 0; entryId < markCount; entryId++)
            {
                Assert.True(session.Record(7, entryId, "WasDisplayed"));
            }

            session.Dispose();

            Assert.Equal(markCount, store.Load().RequireState().EntryCount);
            Assert.Empty(log.Errors);
        }

        [Fact]
        public void Dispose_IsIdempotentAndLeavesTheSessionMergingWithoutWriting()
        {
            // A mark can arrive after the quit signal. It must not throw, must not
            // resurrect the writer, and must not be claimed to have been saved.
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            var session = new UnifiedStateSession(store, log);

            Assert.True(session.Record(3, 17, "WasDisplayed"));
            session.Dispose();
            session.Dispose();

            Assert.True(session.Record(3, 18, "WasOffered"));
            Assert.Equal(SimStatus.WasOffered, session.State.GetStatus(3, 18));
            Assert.False(session.Flush());

            UnifiedConversationState saved = store.Load().RequireState();
            Assert.Equal(SimStatus.WasDisplayed, saved.GetStatus(3, 17));
            Assert.Equal(SimStatus.Untouched, saved.GetStatus(3, 18));
            Assert.Empty(log.Errors);
        }

        [Fact]
        public void Flush_WithNothingRecorded_WritesNothingAndReturnsTrue()
        {
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            using var session = new UnifiedStateSession(store, log);
            session.EnsureInitialized();

            Assert.True(session.Flush());
            Assert.Empty(Directory.GetFiles(dir.Path));
            Assert.Empty(log.Errors);
        }

        // -------------------------------------------------------------------
        // Shutdown says what it did.
        // -------------------------------------------------------------------

        /// <summary>What the plugin passes for Unity's own quit event.</summary>
        private const string QuittingTrigger = "Application.quitting";

        /// <summary>What the plugin passes for the runtime's graceful-exit event.</summary>
        private const string ProcessExitTrigger = "AppDomain.ProcessExit";

        /// <summary>
        /// Returns the one line reporting a finished shutdown flush, failing if there
        /// is not exactly one.
        /// </summary>
        private static string FinishedLine(RecordingLog log, string trigger) =>
            Assert.Single(
                log.All,
                line => line.Contains(
                    $"shutdown flush finished on {trigger}", StringComparison.OrdinalIgnoreCase));

        [Fact]
        public void Shutdown_NamesTheTriggerOnArrivalAndOnCompletion()
        {
            // Success has to be reported: a clean quit that flushed and a handler
            // that never fired would otherwise look identical in a log.
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            var session = new UnifiedStateSession(store, log);

            Assert.True(session.Record(3, 17, "WasDisplayed"));

            // Flushing first is what makes the counts below deterministic rather than a
            // race with the writer: exactly one write, and nothing left pending.
            Assert.True(session.Flush());

            session.Shutdown(QuittingTrigger);

            Assert.Contains(
                log.Info,
                line => line.Contains(
                    $"shutdown flush triggered by {QuittingTrigger}.", StringComparison.OrdinalIgnoreCase));

            string finished = FinishedLine(log, QuittingTrigger);
            Assert.Contains("nothing was pending", finished, StringComparison.Ordinal);
            Assert.Contains(
                "1 status(es) recorded during play and 1 write(s) reached disk this session",
                finished,
                StringComparison.Ordinal);
            Assert.Contains("the writer thread stopped", finished, StringComparison.Ordinal);
            Assert.Contains(" ms:", finished, StringComparison.Ordinal);
            Assert.Empty(log.Warnings);
            Assert.Empty(log.Errors);
        }

        [Fact]
        public void Shutdown_WithNothingEverRecorded_SaysSoRatherThanStayingSilent()
        {
            // A run that raised nothing leaves a perfectly working flush with
            // nothing to write. "Nothing pending" is a finding; it must not be
            // indistinguishable from a handler that never ran.
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            var session = new UnifiedStateSession(store, log);
            session.EnsureInitialized();

            session.Shutdown(ProcessExitTrigger);

            string finished = FinishedLine(log, ProcessExitTrigger);
            Assert.Contains("nothing was ever recorded this session", finished, StringComparison.Ordinal);
            Assert.Contains(
                "0 status(es) recorded during play and 0 write(s) reached disk this session",
                finished,
                StringComparison.Ordinal);
            Assert.Contains("no writer thread was ever started", finished, StringComparison.Ordinal);
            Assert.Empty(log.Warnings);
            Assert.Empty(log.Errors);
        }

        [Fact]
        public void Shutdown_FiringTwice_SaysWhichTriggerAlreadyDidTheWork()
        {
            // Both events are registered because neither is confirmed to fire under
            // BepInEx's IL2CPP chainloader. If both fire, the log has to say so - that
            // is the answer to "which shutdown event does this game actually raise".
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            var session = new UnifiedStateSession(store, log);

            Assert.True(session.Record(3, 17, "WasDisplayed"));
            session.Shutdown(QuittingTrigger);
            session.Shutdown(ProcessExitTrigger);

            FinishedLine(log, QuittingTrigger);
            Assert.Contains(
                log.Info,
                line => line.Contains(
                    $"already ran on {QuittingTrigger}, so {ProcessExitTrigger} had nothing left to do",
                    StringComparison.OrdinalIgnoreCase));

            // The second trigger did no work, so it must not claim to have finished one.
            Assert.DoesNotContain(
                log.All,
                line => line.Contains(
                    $"shutdown flush finished on {ProcessExitTrigger}", StringComparison.OrdinalIgnoreCase));
            Assert.Empty(log.Errors);
        }

        [Fact]
        public void Dispose_WithNoTriggerNamed_StillSaysItRan()
        {
            // using-blocks, tests and tools have only one way out, so they get a name
            // rather than a blank.
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            var session = new UnifiedStateSession(store, log);
            session.EnsureInitialized();

            session.Dispose();

            FinishedLine(log, "Dispose");
            Assert.Empty(log.Errors);
        }

        // -------------------------------------------------------------------
        // Under concurrency.
        // -------------------------------------------------------------------

        [Fact]
        public void Record_FromManyThreadsWhileTheWriterRuns_LosesNothing()
        {
            // A soak rather than a scenario: a green run of the tests above says the
            // design is right, not that the interleavings are. Every writer the design
            // has runs at once here - the marking path, the synchronous save and the
            // flush - against one session and one file, and the invariant checked at
            // the end is total: every status recorded is in memory, and the file
            // matches memory exactly.
            const int recorderCount = 4;
            const int marksPerRecorder = 250;
            const int flushCount = 50;
            const int trySaveCount = 20;

            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            var session = new UnifiedStateSession(store, log);

            var tasks = new List<Task>();
            for (int recorder = 0; recorder < recorderCount; recorder++)
            {
                int conversationId = recorder;
                tasks.Add(Task.Run(() =>
                {
                    for (int entryId = 0; entryId < marksPerRecorder; entryId++)
                    {
                        // Offered then displayed, so entries are both created and
                        // raised, and each pair is one changed merge followed by one
                        // no-op merge.
                        session.Record(conversationId, entryId, "WasOffered");
                        session.Record(conversationId, entryId, "WasDisplayed");
                        session.Record(conversationId, entryId, "Untouched");
                    }
                }));
            }

            tasks.Add(Task.Run(() =>
            {
                for (int i = 0; i < flushCount; i++)
                {
                    session.Flush();
                }
            }));

            tasks.Add(Task.Run(() =>
            {
                for (int i = 0; i < trySaveCount; i++)
                {
                    Assert.True(session.TrySave());
                }
            }));

            AssertCompletes("The soak", Task.WhenAll(tasks));

            const int expectedEntryCount = recorderCount * marksPerRecorder;
            Assert.Equal(expectedEntryCount, session.State.EntryCount);

            session.Dispose();

            UnifiedConversationState saved = store.Load().RequireState();
            Assert.Equal(expectedEntryCount, saved.EntryCount);
            for (int conversationId = 0; conversationId < recorderCount; conversationId++)
            {
                for (int entryId = 0; entryId < marksPerRecorder; entryId++)
                {
                    Assert.Equal(SimStatus.WasDisplayed, saved.GetStatus(conversationId, entryId));
                }
            }

            Assert.Empty(log.Errors);
        }

        [Fact]
        public void Dispose_WhileMarksAreStillArriving_StillLeavesAReadableFile()
        {
            // Quitting mid-conversation. Whatever the race between the shutdown flush
            // and the marks still coming in, the file has to be complete and readable
            // and hold everything written before the flush started - the store's
            // rotation guarantees the first part, and Dispose draining the queue
            // guarantees the second.
            const int markCount = 2000;

            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            var session = new UnifiedStateSession(store, log);

            // One mark up front, so there is definitely a file to assert about however
            // the race between the flush and the rest of the marks falls out.
            Assert.True(session.Record(5, 0, "WasDisplayed"));

            var marking = Task.Run(() =>
            {
                for (int entryId = 1; entryId < markCount; entryId++)
                {
                    session.Record(5, entryId, "WasDisplayed");
                }
            });

            session.Dispose();
            AssertCompletes("Marking blocked on the shutdown flush; it", marking);

            UnifiedStateLoadResult reloaded = store.Load();
            Assert.True(reloaded.IsLoaded);
            Assert.Equal(0, reloaded.SkippedRowCount);

            // Marks that arrived after the flush are in memory but not on disk; that
            // is the accepted cost, and nothing in between may be corrupt.
            UnifiedConversationState saved = reloaded.RequireState();
            Assert.InRange(saved.EntryCount, 1, markCount);
            Assert.Empty(log.Errors);
        }
    }
}
