// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using System.Text.Json;
using GlobalConversationTracker.LookAhead;
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

        /// <summary>
        /// Whether a crawl that overflowed should be walked a second time to find out
        /// why.
        /// </summary>
        /// <remarks>
        /// The tally that explains an overflow costs a dictionary write per state, in the
        /// loop that decides what the feature costs. Rather than make every crawl pay for
        /// a report almost none of them will produce, the normal walk keeps no tally and
        /// an overflow is reproduced by walking again with one. The second walk sees the
        /// same answer because the world it reads is a snapshot taken before the first.
        /// </remarks>
        internal bool RetriesOverflowsWithTrace => LogOverflows;

        /// <summary>Records one finished crawl.</summary>
        /// <param name="start">The option it began at.</param>
        /// <param name="result">What it found.</param>
        /// <param name="milliseconds">How long it took.</param>
        /// <param name="budget">The state budget it was given.</param>
        /// <param name="trace">
        /// Why it overflowed, from a second traced walk, or null if none was made.
        /// </param>
        internal void Record(
            DialogueNodeId start,
            LookAheadResult result,
            double milliseconds,
            int budget,
            LookAheadTrace? trace = null)
        {
            if (result == null)
            {
                return;
            }

            if (KeepStatistics)
            {
                _statistics.Record(start, result, milliseconds);
                if (++_sinceLastWrite >= StatisticsWriteInterval)
                {
                    _sinceLastWrite = 0;
                    WriteStatistics();
                }
            }

            if (LogOverflows && result.BudgetExhausted)
            {
                AppendOverflow(result, milliseconds, budget, trace);
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
            LookAheadResult result, double milliseconds, int budget, LookAheadTrace? trace)
        {
            if (_overflowLogFailed)
            {
                return;
            }

            try
            {
                File.AppendAllText(
                    OverflowLogPath, DescribeOverflow(result, milliseconds, budget, trace));
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
        private static string DescribeOverflow(
            LookAheadResult result, double milliseconds, int budget, LookAheadTrace? trace)
        {
            var text = new StringBuilder();
            text.Append("=== ")
                .Append(DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss", CultureInfo.InvariantCulture))
                .AppendLine(" budget exhausted ===");

            if (trace == null)
            {
                text.AppendLine("  (the traced re-walk produced nothing)");
                text.AppendLine();
                return text.ToString();
            }

            text.Append("  option         ").Append(trace.Start.ConversationId)
                .Append(':').Append(trace.Start.EntryId).AppendLine();
            text.Append("  budget         ").Append(budget).AppendLine();
            text.Append("  states         ").Append(result.StatesExplored).AppendLine();
            text.Append("  entries        ").Append(result.NodesReached)
                .Append(" of ").Append(trace.GraphNodeCount).AppendLine(" in the group");
            text.Append("  state slots    ").Append(trace.TrackedSlots).AppendLine();
            text.Append("  elapsed        ")
                .Append(milliseconds.ToString("F1", CultureInfo.InvariantCulture))
                .AppendLine(" ms (the untraced walk; the re-walk is not counted)");
            text.Append("  best found     ").Append(result.Best).AppendLine();
            text.Append("  money          ").Append(trace.Money).AppendLine();
            text.Append("  clock          ")
                .Append(ClockTime.HoursOf(trace.DayMinutes).ToString("00", CultureInfo.InvariantCulture))
                .Append(':')
                .Append((trace.DayMinutes % 60).ToString("00", CultureInfo.InvariantCulture))
                .Append(" on day ").Append(trace.DayCounter)
                .AppendLine(trace.ClockLocked ? " (locked)" : string.Empty);

            text.AppendLine("  entries reached in the most distinct states:");
            foreach (NodeStateCount hot in trace.HottestNodes)
            {
                text.Append("    ").Append(hot.Node.ConversationId).Append(':')
                    .Append(hot.Node.EntryId).Append("  ").Append(hot.States)
                    .AppendLine(" states");
            }

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
