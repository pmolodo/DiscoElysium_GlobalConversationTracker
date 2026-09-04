// SPDX-License-Identifier: MIT
using System;
using System.Globalization;
using System.IO;
using System.Text.RegularExpressions;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// What the plugin said about the index it opened, read back out of the BepInEx log.
    /// </summary>
    /// <remarks>
    /// <para>The index the mod ships is a cache of the dialogue database, checked per group
    /// on first use. Two things about that can only be learned from a running game: whether
    /// the check passes on an unmodified install - it must, or every launch would rebuild -
    /// and what it COSTS, since the case for checking synchronously rests on a walk of the
    /// live object graph being cheap and the estimate behind that was of the data rather
    /// than of the walk.</para>
    ///
    /// <para>Both are in one line. This reads it.</para>
    /// </remarks>
    public sealed class IndexCacheReport
    {
        /// <summary>
        /// What the plugin prefixes these lines with.
        /// </summary>
        /// <remarks>
        /// Must match <c>LookAheadIndex.LogPrefix</c>, for the reason
        /// <see cref="NativeEngineReport.LogPrefix"/> gives.
        /// </remarks>
        public const string LogPrefix = "Look-ahead index:";

        private static readonly Regex OpenedPattern = new Regex(
            @"Look-ahead index: opened (?<file>\S+), (?<count>\d+) conversations, "
            + @"format (?<format>\d+)\.",
            RegexOptions.Compiled);

        private static readonly Regex MatchedPattern = new Regex(
            @"Look-ahead index: group (?<group>[\d,]+) matches the loaded database "
            + @"\((?<ms>[\d,]+) ms\)\.",
            RegexOptions.Compiled);

        private static readonly Regex RebuiltPattern = new Regex(
            @"Look-ahead index: rebuilt (?<count>[\d,]+) conversations",
            RegexOptions.Compiled);

        private IndexCacheReport(
            string? opened,
            int conversations,
            int format,
            string? group,
            double checkMilliseconds,
            bool rebuilt,
            string? line)
        {
            Opened = opened;
            Conversations = conversations;
            Format = format;
            Group = group;
            CheckMilliseconds = checkMilliseconds;
            Rebuilt = rebuilt;
            Line = line;
        }

        /// <summary>Which index file was opened, or null if none was.</summary>
        public string? Opened { get; }

        /// <summary>How many conversations it held, or -1 if none was opened.</summary>
        public int Conversations { get; }

        /// <summary>
        /// What format it said it was, or -1 if none was opened.
        /// </summary>
        /// <remarks>
        /// Zero is a real answer: an index with no header, which carries no hashes and so
        /// cannot be checked against anything.
        /// </remarks>
        public int Format { get; }

        /// <summary>The group that was checked, as the plugin named it, or null.</summary>
        public string? Group { get; }

        /// <summary>
        /// How long checking that group took, or -1 where none was checked.
        /// </summary>
        /// <remarks>
        /// The number the synchronous check has to justify. A whole-database check at load
        /// would walk 112,962 entries before the main menu draws; the argument for checking
        /// a group instead is that six conversations cost almost nothing at a moment when
        /// the look-ahead is about to spend far more anyway. This is what that turned out
        /// to be.
        /// </remarks>
        public double CheckMilliseconds { get; }

        /// <summary>Whether the plugin rebuilt the index from the loaded database.</summary>
        /// <remarks>
        /// On an unmodified install it must not have. A rebuild there would mean the two
        /// canonicalisations disagree about a database neither of them changed, which is
        /// the drift the shared routine exists to prevent.
        /// </remarks>
        public bool Rebuilt { get; }

        /// <summary>The last matching line, for a failure message worth reading.</summary>
        public string? Line { get; }

        /// <summary>Whether the plugin said anything about its index at all.</summary>
        public bool Reported => Line != null;

        /// <summary>Reads the report out of a BepInEx log.</summary>
        /// <param name="logPath">The log file.</param>
        public static IndexCacheReport FromLog(string logPath)
        {
            if (!File.Exists(logPath))
            {
                return new IndexCacheReport(null, -1, -1, null, -1, false, null);
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
        /// <param name="text">The log's contents.</param>
        public static IndexCacheReport FromText(string text)
        {
            if (text == null)
            {
                throw new ArgumentNullException(nameof(text));
            }

            Match opened = OpenedPattern.Match(text);
            Match matched = MatchedPattern.Match(text);

            string? line = null;
            foreach (string candidate in text.Split('\n'))
            {
                if (candidate.Contains(LogPrefix))
                {
                    line = candidate.Trim();
                }
            }

            return new IndexCacheReport(
                opened.Success ? opened.Groups["file"].Value : null,
                opened.Success ? Number(opened.Groups["count"].Value) : -1,
                opened.Success ? Number(opened.Groups["format"].Value) : -1,
                matched.Success ? matched.Groups["group"].Value : null,
                matched.Success ? Number(matched.Groups["ms"].Value) : -1,
                RebuiltPattern.IsMatch(text),
                line);
        }

        /// <summary>One captured number, or -1 where it will not parse.</summary>
        /// <remarks>
        /// Thousands separators allowed, because the plugin formats its counts and
        /// milliseconds with them and a log is written for a person first.
        /// </remarks>
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

        /// <summary>A one-line description, for a test's failure message.</summary>
        public override string ToString()
        {
            if (!Reported)
            {
                return "the plugin said nothing about its look-ahead index";
            }

            if (Opened == null)
            {
                return $"no index was opened: {Line}";
            }

            string checking = CheckMilliseconds >= 0
                ? $", {Group} checked in {CheckMilliseconds:N0} ms"
                : ", no group checked";
            return $"{Opened}, {Conversations} conversations, format {Format}{checking}"
                + (Rebuilt ? ", REBUILT from the loaded database" : string.Empty);
        }
    }
}
