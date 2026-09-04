// SPDX-License-Identifier: MIT
using System;
using System.Globalization;
using System.IO;
using System.Text.RegularExpressions;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Whether the two look-ahead engines agreed, read back out of the BepInEx log.
    /// </summary>
    /// <remarks>
    /// <para>The look-ahead is moving from C# to Rust and both engines run for now, with
    /// the managed one's answer drawn. The plugin writes a summary of what they said; this
    /// reads it, so a run can assert on agreement instead of somebody scrolling a log.</para>
    ///
    /// <para>The COUNT is as important as the disagreements. A comparison that reports
    /// nothing wrong because it never ran looks identical to one that ran and found nothing,
    /// and only one of those is evidence.</para>
    /// </remarks>
    public sealed class BridgeComparisonReport
    {
        /// <summary>
        /// What the plugin prefixes these lines with.
        /// </summary>
        /// <remarks>
        /// Must match <c>BridgeComparison.LogPrefix</c>, for the reason
        /// <see cref="NativeEngineReport.LogPrefix"/> gives.
        /// </remarks>
        public const string LogPrefix = "Look-ahead bridge:";

        private static readonly Regex SummaryPattern = new Regex(
            @"Look-ahead bridge: (?<compared>[\d,]+) options compared over (?<menus>[\d,]+) "
            + @"menus, (?<disagreed>[\d,]+) disagreed, (?<incomplete>[\d,]+) answered from a "
            + @"cut-short search, (?<unanswered>[\d,]+) the bridge could not answer, "
            + @"(?<notcrawled>[\d,]+) the managed engine answered without crawling\.",
            RegexOptions.Compiled);

        private static readonly Regex TimingPattern = new Regex(
            @"(?<bridge>[\d.,]+) ms per menu across the bridge against (?<managed>[\d.,]+) ms "
            + @"per option in the managed engine",
            RegexOptions.Compiled);

        private BridgeComparisonReport(
            bool reported,
            int compared,
            int menus,
            int disagreed,
            int incomplete,
            int unanswered,
            int notCrawled,
            double bridgeMillisecondsPerMenu,
            double managedMillisecondsPerOption,
            string? line)
        {
            Reported = reported;
            Compared = compared;
            Menus = menus;
            Disagreed = disagreed;
            Incomplete = incomplete;
            Unanswered = unanswered;
            NotCrawled = notCrawled;
            BridgeMillisecondsPerMenu = bridgeMillisecondsPerMenu;
            ManagedMillisecondsPerOption = managedMillisecondsPerOption;
            Line = line;
        }

        /// <summary>Whether the plugin wrote a summary at all.</summary>
        public bool Reported { get; }

        /// <summary>How many options both engines answered.</summary>
        public int Compared { get; }

        /// <summary>How many response menus the bridge was asked about.</summary>
        public int Menus { get; }

        /// <summary>How many of the compared options they disagreed about.</summary>
        public int Disagreed { get; }

        /// <summary>How many bridge answers came from a search that ran out.</summary>
        /// <remarks>
        /// Not a disagreement: such an answer is a lower bound rather than a different
        /// opinion. It is what de-pvq is about being able to show.
        /// </remarks>
        public int Incomplete { get; }

        /// <summary>How many options the bridge had nothing to say about.</summary>
        public int Unanswered { get; }

        /// <summary>
        /// How many options the managed engine answered without crawling.
        /// </summary>
        /// <remarks>
        /// Skipped by the comparison on purpose - the managed engine produces no
        /// best-reachable figure in that case, only the knowledge that nothing can beat what
        /// the option already shows, so comparing numbers there would mean nothing.
        /// </remarks>
        public int NotCrawled { get; }

        /// <summary>
        /// What one bridge call cost, per menu, including everything around it.
        /// </summary>
        /// <remarks>
        /// The number the whole migration has to justify. A bridge that answers in two
        /// milliseconds and spends ten marshalling has not helped, and nothing measured that
        /// until this line existed.
        /// </remarks>
        public double BridgeMillisecondsPerMenu { get; }

        /// <summary>What one managed crawl cost, per option.</summary>
        public double ManagedMillisecondsPerOption { get; }

        /// <summary>The summary line, for a failure message worth reading.</summary>
        public string? Line { get; }

        /// <summary>Whether both engines ran and said the same thing throughout.</summary>
        public bool Agreed => Reported && Compared > 0 && Disagreed == 0;

        /// <summary>
        /// How many summaries a log holds, so a caller can tell a later one from an earlier.
        /// </summary>
        /// <param name="logPath">The log file.</param>
        public static int CountIn(string logPath)
        {
            return File.Exists(logPath) ? SummaryPattern.Matches(Read(logPath)).Count : 0;
        }

        /// <summary>Reads the report out of a BepInEx log.</summary>
        /// <param name="logPath">The log file.</param>
        /// <param name="alreadySeen">
        /// How many summaries the log already held before the thing being asked about
        /// started. Anything at or below this belongs to something earlier, and reporting it
        /// would answer a question about the wrong suite.
        /// </param>
        /// <remarks>
        /// The count matters because the plugin writes a summary only when something ran.
        /// A suite with the look-ahead switched off writes none - and without this, the
        /// previous suite's summary was read as that one's, so a suite that compared nothing
        /// inherited another's verdict.
        /// </remarks>
        public static BridgeComparisonReport FromLog(string logPath, int alreadySeen = 0)
        {
            if (!File.Exists(logPath))
            {
                return NotReported();
            }

            return FromText(Read(logPath), alreadySeen);
        }

        /// <summary>The log's text, read while the game still has it open.</summary>
        private static string Read(string logPath)
        {

            // Shared read-write-delete: the game still has this open, and on Windows an
            // exclusive open would simply fail while it runs.
            using var stream = new FileStream(
                logPath, FileMode.Open, FileAccess.Read,
                FileShare.ReadWrite | FileShare.Delete);
            using var reader = new StreamReader(stream);
            return reader.ReadToEnd();
        }

        /// <summary>The same, over log text already in hand.</summary>
        /// <param name="text">The log's contents.</param>
        /// <param name="alreadySeen">How many summaries belong to something earlier.</param>
        public static BridgeComparisonReport FromText(string text, int alreadySeen = 0)
        {
            if (text == null)
            {
                throw new ArgumentNullException(nameof(text));
            }

            // The last summary that is NEW. A suite run writes one per suite, and a suite
            // that ran no crawls writes none - so taking the last one in the file would
            // report the previous suite's verdict as this one's.
            Match summary = Match.Empty;
            int seen = 0;
            foreach (Match candidate in SummaryPattern.Matches(text))
            {
                seen++;
                if (seen > alreadySeen)
                {
                    summary = candidate;
                }
            }

            if (!summary.Success)
            {
                return NotReported();
            }

            Match timing = TimingPattern.Match(summary.Value.Length < text.Length
                ? text.Substring(summary.Index)
                : text);

            return new BridgeComparisonReport(
                true,
                Number(summary.Groups["compared"].Value),
                Number(summary.Groups["menus"].Value),
                Number(summary.Groups["disagreed"].Value),
                Number(summary.Groups["incomplete"].Value),
                Number(summary.Groups["unanswered"].Value),
                Number(summary.Groups["notcrawled"].Value),
                timing.Success ? Real(timing.Groups["bridge"].Value) : -1,
                timing.Success ? Real(timing.Groups["managed"].Value) : -1,
                LineAround(text, summary.Index));
        }

        private static BridgeComparisonReport NotReported()
        {
            return new BridgeComparisonReport(false, -1, -1, -1, -1, -1, -1, -1, -1, null);
        }

        /// <summary>The whole log line a match fell inside.</summary>
        private static string LineAround(string text, int index)
        {
            int start = text.LastIndexOf('\n', Math.Min(index, text.Length - 1)) + 1;
            int end = text.IndexOf('\n', index);
            return (end < 0 ? text.Substring(start) : text.Substring(start, end - start)).Trim();
        }

        private static int Number(string text)
        {
            return int.TryParse(
                text,
                NumberStyles.Integer | NumberStyles.AllowThousands,
                CultureInfo.InvariantCulture,
                out int parsed)
                ? parsed
                : -1;
        }

        private static double Real(string text)
        {
            return double.TryParse(
                text,
                NumberStyles.Float | NumberStyles.AllowThousands,
                CultureInfo.InvariantCulture,
                out double parsed)
                ? parsed
                : -1;
        }

        /// <summary>A one-line description, for a test's failure message.</summary>
        public override string ToString()
        {
            if (!Reported)
            {
                return "the plugin said nothing about the two look-ahead engines";
            }

            string timing = BridgeMillisecondsPerMenu >= 0
                ? $", {BridgeMillisecondsPerMenu:N1} ms per menu across the bridge against "
                    + $"{ManagedMillisecondsPerOption:N1} ms per managed crawl"
                : ", no timings";

            return $"{Compared} options compared over {Menus} menus, {Disagreed} disagreed"
                + $"{timing}";
        }
    }
}
