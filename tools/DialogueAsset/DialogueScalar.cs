// SPDX-License-Identifier: MIT
using System.Globalization;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>Reads a YAML flow scalar as Unity writes it into the database .asset.</summary>
    public static class DialogueScalar
    {
        /// <summary>Decodes a plain, single-quoted or double-quoted scalar.</summary>
        /// <remarks>
        /// The double-quoted case unescapes only \" and \\, in that order, which is what the
        /// conversation index has always done and therefore what its output depends on. It
        /// is not a general YAML unescape: \n and \t are left alone. A reader that needs
        /// those - the guard and script corpus does - wants its own decoder rather than a
        /// change here, because widening this one would silently rewrite the index.
        /// </remarks>
        public static string Decode(string value)
        {
            value = value.Trim();
            if (value.Length >= 2 && value[0] == '\'' && value[^1] == '\'')
            {
                return value[1..^1].Replace("''", "'");
            }

            if (value.Length >= 2 && value[0] == '"' && value[^1] == '"')
            {
                return value[1..^1].Replace("\\\"", "\"").Replace("\\\\", "\\");
            }

            return value;
        }

        /// <summary>Parses an id, which the database always writes as a bare integer.</summary>
        /// <remarks>Throws rather than guessing: an id that will not parse is a broken asset.</remarks>
        public static int ParseId(string value)
        {
            return int.Parse(value.Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture);
        }

        /// <summary>
        /// Reads a number the database wrote as a string, or null where there is no number.
        /// </summary>
        public static int? AsInt(string value)
        {
            return int.TryParse(value, NumberStyles.Integer, CultureInfo.InvariantCulture, out int parsed)
                ? parsed : null;
        }
    }
}
