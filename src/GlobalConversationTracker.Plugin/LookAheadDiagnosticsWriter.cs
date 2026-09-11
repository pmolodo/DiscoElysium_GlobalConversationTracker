// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using System.Text.Json;
using GlobalConversationTracker.Core;
using GlobalConversationTracker.Engine;
using GlobalConversationTracker.Session;

namespace GlobalConversationTracker
{
    /// <summary>
    /// Writes the look-ahead's two diagnostic files, beside the global state they sit
    /// alongside in the SaveGames directory.
    /// </summary>
    /// <remarks>
    /// <para>The SaveGames directory rather than next to the BepInEx log, for three
    /// reasons: the mod already resolves and writes there, the files are about the
    /// player's own save rather than about loading the plugin, and a BepInEx log is
    /// truncated per run while these want to accumulate.</para>
    ///
    /// <para>Both are strictly optional and off unless configured. A failure to write
    /// one must never cost the marker it is describing, so every entry point swallows
    /// its own IO errors after reporting them once.</para>
    /// </remarks>
    internal sealed class LookAheadDiagnosticsWriter
    {
        /// <summary>The append-only record of crawls that ran out of budget.</summary>
        internal const string OverflowLogName = "look-ahead-budget-overflows.log";

        /// <summary>The rewritten-in-place summary of what crawls have cost.</summary>
        internal const string StatisticsFileName = "look-ahead-stats.json";

        /// <summary>The version of that summary's shape this build writes.</summary>
        /// <remarks>
        /// The overflow log has no version and wants none: it is append-only prose, read
        /// by a person and by one substring search, and nothing about it can be
        /// half-understood. The statistics file is parsed.
        /// </remarks>
        internal const int FormatVersion = 1;

        /// <summary>
        /// How often the statistics file is rewritten, in crawls. Every menu draws
        /// several, and the file is a summary rather than a journal, so writing it on
        /// every one would be pure IO for no extra information.
        /// </summary>
        private const int StatisticsWriteInterval = 200;

        private readonly IGlobalStateLog _log;
        private readonly LookAheadStatistics _statistics = new LookAheadStatistics();
        private long _sinceLastWrite;
        private bool _overflowLogFailed;
        private bool _statisticsFailed;

        /// <summary>Creates a writer.</summary>
        /// <param name="directoryPath">The SaveGames directory.</param>
        /// <param name="log">Where write failures are reported.</param>
        /// <param name="logOverflows">Whether to record budget overflows.</param>
        /// <param name="keepStatistics">Whether to accumulate and write statistics.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        internal LookAheadDiagnosticsWriter(
            string directoryPath, IGlobalStateLog log, bool logOverflows, bool keepStatistics)
        {
            if (string.IsNullOrWhiteSpace(directoryPath))
            {
                throw new ArgumentNullException(nameof(directoryPath));
            }

            _log = log ?? throw new ArgumentNullException(nameof(log));
            LogOverflows = logOverflows;
            KeepStatistics = keepStatistics;
            OverflowLogPath = Path.Combine(directoryPath, OverflowLogName);
            StatisticsPath = Path.Combine(directoryPath, StatisticsFileName);
        }

        /// <summary>Whether budget overflows are being recorded.</summary>
        internal bool LogOverflows { get; }

        /// <summary>Whether statistics are being accumulated.</summary>
        internal bool KeepStatistics { get; }

        /// <summary>Where overflows are appended.</summary>
        internal string OverflowLogPath { get; }

        /// <summary>Where the statistics summary is written.</summary>
        internal string StatisticsPath { get; }

        /// <summary>Whether anything at all needs recording.</summary>
        internal bool Enabled => LogOverflows || KeepStatistics;


        /// <summary>Records one finished crawl.</summary>
        /// <param name="answer">
        /// What the bridge said about it: where it started, what it found, how big it got
        /// and how long it took.
        /// </param>
        /// <param name="memoryBudgetMb">The memory budget it was given, in megabytes.</param>
        /// <param name="groupEntryCount">
        /// How many entries the group has, so a reader can see how much of it was reached.
        /// </param>
        /// <param name="world">The world it crawled from.</param>
        internal void Record(
            LookAheadAnswer answer,
            int memoryBudgetMb,
            int groupEntryCount,
            WorldSnapshot world)
        {
            if (KeepStatistics)
            {
                _statistics.Record(answer.Start, answer, answer.ElapsedMs);
                if (++_sinceLastWrite >= StatisticsWriteInterval)
                {
                    _sinceLastWrite = 0;
                    WriteStatistics();
                }
            }

            if (LogOverflows && !answer.Complete)
            {
                AppendOverflow(answer, memoryBudgetMb, groupEntryCount, world);
            }
        }

        /// <summary>Writes the statistics out now, whatever the interval says.</summary>
        /// <remarks>Called at shutdown, so a session's last crawls are not lost.</remarks>
        internal void Flush()
        {
            if (KeepStatistics && _statistics.Crawls > 0)
            {
                WriteStatistics();
            }
        }

        private void AppendOverflow(
            LookAheadAnswer answer, int memoryBudgetMb, int groupEntryCount, WorldSnapshot world)
        {
            if (_overflowLogFailed)
            {
                return;
            }

            try
            {
                File.AppendAllText(
                    OverflowLogPath,
                    DescribeOverflow(answer, memoryBudgetMb, groupEntryCount, world));
            }
            catch (Exception ex)
            {
                _overflowLogFailed = true;
                _log.Warning(
                    $"Could not append to '{OverflowLogPath}' ({ex.Message}). Budget overflows "
                    + "will not be recorded for the rest of this session; the marker itself is "
                    + "unaffected.");
            }
        }

        /// <summary>
        /// Renders one overflow. Plain text and one block per event, because this is read
        /// by a person hunting a slow menu, not by a program.
        /// </summary>
        /// <remarks>
        /// <para>WHAT THIS USED TO SAY AND NO LONGER CAN: the entries reached in the most
        /// distinct states, and how many state slots the group tracked. Both came from a
        /// second traced walk of the managed engine, which does not exist any more
        /// (de-i5xj.6) - the crawl runs on the other side of the bridge now, and neither
        /// the per-entry tally nor the symbol table crosses it.</para>
        ///
        /// <para>What is left is most of what the report was for. WHICH option blew up,
        /// HOW BIG it got before it did, WHICH limit stopped it, and WHAT WORLD it was
        /// crawling from - the last of those from the snapshot this plugin sent, which is
        /// the same world the crawl saw. A reader hunting a slow menu wants the option and
        /// the size; the tally was the refinement.</para>
        /// </remarks>
        private static string DescribeOverflow(
            LookAheadAnswer answer, int memoryBudgetMb, int groupEntryCount, WorldSnapshot world)
        {
            var text = new StringBuilder();
            text.Append("=== ")
                .Append(DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss", CultureInfo.InvariantCulture))
                .AppendLine(" budget exhausted ===");

            text.Append("  option         ").Append(answer.Start.Conversation)
                .Append(':').Append(answer.Start.Entry).AppendLine();
            text.Append("  stopped by     ")
                .AppendLine(answer.StoppedBy.Length == 0 ? "(not said)" : answer.StoppedBy);
            text.Append("  memory budget  ").Append(memoryBudgetMb).AppendLine(" MB");
            text.Append("  states         ").Append(answer.StatesExplored).AppendLine();
            text.Append("  entries        ").Append(answer.NodesReached)
                .Append(" of ").Append(groupEntryCount).AppendLine(" in the group");
            text.Append("  elapsed        ")
                .Append(answer.ElapsedMs.ToString(CultureInfo.InvariantCulture))
                .AppendLine(" ms");
            text.Append("  best found     ").Append((Novelty)answer.Best).AppendLine();
            text.Append("  money          ").Append(world.Money).AppendLine();
            text.Append("  clock          ")
                .Append((world.DayMinutes / 60).ToString("00", CultureInfo.InvariantCulture))
                .Append(':')
                .Append((world.DayMinutes % 60).ToString("00", CultureInfo.InvariantCulture))
                .Append(" on day ").Append(world.DayCounter)
                .AppendLine(world.ClockLocked ? " (locked)" : string.Empty);

            text.AppendLine();
            return text.ToString();
        }

        private void WriteStatistics()
        {
            if (_statisticsFailed)
            {
                return;
            }

            try
            {
                File.WriteAllText(StatisticsPath, RenderStatistics(_statistics));
            }
            catch (Exception ex)
            {
                _statisticsFailed = true;
                _log.Warning(
                    $"Could not write '{StatisticsPath}' ({ex.Message}). Look-ahead statistics "
                    + "will not be kept for the rest of this session; the marker itself is "
                    + "unaffected.");
            }
        }

        /// <summary>
        /// Renders the statistics as JSON. Indented, because a person opens this file.
        /// </summary>
        internal static string RenderStatistics(LookAheadStatistics statistics)
        {
            if (statistics == null)
            {
                throw new ArgumentNullException(nameof(statistics));
            }

            using var buffer = new MemoryStream();
            using (var writer = new Utf8JsonWriter(buffer, new JsonWriterOptions { Indented = true }))
            {
                writer.WriteStartObject();

                // IN SCOPE BECAUSE IT IS READ BACK BY CODE, not only by a person: the
                // in-game suites parse this file and assert on it (see the artefact checks
                // in tools/GameHarness), so a shape change here fails a suite in terms of
                // whatever the parse happened to produce rather than in terms of the
                // version. A log nothing reads would not need this.
                writer.WriteNumber(FormatStamp.VersionPropertyName, FormatVersion);
                writer.WriteString(
                    "written",
                    DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss", CultureInfo.InvariantCulture));
                writer.WriteNumber("crawls", statistics.Crawls);
                writer.WriteNumber("budgetExhausted", statistics.BudgetExhausted);
                writer.WriteNumber("timeExhausted", statistics.TimeExhausted);

                writer.WriteStartObject("states");
                writer.WriteNumber("total", statistics.TotalStates);
                writer.WriteNumber("min", statistics.MinStatesOrZero);
                writer.WriteNumber("mean", Round(statistics.MeanStates));
                writer.WriteNumber("max", statistics.MaxStates);
                writer.WriteEndObject();

                writer.WriteStartObject("entriesReached");
                writer.WriteNumber("total", statistics.TotalNodes);
                writer.WriteNumber("max", statistics.MaxNodes);
                writer.WriteEndObject();

                writer.WriteStartObject("milliseconds");
                writer.WriteNumber("total", Round(statistics.TotalMilliseconds));
                writer.WriteNumber("mean", Round(statistics.MeanMilliseconds));
                writer.WriteNumber("max", Round(statistics.MaxMilliseconds));
                writer.WriteEndObject();

                writer.WriteStartObject("found");
                writer.WriteNumber("nothing", statistics.FoundNothing);
                writer.WriteNumber("unseenThisGame", statistics.FoundUnseenThisGame);
                writer.WriteNumber("unseenAnyGame", statistics.FoundUnseenAnyGame);
                writer.WriteEndObject();

                // The histogram, not percentiles: a mean hides the one menu in a thousand
                // that costs a hundred times the rest, and that is the one worth finding.
                writer.WriteStartObject("statesHistogram");
                for (int i = 0; i < statistics.Buckets.Count; i++)
                {
                    writer.WriteNumber(
                        LookAheadStatistics.BucketLabel(i), statistics.Buckets[i]);
                }

                writer.WriteEndObject();

                writer.WriteStartArray("byConversation");
                foreach (KeyValuePair<int, ConversationStatistics> pair in Sorted(statistics))
                {
                    writer.WriteStartObject();
                    writer.WriteNumber("conversation", pair.Key);
                    writer.WriteNumber("crawls", pair.Value.Crawls);
                    writer.WriteNumber("meanStates", Round(pair.Value.MeanStates));
                    writer.WriteNumber("maxStates", pair.Value.MaxStates);
                    writer.WriteNumber("maxMs", Round(pair.Value.MaxMilliseconds));
                    writer.WriteNumber("budgetExhausted", pair.Value.BudgetExhausted);
                    writer.WriteNumber("timeExhausted", pair.Value.TimeExhausted);
                    writer.WriteEndObject();
                }

                writer.WriteEndArray();
                writer.WriteEndObject();
            }

            return Encoding.UTF8.GetString(buffer.ToArray());
        }

        /// <summary>
        /// Conversations worst first, so the file opens on whatever is costing the most.
        /// </summary>
        private static List<KeyValuePair<int, ConversationStatistics>> Sorted(
            LookAheadStatistics statistics)
        {
            var rows = new List<KeyValuePair<int, ConversationStatistics>>(
                statistics.ByConversation);
            rows.Sort((left, right) => right.Value.MaxStates.CompareTo(left.Value.MaxStates));
            return rows;
        }

        private static double Round(double value)
        {
            return Math.Round(value, 2, MidpointRounding.AwayFromZero);
        }
    }
}
