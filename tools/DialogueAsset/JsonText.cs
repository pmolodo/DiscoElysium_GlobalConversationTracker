// SPDX-License-Identifier: MIT
using System.Globalization;
using System.Text;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>JSON text, written by hand.</summary>
    /// <remarks>
    /// Not through a serializer, because this assembly has no package references and is not
    /// about to grow one for a handful of fields. System.Text.Json would not do either: its
    /// encoder escapes a set of its own - &amp;, &lt;, +, ' among them - and these files are
    /// diffed against the last copy, so a run that escaped differently would rewrite lines
    /// nothing had changed.
    /// </remarks>
    internal static class JsonText
    {
        /// <summary>A JSON string literal.</summary>
        /// <remarks>
        /// The escapes are the complete set JSON requires for the characters these values
        /// can hold: the backslash first so unescaping is unambiguous, then the quote, then
        /// the control characters. Most of it is not expected to fire - a variable name is
        /// an identifier and an actor's name is a few words - it is here so that a database
        /// which surprises us produces valid JSON rather than a broken line.
        /// </remarks>
        internal static string Quote(string value)
        {
            var text = new StringBuilder(value.Length + 2);
            text.Append('"');
            foreach (char character in value)
            {
                switch (character)
                {
                    case '\\': text.Append("\\\\"); break;
                    case '"': text.Append("\\\""); break;
                    case '\n': text.Append("\\n"); break;
                    case '\r': text.Append("\\r"); break;
                    case '\t': text.Append("\\t"); break;
                    default:
                        if (character < ' ')
                        {
                            text.Append(
                                string.Format(
                                    CultureInfo.InvariantCulture, "\\u{0:x4}", (int)character));
                        }
                        else
                        {
                            text.Append(character);
                        }

                        break;
                }
            }

            text.Append('"');
            return text.ToString();
        }
    }
}
