// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>Writes the item table: one JSON object per line, id first.</summary>
    /// <remarks>
    /// <para>WHAT IT IS FOR. An offline world answers <c>CheckItem</c> out of a save, and the
    /// save records what is held in three places that do not agree about what an item is: the
    /// bag and the equipment carry ids, and the key pocket carries English display names. This
    /// is what turns one into the other, and what says which of the three a given item is
    /// answered from - see <see cref="DialogueItem"/>.</para>
    ///
    /// <para>JSON per line, LF, no BOM, matching the tables beside it: readable a record at a
    /// time, diffed line by line, and grepped.</para>
    /// </remarks>
    public static class ItemTableFile
    {
        /// <summary>The file the item table is written to.</summary>
        public const string FileName = "item_names.jsonl";

        /// <summary>Writes <paramref name="items"/> to <paramref name="path"/>.</summary>
        /// <param name="path">Where to write.</param>
        /// <param name="items">The items.</param>
        public static void Write(string path, IEnumerable<DialogueItem> items)
        {
            using var writer = new StreamWriter(path, append: false, new UTF8Encoding(false))
            {
                NewLine = "\n",
            };
            Write(writer, items);
        }

        /// <summary>Writes <paramref name="items"/> to an already-open file.</summary>
        /// <param name="writer">Where to write.</param>
        /// <param name="items">The items.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public static void Write(TextWriter writer, IEnumerable<DialogueItem> items)
        {
            if (writer == null)
            {
                throw new ArgumentNullException(nameof(writer));
            }

            if (items == null)
            {
                throw new ArgumentNullException(nameof(items));
            }

            foreach (DialogueItem item in items)
            {
                writer.WriteLine(
                    string.Format(
                        CultureInfo.InvariantCulture,
                        "{{\"name\":{0},\"stack\":{1},\"display\":{2},\"group\":{3}}}",
                        JsonText.Quote(item.Name),
                        JsonText.Quote(item.StackName),
                        JsonText.Quote(item.DisplayName),
                        JsonText.Quote(item.Group)));
            }
        }
    }
}
