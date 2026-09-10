// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>One actor, as the database names it.</summary>
    /// <remarks>
    /// The NAME is the point. An entry says who speaks it by id - a passive check's Actor
    /// field is 424 rather than "Perception (Sight)" - and the id alone says nothing about
    /// which skill the check tests. The game resolves it through the actor's Articy id and
    /// a table keyed by it; the name is the same answer in a form anything can read, and it
    /// is exactly the list <c>Skill.actorSkillNames</c> holds.
    /// </remarks>
    public sealed class DialogueActor
    {
        internal DialogueActor(int id, string name)
        {
            Id = id;
            Name = name;
        }

        /// <summary>The id an entry's <c>Actor</c> field carries.</summary>
        public int Id { get; }

        /// <summary>The actor's name, as the database spells it.</summary>
        public string Name { get; }

        /// <inheritdoc/>
        public override string ToString()
        {
            return $"{Id}: {Name}";
        }
    }

    /// <summary>Reads the database's actor table.</summary>
    /// <remarks>
    /// Its own scanner, for the reason <see cref="VariableTableExtractor"/> has one:
    /// <see cref="DialogueAssetScanner"/> exists to stream the conversations section and
    /// stops at the one after it, and the actors section comes before conversations and has
    /// a different shape. Small readers over one file beat one reader with three modes.
    /// </remarks>
    public static class ActorTableExtractor
    {
        /// <summary>Where the table starts.</summary>
        private const string ActorsSection = "  actors:";

        /// <summary>A field title carrying the actor's name.</summary>
        private const string NameTitle = "Name";

        private const string ActorStartPrefix = "  - id:";
        private const string FieldStartPrefix = "    - title:";
        private const string ValuePrefix = "      value:";

        /// <summary>Reads the actor table out of the database at <paramref name="path"/>.</summary>
        public static IReadOnlyList<DialogueActor> Extract(string path)
        {
            // Replacement rather than an exception on a malformed byte, matching the
            // conversations scanner: 170 MB of exported game text is not worth failing
            // over one bad byte.
            using var reader = new StreamReader(
                path, Encoding.UTF8, detectEncodingFromByteOrderMarks: true);
            return Extract(reader);
        }

        /// <summary>Reads the actor table from an already-open database.</summary>
        public static IReadOnlyList<DialogueActor> Extract(TextReader reader)
        {
            if (reader == null)
            {
                throw new ArgumentNullException(nameof(reader));
            }

            var found = new List<DialogueActor>();
            bool inside = false;
            int? id = null;
            string? name = null;
            string? pendingField = null;

            string? line;
            while ((line = reader.ReadLine()) != null)
            {
                if (!inside)
                {
                    inside = line == ActorsSection;
                    continue;
                }

                // The section ends at the next key at the same indentation. Checked by
                // shape rather than by naming the section that follows, so a database that
                // orders its sections differently still reads.
                if (line.Length > 2
                    && line[0] == ' ' && line[1] == ' ' && line[2] != ' ' && line[2] != '-')
                {
                    break;
                }

                if (line.StartsWith(ActorStartPrefix, StringComparison.Ordinal))
                {
                    Flush(found, id, name);
                    id = ParseId(line.Substring(ActorStartPrefix.Length));
                    name = null;
                    pendingField = null;
                    continue;
                }

                if (line.StartsWith(FieldStartPrefix, StringComparison.Ordinal))
                {
                    pendingField = DialogueScalar.Decode(
                        line.Substring(FieldStartPrefix.Length));
                    continue;
                }

                if (line.StartsWith(ValuePrefix, StringComparison.Ordinal)
                    && pendingField == NameTitle)
                {
                    name = DialogueScalar.Decode(line.Substring(ValuePrefix.Length));
                }
            }

            Flush(found, id, name);
            return found;
        }

        /// <summary>The id on an actor's opening line, or nothing where it is not a number.</summary>
        private static int? ParseId(string text)
        {
            return int.TryParse(
                text.Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture, out int value)
                ? value
                : null;
        }

        /// <summary>Records an actor, if the lines so far amounted to one.</summary>
        /// <remarks>
        /// A nameless or idless entry is dropped rather than recorded blank, matching the
        /// variable table: the table is looked up by id to get a name, and an entry missing
        /// either could never answer.
        /// </remarks>
        private static void Flush(List<DialogueActor> found, int? id, string? name)
        {
            if (id == null || string.IsNullOrEmpty(name))
            {
                return;
            }

            found.Add(new DialogueActor(id.Value, name!));
        }
    }
}
