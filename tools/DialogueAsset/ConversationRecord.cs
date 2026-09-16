// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.Text.Json.Serialization;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>One conversation, as one line of the conversation index.</summary>
    public sealed class ConversationRecord
    {
        /// <summary>The conversation id the game knows it by.</summary>
        public int Id { get; set; }

        /// <summary>The conversation's Title field, or null where it has none.</summary>
        public string? Title { get; set; }

        /// <summary>The conversation's Actor field as a number, or null where it has none.</summary>
        public int? Actor { get; set; }

        /// <summary>The conversation's Conversant field as a number, or null where it has none.</summary>
        public int? Conversant { get; set; }

        /// <summary>
        /// Every field on the conversation, as entries already keep theirs. The three
        /// above are read out of this one and kept beside it because the index line
        /// carries them by name; the rest are here for readers that want a field the
        /// index never named, such as the Articy Id the articy-ids output is built from.
        /// </summary>
        /// <remarks>
        /// Not written to the conversation index and not read back from it: the index is
        /// compared byte for byte against the last one written, so the line's shape is
        /// fixed. A reader that needs these has to extract from the .asset.
        /// </remarks>
        [JsonIgnore]
        public OrderedDictionary<string, string> Fields { get; set; } = new OrderedDictionary<string, string>();

        /// <summary>
        /// The conversation fields the engine reads - a journal task's conditions, named in
        /// <c>IndexFields.ConversationRead</c> - or null where it has none.
        /// </summary>
        /// <remarks>
        /// Written to the index as <c>fields</c>, and only where it is not null, so every line
        /// of a conversation that is not a journal task is what it was before.
        /// </remarks>
        [JsonPropertyName("fields")]
        public OrderedDictionary<string, string>? IndexedFields { get; set; }

        /// <summary>
        /// What this conversation's content reduces to, or null where nothing computed it.
        /// </summary>
        /// <remarks>
        /// Carried by the SHIPPED index and not by the full one. The shipped index is a
        /// cache of a database the plugin can also read for itself, and this is what makes
        /// the two comparable - see <c>ConversationHasher</c>. The full index is a build
        /// intermediate that nothing validates against anything, so it carries none and the
        /// key stays absent from its lines.
        /// </remarks>
        public string? Hash { get; set; }

        /// <summary>Its dialogue entries, in the order the database writes them.</summary>
        public List<EntryRecord> Entries { get; set; } = new List<EntryRecord>();
    }

    /// <summary>One dialogue entry within a conversation.</summary>
    public sealed class EntryRecord
    {
        /// <summary>The entry id, unique within its conversation.</summary>
        public int Id { get; set; }

        /// <summary>
        /// The conversation this entry says it belongs to, or null where it says nothing.
        /// Always the id of the conversation it was written inside, in a database that is
        /// not corrupt, which is worth checking before trusting either.
        /// </summary>
        /// <remarks>Not part of the index line: an entry is already inside its conversation there.</remarks>
        [JsonIgnore]
        public int? ConversationId { get; set; }

        /// <summary>Whether the entry is a group node rather than a selectable line.</summary>
        public bool Group { get; set; }

        /// <summary>The entry's conditionsString, verbatim.</summary>
        public string Guard { get; set; } = string.Empty;

        /// <summary>The entry's userScript, verbatim.</summary>
        public string Script { get; set; } = string.Empty;

        /// <summary>The destination entry id of each outgoing link, in file order.</summary>
        public List<int> To { get; set; } = new List<int>();

        /// <summary>The entry's Title field, or null where it has none.</summary>
        public string? Title { get; set; }

        /// <summary>
        /// Every field on the entry, not a hand-maintained subset: the offline crawler
        /// reads several of them by presence alone (DifficultyPass, DifficultyRed,
        /// kim_watch and others), and keeping them all makes a newer graph model possible
        /// without regenerating an index from the 170 MB source asset.
        /// </summary>
        /// <remarks>
        /// Ordered, because the index is compared byte for byte against what the database
        /// produced last time and field order is part of those bytes.
        /// </remarks>
        public OrderedDictionary<string, string> Fields { get; set; } = new OrderedDictionary<string, string>();

        /// <summary>
        /// The destination conversation id of each outgoing link, in file order, or null
        /// where the entry has no links at all. Null rather than empty because the key is
        /// absent from the index in that case.
        /// </summary>
        [JsonPropertyName("to_conversation")]
        public List<int>? ToConversation { get; set; }
    }
}
