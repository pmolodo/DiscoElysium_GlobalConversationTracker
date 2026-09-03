// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>One dialogue variable, as the database declares it.</summary>
    /// <remarks>
    /// The declared TYPE is the point. Everything downstream can see what a variable is
    /// named and guess what it holds; only the database says whether it is a flag or a
    /// counter, and that difference decides whether a comparison can be answered at all.
    /// </remarks>
    public sealed class DialogueVariable
    {
        internal DialogueVariable(string name, string type, string initialValue)
        {
            Name = name;
            Type = type;
            InitialValue = initialValue;
        }

        /// <summary>The name guards read it by, as it appears in <c>Variable["..."]</c>.</summary>
        public string Name { get; }

        /// <summary>
        /// The declared type, with the database's <c>CustomFieldType_</c> prefix removed -
        /// so "Boolean" or "Number". Empty where the database declares none.
        /// </summary>
        public string Type { get; }

        /// <summary>The value the database says it starts at, undecoded.</summary>
        public string InitialValue { get; }

        /// <inheritdoc/>
        public override string ToString()
        {
            return $"{Name}: {Type} = {InitialValue}";
        }
    }

    /// <summary>
    /// Reads the database's variable table - the part the conversation index never carried.
    /// </summary>
    /// <remarks>
    /// <para>Why it matters, in one example. A world that has never been told about
    /// <c>jam.jammystery_lorrymans_questioned</c> answers it BOOLEAN FALSE, because an
    /// unset Lua variable is nil and nil is falsy - and that is right for the great
    /// majority of guards, including the 5,994 of 13,059 distinct ones that end in
    /// <c>== false</c>. It is wrong for a counter: <c>>= 3</c> against a boolean cannot be
    /// evaluated at all, so the guard turns undecidable and the branch stays open.</para>
    ///
    /// <para>Answering number zero for everything instead is a worse bug, because
    /// <c>GuardValue.equals</c> is kind-sensitive and every one of those 5,994 guards
    /// would start answering false. The declared type is the only thing that settles it,
    /// and the database has it: 10,645 variables, of which 142 are numbers.</para>
    ///
    /// <para>Its own scanner rather than an extension of
    /// <see cref="DialogueAssetScanner"/>: that one exists to stream the conversations
    /// section and stops at the one after it, and the variables section comes BEFORE
    /// conversations and has a different shape. Two small readers over one file are
    /// clearer than one reader with two modes.</para>
    /// </remarks>
    public static class VariableTableExtractor
    {
        /// <summary>Where the table starts.</summary>
        private const string VariablesSection = "  variables:";

        /// <summary>A field title carrying the variable's name.</summary>
        private const string NameTitle = "Name";

        /// <summary>A field title carrying the value the variable starts at.</summary>
        private const string InitialValueTitle = "Initial Value";

        /// <summary>What the database prefixes a declared type with.</summary>
        private const string TypePrefix = "CustomFieldType_";

        private const string VariableStartPrefix = "  - id:";
        private const string FieldStartPrefix = "    - title:";
        private const string ValuePrefix = "      value:";
        private const string TypeStringPrefix = "      typeString:";

        /// <summary>Reads the variable table out of the database at <paramref name="path"/>.</summary>
        public static IReadOnlyList<DialogueVariable> Extract(string path)
        {
            // Replacement rather than an exception on a malformed byte, matching the
            // conversations scanner: 170 MB of exported game text is not worth failing
            // over one bad byte.
            using var reader = new StreamReader(
                path, Encoding.UTF8, detectEncodingFromByteOrderMarks: true);
            return Extract(reader);
        }

        /// <summary>Reads the variable table from an already-open database.</summary>
        public static IReadOnlyList<DialogueVariable> Extract(TextReader reader)
        {
            if (reader == null)
            {
                throw new ArgumentNullException(nameof(reader));
            }

            var found = new List<DialogueVariable>();
            bool inside = false;
            string? name = null;
            string? type = null;
            string? initial = null;
            string? pendingField = null;

            string? line;
            while ((line = reader.ReadLine()) != null)
            {
                if (!inside)
                {
                    inside = line == VariablesSection;
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

                if (line.StartsWith(VariableStartPrefix, StringComparison.Ordinal))
                {
                    Flush(found, name, type, initial);
                    name = null;
                    type = null;
                    initial = null;
                    pendingField = null;
                    continue;
                }

                if (line.StartsWith(FieldStartPrefix, StringComparison.Ordinal))
                {
                    pendingField = DialogueScalar.Decode(
                        line.Substring(FieldStartPrefix.Length));
                    continue;
                }

                if (line.StartsWith(ValuePrefix, StringComparison.Ordinal))
                {
                    string value = DialogueScalar.Decode(line.Substring(ValuePrefix.Length));
                    if (pendingField == NameTitle)
                    {
                        name = value;
                    }
                    else if (pendingField == InitialValueTitle)
                    {
                        initial = value;
                    }

                    continue;
                }

                if (line.StartsWith(TypeStringPrefix, StringComparison.Ordinal)
                    && pendingField == InitialValueTitle)
                {
                    string declared = DialogueScalar.Decode(
                        line.Substring(TypeStringPrefix.Length));
                    type = declared.StartsWith(TypePrefix, StringComparison.Ordinal)
                        ? declared.Substring(TypePrefix.Length)
                        : declared;
                }
            }

            Flush(found, name, type, initial);
            return found;
        }

        /// <summary>Records a variable, if the lines so far amounted to one.</summary>
        /// <remarks>
        /// A nameless entry is dropped rather than recorded blank. The table is read by
        /// name, so an entry without one could never be looked up, and keeping it would
        /// only make the count disagree with what is usable.
        /// </remarks>
        private static void Flush(
            List<DialogueVariable> found, string? name, string? type, string? initial)
        {
            if (string.IsNullOrEmpty(name))
            {
                return;
            }

            found.Add(new DialogueVariable(name!, type ?? string.Empty, initial ?? string.Empty));
        }
    }
}
