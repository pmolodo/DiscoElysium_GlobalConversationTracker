// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;

using GlobalConversationTracker.Core;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>
    /// Builds the global state that makes a look-ahead crawl as expensive as it can be:
    /// every dialogue entry of every conversation in the index recorded as displayed.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Both halves of "everything" matter. No option's own novelty is unseen-anywhere, so
    /// MarkerFor cannot answer from the option alone and return before it builds a graph -
    /// the crawl runs. And nothing the crawl reaches is unseen-anywhere either, so it can
    /// never stop the instant it finds something: it has to explore everything reachable.
    /// </para>
    /// <para>
    /// Recording only the conversations a scenario opens is not enough, and fails quietly.
    /// Options in a menu often belong to a different conversation than the one that is
    /// active - opening 28 (WHIRLING F1 / GARTE MAIN) draws a menu whose options are
    /// entries of 13 (WHIRLING F1 / GARTE) - and an option whose own conversation is
    /// absent from the state is unseen-anywhere, takes the early exit, and is never
    /// crawled at all. The measurement then reports nothing for that conversation while
    /// looking like it ran.
    /// </para>
    /// <para>
    /// The source is the conversation index, which is why this sits beside
    /// <see cref="ConversationIndexFile"/> rather than in the CLI over it. Regenerate
    /// testing/scenarios/global-state-worst-case.json with
    /// <c>dotnet run --project tools/DialogueExtract -- worst-case-state</c>.
    /// </para>
    /// </remarks>
    public static class WorstCaseGlobalState
    {
        /// <summary>
        /// The format version the state is written in: grouped by status, then by
        /// conversation, then the entry ids run-encoded.
        /// </summary>
        /// <remarks>
        /// The same version GlobalStateJson writes, and deliberately the same bytes but
        /// for one property: no "orbs" is written here, since the reader treats an absent
        /// "orbs" as no orbs and a worst-case crawl does not look at them.
        ///
        /// Version 4 run-encodes the entry ids, and it is where this file earns the change
        /// most: recording every entry of every conversation in the game, it went from
        /// 423 KB to 22.5 KB - a state made of whole conversations is nothing but runs.
        /// Version 5 adds the header every document here carries.
        /// </remarks>
        public const int FormatVersion = 5;

        /// <summary>What the document calls itself, as the reader expects to find it.</summary>
        public const string FormatName = "global-state";

        /// <summary>What the game calls an entry the player has been shown.</summary>
        /// <remarks>
        /// Written as the game's own string rather than an enum integer, the same way
        /// GlobalStateJson writes it, so the file survives the enum being renumbered.
        /// </remarks>
        public const string DisplayedStatus = "WasDisplayed";

        /// <summary>
        /// Every conversation in <paramref name="conversations"/> that has entries, by id
        /// ascending, each holding its entry ids ascending and de-duplicated.
        /// </summary>
        /// <remarks>
        /// A conversation with no entries contributes nothing and is skipped: an empty
        /// array would say "seen nothing here", which is what an absent conversation
        /// already says.
        /// </remarks>
        /// <exception cref="InvalidDataException">
        /// The index described no conversations at all. That is a broken or truncated
        /// index rather than an unusual one, and a state built from it would silently
        /// measure nothing.
        /// </exception>
        public static SortedDictionary<int, List<int>> Build(IEnumerable<ConversationRecord> conversations)
        {
            // Sorted by the integer id, not by the string it is written as: keyed on text,
            // "10" would sort before "9".
            var displayed = new SortedDictionary<int, List<int>>();
            foreach (ConversationRecord conversation in conversations)
            {
                var ids = new SortedSet<int>();
                foreach (EntryRecord entry in conversation.Entries)
                {
                    ids.Add(entry.Id);
                }

                if (ids.Count == 0)
                {
                    continue;
                }

                displayed[conversation.Id] = new List<int>(ids);
            }

            if (displayed.Count == 0)
            {
                throw new InvalidDataException("The conversation index described no conversations at all.");
            }

            return displayed;
        }

        /// <summary>Renders a built state as the single line the file holds it on.</summary>
        /// <remarks>
        /// Written by hand rather than by a serializer for the same reason the index is:
        /// the output is committed and diffed, so the bytes are the contract. Every key
        /// and value here is a number, so there is nothing to escape - only the compact
        /// separators and the ordering the parameter type already guarantees.
        /// </remarks>
        public static string ToJson(SortedDictionary<int, List<int>> displayed)
        {
            var json = new StringBuilder();
            json.Append("{\"_format\":\"").Append(FormatName).Append("\",\"_formatVersion\":")
                .Append(FormatVersion.ToString(CultureInfo.InvariantCulture));
            json.Append(",\"conversations\":{\"").Append(DisplayedStatus).Append("\":{");
            bool firstConversation = true;
            foreach (KeyValuePair<int, List<int>> conversation in displayed)
            {
                if (!firstConversation)
                {
                    json.Append(',');
                }

                firstConversation = false;
                json.Append('"').Append(conversation.Key.ToString(CultureInfo.InvariantCulture));

                // RUN-ENCODED, through the encoder GlobalStateJson uses, so the two writers
                // of this format cannot come to spell a run differently. Nothing to escape
                // still: the encoding is digits, commas and hyphens.
                json.Append("\":\"")
                    .Append(SparseOrder.PackRange(conversation.Value.ConvertAll(id => (long)id)))
                    .Append('"');
            }

            json.Append("}}}");
            return json.ToString();
        }

        /// <summary>
        /// Writes the worst-case state to <paramref name="path"/> and returns what it
        /// holds, so the caller can report on it.
        /// </summary>
        /// <remarks>
        /// UTF-8 without a BOM, unindented, and with a trailing newline - which the
        /// repository's end-of-file hook adds anyway and which JSON ignores. Without it
        /// every regeneration would dirty the tree. The newline is written as a bare LF
        /// rather than the platform's, since the file is committed.
        /// </remarks>
        public static SortedDictionary<int, List<int>> Write(
            string path, IEnumerable<ConversationRecord> conversations)
        {
            SortedDictionary<int, List<int>> displayed = Build(conversations);
            using (var writer = new StreamWriter(path, append: false, new UTF8Encoding(false)))
            {
                writer.Write(ToJson(displayed));
                writer.Write('\n');
            }

            return displayed;
        }
    }
}
