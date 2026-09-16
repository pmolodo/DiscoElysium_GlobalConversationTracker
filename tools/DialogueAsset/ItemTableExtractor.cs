// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>Reads the item table out of the Dialogue System database.</summary>
    /// <remarks>
    /// <para>THE SAME SHAPE AS <see cref="ActorTableExtractor"/>, because the sections are the
    /// same shape: a list of records under a two-space key, each with an id line and a list of
    /// titled fields. Only the two fields wanted differ.</para>
    ///
    /// <para>The display name is NOT in the database - it is a localization term, and
    /// <see cref="ItemNameExtractor"/> reads it from the lockit.</para>
    /// </remarks>
    public static class ItemTableExtractor
    {
        /// <summary>Where the table starts.</summary>
        private const string ItemsSection = "  items:";

        /// <summary>The field carrying the item's id.</summary>
        private const string NameTitle = "Name";

        /// <summary>The field carrying what it stacks as, which decides how it is asked for.</summary>
        private const string StackTitle = "stackName";

        private const string ItemStartPrefix = "  - id:";
        private const string FieldStartPrefix = "    - title:";
        private const string ValuePrefix = "      value:";

        /// <summary>Reads the item table out of the database at <paramref name="path"/>.</summary>
        /// <param name="path">The database .asset.</param>
        /// <returns>Every item, with its stack name; display names are left empty.</returns>
        public static IReadOnlyList<DialogueItem> Extract(string path)
        {
            // Replacement rather than an exception on a malformed byte, as the other
            // scanners do: 170 MB of exported game text is not worth failing over one byte.
            using var reader = new StreamReader(
                path, Encoding.UTF8, detectEncodingFromByteOrderMarks: true);
            return Extract(reader);
        }

        /// <summary>Reads the item table from an already-open database.</summary>
        /// <param name="reader">The database.</param>
        /// <returns>Every item, with its stack name; display names are left empty.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="reader"/> is null.</exception>
        public static IReadOnlyList<DialogueItem> Extract(TextReader reader)
        {
            if (reader == null)
            {
                throw new ArgumentNullException(nameof(reader));
            }

            var found = new List<DialogueItem>();
            bool inside = false;
            string? name = null;
            string? stack = null;
            string? pendingField = null;

            string? line;
            while ((line = reader.ReadLine()) != null)
            {
                if (!inside)
                {
                    inside = line == ItemsSection;
                    continue;
                }

                // The section ends at the next key at the same indentation. Checked by shape
                // rather than by naming the section that follows, so a database that orders
                // its sections differently still reads.
                if (line.Length > 2
                    && line[0] == ' ' && line[1] == ' ' && line[2] != ' ' && line[2] != '-')
                {
                    break;
                }

                if (line.StartsWith(ItemStartPrefix, StringComparison.Ordinal))
                {
                    Flush(found, name, stack);
                    name = null;
                    stack = null;
                    pendingField = null;
                    continue;
                }

                if (line.StartsWith(FieldStartPrefix, StringComparison.Ordinal))
                {
                    pendingField = DialogueScalar.Decode(
                        line.Substring(FieldStartPrefix.Length));
                    continue;
                }

                if (!line.StartsWith(ValuePrefix, StringComparison.Ordinal))
                {
                    continue;
                }

                string value = DialogueScalar.Decode(line.Substring(ValuePrefix.Length));
                if (pendingField == NameTitle)
                {
                    name = value;
                }
                else if (pendingField == StackTitle)
                {
                    stack = value;
                }
            }

            Flush(found, name, stack);
            return found;
        }

        /// <summary>Keeps an item, where the record carried a name to keep it under.</summary>
        private static void Flush(List<DialogueItem> found, string? name, string? stack)
        {
            if (!string.IsNullOrEmpty(name))
            {
                found.Add(new DialogueItem(name!, stack ?? string.Empty, string.Empty));
            }
        }
    }
}
