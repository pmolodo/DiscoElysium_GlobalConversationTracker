using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Threading;
using UnifiedConversationTracker;
using UnifiedConversationTracker.Core;
using UnifiedConversationTracker.Persistence;
using UnifiedConversationTracker.Session;

namespace UnifiedStateBenchmark
{
    /// <summary>
    /// Times what the unified state costs the thread it is written from, at a sweep
    /// of entry counts. Investigatory tooling only - nothing here ships.
    /// </summary>
    /// <remarks>
    /// <para>Three questions, added in that order. de-omm.11 asked how expensive one
    /// whole-file rewrite is, broken out by phase. de-omm.22 moved the write off the
    /// caller's thread and added the two <c>Record</c> measurements, to show what the
    /// caller pays now. de-0m0.5 asked what the caller pays in the <em>worst</em> case,
    /// since the writer still serializes under the session lock: an average hides that
    /// completely, so the last section reports percentiles and a maximum.</para>
    /// <para>The phase breakdown is a hand-rolled replay of <see cref="UnifiedStateStore.Save"/>
    /// using the store's own public paths, because Save itself is one opaque call.
    /// Every run also times the real Save end to end and prints both, so a drift
    /// between the replay and the real thing is visible rather than assumed away.</para>
    /// </remarks>
    public static class Program
    {
        /// <summary>
        /// Entry counts to sweep. 1,473 is the real MARTINAISE DAY 1 12-33 save
        /// measured in de-omm.9 (142 WasOffered + 1,331 WasDisplayed; Untouched is
        /// never stored). 10k and 30k stand in for a mid and late playthrough.
        /// 112,940 is the pathological ceiling: every row in the master database
        /// raised above Untouched, which cannot happen in a real playthrough.
        /// </summary>
        private static readonly int[] EntryCounts = { 1_473, 10_000, 30_000, 112_940 };

        /// <summary>
        /// Entries per conversation in the synthetic states. Disco Elysium's master
        /// database holds ~112,940 dialogue entries across roughly 1,300
        /// conversations, so ~85 entries per conversation is the right shape. The
        /// number matters because serialization opens one JSON object per
        /// conversation and sorts each conversation's entries separately.
        /// </summary>
        private const int EntriesPerConversation = 85;

        /// <summary>Timed iterations per measurement, after the warmup.</summary>
        private const int Iterations = 25;

        /// <summary>Untimed iterations run first, to get past JIT and first-touch costs.</summary>
        private const int WarmupIterations = 3;

        /// <summary>
        /// Share of stored entries that are WasOffered rather than WasDisplayed.
        /// de-omm.9 measured 142 of 1,473, so roughly one in ten.
        /// </summary>
        private const int WasOfferedEveryNth = 10;

        /// <summary>
        /// Entry counts the contention measurement sweeps (de-0m0.5). A subset of
        /// <see cref="EntryCounts"/>: the realistic Day-1 save, a late playthrough, and
        /// the pathological ceiling. Each size costs
        /// <see cref="ContentionProbeCount"/> x <see cref="ContentionProbeIntervalMs"/>
        /// of wall time, so the sweep is kept short deliberately.
        /// </summary>
        private static readonly int[] ContentionEntryCounts = { 1_473, 30_000, 112_940 };

        /// <summary>How many individually timed <c>Record</c> calls each contention size takes.</summary>
        private const int ContentionProbeCount = 3_000;

        /// <summary>
        /// Milliseconds between contention probes. The pacing is the whole point: a
        /// tight loop of <c>Record</c> calls finishes in microseconds and never overlaps
        /// a serialize at all, so it measures nothing. 1 kHz is already orders of
        /// magnitude faster than <c>DialogueLua.MarkDialogueEntry</c> can fire in a real
        /// conversation, which makes every collision figure here an upper bound.
        /// </summary>
        private const double ContentionProbeIntervalMs = 1.0;

        /// <summary>
        /// Every nth contention probe raises a status instead of being a no-op, which is
        /// what keeps the background writer working. One raise per 10 ms is far above the
        /// game's rate and is chosen so the writer is busy for most of the run rather
        /// than to be realistic.
        /// </summary>
        private const int ContentionRaiseEveryNth = 10;

        /// <summary>Untimed contention probes run first, to get past JIT.</summary>
        private const int ContentionWarmupProbes = 200;

        /// <summary>
        /// Conversation ID the contention measurement uses for its no-op probes and, once
        /// the pool of raisable entries runs out, for fresh ones. Above every ID
        /// <see cref="BuildState"/> produces.
        /// </summary>
        private const int ScratchConversationId = 1_000_000;

        /// <summary>
        /// How many samples the contention measurement takes of each of the two halves
        /// of a snapshot: the copy that is under the lock, and the serialize that used
        /// to be and is not since de-0m0.5.
        /// </summary>
        private const int LockHoldReferenceSamples = 25;

        /// <summary>
        /// One 60 fps frame, in milliseconds. The threshold the whole question is about:
        /// a Record that blocks for longer than this has cost the player a frame.
        /// </summary>
        private const double FrameBudgetMs = 1000.0 / 60.0;

        /// <summary>
        /// The other latency threshold reported, well below a frame but far enough above
        /// the ~100 ns uncontended cost to mean the call definitely waited on the writer.
        /// </summary>
        private const double BlockedThresholdMs = 1.0;

        public static int Main(string[] args)
        {
            string directory = args.Length > 0
                ? args[0]
                : Path.Combine(Path.GetTempPath(), "de-omm-11-bench");

            try
            {
                Run(directory);
            }
            catch (Exception)
            {
                Console.Error.WriteLine("Benchmark failed.");
                throw;
            }

            return 0;
        }

        private static void Run(string directory)
        {
            Directory.CreateDirectory(directory);
            string fullDirectory = Path.GetFullPath(directory);

            Console.WriteLine($"Benchmark directory : {fullDirectory}");
            Console.WriteLine($"Volume              : {Path.GetPathRoot(fullDirectory)}");
            Console.WriteLine($"Runtime             : {Environment.Version} ({(Environment.Is64BitProcess ? "x64" : "x86")})");
            Console.WriteLine($"Server GC           : {System.Runtime.GCSettings.IsServerGC}");
            Console.WriteLine($"Processors          : {Environment.ProcessorCount}");
            Console.WriteLine($"Iterations          : {Iterations} timed, {WarmupIterations} warmup");
            Console.WriteLine();

            foreach (int entryCount in EntryCounts)
            {
                MeasureSize(fullDirectory, entryCount);
            }

            MeasureNoOpRecord(fullDirectory);
            MeasureRaisingRecord(fullDirectory);

            foreach (int entryCount in ContentionEntryCounts)
            {
                MeasureRecordUnderContention(fullDirectory, entryCount);
            }
        }

        private static void MeasureSize(string directory, int entryCount)
        {
            string sizeDirectory = Path.Combine(directory, entryCount.ToString(CultureInfo.InvariantCulture));
            ResetDirectory(sizeDirectory);

            var store = new UnifiedStateStore(sizeDirectory);
            UnifiedConversationState state = BuildState(entryCount);
            int payloadBytes = UnifiedStateJson.SerializeToUtf8Bytes(state).Length;

            Console.WriteLine(
                $"=== {entryCount:N0} entries / {state.ConversationCount:N0} conversations / "
                + $"{payloadBytes:N0} bytes on disk ({payloadBytes / 1024.0:F1} KiB) ===");

            var order = new List<double>();
            var serialize = new List<double>();
            var createDirectory = new List<double>();
            var openAndWrite = new List<double>();
            var flushToDisk = new List<double>();
            var rotate = new List<double>();
            var promote = new List<double>();
            var replayTotal = new List<double>();

            for (int i = 0; i < WarmupIterations + Iterations; i++)
            {
                bool timed = i >= WarmupIterations;
                ReplaySave(store, state, timed ? order : null, timed ? serialize : null,
                    timed ? createDirectory : null, timed ? openAndWrite : null,
                    timed ? flushToDisk : null, timed ? rotate : null, timed ? promote : null,
                    timed ? replayTotal : null);
            }

            var realSave = new List<double>();
            for (int i = 0; i < WarmupIterations + Iterations; i++)
            {
                long start = Stopwatch.GetTimestamp();
                store.Save(state);
                double elapsed = ToMilliseconds(Stopwatch.GetTimestamp() - start);
                if (i >= WarmupIterations)
                {
                    realSave.Add(elapsed);
                }
            }

            var load = new List<double>();
            for (int i = 0; i < WarmupIterations + Iterations; i++)
            {
                long start = Stopwatch.GetTimestamp();
                UnifiedStateRecovery recovery = store.LoadWithBackupFallback();
                double elapsed = ToMilliseconds(Stopwatch.GetTimestamp() - start);
                if (!recovery.Live.IsLoaded)
                {
                    throw new InvalidOperationException("The live file did not load back.");
                }

                if (i >= WarmupIterations)
                {
                    load.Add(elapsed);
                }
            }

            Report("  sort (OrderBy, inside serialize)", order);
            Report("  serialize (incl. sort)", serialize);
            Report("  Directory.CreateDirectory", createDirectory);
            Report("  open temp + write bytes", openAndWrite);
            Report("  Flush(flushToDisk: true)", flushToDisk);
            Report("  rename live -> .bak", rotate);
            Report("  rename .tmp -> live", promote);
            Report("  REPLAY TOTAL", replayTotal);
            Report("  REAL UnifiedStateStore.Save", realSave);
            Report("  LoadWithBackupFallback", load);
            Console.WriteLine();

            ResetDirectory(sizeDirectory);
            Directory.Delete(sizeDirectory);
        }

        /// <summary>
        /// Replays the exact step sequence of <see cref="UnifiedStateStore.Save"/>,
        /// timing each step. Pass null lists to run a warmup iteration untimed.
        /// </summary>
        private static void ReplaySave(
            UnifiedStateStore store,
            UnifiedConversationState state,
            List<double>? order,
            List<double>? serialize,
            List<double>? createDirectory,
            List<double>? openAndWrite,
            List<double>? flushToDisk,
            List<double>? rotate,
            List<double>? promote,
            List<double>? total)
        {
            long overallStart = Stopwatch.GetTimestamp();

            // The sort is not separable from serialization in production code, so it
            // is measured here as an extra pass purely for attribution. Its cost is
            // included in the serialize figure below, not additional to it.
            long start = Stopwatch.GetTimestamp();
            int sorted = 0;
            foreach (UnifiedStatusEntry entry in state.EnumerateEntriesInIdOrder())
            {
                sorted += entry.DialogueEntryId;
            }

            Add(order, start);
            GC.KeepAlive(sorted);

            start = Stopwatch.GetTimestamp();
            byte[] payload = UnifiedStateJson.SerializeToUtf8Bytes(state);
            Add(serialize, start);

            start = Stopwatch.GetTimestamp();
            Directory.CreateDirectory(store.DirectoryPath);
            Add(createDirectory, start);

            using (var stream = new FileStream(store.TempPath, FileMode.Create, FileAccess.Write, FileShare.None))
            {
                start = Stopwatch.GetTimestamp();
                stream.Write(payload, 0, payload.Length);
                Add(openAndWrite, start);

                start = Stopwatch.GetTimestamp();
                stream.Flush(flushToDisk: true);
                Add(flushToDisk, start);
            }

            if (File.Exists(store.LivePath))
            {
                start = Stopwatch.GetTimestamp();
                File.Move(store.LivePath, store.BackupPath, overwrite: true);
                Add(rotate, start);
            }

            start = Stopwatch.GetTimestamp();
            File.Move(store.TempPath, store.LivePath);
            Add(promote, start);

            Add(total, overallStart);
        }

        /// <summary>
        /// Times the path the hook takes on a mark that changes nothing, which
        /// de-omm.8 established is the overwhelming majority of calls: merge finds
        /// an equal-or-lower status, returns changed=false, and no file is written.
        /// </summary>
        private static void MeasureNoOpRecord(string directory)
        {
            const int noOpCalls = 200_000;
            string sizeDirectory = Path.Combine(directory, "noop");
            ResetDirectory(sizeDirectory);

            var store = new UnifiedStateStore(sizeDirectory);
            UnifiedConversationState state = BuildState(EntryCounts[0]);
            store.Save(state);

            var session = new UnifiedStateSession(store, new EmptySimStatusSource(), NullUnifiedStateLog.Instance);
            session.EnsureInitialized();

            // Every one of these re-marks an entry that is already WasDisplayed, so
            // the merge cannot raise anything and no save happens.
            for (int i = 0; i < 1_000; i++)
            {
                session.Record(0, 0, SimStatusNames.WasDisplayed);
            }

            long start = Stopwatch.GetTimestamp();
            for (int i = 0; i < noOpCalls; i++)
            {
                if (session.Record(0, 0, SimStatusNames.WasDisplayed))
                {
                    throw new InvalidOperationException("A no-op record reported a change.");
                }
            }

            double elapsed = ToMilliseconds(Stopwatch.GetTimestamp() - start);
            Console.WriteLine("=== Record() on a mark that changes nothing (no file written) ===");
            Console.WriteLine(
                $"  {noOpCalls:N0} calls in {elapsed:F2} ms "
                + $"= {elapsed * 1000.0 * 1000.0 / noOpCalls:F1} ns/call");
            Console.WriteLine();

            ResetDirectory(sizeDirectory);
            Directory.Delete(sizeDirectory);
        }

        /// <summary>
        /// Times the path the hook takes on a mark that <em>does</em> raise a status,
        /// which before de-omm.22 meant a whole synchronous file rewrite - 6.7-7.4 ms
        /// at this size - on the Unity main thread. It now marks a dirty flag and
        /// returns, so what is measured here is the caller's share and nothing else:
        /// the merge, the flag, and whatever contention the background writer causes
        /// by holding the session lock while it copies the state.
        /// </summary>
        /// <remarks>
        /// <para>The writer is deliberately left running throughout, writing the file
        /// over and over, because a measurement taken with it idle would flatter the
        /// design by leaving out the only cost it added.</para>
        /// <para>This is an average over a tight loop, which is the right shape for "what
        /// does a mark cost on the common path" and the wrong shape for "how bad can one
        /// mark get" - 20,000 calls run in about 3 ms of wall time, so almost none of
        /// them overlap the writer at all. <see cref="MeasureRecordUnderContention"/> is
        /// the one that answers the second question.</para>
        /// </remarks>
        private static void MeasureRaisingRecord(string directory)
        {
            const int raisingCalls = 20_000;
            const int warmupCalls = 1_000;

            // Above every conversation ID BuildState uses, so each call is a new entry
            // and therefore genuinely raises something.
            const int freshConversationId = 1_000_000;

            string sizeDirectory = Path.Combine(directory, "raising");
            ResetDirectory(sizeDirectory);

            var store = new UnifiedStateStore(sizeDirectory);
            UnifiedConversationState state = BuildState(EntryCounts[0]);
            store.Save(state);

            using var session = new UnifiedStateSession(
                store, new EmptySimStatusSource(), NullUnifiedStateLog.Instance);
            session.EnsureInitialized();

            for (int i = 0; i < warmupCalls; i++)
            {
                session.Record(freshConversationId, i, SimStatusNames.WasDisplayed);
            }

            long start = Stopwatch.GetTimestamp();
            for (int i = 0; i < raisingCalls; i++)
            {
                if (!session.Record(freshConversationId, warmupCalls + i, SimStatusNames.WasDisplayed))
                {
                    throw new InvalidOperationException("A raising record reported no change.");
                }
            }

            double elapsed = ToMilliseconds(Stopwatch.GetTimestamp() - start);
            Console.WriteLine("=== Record() on a mark that raises a status (write deferred, de-omm.22) ===");
            Console.WriteLine(
                $"  {raisingCalls:N0} calls in {elapsed:F2} ms "
                + $"= {elapsed * 1000.0 * 1000.0 / raisingCalls:F1} ns/call");

            long flushStart = Stopwatch.GetTimestamp();
            bool flushed = session.Flush();
            Console.WriteLine(
                $"  final Flush(): {ToMilliseconds(Stopwatch.GetTimestamp() - flushStart):F2} ms, "
                + $"succeeded={flushed}, {session.State.EntryCount:N0} entries");
            Console.WriteLine();

            // Safe without stopping the writer first: the flush above left nothing
            // dirty, so nothing is going to recreate these files.
            ResetDirectory(sizeDirectory);
            Directory.Delete(sizeDirectory);
        }

        /// <summary>
        /// The tail of <c>Record</c>'s latency while the background writer is running
        /// (de-0m0.5): not the average, which hides the thing being asked about, but the
        /// worst case and the high percentiles.
        /// </summary>
        /// <remarks>
        /// <para><b>The question.</b> de-omm.22 moved the file write off the caller's
        /// thread but not the serialize: <c>WriterLoop</c> called
        /// <see cref="UnifiedStateJson.SerializeToUtf8Bytes"/> inside the session lock,
        /// and <c>Record</c> takes that same lock, so a mark arriving mid-serialize
        /// waited for it on the Unity main thread. The average cannot see that - the
        /// collision is rare, so it disappears into hundreds of thousands of ~100 ns
        /// calls - which is why this reports percentiles and a maximum instead. That is
        /// how de-0m0.5 found the tail (max 16 ms at 30,000 entries, 39 ms at the
        /// 112,940-entry ceiling, both a dropped frame) and why the writer now copies
        /// under the lock and serializes outside it.</para>
        ///
        /// <para><b>Why the probes are paced.</b> A tight loop of <c>Record</c> calls
        /// runs 20,000 of them in about 2 ms of wall time, during which the writer
        /// completes at most one pass: almost nothing overlaps a serialize, and the tail
        /// is empty for the wrong reason. Pacing at
        /// <see cref="ContentionProbeIntervalMs"/> spreads the same number of calls over
        /// seconds of writer activity, so overlaps happen at their natural rate.</para>
        ///
        /// <para><b>Why most probes are no-ops.</b> <c>Record</c> takes the lock before
        /// it knows whether the mark changes anything, so a no-op mark blocks on the
        /// writer exactly as a raising one does - and de-omm.8 established the no-op is
        /// the overwhelming majority of real calls. Every
        /// <see cref="ContentionRaiseEveryNth"/>th probe raises instead, which is what
        /// keeps the writer with something to write.</para>
        ///
        /// <para>The raising probes come from the state's existing WasOffered entries, so
        /// raising them to WasDisplayed leaves the entry count - and therefore the
        /// serialize cost being measured - unchanged. Only the smallest size has too few
        /// of them, and the run reports how many fresh entries it had to add.</para>
        /// </remarks>
        private static void MeasureRecordUnderContention(string directory, int entryCount)
        {
            string sizeDirectory = Path.Combine(
                directory, "contention-" + entryCount.ToString(CultureInfo.InvariantCulture));
            ResetDirectory(sizeDirectory);

            var store = new UnifiedStateStore(sizeDirectory);
            UnifiedConversationState state = BuildState(entryCount);
            store.Save(state);

            // Timed on a state that is not the session's, so nothing here contends with
            // the writer; these are the references the tail is compared against. The copy
            // is what the writer holds the lock for, so it is the ceiling on how long a
            // Record can be made to wait. The serialize is what it used to hold the lock
            // for (de-omm.22, until de-0m0.5 moved it out), so the gap between the two is
            // what that move bought.
            var serialize = new List<double>();
            var snapshot = new List<double>();
            for (int i = 0; i < WarmupIterations + LockHoldReferenceSamples; i++)
            {
                bool timed = i >= WarmupIterations;

                long start = Stopwatch.GetTimestamp();
                UnifiedConversationState copy = state.Snapshot();
                Add(timed ? snapshot : null, start);

                start = Stopwatch.GetTimestamp();
                byte[] payload = UnifiedStateJson.SerializeToUtf8Bytes(copy);
                Add(timed ? serialize : null, start);
                GC.KeepAlive(payload);
            }

            List<UnifiedStatusEntry> raisable = FindRaisableEntries(state);

            using var session = new UnifiedStateSession(
                store, new EmptySimStatusSource(), NullUnifiedStateLog.Instance);
            session.EnsureInitialized();

            var latencies = new List<double>(ContentionProbeCount);
            int freshEntries = 0;
            int nextRaisable = 0;
            long ticksPerProbe = (long)(ContentionProbeIntervalMs * Stopwatch.Frequency / 1000.0);
            long deadline = Stopwatch.GetTimestamp();

            for (int i = -ContentionWarmupProbes; i < ContentionProbeCount; i++)
            {
                SpinUntil(deadline);
                deadline += ticksPerProbe;

                bool raising = i % ContentionRaiseEveryNth == 0;
                int conversationId = ScratchConversationId;
                int dialogueEntryId = 0;
                string status = SimStatusNames.Untouched;

                if (raising)
                {
                    if (nextRaisable < raisable.Count)
                    {
                        UnifiedStatusEntry entry = raisable[nextRaisable++];
                        conversationId = entry.ConversationId;
                        dialogueEntryId = entry.DialogueEntryId;
                    }
                    else
                    {
                        // Out of entries to raise in place. Adding one grows the state,
                        // and therefore the serialize this is measuring, so it is counted
                        // and reported rather than hidden.
                        dialogueEntryId = ++freshEntries;
                    }

                    status = SimStatusNames.WasDisplayed;
                }

                long start = Stopwatch.GetTimestamp();
                bool changed = session.Record(conversationId, dialogueEntryId, status);
                double elapsed = ToMilliseconds(Stopwatch.GetTimestamp() - start);

                if (raising && !changed)
                {
                    throw new InvalidOperationException(
                        $"A raising probe at {conversationId}/{dialogueEntryId} reported no change.");
                }

                if (i >= 0)
                {
                    latencies.Add(elapsed);
                }
            }

            session.Flush();

            Console.WriteLine(
                $"=== Record() latency with the writer running, {entryCount:N0} entries "
                + $"/ {state.ConversationCount:N0} conversations ===");
            Console.WriteLine(
                $"  {ContentionProbeCount:N0} probes {ContentionProbeIntervalMs:F1} ms apart, "
                + $"1 in {ContentionRaiseEveryNth} raising; ended at {session.State.EntryCount:N0} entries "
                + $"({freshEntries:N0} added because the raisable pool ran out)");
            Report("  under the lock: Snapshot()", snapshot);
            Report("  was under it: serialize", serialize);
            ReportLatencyTail("  Record()", latencies);
            Console.WriteLine();

            // Dispose (via the using) still has to run against a live directory, so the
            // files are cleared after it rather than here.
            session.Dispose();
            ResetDirectory(sizeDirectory);
            Directory.Delete(sizeDirectory);
        }

        /// <summary>
        /// The entries a contention run can raise without changing the entry count: the
        /// WasOffered ones, which merging WasDisplayed over promotes in place.
        /// </summary>
        private static List<UnifiedStatusEntry> FindRaisableEntries(UnifiedConversationState state)
        {
            var raisable = new List<UnifiedStatusEntry>();
            foreach (UnifiedStatusEntry entry in state.EnumerateEntriesInIdOrder())
            {
                if (entry.Status == SimStatus.WasOffered)
                {
                    raisable.Add(entry);
                }
            }

            return raisable;
        }

        /// <summary>
        /// Busy-waits until <paramref name="deadline"/>. A spin rather than a sleep
        /// because the pacing interval is 1 ms and Windows' timer granularity is around
        /// 15 ms, which would turn the intended rate into something else entirely.
        /// </summary>
        private static void SpinUntil(long deadline)
        {
            while (Stopwatch.GetTimestamp() < deadline)
            {
                Thread.SpinWait(20);
            }
        }

        private static void Add(List<double>? samples, long startTimestamp)
        {
            samples?.Add(ToMilliseconds(Stopwatch.GetTimestamp() - startTimestamp));
        }

        private static double ToMilliseconds(long ticks) => ticks * 1000.0 / Stopwatch.Frequency;

        private static void Report(string label, List<double> samples)
        {
            if (samples.Count == 0)
            {
                Console.WriteLine($"{label,-36} (no samples)");
                return;
            }

            List<double> sorted = samples.OrderBy(x => x).ToList();
            double median = sorted[sorted.Count / 2];
            Console.WriteLine(
                $"{label,-36} median {median,9:F3} ms   min {sorted[0],9:F3}   "
                + $"max {sorted[sorted.Count - 1],9:F3}   mean {samples.Average(),9:F3}   n={samples.Count}");
        }

        /// <summary>
        /// Reports a latency distribution by its tail rather than its centre, because
        /// the tail is the question: a mean over calls that mostly cost ~100 ns says
        /// nothing about the rare one that waited on a serialize.
        /// </summary>
        private static void ReportLatencyTail(string label, List<double> samples)
        {
            if (samples.Count == 0)
            {
                Console.WriteLine($"{label,-36} (no samples)");
                return;
            }

            List<double> sorted = samples.OrderBy(x => x).ToList();
            int overThreshold = samples.Count(x => x >= BlockedThresholdMs);
            int overFrame = samples.Count(x => x >= FrameBudgetMs);

            Console.WriteLine(
                $"{label,-36} mean {samples.Average() * 1000.0,8:F1} us   "
                + $"p50 {Percentile(sorted, 0.50) * 1000.0,8:F1}   "
                + $"p90 {Percentile(sorted, 0.90) * 1000.0,8:F1}   "
                + $"p99 {Percentile(sorted, 0.99) * 1000.0,8:F1}   n={samples.Count}");
            Console.WriteLine(
                $"{string.Empty,-36} p99.9 {Percentile(sorted, 0.999) * 1000.0,6:F1} us   "
                + $"MAX {sorted[sorted.Count - 1] * 1000.0,8:F1} us "
                + $"({sorted[sorted.Count - 1]:F2} ms)");
            Console.WriteLine(
                $"{string.Empty,-36} over {BlockedThresholdMs:F1} ms: {overThreshold} "
                + $"({100.0 * overThreshold / samples.Count:F2}%)   "
                + $"over one {FrameBudgetMs:F1} ms frame: {overFrame} "
                + $"({100.0 * overFrame / samples.Count:F2}%)");
        }

        /// <summary>
        /// Nearest-rank percentile of an already-sorted sample list. Nearest-rank rather
        /// than interpolated so every value reported is a value that actually happened.
        /// </summary>
        private static double Percentile(List<double> sorted, double fraction)
        {
            int rank = (int)Math.Ceiling(fraction * sorted.Count) - 1;
            return sorted[Math.Clamp(rank, 0, sorted.Count - 1)];
        }

        private static UnifiedConversationState BuildState(int entryCount)
        {
            var state = new UnifiedConversationState();
            for (int i = 0; i < entryCount; i++)
            {
                int conversationId = i / EntriesPerConversation;
                int dialogueEntryId = i % EntriesPerConversation;
                SimStatus status = i % WasOfferedEveryNth == 0 ? SimStatus.WasOffered : SimStatus.WasDisplayed;
                state.Merge(conversationId, dialogueEntryId, status);
            }

            if (state.EntryCount != entryCount)
            {
                throw new InvalidOperationException(
                    $"Built {state.EntryCount} entries, expected {entryCount}.");
            }

            return state;
        }

        private static void ResetDirectory(string directory)
        {
            if (!Directory.Exists(directory))
            {
                Directory.CreateDirectory(directory);
                return;
            }

            foreach (string file in Directory.GetFiles(directory))
            {
                File.Delete(file);
            }
        }

        /// <summary>
        /// A source with nothing in it. The no-op measurement supplies its own state
        /// through the file, so reading a game would only add noise.
        /// </summary>
        private sealed class EmptySimStatusSource : ISimStatusSource
        {
            public string Description => "benchmark stub";

            public bool IsReady => true;

            public IEnumerable<SimStatusRow> EnumerateSimStatuses() => Array.Empty<SimStatusRow>();
        }
    }
}
