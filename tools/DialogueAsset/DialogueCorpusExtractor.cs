// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>Every distinct guard and script the database contains, sorted.</summary>
    public sealed class DialogueCorpus
    {
        internal DialogueCorpus(IReadOnlyList<string> guards, IReadOnlyList<string> scripts)
        {
            Guards = guards;
            Scripts = scripts;
        }

        /// <summary>Each distinct non-blank conditionsString, in ordinal order.</summary>
        public IReadOnlyList<string> Guards { get; }

        /// <summary>Each distinct non-blank userScript, in ordinal order.</summary>
        public IReadOnlyList<string> Scripts { get; }
    }

    /// <summary>
    /// Collects the distinct conditions and actions out of the streamed conversations
    /// section.
    /// </summary>
    /// <remarks>
    /// This corpus is what the guard and action parsers are tested against: every
    /// condition and action the shipped game actually contains, rather than a handful
    /// someone thought to write down.
    /// </remarks>
    public static class DialogueCorpusExtractor
    {
        /// <summary>Extracts the corpus from the database at <paramref name="path"/>.</summary>
        public static DialogueCorpus Extract(string path)
        {
            return Extract(DialogueAssetScanner.Scan(path));
        }

        /// <summary>Extracts the corpus from an already-open database.</summary>
        public static DialogueCorpus Extract(TextReader reader)
        {
            return Extract(DialogueAssetScanner.Scan(reader));
        }

        /// <summary>
        /// Decodes a scalar the way the corpus reads one, which is not the way
        /// <see cref="DialogueScalar.Decode"/> does.
        /// </summary>
        /// <remarks>
        /// Two deliberate differences from the conversation index's reading, both of which
        /// its output depends on and neither of which may leak into it. A double-quoted
        /// body is unescaped in one pass over the characters, turning \n, \t and \r into
        /// real breaks and any other backslash pair into the character it precedes; and
        /// nothing is trimmed, because the record kept here is the scalar as written.
        /// </remarks>
        public static string Decode(string value)
        {
            if (value.Length >= 2 && value[0] == '"' && value[^1] == '"')
            {
                string body = value[1..^1];
                var text = new StringBuilder(body.Length);
                for (int i = 0; i < body.Length; i++)
                {
                    if (body[i] != '\\' || i + 1 >= body.Length)
                    {
                        text.Append(body[i]);
                        continue;
                    }

                    char escaped = body[i + 1];
                    text.Append(escaped switch
                    {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        _ => escaped,
                    });
                    i++;
                }

                return text.ToString();
            }

            if (value.Length >= 2 && value[0] == '\'' && value[^1] == '\'')
            {
                return value[1..^1].Replace("''", "'");
            }

            return value;
        }

        private static DialogueCorpus Extract(IEnumerable<DialogueAssetEvent> scan)
        {
            // Sorted and distinct as they arrive: ordinal order is the order the corpus
            // files have always been written in, and the database holds far more repeats
            // of a guard than distinct ones.
            var guards = new SortedSet<string>(StringComparer.Ordinal);
            var scripts = new SortedSet<string>(StringComparer.Ordinal);

            foreach (DialogueAssetEvent item in scan)
            {
                if (item.Kind != DialogueAssetEventKind.EntryProperty)
                {
                    continue;
                }

                SortedSet<string> into;
                switch (item.Name)
                {
                    case DialogueAssetScanner.ConditionsProperty:
                        into = guards;
                        break;
                    case DialogueAssetScanner.ScriptProperty:
                        into = scripts;
                        break;
                    default:
                        continue;
                }

                string value = Decode(item.Text);
                if (value.Trim().Length > 0)
                {
                    // Whitespace-only is nothing to parse, but what is kept is the
                    // untrimmed value: leading space inside a real condition is part of it.
                    into.Add(value);
                }
            }

            return new DialogueCorpus(new List<string>(guards), new List<string>(scripts));
        }
    }
}
