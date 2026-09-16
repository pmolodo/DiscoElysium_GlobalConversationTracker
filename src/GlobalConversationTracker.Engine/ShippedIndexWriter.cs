// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// Writes an index in the shape the look-ahead library reads.
    /// </summary>
    /// <remarks>
    /// <para>For the plugin, which rebuilds the index from the live dialogue database when
    /// the one it shipped turns out to describe a different game. The extractor has a
    /// writer of its own - <c>ConversationIndexFile</c> - and this is deliberately not it:
    /// that one exists to produce byte-reproducible output for a 50 MB file that is diffed
    /// against the last copy, escaping character for character the way the Python script
    /// before it did. Nothing diffs a rebuilt index, and the plugin cannot reach that
    /// assembly anyway.</para>
    ///
    /// <para>What keeps the two from drifting is not a convention: they produce the SAME
    /// LINE for the same conversation, and a test compares them. The format is defined by
    /// the one thing that reads it, which is the Rust engine, and a second test writes an
    /// index with this and opens it with the real library.</para>
    /// </remarks>
    public static class ShippedIndexWriter
    {
        /// <summary>The header line, naming the format the file is in.</summary>
        /// <remarks>
        /// Must match <c>ShippedIndex.Header()</c> and
        /// <c>lookahead_engine::index::FORMAT_VERSION</c>, which write and read the same
        /// line.
        /// </remarks>
        public const int FormatVersion = 4;

        private const char LastPrintableAscii = '~';

        /// <summary>Writes a whole index, header first.</summary>
        /// <param name="path">Where the file goes.</param>
        /// <param name="conversations">What to write.</param>
        /// <returns>How many conversations were written.</returns>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public static int Write(string path, IEnumerable<IndexConversation> conversations)
        {
            if (path == null)
            {
                throw new ArgumentNullException(nameof(path));
            }

            using var writer = new StreamWriter(path, append: false, new UTF8Encoding(false));
            return Write(writer, conversations);
        }

        /// <summary>Writes a whole index to an open writer, header first.</summary>
        /// <param name="writer">Where the file goes.</param>
        /// <param name="conversations">What to write.</param>
        /// <returns>How many conversations were written.</returns>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public static int Write(TextWriter writer, IEnumerable<IndexConversation> conversations)
        {
            if (writer == null)
            {
                throw new ArgumentNullException(nameof(writer));
            }

            if (conversations == null)
            {
                throw new ArgumentNullException(nameof(conversations));
            }

            writer.WriteLine(Header());

            int written = 0;
            foreach (IndexConversation conversation in conversations)
            {
                writer.WriteLine(ToJson(conversation));
                written++;
            }

            return written;
        }

        /// <summary>The line that opens an index.</summary>
        public static string Header()
        {
            return "{\"format\":" + FormatVersion.ToString(CultureInfo.InvariantCulture) + "}";
        }

        /// <summary>One conversation as the single line an index holds it on.</summary>
        /// <param name="conversation">The conversation.</param>
        /// <exception cref="ArgumentNullException">The conversation is null.</exception>
        public static string ToJson(IndexConversation conversation)
        {
            if (conversation == null)
            {
                throw new ArgumentNullException(nameof(conversation));
            }

            var json = new StringBuilder();
            json.Append("{\"id\":").Append(Number(conversation.Id));

            // Null on purpose rather than absent, matching what the extractor writes: it
            // trims these away by emptying them, not by dropping the keys, and the two
            // writers produce the same line.
            json.Append(",\"title\":null,\"actor\":null,\"conversant\":null");
            json.Append(",\"hash\":");
            AppendString(json, conversation.Hash());

            // A journal task's conditions, and nothing at all for any other conversation - the
            // extractor writes the key only where it has something to put in it.
            var kept = new List<KeyValuePair<string, string>>();
            foreach (KeyValuePair<string, string> field in conversation.Fields)
            {
                if (Array.IndexOf(IndexFields.ConversationRead, field.Key) >= 0)
                {
                    kept.Add(field);
                }
            }

            if (kept.Count > 0)
            {
                json.Append(",\"fields\":");
                AppendFields(json, kept);
            }

            json.Append(",\"entries\":[");

            bool firstEntry = true;
            foreach (IndexEntry entry in conversation.Entries)
            {
                if (!firstEntry)
                {
                    json.Append(',');
                }

                firstEntry = false;
                Append(json, conversation.Id, entry);
            }

            return json.Append("]}").ToString();
        }

        private static void Append(StringBuilder json, int conversationId, IndexEntry entry)
        {
            json.Append("{\"id\":").Append(Number(entry.Id));
            json.Append(",\"group\":").Append(entry.Group ? "true" : "false");
            json.Append(",\"guard\":");
            AppendString(json, entry.Guard);
            json.Append(",\"script\":");
            AppendString(json, entry.Script);

            json.Append(",\"to\":[");
            for (int index = 0; index < entry.Links.Count; index++)
            {
                if (index > 0)
                {
                    json.Append(',');
                }

                json.Append(Number(entry.Links[index].Value));
            }

            json.Append("],\"title\":null,\"fields\":");
            var kept = new List<KeyValuePair<string, string>>();
            foreach (KeyValuePair<string, string> field in entry.Fields)
            {
                if (Array.IndexOf(IndexFields.Read, field.Key) >= 0)
                {
                    kept.Add(field);
                }
            }

            AppendFields(json, kept);

            // Absent where the entry has no links at all, which is what the extractor
            // writes and what the reader's "a missing element means this conversation"
            // rule is worded against.
            if (entry.Links.Count > 0)
            {
                json.Append(",\"to_conversation\":[");
                for (int index = 0; index < entry.Links.Count; index++)
                {
                    if (index > 0)
                    {
                        json.Append(',');
                    }

                    json.Append(Number(entry.Links[index].Key));
                }

                json.Append(']');
            }

            json.Append('}');
        }

        /// <summary>A fields object, in the order given.</summary>
        private static void AppendFields(StringBuilder json, List<KeyValuePair<string, string>> fields)
        {
            json.Append('{');
            for (int index = 0; index < fields.Count; index++)
            {
                if (index > 0)
                {
                    json.Append(',');
                }

                AppendString(json, fields[index].Key);
                json.Append(':');
                AppendString(json, fields[index].Value ?? string.Empty);
            }

            json.Append('}');
        }

        private static string Number(int value)
        {
            return value.ToString(CultureInfo.InvariantCulture);
        }

        /// <summary>
        /// Writes a JSON string, escaping everything outside printable ASCII.
        /// </summary>
        /// <remarks>
        /// The same escaping the extractor uses, so the two writers produce identical
        /// bytes for identical content and a test can say so. It is also the conservative
        /// choice on its own terms: the file is read by a Rust parser and by whoever is
        /// looking at a bug report, and dialogue guards carry names that leave ASCII.
        /// </remarks>
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
                        if (c < ' ' || c > LastPrintableAscii)
                        {
                            // Including each half of a surrogate pair on its own, which is
                            // what an ASCII-only encoder does with an astral character.
                            json.Append("\\u")
                                .Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
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
