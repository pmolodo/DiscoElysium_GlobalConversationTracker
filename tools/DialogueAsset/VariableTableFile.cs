// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>Writes the variable table: one JSON object per line, name first.</summary>
    /// <remarks>
    /// <para>A file of its own rather than a section inside the conversation index. The
    /// index is one conversation per line and is streamed by a reader that expects exactly
    /// that; a table bolted onto it would change a format other things already depend on,
    /// including the copy that ships with the mod. A second file costs one path and
    /// changes nothing that reads the first.</para>
    ///
    /// <para>JSON per line rather than one array, matching the index next to it: it can be
    /// read a record at a time, diffed line by line, and grepped.</para>
    /// </remarks>
    public static class VariableTableFile
    {
        /// <summary>The file the variable table is written to.</summary>
        public const string FileName = "variables.jsonl";

        /// <summary>Writes <paramref name="variables"/> to <paramref name="path"/>.</summary>
        /// <remarks>
        /// LF and no BOM, for the same reason the corpus files use them: these are diffed
        /// against the last copy, and a run that wrote CRLF would rewrite every line.
        /// </remarks>
        public static void Write(string path, IEnumerable<DialogueVariable> variables)
        {
            using var writer = new StreamWriter(path, append: false, new UTF8Encoding(false))
            {
                NewLine = "\n",
            };
            Write(writer, variables);
        }

        /// <summary>Writes <paramref name="variables"/> to an already-open file.</summary>
        public static void Write(TextWriter writer, IEnumerable<DialogueVariable> variables)
        {
            if (writer == null)
            {
                throw new ArgumentNullException(nameof(writer));
            }

            if (variables == null)
            {
                throw new ArgumentNullException(nameof(variables));
            }

            foreach (DialogueVariable variable in variables)
            {
                writer.WriteLine(
                    string.Format(
                        CultureInfo.InvariantCulture,
                        "{{\"name\":{0},\"type\":{1},\"initial\":{2}}}",
                        JsonText.Quote(variable.Name),
                        JsonText.Quote(variable.Type),
                        JsonText.Quote(variable.InitialValue)));
            }
        }
    }
}
