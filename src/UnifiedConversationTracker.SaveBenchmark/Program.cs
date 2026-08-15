using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using UnifiedConversationTracker;
using UnifiedConversationTracker.Persistence;
using UnifiedConversationTracker.Session;

namespace UnifiedConversationTracker.SaveBenchmark
{
    /// <summary>
    /// Times one whole-file rewrite of the unified state, broken out by phase, at
    /// a sweep of entry counts. Investigatory tooling for de-omm.11; it answers
    /// "how expensive is <see cref="UnifiedStateStore.Save"/>" and nothing else.
    /// </summary>
    /// <remarks>
    /// The phase breakdown is a hand-rolled replay of <see cref="UnifiedStateStore.Save"/>
    /// using the store's own public paths, because Save itself is one opaque call.
    /// Every run also times the real Save end to end and prints both, so a drift
    /// between the replay and the real thing is visible rather than assumed away.
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
            Console.WriteLine($"Iterations          : {Iterations} timed, {WarmupIterations} warmup");
            Console.WriteLine();

            foreach (int entryCount in EntryCounts)
            {
                MeasureSize(fullDirectory, entryCount);
            }

            MeasureNoOpRecord(fullDirectory);
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
        /// A source that never seeds. The no-op measurement supplies its own state
        /// through the file, so seeding would only add noise.
        /// </summary>
        private sealed class EmptySimStatusSource : ISimStatusSource
        {
            public string Description => "benchmark stub";

            public bool IsReady => true;

            public IEnumerable<SimStatusRow> EnumerateSimStatuses() => Array.Empty<SimStatusRow>();
        }
    }
}
