// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>Writes the actor table: one JSON object per line, id first.</summary>
    /// <remarks>
    /// One line per actor, like the variable table and the conversation index beside it, so
    /// it can be read a record at a time, diffed line by line, and grepped.
    /// </remarks>
    public static class ActorTableFile
    {
        /// <summary>The file the actor table is written to.</summary>
        public const string FileName = "actors.jsonl";

        /// <summary>Writes <paramref name="actors"/> to <paramref name="path"/>.</summary>
        /// <remarks>
        /// LF and no BOM, for the same reason the corpus files use them: these are diffed
        /// against the last copy, and a run that wrote CRLF would rewrite every line.
        /// </remarks>
        public static void Write(string path, IEnumerable<DialogueActor> actors)
        {
            using var writer = new StreamWriter(path, append: false, new UTF8Encoding(false))
            {
                NewLine = "\n",
            };
            Write(writer, actors);
        }

        /// <summary>Writes <paramref name="actors"/> to an already-open file.</summary>
        public static void Write(TextWriter writer, IEnumerable<DialogueActor> actors)
        {
            if (writer == null)
            {
                throw new ArgumentNullException(nameof(writer));
            }

            if (actors == null)
            {
                throw new ArgumentNullException(nameof(actors));
            }

            foreach (DialogueActor actor in actors)
            {
                writer.WriteLine(
                    string.Format(
                        CultureInfo.InvariantCulture,
                        "{{\"id\":{0},\"name\":{1}}}",
                        actor.Id,
                        JsonText.Quote(actor.Name)));
            }
        }
    }
}
