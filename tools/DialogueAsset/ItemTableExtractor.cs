// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Globalization;
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

        /// <summary>The field carrying which group it belongs to, as an index.</summary>
        private const string GroupTitle = "itemGroup";

        /// <summary>The field carrying what the item moves, as prose a player reads.</summary>
        /// <remarks>
        /// <c>+1 Rhetoric: The heroic deeds (of others)</c>, and several joined by an
        /// escaped newline. Seventy of the database's items carry one.
        /// </remarks>
        private const string BonusTitle = "MediumTextValue";

        /// <summary>
        /// The group names, by the index the database stores, from <c>ItemUtil.itemGroup</c>.
        /// </summary>
        /// <remarks>
        /// <para>Copied from the game rather than invented, and the order is the whole content
        /// of the field - the database stores 0 to 6 and this is what those mean:</para>
        /// <code>
        ///     public static string[] itemGroup = new string[7]
        ///         { "none", "alcohol", "smokes", "ghb", "speed", "pyrholidon", "tare" };
        /// </code>
        /// <para>Measured across the shipped database: 193 of the 206 items are <c>none</c>, and
        /// the 13 that are not are 4 alcohol, 4 tare, 2 smokes, 2 speed and 1 pyrholidon. NO ITEM
        /// IS <c>ghb</c>, so a guard asking for that group can never be true - which is worth
        /// knowing rather than special-casing, since the guards do ask for the other five.</para>
        /// </remarks>
        private static readonly string[] GroupNames =
        {
            "none", "alcohol", "smokes", "ghb", "speed", "pyrholidon", "tare",
        };

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
            string? group = null;
            string? bonusText = null;
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
                    Flush(found, name, stack, group, bonusText);
                    name = null;
                    stack = null;
                    group = null;
                    bonusText = null;
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
                else if (pendingField == GroupTitle)
                {
                    group = value;
                }
                else if (pendingField == BonusTitle)
                {
                    bonusText = value;
                }
            }

            Flush(found, name, stack, group, bonusText);
            return found;
        }

        /// <summary>Keeps an item, where the record carried a name to keep it under.</summary>
        private static void Flush(
            List<DialogueItem> found,
            string? name,
            string? stack,
            string? group,
            string? bonusText)
        {
            if (!string.IsNullOrEmpty(name))
            {
                found.Add(new DialogueItem(
                    name!,
                    stack ?? string.Empty,
                    string.Empty,
                    GroupNameOf(group),
                    BonusesIn(bonusText)));
            }
        }

        /// <summary>The bonuses stated in one item's <see cref="BonusTitle"/>, in order.</summary>
        /// <remarks>
        /// <para>WHAT THE TEXT LOOKS LIKE. A signed amount, what it moves, a colon, and flavour
        /// the player reads - <c>+1 Pain Threshold: Thicker skin</c>. Several are one value
        /// separated by an escaped newline. Some say <c>+1 to X when equipped</c>. Some state no
        /// bonus at all: <c>Heal all Health.</c></para>
        ///
        /// <para>TAKEN VERBATIM, spelling and all. The database is inconsistent with itself -
        /// <c>Electrochemisty</c> beside <c>Electrochemistry</c>, <c>Reaction</c> beside
        /// <c>Reaction Speed</c> - and it names things that are not skills at all: Health,
        /// Morale, the attribute abbreviations on substances, and two thoughts. Deciding what
        /// each one means needs the engine's list of skills, which is over there; see
        /// <see cref="ItemBonus"/>.</para>
        ///
        /// <para>A CONDITION IS NOT READ. One bonus says <c>-1 Suggestion (unless wearing full
        /// armor)</c>, and the parenthetical travels with the name rather than being dropped, so
        /// a reader can see it is there rather than believing the bonus unconditional.</para>
        /// </remarks>
        private static IReadOnlyList<ItemBonus> BonusesIn(string? text)
        {
            if (string.IsNullOrEmpty(text))
            {
                return Array.Empty<ItemBonus>();
            }

            var found = new List<ItemBonus>();
            foreach (string part in text!.Split(EscapedNewline, StringSplitOptions.None))
            {
                ItemBonus? bonus = BonusOf(part);
                if (bonus != null)
                {
                    found.Add(bonus);
                }
            }

            return found.Count == 0 ? Array.Empty<ItemBonus>() : found;
        }

        /// <summary>One line of bonus text, or null where it states none.</summary>
        private static ItemBonus? BonusOf(string line)
        {
            string trimmed = line.Trim();
            if (trimmed.Length < 2 || (trimmed[0] != '+' && trimmed[0] != '-'))
            {
                return null;
            }

            int digits = 1;
            while (digits < trimmed.Length && char.IsDigit(trimmed[digits]))
            {
                digits++;
            }

            if (digits == 1
                || !int.TryParse(
                    trimmed.Substring(0, digits),
                    NumberStyles.AllowLeadingSign,
                    CultureInfo.InvariantCulture,
                    out int amount))
            {
                return null;
            }

            string rest = trimmed.Substring(digits).Trim();

            // `+1 to X when equipped` says the same thing as `+1 X`; the words carry no more.
            if (rest.StartsWith("to ", StringComparison.Ordinal))
            {
                rest = rest.Substring(3);
            }

            int colon = rest.IndexOf(':');
            string moves = (colon < 0 ? rest : rest.Substring(0, colon)).Trim();
            if (moves.EndsWith(" when equipped", StringComparison.Ordinal))
            {
                moves = moves.Substring(0, moves.Length - " when equipped".Length);
            }

            return moves.Length == 0 ? null : new ItemBonus(amount, moves);
        }

        /// <summary>How the database joins several bonuses into one value.</summary>
        private static readonly string[] EscapedNewline = { "\\n" };

        /// <summary>The group's name, from the index the database stores.</summary>
        /// <remarks>
        /// An index outside the table reads as <c>none</c> rather than throwing. The field is a
        /// number in every one of the shipped database's items, so this is about a database that
        /// has been edited rather than about the one that ships - and answering "no group" for
        /// an item nothing can name is what the game's own out-of-range behaviour amounts to.
        /// </remarks>
        private static string GroupNameOf(string? stored)
        {
            if (!int.TryParse(stored, out int index) || index < 0 || index >= GroupNames.Length)
            {
                return GroupNames[0];
            }

            return GroupNames[index];
        }
    }
}
