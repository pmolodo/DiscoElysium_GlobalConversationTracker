// SPDX-License-Identifier: MIT
using System;
using System.Globalization;
using System.IO;
using System.Text.RegularExpressions;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// What the plugin said about the native look-ahead library, read back out of the
    /// BepInEx log.
    /// </summary>
    /// <remarks>
    /// <para>The harness half of de-i5xj.5. The plugin writes one line at load saying
    /// whether it could reach the Rust engine; this reads it, so a run that launches the
    /// game can assert on it instead of a person reading a log.</para>
    ///
    /// <para>Parsing a log line is a weak channel and it is the right one here. The
    /// alternative - a probe command - would need the native library beside the PROBE as
    /// well as beside the plugin, which is a second deployment to keep right in order to
    /// test the first one. The line is already written on every launch, by shipped code,
    /// for the benefit of anyone reading a bug report.</para>
    /// </remarks>
    public sealed class NativeEngineReport
    {
        /// <summary>
        /// What the plugin prefixes these lines with.
        /// </summary>
        /// <remarks>
        /// Must match <c>NativeEngineCheck.LogPrefix</c>. The two cannot share a constant:
        /// this assembly targets the harness and that one is compiled into the plugin
        /// against the game's runtime, and neither references the other.
        /// </remarks>
        public const string LogPrefix = "Native look-ahead:";

        private static readonly Regex LoadedPattern = new Regex(
            @"Native look-ahead: library v(?<version>[^\s]+) loaded\.",
            RegexOptions.Compiled);

        private static readonly Regex IndexPattern = new Regex(
            @"Native look-ahead: index opened, (?<count>\d+) conversations, "
            + @"(?<variables>\d+) declared variables\.",
            RegexOptions.Compiled);

        private NativeEngineReport(
            bool loaded, string? version, int conversations, int variables, string? line)
        {
            Loaded = loaded;
            Version = version;
            Conversations = conversations;
            Variables = variables;
            Line = line;
        }

        /// <summary>Whether the plugin reached the library at all.</summary>
        public bool Loaded { get; }

        /// <summary>The version it reported, or null if it did not load.</summary>
        public string? Version { get; }

        /// <summary>
        /// How many conversations the index held, or -1 if no index was opened.
        /// </summary>
        /// <remarks>
        /// The stronger signal of the two. A version proves the library loaded and can be
        /// called; a conversation count proves it read fifty megabytes of game data from
        /// inside the game's own process.
        /// </remarks>
        public int Conversations { get; }

        /// <summary>
        /// How many variables the deployed table declared, or -1 if no index was opened.
        /// </summary>
        /// <remarks>
        /// Zero is a real answer and not a missing one: it means the index opened and no
        /// variable table was deployed beside it, which is a mod that works and answers
        /// unset dialogue variables less precisely. Worth being able to assert on, because
        /// nothing else would ever notice.
        /// </remarks>
        public int Variables { get; }

        /// <summary>The first matching line, for a failure message worth reading.</summary>
        public string? Line { get; }

        /// <summary>
        /// Reads the report out of a BepInEx log, or returns a "not loaded" report where
        /// the log says nothing about it.
        /// </summary>
        public static NativeEngineReport FromLog(string logPath)
        {
            if (!File.Exists(logPath))
            {
                return new NativeEngineReport(false, null, -1, -1, null);
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
        /// Split out so the parsing can be tested without a game, a log file, or a launch -
        /// which is the whole of what can be checked before somebody runs the real thing.
        /// </remarks>
        public static NativeEngineReport FromText(string text)
        {
            if (text == null)
            {
                throw new ArgumentNullException(nameof(text));
            }

            Match loaded = LoadedPattern.Match(text);
            Match index = IndexPattern.Match(text);

            return new NativeEngineReport(
                loaded.Success,
                loaded.Success ? loaded.Groups["version"].Value : null,
                Number(index, "count"),
                Number(index, "variables"),
                FirstMentioning(text));
        }

        /// <summary>One captured number, or -1 where the match did not happen.</summary>
        private static int Number(Match match, string group)
        {
            return match.Success
                && int.TryParse(
                    match.Groups[group].Value,
                    NumberStyles.None,
                    CultureInfo.InvariantCulture,
                    out int parsed)
                    ? parsed
                    : -1;
        }

        /// <summary>The first line mentioning the bridge, whatever it says about it.</summary>
        private static string? FirstMentioning(string text)
        {
            foreach (string line in text.Split('\n'))
            {
                if (line.Contains(LogPrefix))
                {
                    return line.Trim();
                }
            }

            return null;
        }

        /// <summary>A one-line description, for a test's failure message.</summary>
        public override string ToString()
        {
            if (!Loaded)
            {
                return Line == null
                    ? "the plugin said nothing about the native look-ahead"
                    : $"the native look-ahead did not load: {Line}";
            }

            return Conversations >= 0
                ? $"native look-ahead v{Version}, {Conversations} conversations, "
                    + $"{Variables} declared variables"
                : $"native look-ahead v{Version}, no index opened";
        }
    }
}
