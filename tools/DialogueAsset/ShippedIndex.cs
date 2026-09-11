// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using GlobalConversationTracker.Engine;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>
    /// The index as the mod ships it: everything a crawl reads and nothing else.
    /// </summary>
    /// <remarks>
    /// <para>The full index is 50 MB and nine tenths of it is content for a reader rather
    /// than for a search - 5.7 MB of Dialogue Text, 4.9 MB of titles, 2.0 MB of articy
    /// ids, 3.3 MB of input and output ids. Trimming to what the engine consults gives
    /// 13.6 MB, which is 1.2 MB in a release archive against 7.1 MB for the whole thing,
    /// and the same again off the parse at startup.</para>
    ///
    /// <para>WHAT SURVIVES is decided by what the Rust engine deserialises, which is a
    /// conversation's id and entries, and per entry its id, group flag, guard, script,
    /// links and <see cref="KeptFields"/>. Nothing else is read anywhere, so nothing else
    /// is carried.</para>
    ///
    /// <para>The list below must match <c>lookahead_engine::index::ENTRY_FIELDS_READ</c>.
    /// It cannot share a constant with it across the language boundary, so
    /// <c>tests/shipped_index.rs</c> checks the two against each other - and that check is
    /// not ceremony: the first draft of this trim guessed the names, got all eleven wrong,
    /// and would have produced an index in which no entry was a skill check, with nothing
    /// failing to say so.</para>
    /// </remarks>
    public static class ShippedIndex
    {
        /// <summary>What the trimmed index is called.</summary>
        public const string FileName = "conversation_index.trimmed.jsonl";

        /// <summary>
        /// What this build writes, and the oldest it can read.
        /// </summary>
        /// <remarks>
        /// A CONTENT HASH IS NOT ENOUGH ON ITS OWN. It answers "is this the same game";
        /// this answers "is this an index this engine can read". The format changes - the
        /// hash itself is version 1, and it was version 0 before there was one - and an
        /// index from an older build would pass its content hash while missing fields the
        /// engine has since started reading. That is a cache hit on a file that cannot
        /// answer the question, which is worse than a miss.
        /// </remarks>
        public const int FormatVersion = 2;

        /// <summary>The header line's version property.</summary>
        /// <remarks>
        /// Must match <c>lookahead_engine::index::FORMAT_PROPERTY</c>, which reads the same
        /// line.
        /// </remarks>
        public const string FormatProperty = "format";

        /// <summary>
        /// The entry fields the engine reads, spelled as the asset spells them.
        /// </summary>
        /// <remarks>
        /// The list itself lives in <see cref="IndexFields.Read"/>, because the plugin
        /// canonicalises the live database over the same names and two copies of it in one
        /// language would be two things to keep in step.
        /// </remarks>
        public static string[] KeptFields => IndexFields.Read;

        /// <summary>The line that opens a shipped index.</summary>
        /// <remarks>
        /// Its own line rather than a property on every conversation: it describes the
        /// FILE, and repeating it 1,501 times would invite the file to disagree with
        /// itself.
        /// </remarks>
        public static string Header()
        {
            return "{\"" + FormatProperty + "\":"
                + FormatVersion.ToString(CultureInfo.InvariantCulture) + "}";
        }

        /// <summary>
        /// What one conversation's content reduces to, for comparing a shipped index
        /// against the database a game actually loaded.
        /// </summary>
        /// <remarks>
        /// Over the TRIMMED record, so that what is hashed is what the engine reads. A
        /// caller may hand over an untrimmed one; the hasher ignores everything outside
        /// <see cref="IndexFields.Read"/> for itself.
        /// </remarks>
        public static string HashOf(ConversationRecord conversation)
        {
            if (conversation == null)
            {
                throw new ArgumentNullException(nameof(conversation));
            }

            var hasher = new ConversationHasher(conversation.Id);
            foreach (EntryRecord entry in conversation.Entries)
            {
                hasher.Add(
                    entry.Id,
                    entry.Group,
                    entry.Guard,
                    entry.Script,
                    LinksOf(conversation.Id, entry),
                    entry.Fields);
            }

            return hasher.Finish();
        }

        /// <summary>
        /// Where an entry's links go, as (conversation, entry) pairs.
        /// </summary>
        /// <remarks>
        /// The index omits <c>to_conversation</c> where every link stays inside the entry's
        /// own conversation, and may write a short one - so a missing element means "this
        /// conversation". Resolved here rather than left to the hasher, because the plugin
        /// reads the live database where every link names its destination outright and
        /// there is nothing to resolve.
        /// </remarks>
        private static IEnumerable<KeyValuePair<int, int>> LinksOf(
            int conversationId, EntryRecord entry)
        {
            for (int index = 0; index < entry.To.Count; index++)
            {
                int destination =
                    entry.ToConversation != null && index < entry.ToConversation.Count
                        ? entry.ToConversation[index]
                        : conversationId;
                yield return new KeyValuePair<int, int>(destination, entry.To[index]);
            }
        }

        /// <summary>One conversation with everything the engine ignores removed.</summary>
        public static ConversationRecord Trim(ConversationRecord conversation)
        {
            if (conversation == null)
            {
                throw new ArgumentNullException(nameof(conversation));
            }

            var kept = new HashSet<string>(KeptFields, StringComparer.Ordinal);
            var entries = new List<EntryRecord>(conversation.Entries.Count);
            foreach (EntryRecord entry in conversation.Entries)
            {
                var fields = new OrderedDictionary<string, string>();
                foreach (KeyValuePair<string, string> field in entry.Fields)
                {
                    if (kept.Contains(field.Key))
                    {
                        fields[field.Key] = field.Value;
                    }
                }

                entries.Add(new EntryRecord
                {
                    Id = entry.Id,
                    Group = entry.Group,
                    Guard = entry.Guard,
                    Script = entry.Script,
                    To = entry.To,
                    ToConversation = entry.ToConversation,
                    // The engine never deserialises a title, so it does not travel. It is
                    // 4.9 MB across the database and the single largest thing that goes.
                    Title = null,
                    Fields = fields,
                });
            }

            var trimmed = new ConversationRecord
            {
                Id = conversation.Id,
                Title = null,
                Actor = null,
                Conversant = null,
                Entries = entries,
            };
            trimmed.Hash = HashOf(trimmed);
            return trimmed;
        }

        /// <summary>Every conversation, trimmed.</summary>
        public static IEnumerable<ConversationRecord> Trim(
            IEnumerable<ConversationRecord> conversations)
        {
            return conversations.Select(Trim);
        }
    }
}
