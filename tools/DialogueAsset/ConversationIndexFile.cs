// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using System.Text.Json;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>
    /// Reads and writes the conversation index: one compact JSON object per conversation,
    /// one per line.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The line is written here rather than by <see cref="JsonSerializer"/> because the
    /// index is regenerated from a 170 MB asset and diffed against the last copy, so the
    /// bytes have to be reproducible down to the escaping. This writes what Python's
    /// json.dumps(separators=(",", ":")) writes: no spaces, keys in the order below, and
    /// every character outside printable ASCII as a lower-case \uXXXX escape.
    /// System.Text.Json's encoder escapes a different set - &amp;, &lt;, +, ' among them -
    /// and would rewrite every line of an existing index without changing its meaning.
    /// </para>
    /// <para>
    /// Reading is System.Text.Json, which has no such constraint.
    /// </para>
    /// </remarks>
    public static class ConversationIndexFile
    {
        private const char LastPrintableAscii = '~';

        private static readonly JsonSerializerOptions ReadOptions = new JsonSerializerOptions
        {
            PropertyNameCaseInsensitive = true,
        };

        /// <summary>Writes the index to <paramref name="path"/> and returns how many lines it holds.</summary>
        public static int Write(string path, IEnumerable<ConversationRecord> conversations)
        {
            using var writer = new StreamWriter(path, append: false, new UTF8Encoding(false));
            return Write(writer, conversations);
        }

        /// <summary>Writes the index and returns how many lines it holds.</summary>
        /// <remarks>
        /// One <see cref="TextWriter.WriteLine(string)"/> per record, so the line ending is
        /// the platform's - which is what the Python script wrote before this, its output
        /// file being in text mode.
        /// </remarks>
        public static int Write(TextWriter writer, IEnumerable<ConversationRecord> conversations)
        {
            var line = new StringBuilder();
            int written = 0;
            foreach (ConversationRecord conversation in conversations)
            {
                line.Clear();
                Append(line, conversation);
                writer.WriteLine(line.ToString());
                written++;
            }

            return written;
        }

        /// <summary>Reads the index at <paramref name="path"/>, one record per line.</summary>
        public static IEnumerable<ConversationRecord> Read(string path)
        {
            foreach (string line in File.ReadLines(path))
            {
                ConversationRecord? conversation = Parse(line);
                if (conversation != null)
                {
                    yield return conversation;
                }
            }
        }

        /// <summary>Reads an already-open index, one record per line.</summary>
        public static IEnumerable<ConversationRecord> Read(TextReader reader)
        {
            string? line;
            while ((line = reader.ReadLine()) != null)
            {
                ConversationRecord? conversation = Parse(line);
                if (conversation != null)
                {
                    yield return conversation;
                }
            }
        }

        /// <summary>Renders one conversation as the single line the index holds it on.</summary>
        public static string ToJson(ConversationRecord conversation)
        {
            var line = new StringBuilder();
            Append(line, conversation);
            return line.ToString();
        }

        /// <summary>
        /// Whether a line is the header rather than a conversation.
        /// </summary>
        /// <remarks>
        /// Checked by looking for the property rather than by matching the start of the
        /// line, because getting this wrong is silent: a header deserialised as a
        /// conversation is a record with id 0 and no entries, which reads as a real, empty
        /// conversation and would put an entry-less line into any index built from a
        /// round trip.
        /// </remarks>
        public static bool IsHeader(string line)
        {
            if (string.IsNullOrWhiteSpace(line))
            {
                return false;
            }

            using JsonDocument document = JsonDocument.Parse(line);
            return document.RootElement.ValueKind == JsonValueKind.Object
                && document.RootElement.TryGetProperty(ShippedIndex.FormatProperty, out _)
                && !document.RootElement.TryGetProperty("id", out _);
        }

        private static ConversationRecord? Parse(string line)
        {
            if (string.IsNullOrWhiteSpace(line) || IsHeader(line))
            {
                return null;
            }

            return JsonSerializer.Deserialize<ConversationRecord>(line, ReadOptions)
                ?? throw new InvalidDataException("The conversation index contains a null record.");
        }

        private static void Append(StringBuilder json, ConversationRecord conversation)
        {
            json.Append("{\"id\":");
            AppendInt(json, conversation.Id);
            json.Append(",\"title\":");
            AppendString(json, conversation.Title);
            json.Append(",\"actor\":");
            AppendInt(json, conversation.Actor);
            json.Append(",\"conversant\":");
            AppendInt(json, conversation.Conversant);
            if (conversation.Hash != null)
            {
                // Written only where something computed one, so the full index's lines are
                // byte for byte what they were before there was such a thing as a hash.
                json.Append(",\"hash\":");
                AppendString(json, conversation.Hash);
            }

            json.Append(",\"entries\":[");
            for (int i = 0; i < conversation.Entries.Count; i++)
            {
                if (i > 0)
                {
                    json.Append(',');
                }

                Append(json, conversation.Entries[i]);
            }

            json.Append("]}");
        }

        private static void Append(StringBuilder json, EntryRecord entry)
        {
            json.Append("{\"id\":");
            AppendInt(json, entry.Id);
            json.Append(",\"group\":").Append(entry.Group ? "true" : "false");
            json.Append(",\"guard\":");
            AppendString(json, entry.Guard);
            json.Append(",\"script\":");
            AppendString(json, entry.Script);
            json.Append(",\"to\":");
            AppendInts(json, entry.To);
            json.Append(",\"title\":");
            AppendString(json, entry.Title);
            json.Append(",\"fields\":{");
            bool first = true;
            foreach (KeyValuePair<string, string> field in entry.Fields)
            {
                if (!first)
                {
                    json.Append(',');
                }

                first = false;
                AppendString(json, field.Key);
                json.Append(':');
                AppendString(json, field.Value);
            }

            json.Append('}');
            if (entry.ToConversation != null)
            {
                json.Append(",\"to_conversation\":");
                AppendInts(json, entry.ToConversation);
            }

            json.Append('}');
        }

        private static void AppendInts(StringBuilder json, List<int> values)
        {
            json.Append('[');
            for (int i = 0; i < values.Count; i++)
            {
                if (i > 0)
                {
                    json.Append(',');
                }

                AppendInt(json, values[i]);
            }

            json.Append(']');
        }

        private static void AppendInt(StringBuilder json, int? value)
        {
            if (value.HasValue)
            {
                json.Append(value.Value.ToString(CultureInfo.InvariantCulture));
            }
            else
            {
                json.Append("null");
            }
        }

        private static void AppendString(StringBuilder json, string? value)
        {
            if (value == null)
            {
                json.Append("null");
                return;
            }

            json.Append('"');
            foreach (char c in value)
            {
                switch (c)
                {
                    case '"':
                        json.Append("\\\"");
                        break;
                    case '\\':
                        json.Append("\\\\");
                        break;
                    case '\b':
                        json.Append("\\b");
                        break;
                    case '\f':
                        json.Append("\\f");
                        break;
                    case '\n':
                        json.Append("\\n");
                        break;
                    case '\r':
                        json.Append("\\r");
                        break;
                    case '\t':
                        json.Append("\\t");
                        break;
                    default:
                        if (c < ' ' || c > LastPrintableAscii)
                        {
                            // Including each half of a surrogate pair on its own, which is
                            // what an ASCII-only encoder does with an astral character.
                            json.Append("\\u").Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                        }
                        else
                        {
                            json.Append(c);
                        }

                        break;
                }
            }

            json.Append('"');
        }
    }
}
