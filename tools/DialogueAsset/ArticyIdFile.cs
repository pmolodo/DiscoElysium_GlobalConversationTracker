// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using System.Text.Json;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>Writes the articy id maps as articy_ids_final_cut.json holds them.</summary>
    /// <remarks>
    /// <para>
    /// Written by hand rather than by <see cref="JsonSerializer"/> for the same reason the
    /// conversation index is: the file is regenerated from a 170 MB asset and diffed
    /// against the copy this repository carries, so the bytes have to be reproducible.
    /// This writes what Python's json.dumps(indent=2, ensure_ascii=False) wrote - two
    /// spaces per level, ": " after a key, every element of a list on its own line, a
    /// character outside ASCII written as itself, and no line break after the last brace.
    /// </para>
    /// <para>
    /// The line ending is <see cref="TextWriter.NewLine"/>, the platform's by default,
    /// which is what the Python wrote through a text-mode file.
    /// </para>
    /// </remarks>
    public static class ArticyIdFile
    {
        private const string Indent = "  ";
        private const string ConversationsKey = "conversations";
        private const string EntriesKey = "dialogue_entries";

        /// <summary>Writes the index to <paramref name="path"/>.</summary>
        public static void Write(string path, ArticyIdIndex index)
        {
            using var writer = new StreamWriter(path, append: false, new UTF8Encoding(false));
            Write(writer, index);
        }

        /// <summary>Writes the index to an already-open destination.</summary>
        public static void Write(TextWriter writer, ArticyIdIndex index)
        {
            writer.Write('{');
            WriteConversations(writer, index.Conversations);
            WriteEntries(writer, index.DialogueEntries);
            Line(writer, "}");
        }

        /// <summary>Renders the index as the text the file holds, line breaks and all.</summary>
        public static string ToJson(ArticyIdIndex index)
        {
            var writer = new StringWriter();
            Write(writer, index);
            return writer.ToString();
        }

        private static void WriteConversations(TextWriter writer, OrderedDictionary<string, int> conversations)
        {
            if (conversations.Count == 0)
            {
                Line(writer, $"{Indent}{Key(ConversationsKey)} {{}},");
                return;
            }

            Line(writer, $"{Indent}{Key(ConversationsKey)} {{");
            int remaining = conversations.Count;
            foreach (KeyValuePair<string, int> conversation in conversations)
            {
                Line(writer, $"{Indent}{Indent}{Key(conversation.Key)} {Number(conversation.Value)}"
                    + Comma(--remaining));
            }

            Line(writer, $"{Indent}}},");
        }

        private static void WriteEntries(TextWriter writer, OrderedDictionary<string, ArticyEntryIds> entries)
        {
            if (entries.Count == 0)
            {
                Line(writer, $"{Indent}{Key(EntriesKey)} {{}}");
                return;
            }

            Line(writer, $"{Indent}{Key(EntriesKey)} {{");
            int remaining = entries.Count;
            foreach (KeyValuePair<string, ArticyEntryIds> entry in entries)
            {
                Line(writer, $"{Indent}{Indent}{Key(entry.Key)} [");
                Line(writer, $"{Indent}{Indent}{Indent}{Number(entry.Value.ConversationId)},");
                Line(writer, $"{Indent}{Indent}{Indent}[");
                List<int> ids = entry.Value.EntryIds;
                for (int i = 0; i < ids.Count; i++)
                {
                    Line(writer, $"{Indent}{Indent}{Indent}{Indent}{Number(ids[i])}"
                        + Comma(ids.Count - 1 - i));
                }

                Line(writer, $"{Indent}{Indent}{Indent}]");
                Line(writer, $"{Indent}{Indent}]" + Comma(--remaining));
            }

            Line(writer, $"{Indent}}}");
        }

        private static string Comma(int remaining)
        {
            return remaining > 0 ? "," : string.Empty;
        }

        private static string Number(int value)
        {
            return value.ToString(CultureInfo.InvariantCulture);
        }

        private static string Key(string name)
        {
            var text = new StringBuilder();
            AppendString(text, name);
            return text.Append(':').ToString();
        }

        private static void Line(TextWriter writer, string text)
        {
            // Before rather than after, so the file ends where the JSON does.
            writer.Write(writer.NewLine);
            writer.Write(text);
        }

        private static void AppendString(StringBuilder json, string value)
        {
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
                        if (c < ' ')
                        {
                            json.Append("\\u").Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                        }
                        else
                        {
                            // Everything else as itself, printable or not: this file is not
                            // ASCII-escaped, unlike the conversation index beside it.
                            json.Append(c);
                        }

                        break;
                }
            }

            json.Append('"');
        }
    }
}
