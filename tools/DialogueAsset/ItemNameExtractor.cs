// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>Reads each item's English display name out of a lockit.</summary>
    /// <remarks>
    /// <para>WHY A SECOND FILE IS READ AT ALL. A save writes the key pocket as a list of
    /// DISPLAY NAMES - "Key to Basement Apartment" - and the game turns them back into items on
    /// load by matching that name. So a reader that has only the save and the database cannot
    /// say which key is held, and the names live in the localization asset rather than in
    /// either.</para>
    ///
    /// <para>The terms wanted read:</para>
    /// <code>
    ///     - Term: Items/key_basement_apartment/DisplayName
    ///       TermType: 0
    ///       Languages:
    ///       - Key to Basement Apartment
    /// </code>
    /// <para>ENGLISH, because that is the language the game itself matches in:
    /// <c>InventoryViewPersister</c> writes <c>GetTranslatedString("English")</c> whatever the
    /// player is reading, so a save's pocket is English in every localization.</para>
    /// </remarks>
    public static class ItemNameExtractor
    {
        /// <summary>What a term line looks like, up to the item's id.</summary>
        private const string TermPrefix = "    - Term: Items/";

        /// <summary>What the id is followed by on a display-name term.</summary>
        private const string DisplayNameSuffix = "/DisplayName";

        /// <summary>The line that opens the translations, one per language.</summary>
        private const string LanguagesLine = "      Languages:";

        /// <summary>What the first translation line starts with.</summary>
        private const string TranslationPrefix = "      - ";

        /// <summary>Reads the display names out of the lockit at <paramref name="path"/>.</summary>
        /// <param name="path">A lockit .asset.</param>
        /// <returns>The English display name of every item that has one, by item id.</returns>
        public static IReadOnlyDictionary<string, string> Extract(string path)
        {
            using var reader = new StreamReader(
                path, Encoding.UTF8, detectEncodingFromByteOrderMarks: true);
            return Extract(reader);
        }

        /// <summary>Reads the display names from an already-open lockit.</summary>
        /// <param name="reader">The lockit.</param>
        /// <returns>The English display name of every item that has one, by item id.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="reader"/> is null.</exception>
        public static IReadOnlyDictionary<string, string> Extract(TextReader reader)
        {
            if (reader == null)
            {
                throw new ArgumentNullException(nameof(reader));
            }

            var names = new Dictionary<string, string>(StringComparer.Ordinal);
            string? item = null;
            bool translating = false;

            string? line;
            while ((line = reader.ReadLine()) != null)
            {
                if (line.StartsWith(TermPrefix, StringComparison.Ordinal))
                {
                    string term = line.Substring(TermPrefix.Length);
                    item = term.EndsWith(DisplayNameSuffix, StringComparison.Ordinal)
                        ? term.Substring(0, term.Length - DisplayNameSuffix.Length)
                        : null;
                    translating = false;
                    continue;
                }

                if (item == null)
                {
                    continue;
                }

                if (line == LanguagesLine)
                {
                    translating = true;
                    continue;
                }

                // THE FIRST TRANSLATION IS THE ENGLISH ONE, in an English lockit: each file
                // carries one language, and the term's list is in the order the file declares.
                if (translating && line.StartsWith(TranslationPrefix, StringComparison.Ordinal))
                {
                    names[item] = line.Substring(TranslationPrefix.Length).Trim();
                    item = null;
                    translating = false;
                }
            }

            return names;
        }
    }
}
