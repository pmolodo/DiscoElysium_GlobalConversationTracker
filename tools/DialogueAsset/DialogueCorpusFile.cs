// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>Writes a corpus file: one record per line, breaks escaped.</summary>
    /// <remarks>
    /// A guard or a script may itself contain a newline, so the only way one record can
    /// occupy one line is to escape the breaks. The reader that undoes this lives in
    /// CorpusTests.
    /// </remarks>
    public static class DialogueCorpusFile
    {
        /// <summary>The file the distinct conditionsStrings are written to.</summary>
        public const string GuardFileName = "distinct_guards.txt";

        /// <summary>The file the distinct userScripts are written to.</summary>
        public const string ScriptFileName = "distinct_scripts.txt";

        /// <summary>Writes <paramref name="rows"/> to <paramref name="path"/>.</summary>
        /// <remarks>
        /// LF regardless of platform and no BOM, which is what these files hold and what a
        /// regenerated corpus has to keep holding: they are diffed against the last copy,
        /// and a Windows run that wrote CRLF would rewrite all twenty thousand lines.
        /// </remarks>
        public static void Write(string path, IEnumerable<string> rows)
        {
            using var writer = new StreamWriter(path, append: false, new UTF8Encoding(false))
            {
                NewLine = "\n",
            };
            Write(writer, rows);
        }

        /// <summary>Writes <paramref name="rows"/> to an already-open file.</summary>
        public static void Write(TextWriter writer, IEnumerable<string> rows)
        {
            foreach (string row in rows)
            {
                writer.WriteLine(Escape(row));
            }
        }

        /// <summary>
        /// Escapes one record: the backslash first, so unescaping it back is unambiguous,
        /// then the two breaks that would otherwise end the line.
        /// </summary>
        public static string Escape(string text)
        {
            return text.Replace("\\", "\\\\").Replace("\r", "\\r").Replace("\n", "\\n");
        }
    }
}
