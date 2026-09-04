// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Text.Json;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// Everything a crawl over one conversation group can ask the world, as the engine
    /// named it.
    /// </summary>
    /// <remarks>
    /// <para>THE ENGINE ASKS THE QUESTIONS, and this is the list. Every answer must come
    /// back under the exact key given here - see <see cref="WorldSnapshot"/> - because the
    /// alternative was for both sides to render <c>CheckEquipped("neck_tie")</c>
    /// identically forever, including how a number is formatted and how a string is
    /// escaped. One disagreement there and the answer silently goes missing, the query
    /// reads Unknown, the guard turns permissive, and the marker is wrong with nothing to
    /// report it.</para>
    ///
    /// <para>Worth caching against a conversation. The questions cannot change while the
    /// game is running, the walk over a group's guards is not free, and the lists are long
    /// - 306 variables and 4,514 entries for conversation 631.</para>
    /// </remarks>
    public sealed class LookAheadQuestions
    {
        private LookAheadQuestions(
            IReadOnlyList<int> conversations,
            IReadOnlyList<string> variables,
            IReadOnlyList<string> queries,
            IReadOnlyList<string> items,
            IReadOnlyList<string> tasks,
            IReadOnlyList<string> thoughts,
            IReadOnlyList<NodeRef> checks,
            IReadOnlyList<NodeRef> entries)
        {
            Conversations = conversations;
            Variables = variables;
            Queries = queries;
            Items = items;
            Tasks = tasks;
            Thoughts = thoughts;
            Checks = checks;
            Entries = entries;
        }

        /// <summary>
        /// The conversations the group covers, so the caller knows what it committed to.
        /// </summary>
        public IReadOnlyList<int> Conversations { get; }

        /// <summary>Dialogue variables some guard in the group reads, by name.</summary>
        public IReadOnlyList<string> Variables { get; }

        /// <summary>
        /// World queries, by the key their answers must come back under - the rendered
        /// call, such as <c>CheckEquipped("neck_tie")</c>.
        /// </summary>
        public IReadOnlyList<string> Queries { get; }

        /// <summary>Items some guard asks about, by name.</summary>
        public IReadOnlyList<string> Items { get; }

        /// <summary>Journal tasks some guard asks about, by name.</summary>
        public IReadOnlyList<string> Tasks { get; }

        /// <summary>Thoughts some guard asks about, by name.</summary>
        public IReadOnlyList<string> Thoughts { get; }

        /// <summary>Entries carrying a skill check, whose outcome the world decides.</summary>
        public IReadOnlyList<NodeRef> Checks { get; }

        /// <summary>Every entry in the group, because any of them may have been seen.</summary>
        public IReadOnlyList<NodeRef> Entries { get; }

        /// <summary>Reads what <c>gct_questions</c> returned.</summary>
        /// <param name="json">The engine's answer.</param>
        /// <exception cref="ArgumentNullException">The JSON is null.</exception>
        /// <exception cref="FormatException">It is not a questions document.</exception>
        public static LookAheadQuestions Parse(string json)
        {
            if (json == null)
            {
                throw new ArgumentNullException(nameof(json));
            }

            try
            {
                using JsonDocument document = JsonDocument.Parse(json);
                JsonElement root = document.RootElement;

                return new LookAheadQuestions(
                    Numbers(root, "conversations"),
                    Texts(root, "variables"),
                    Texts(root, "queries"),
                    Texts(root, "items"),
                    Texts(root, "tasks"),
                    Texts(root, "thoughts"),
                    Nodes(root, "checks"),
                    Nodes(root, "entries"));
            }
            catch (JsonException error)
            {
                // Rethrown rather than swallowed. The only thing that produces this is the
                // native library, so a document that will not parse means the two sides
                // disagree about the format - which must be loud, not permissive.
                throw new FormatException(
                    "the look-ahead library's questions could not be read: " + error.Message,
                    error);
            }
        }

        private static IReadOnlyList<string> Texts(JsonElement root, string name)
        {
            var found = new List<string>();
            foreach (JsonElement item in Array(root, name))
            {
                found.Add(item.GetString() ?? string.Empty);
            }

            return found;
        }

        private static IReadOnlyList<int> Numbers(JsonElement root, string name)
        {
            var found = new List<int>();
            foreach (JsonElement item in Array(root, name))
            {
                found.Add(item.GetInt32());
            }

            return found;
        }

        private static IReadOnlyList<NodeRef> Nodes(JsonElement root, string name)
        {
            var found = new List<NodeRef>();
            foreach (JsonElement item in Array(root, name))
            {
                found.Add(new NodeRef(
                    item.GetProperty("conversation").GetInt32(),
                    item.GetProperty("entry").GetInt32()));
            }

            return found;
        }

        /// <summary>
        /// One array property, or nothing where the engine omitted it.
        /// </summary>
        /// <remarks>
        /// Absent reads as empty rather than as an error, so a hand-written fixture may
        /// say only what it cares about. A property that is present and is NOT an array is
        /// a disagreement about the format, and throws.
        /// </remarks>
        private static IEnumerable<JsonElement> Array(JsonElement root, string name)
        {
            if (!root.TryGetProperty(name, out JsonElement value)
                || value.ValueKind == JsonValueKind.Null)
            {
                yield break;
            }

            foreach (JsonElement item in value.EnumerateArray())
            {
                yield return item;
            }
        }
    }
}
