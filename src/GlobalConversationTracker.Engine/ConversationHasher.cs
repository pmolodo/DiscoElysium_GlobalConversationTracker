// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.Security.Cryptography;
using System.Text;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// The content of one conversation, reduced to a string two different readers can
    /// agree on.
    /// </summary>
    /// <remarks>
    /// <para>The index the mod ships is a CACHE of the dialogue database, not ground truth.
    /// It is valid exactly while it still describes the database the player's game actually
    /// loaded, and a game patch, a localisation or another mod can end that. This is how
    /// the two are compared.</para>
    ///
    /// <para>ONE ROUTINE, TWO CALLERS, and that is the whole point. The extractor feeds it
    /// records parsed out of 170 MB of YAML; the plugin feeds it the live
    /// <c>DialogueDatabase</c> out of the running game. Two representations that must
    /// produce identical bytes is already a drift risk, and writing the reduction twice
    /// would make it a certainty. Rust never hashes at all - it stores what the extractor
    /// wrote and hands it back - so there is no third writer.</para>
    ///
    /// <para>WHAT IS COMPARED is what the engine reads: a conversation's id, its fields in
    /// <see cref="IndexFields.ConversationRead"/> where it has any, and per entry its id, group
    /// flag, guard, script, links and the fields in <see cref="IndexFields.Read"/>. Nothing
    /// else. A patch that rewrites dialogue text
    /// changes nothing a crawl can observe, and a cache check that rebuilt the index over
    /// it would be reporting a difference that does not exist.</para>
    ///
    /// <para>WHAT IS SORTED, and why it is not laziness: entries by id, fields by name, and
    /// links by where they go. The two sources walk their data in their own orders, and an
    /// order that differs is not a difference in the graph - so sorting removes a whole
    /// class of rebuild that would have been triggered by nothing at all. What is NOT
    /// sorted away is content: two conversations differing in any value differ here.</para>
    ///
    /// <para>SHA-256, and not <see cref="object.GetHashCode"/>. String hashing is
    /// randomised per process in .NET Core; it is the obvious thing to reach for, the
    /// extractor and the plugin are different processes, and the result would be a cache
    /// miss on every single launch - a bug that reads as the game updating constantly.
    /// </para>
    /// </remarks>
    public sealed class ConversationHasher
    {
        /// <summary>
        /// Separates the parts of one entry.
        /// </summary>
        /// <remarks>
        /// Cosmetic rather than structural: every value is length-prefixed, so a value
        /// that contains this character changes nothing. It is here so a canonical form
        /// somebody has to read in a diff has somewhere to break.
        /// </remarks>
        private const char Separator = '|';

        private readonly int _conversation;
        private readonly SortedDictionary<int, string> _entries = new SortedDictionary<int, string>();
        private readonly SortedDictionary<string, string> _fields =
            new SortedDictionary<string, string>(StringComparer.Ordinal);

        /// <summary>Begins a conversation.</summary>
        /// <param name="conversation">Its id.</param>
        public ConversationHasher(int conversation)
        {
            _conversation = conversation;
        }

        /// <summary>Adds one dialogue entry.</summary>
        /// <param name="id">The entry id, unique within the conversation.</param>
        /// <param name="group">Whether it is a group node rather than a selectable line.</param>
        /// <param name="guard">Its conditions, verbatim.</param>
        /// <param name="script">Its user script, verbatim.</param>
        /// <param name="links">
        /// Where its outgoing links go, as (conversation, entry) pairs. A link inside the
        /// entry's own conversation carries that conversation's id, not a placeholder: the
        /// index writes it as an absent key, and both callers have to resolve that the same
        /// way before getting here.
        /// </param>
        /// <param name="fields">
        /// Its fields. Anything outside <see cref="IndexFields.Read"/> is ignored here, so
        /// a caller may hand over everything it has.
        /// </param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="ArgumentException">The entry id was already added.</exception>
        public void Add(
            int id,
            bool group,
            string guard,
            string script,
            IEnumerable<KeyValuePair<int, int>> links,
            IEnumerable<KeyValuePair<string, string>> fields)
        {
            if (guard == null)
            {
                throw new ArgumentNullException(nameof(guard));
            }

            if (script == null)
            {
                throw new ArgumentNullException(nameof(script));
            }

            if (links == null)
            {
                throw new ArgumentNullException(nameof(links));
            }

            if (fields == null)
            {
                throw new ArgumentNullException(nameof(fields));
            }

            if (_entries.ContainsKey(id))
            {
                // Not tolerated, because the two sources would have to disagree about the
                // database's shape for it to happen, and quietly keeping one of the two
                // would make the hashes differ for a reason nothing reports.
                throw new ArgumentException(
                    $"Entry {id} was added to conversation {_conversation} twice.",
                    nameof(id));
            }

            var canonical = new StringBuilder();
            Append(canonical, id.ToString(CultureInfo.InvariantCulture));
            Append(canonical, group ? "1" : "0");
            Append(canonical, Normalised(guard));
            Append(canonical, Normalised(script));

            var destinations = new List<KeyValuePair<int, int>>(links);
            destinations.Sort(CompareLinks);
            Append(canonical, destinations.Count.ToString(CultureInfo.InvariantCulture));
            foreach (KeyValuePair<int, int> link in destinations)
            {
                Append(canonical, link.Key.ToString(CultureInfo.InvariantCulture));
                Append(canonical, link.Value.ToString(CultureInfo.InvariantCulture));
            }

            var kept = new SortedDictionary<string, string>(StringComparer.Ordinal);
            foreach (KeyValuePair<string, string> field in fields)
            {
                if (Array.IndexOf(IndexFields.Read, field.Key) >= 0)
                {
                    kept[field.Key] = field.Value ?? string.Empty;
                }
            }

            Append(canonical, kept.Count.ToString(CultureInfo.InvariantCulture));
            foreach (KeyValuePair<string, string> field in kept)
            {
                Append(canonical, field.Key);
                Append(canonical, field.Value);
            }

            _entries[id] = canonical.ToString();
        }

        /// <summary>Adds the conversation's own fields.</summary>
        /// <param name="fields">
        /// Its fields. Anything outside <see cref="IndexFields.ConversationRead"/> is ignored, so a
        /// caller may hand over everything it has.
        /// </param>
        /// <exception cref="ArgumentNullException">The fields are null.</exception>
        public void AddConversationFields(IEnumerable<KeyValuePair<string, string>> fields)
        {
            if (fields == null)
            {
                throw new ArgumentNullException(nameof(fields));
            }

            foreach (KeyValuePair<string, string> field in fields)
            {
                if (Array.IndexOf(IndexFields.ConversationRead, field.Key) >= 0)
                {
                    _fields[field.Key] = field.Value ?? string.Empty;
                }
            }
        }

        /// <summary>
        /// The hash of everything added, as lower-case hex.
        /// </summary>
        /// <remarks>
        /// May be called more than once; nothing is consumed.
        /// </remarks>
        public string Finish()
        {
            using var sha = SHA256.Create();
            byte[] digest = sha.ComputeHash(new UTF8Encoding(false).GetBytes(Canonical()));

            var hex = new StringBuilder(digest.Length * 2);
            foreach (byte value in digest)
            {
                hex.Append(value.ToString("x2", CultureInfo.InvariantCulture));
            }

            return hex.ToString();
        }

        /// <summary>The canonical form itself, for a test that needs to see it.</summary>
        /// <remarks>
        /// Exposed because a hash that differs says only that something differs. When the
        /// extractor and the plugin disagree, the useful question is WHERE, and comparing
        /// two canonical strings answers it in one diff.
        /// </remarks>
        public string Canonical()
        {
            var canonical = new StringBuilder();
            Append(canonical, _conversation.ToString(CultureInfo.InvariantCulture));
            Append(canonical, _entries.Count.ToString(CultureInfo.InvariantCulture));
            foreach (KeyValuePair<int, string> entry in _entries)
            {
                canonical.Append(entry.Value);
            }

            // ONLY WHERE THERE ARE ANY, so a conversation that is not a journal task reduces to
            // exactly what it did before conversations carried fields at all.
            if (_fields.Count > 0)
            {
                Append(canonical, _fields.Count.ToString(CultureInfo.InvariantCulture));
                foreach (KeyValuePair<string, string> field in _fields)
                {
                    Append(canonical, field.Key);
                    Append(canonical, field.Value);
                }
            }

            return canonical.ToString();
        }

        /// <summary>
        /// One text, with the statement separator written the same way whichever source it
        /// came from.
        /// </summary>
        /// <remarks>
        /// <para>THE TWO SOURCES SPELL IT DIFFERENTLY, and this is not a detail. A script
        /// separates its statements with a backslash and the letter n. The extractor reads
        /// the database's YAML, where those are two literal characters; the plugin reads
        /// the live PixelCrushers objects, where the Dialogue System has already turned
        /// them into a real newline. Measured in the game: conversation 451's entries 16
        /// and 80 differ in exactly that and nothing else.</para>
        ///
        /// <para>Left alone, every conversation with a two-statement script anywhere in it
        /// hashes differently from itself, the cache misses, and the plugin rebuilds a 15 MB
        /// index on first launch for no reason at all. Normalising is the correct fix rather
        /// than a convenience: the ACTION PARSER already turns the escape into a newline
        /// before it reads anything, so the two forms are the same script as far as anything
        /// downstream is concerned - and a hash that separates things the engine cannot tell
        /// apart is reporting a difference that does not exist.</para>
        /// </remarks>
        private static string Normalised(string text)
        {
            return text.IndexOf(SeparatorEscape, StringComparison.Ordinal) < 0
                ? text
                : text.Replace(SeparatorEscape, "\n");
        }

        /// <summary>
        /// How a script separates two statements, as the database's own text spells it.
        /// </summary>
        /// <remarks>
        /// Two characters, a backslash and an n, and not an escape sequence in this source
        /// file - which is why it is written with a doubled backslash.
        /// </remarks>
        private const string SeparatorEscape = "\\n";

        /// <summary>By destination conversation, then by destination entry.</summary>
        private static int CompareLinks(KeyValuePair<int, int> left, KeyValuePair<int, int> right)
        {
            int byConversation = left.Key.CompareTo(right.Key);
            return byConversation != 0 ? byConversation : left.Value.CompareTo(right.Value);
        }

        /// <summary>
        /// Writes one value, prefixed by its length.
        /// </summary>
        /// <remarks>
        /// LENGTH-PREFIXED, so the reduction is injective: without it a guard ending in the
        /// separator and an empty script would canonicalise to the same bytes as a shorter
        /// guard and a script that began with one. That is a collision by construction
        /// rather than by luck, and it would make two different conversations look like the
        /// same cached one.
        /// </remarks>
        private static void Append(StringBuilder canonical, string value)
        {
            canonical
                .Append(value.Length.ToString(CultureInfo.InvariantCulture))
                .Append(Separator)
                .Append(value)
                .Append(Separator);
        }
    }
}
