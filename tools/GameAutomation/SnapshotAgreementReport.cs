// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text.RegularExpressions;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// What the plugin found when it compared its two look-ahead worlds, read back out of
    /// the BepInEx log.
    /// </summary>
    /// <remarks>
    /// <para>The harness half of de-i5xj.7. The mod's managed world is the one whose
    /// answers reach a player today; the snapshot is the one that will, once the look-ahead
    /// runs on the Rust engine. Asked to, the plugin puts the same questions to both and
    /// writes what it found. This reads that, so a run can assert on it.</para>
    ///
    /// <para>The line reads, when they agree:</para>
    /// <code>
    /// Snapshot agreement: conversation 631: money and clock agree, 306 variables
    /// (0 differ), 8 CheckItem (0 differ), ..., 13 queries (13 answered)
    /// </code>
    /// <para>and carries a <c>FIRST DIFFERENCES:</c> tail when they do not.</para>
    /// </remarks>
    public sealed class SnapshotAgreementReport
    {
        /// <summary>
        /// What the plugin prefixes these lines with.
        /// </summary>
        /// <remarks>
        /// Must match <c>SnapshotAgreementCheck.LogPrefix</c>. The two cannot share a
        /// constant, for the reason <see cref="NativeEngineReport.LogPrefix"/> gives.
        /// </remarks>
        public const string LogPrefix = "Snapshot agreement:";

        /// <summary>What the plugin writes before naming the differences it found.</summary>
        private const string DifferencesMarker = "FIRST DIFFERENCES:";

        private static readonly Regex CountPattern = new Regex(
            @"(?<asked>\d+) (?<what>[A-Za-z]+) \((?<differ>\d+) differ\)",
            RegexOptions.Compiled);

        private static readonly Regex AnsweredPattern = new Regex(
            @"(?<asked>\d+) queries \((?<answered>\d+) answered\)",
            RegexOptions.Compiled);

        private SnapshotAgreementReport(
            bool reported,
            IReadOnlyDictionary<string, (int Asked, int Differ)> counts,
            int queriesAsked,
            int queriesAnswered,
            string? differences,
            string? line)
        {
            Reported = reported;
            Counts = counts;
            QueriesAsked = queriesAsked;
            QueriesAnswered = queriesAnswered;
            Differences = differences;
            Line = line;
        }

        /// <summary>Whether the comparison ran at all.</summary>
        public bool Reported { get; }

        /// <summary>
        /// How many of each kind of question were asked, and how many disagreed.
        /// </summary>
        /// <remarks>
        /// Keyed by what the plugin called them - <c>variables</c>, <c>checks</c>,
        /// <c>entries</c>, and the query name for each membership set.
        /// </remarks>
        public IReadOnlyDictionary<string, (int Asked, int Differ)> Counts { get; }

        /// <summary>How many world queries the group asks, or -1 if it did not report.</summary>
        public int QueriesAsked { get; }

        /// <summary>
        /// How many of them the game actually answered.
        /// </summary>
        /// <remarks>
        /// The one thing the comparison cannot check name for name, and the one worth
        /// watching: the engine hands out a RENDERED call as each query's key and the
        /// plugin runs it as Lua. If the two ever stopped agreeing about that rendering,
        /// every query in the group would answer Unknown, silently, and every guard over
        /// one would turn permissive. Zero answered out of thirteen would say so.
        /// </remarks>
        public int QueriesAnswered { get; }

        /// <summary>The differences the plugin named, or null if it found none.</summary>
        public string? Differences { get; }

        /// <summary>The line itself, for a failure message worth reading.</summary>
        public string? Line { get; }

        /// <summary>Whether the two worlds gave the same answers to everything comparable.</summary>
        public bool Agreed => Reported && Differences == null;

        /// <summary>Reads the report out of a BepInEx log.</summary>
        /// <param name="logPath">The log file.</param>
        public static SnapshotAgreementReport FromLog(string logPath)
        {
            if (!File.Exists(logPath))
            {
                return NotReported();
            }

            // Shared read-write-delete: the game still has this open, and on Windows an
            // exclusive open would simply fail while it runs.
            using var stream = new FileStream(
                logPath, FileMode.Open, FileAccess.Read,
                FileShare.ReadWrite | FileShare.Delete);
            using var reader = new StreamReader(stream);
            return FromText(reader.ReadToEnd());
        }

        /// <summary>The same, over log text already in hand.</summary>
        /// <remarks>
        /// Split out so the parsing can be tested without a game, a log file or a launch,
        /// which is the whole of what can be checked before somebody runs the real thing.
        /// </remarks>
        /// <param name="text">The log's contents.</param>
        public static SnapshotAgreementReport FromText(string text)
        {
            if (text == null)
            {
                throw new ArgumentNullException(nameof(text));
            }

            string? line = null;
            foreach (string candidate in text.Split('\n'))
            {
                if (candidate.Contains(LogPrefix))
                {
                    // The LAST one, not the first: a run may compare more than one group,
                    // and a harness asking about the newest wants the newest.
                    line = candidate.Trim();
                }
            }

            if (line == null)
            {
                return NotReported();
            }

            int marker = line.IndexOf(DifferencesMarker, StringComparison.Ordinal);
            string? differences = marker < 0
                ? null
                : line.Substring(marker + DifferencesMarker.Length).Trim();

            // Only the summary, so a difference that happens to read like a count - an
            // entry id, a variable's value - cannot be mistaken for one.
            string summary = marker < 0 ? line : line.Substring(0, marker);

            var counts = new Dictionary<string, (int Asked, int Differ)>(
                StringComparer.Ordinal);
            foreach (Match match in CountPattern.Matches(summary))
            {
                counts[match.Groups["what"].Value] = (
                    Number(match.Groups["asked"].Value),
                    Number(match.Groups["differ"].Value));
            }

            Match answered = AnsweredPattern.Match(summary);
            return new SnapshotAgreementReport(
                true,
                counts,
                answered.Success ? Number(answered.Groups["asked"].Value) : -1,
                answered.Success ? Number(answered.Groups["answered"].Value) : -1,
                differences,
                line);
        }

        private static SnapshotAgreementReport NotReported()
        {
            return new SnapshotAgreementReport(
                false,
                new Dictionary<string, (int Asked, int Differ)>(StringComparer.Ordinal),
                -1,
                -1,
                null,
                null);
        }

        private static int Number(string text)
        {
            return int.TryParse(
                text, NumberStyles.None, CultureInfo.InvariantCulture, out int parsed)
                ? parsed
                : -1;
        }

        /// <summary>A one-line description, for a test's failure message.</summary>
        public override string ToString()
        {
            if (!Reported)
            {
                return "the plugin said nothing about the look-ahead snapshot";
            }

            return Agreed
                ? $"the two worlds agreed: {Line}"
                : $"the two worlds DISAGREED: {Line}";
        }
    }
}
