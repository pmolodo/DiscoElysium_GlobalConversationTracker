// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>The dialogue entries one articy id names, and the conversation holding them.</summary>
    /// <remarks>
    /// Several entries can share one articy id - about 8.4k of them do - but only ever
    /// within a single conversation, so one conversation id covers the whole group.
    /// </remarks>
    public sealed class ArticyEntryIds
    {
        internal ArticyEntryIds(int conversationId, int entryId)
        {
            ConversationId = conversationId;
            EntryIds = new List<int> { entryId };
        }

        /// <summary>The conversation every entry in <see cref="EntryIds"/> belongs to.</summary>
        public int ConversationId { get; }

        /// <summary>The entry ids sharing the articy id, in the order the database writes them.</summary>
        public List<int> EntryIds { get; }
    }

    /// <summary>
    /// What articy id maps to what: articy id to conversation id, and articy id to the
    /// dialogue entries carrying it.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The game's saves talk about dialogue in articy ids, and the database in its own
    /// conversation and entry ids, so decoding a save needs the crossing. NtwtfDecode
    /// reads the written form of this to rebuild a save's Conversation_SimX_* strings.
    /// </para>
    /// <para>
    /// Built from the same scan as the conversation index, on the conversation records
    /// that scan produces, rather than from a second read of the 170 MB .asset.
    /// </para>
    /// </remarks>
    public sealed class ArticyIdIndex
    {
        /// <summary>The field an articy id is the value of, on a conversation and on an entry.</summary>
        public const string ArticyIdField = "Articy Id";

        private const string HexPrefix = "0x";

        // Articy ids are 64-bit values, written as 0x and 16 hex digits, upper case.
        private const string HexFormat = "X16";

        /// <summary>Articy id to conversation id, in the order the database writes them.</summary>
        public OrderedDictionary<string, int> Conversations { get; } =
            new OrderedDictionary<string, int>(StringComparer.Ordinal);

        /// <summary>Articy id to the entries carrying it, in the order they are first seen.</summary>
        public OrderedDictionary<string, ArticyEntryIds> DialogueEntries { get; } =
            new OrderedDictionary<string, ArticyEntryIds>(StringComparer.Ordinal);

        /// <summary>Builds the index from the database at <paramref name="path"/>.</summary>
        public static ArticyIdIndex Build(string path)
        {
            return Build(ConversationIndexExtractor.Extract(path));
        }

        /// <summary>Builds the index from already-extracted conversations.</summary>
        /// <remarks>
        /// Fails fast on anything that would make the result a lie: a conversation or entry
        /// with no articy id, an entry that disagrees with its parent about which
        /// conversation it is in, two conversations on one articy id, or one articy id
        /// shared across conversations. A missing id needs no check of its own - a record
        /// exists only because its "id:" line did, and an unreadable one throws there.
        /// </remarks>
        public static ArticyIdIndex Build(IEnumerable<ConversationRecord> conversations)
        {
            var index = new ArticyIdIndex();
            foreach (ConversationRecord conversation in conversations)
            {
                string articyId = ArticyIdOf(conversation.Fields)
                    ?? throw new InvalidDataException(
                        $"Conversation {conversation.Id} is missing an Articy Id");
                if (!index.Conversations.TryAdd(articyId, conversation.Id))
                {
                    throw new InvalidDataException($"Duplicate Articy Id {articyId} for conversation "
                        + $"{conversation.Id} (already mapped to {index.Conversations[articyId]})");
                }

                foreach (EntryRecord entry in conversation.Entries)
                {
                    index.Add(conversation, entry);
                }
            }

            return index;
        }

        /// <summary>Renders an articy id as the map keys it on.</summary>
        /// <remarks>
        /// A bare hexadecimal id is canonicalised to <c>0x</c> and 16 upper-case digits: the
        /// Python this was ported from read the .asset as YAML, which turned that scalar
        /// into an integer and rendered it back in that one form. An id carrying a suffix
        /// ("0x0000000000000002-START", "...-FORK") was a string there, and one that is not
        /// hexadecimal at all ("xic5nycau3n8v6nt") never looked like a number, so both pass
        /// through untouched.
        /// </remarks>
        public static string Canonicalize(string value)
        {
            if (!value.StartsWith(HexPrefix, StringComparison.Ordinal))
            {
                return value;
            }

            string digits = value.Substring(HexPrefix.Length);
            if (digits.Length == 0 || !IsHex(digits))
            {
                return value;
            }

            if (!ulong.TryParse(digits, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out ulong id))
            {
                throw new InvalidDataException(
                    $"Articy Id {value} is hexadecimal but does not fit in the 64 bits one takes");
            }

            return HexPrefix + id.ToString(HexFormat, CultureInfo.InvariantCulture);
        }

        private static string? ArticyIdOf(OrderedDictionary<string, string> fields)
        {
            if (!fields.TryGetValue(ArticyIdField, out string? value) || value.Length == 0)
            {
                return null;
            }

            return Canonicalize(value);
        }

        private static bool IsHex(string value)
        {
            foreach (char c in value)
            {
                if (!Uri.IsHexDigit(c))
                {
                    return false;
                }
            }

            return true;
        }

        private void Add(ConversationRecord conversation, EntryRecord entry)
        {
            if (entry.ConversationId != conversation.Id)
            {
                throw new InvalidDataException($"Dialogue entry {entry.Id} has conversationID "
                    + $"{entry.ConversationId?.ToString(CultureInfo.InvariantCulture) ?? "none"}, which does "
                    + $"not match parent conversation id {conversation.Id}");
            }

            string articyId = ArticyIdOf(entry.Fields)
                ?? throw new InvalidDataException(
                    $"Dialogue entry {conversation.Id}:{entry.Id} is missing an Articy Id");
            if (!DialogueEntries.TryGetValue(articyId, out ArticyEntryIds? existing))
            {
                DialogueEntries.Add(articyId, new ArticyEntryIds(conversation.Id, entry.Id));
                return;
            }

            if (existing.ConversationId != conversation.Id)
            {
                throw new InvalidDataException($"Articy Id {articyId} is shared by dialogue entries in "
                    + $"different conversations ({existing.ConversationId} and {conversation.Id})");
            }

            existing.EntryIds.Add(entry.Id);
        }
    }
}
