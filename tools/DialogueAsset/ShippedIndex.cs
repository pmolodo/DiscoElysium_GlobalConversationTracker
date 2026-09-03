// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Linq;

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
        /// The entry fields the engine reads, spelled as the asset spells them.
        /// </summary>
        /// <remarks>
        /// Kept in the same order as the Rust constant, so the two can be read side by
        /// side. Alphabetical would be tidier and would make the comparison harder.
        /// </remarks>
        public static readonly string[] KeptFields =
        {
            "DifficultyPass",
            "DifficultyRed",
            "DifficultyWhite",
            "DifficultyAtmo",
            "HiddenTest",
            "kim_watch",
            "boolean_only",
            "FlagName",
            "ClickCost",
            "CostOnce",
            "HiddenNotEnough",
        };

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

            return new ConversationRecord
            {
                Id = conversation.Id,
                Title = null,
                Actor = null,
                Conversant = null,
                Entries = entries,
            };
        }

        /// <summary>Every conversation, trimmed.</summary>
        public static IEnumerable<ConversationRecord> Trim(
            IEnumerable<ConversationRecord> conversations)
        {
            return conversations.Select(Trim);
        }
    }
}
