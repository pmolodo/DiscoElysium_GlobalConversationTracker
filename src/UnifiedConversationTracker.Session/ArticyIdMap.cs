using System;
using System.Collections.Generic;
using System.IO;
using System.Text.Json;

namespace UnifiedConversationTracker.Session
{
    /// <summary>
    /// One conversation, as the compressed-SimStatus path needs to address it: the
    /// articy id the savegame keys it by, the Lua variable name that implies, and the
    /// integer conversation id the unified state stores.
    /// </summary>
    public readonly struct ArticyConversation
    {
        /// <summary>Creates a conversation entry.</summary>
        /// <param name="articyId">The conversation's raw articy id.</param>
        /// <param name="variableName">The Lua <c>Variable</c> key its blob lives under.</param>
        /// <param name="conversationId">The conversation's integer id.</param>
        public ArticyConversation(string articyId, string variableName, int conversationId)
        {
            ArticyId = articyId;
            VariableName = variableName;
            ConversationId = conversationId;
        }

        /// <summary>The raw articy id, exactly as the dialogue database spells it.</summary>
        public string ArticyId { get; }

        /// <summary>
        /// The key into the Lua <c>Variable</c> table this conversation's compressed
        /// blob is stored under, precomputed so the load path does no string work.
        /// </summary>
        public string VariableName { get; }

        /// <summary>The conversation's integer id, as the unified state stores it.</summary>
        public int ConversationId { get; }

        /// <inheritdoc />
        public override string ToString() =>
            $"ArticyConversation({ArticyId}, {VariableName}, {ConversationId})";
    }

    /// <summary>
    /// Translates the articy ids a savegame's compressed SimStatus blobs are keyed by
    /// into the (conversation id, dialogue entry id) pairs the unified state stores.
    /// </summary>
    /// <remarks>
    /// <para><b>Why this is needed at all</b> (de-0m0.19). Disco Elysium sets
    /// <c>PersistentDataManager.saveConversationSimStatusWithField</c> and
    /// <c>saveDialogueEntrySimStatusWithField</c> to non-empty field names, which puts
    /// the Dialogue System on the branch that keys the compressed form by ARTICY ID
    /// rather than by integer id. Confirmed from real Final Cut save data, where both
    /// the conversation key and every entry key inside the blob is an articy id.</para>
    ///
    /// <para><b>Where the mapping comes from.</b> A JSON file produced by
    /// <c>read_articy_ids.py</c> from the Unity-serialized dialogue database, with two
    /// objects: <c>conversations</c> mapping articy id to conversation id, and
    /// <c>dialogue_entries</c> mapping articy id to
    /// <c>[conversationId, [entryId, ...]]</c>. Loading the real 7.7 MB file takes
    /// 130-150 ms, which is why the plugin does it at startup rather than on the load
    /// path. Run through this class and <see cref="CompressedSimStatusDecoder"/>
    /// against a real savegame it resolved 1,494 of 1,494 conversations and 112,940 of
    /// 112,940 entry ids, with nothing unresolved, nothing foreign, and a row set
    /// identical to that save's own expanded SimStatus. Its PROVENANCE has still not
    /// been audited (de-0m0.21) - resolving one save perfectly is not the same as being
    /// derived correctly - which is why nothing here assumes the map is present or
    /// right: a missing map disables interception, and an id this cannot resolve makes
    /// the caller fall back to walking the master database.</para>
    ///
    /// <para><b>Last-wins is baked into the entry lookup, deliberately.</b> Articy ids
    /// are not unique within a conversation - 26.3% of the pairs in a real save use a
    /// duplicated one - and the game resolves the ambiguity by building
    /// <c>Dictionary&lt;string, int&gt;</c> over the conversation's entries in order
    /// and assigning <c>dictionary[articyValue] = entry.id</c>
    /// (PersistentDataManager.cs:800-805), so the LAST entry carrying an articy id is
    /// the one that id addresses. This keeps only that last id, for the same reason:
    /// reproducing the game's post-load state exactly matters more than recovering
    /// statuses the game itself drops. See <see cref="CompressedSimStatusDecoder"/>
    /// for the other half of the same rule.</para>
    ///
    /// <para><b>Load it off the load path.</b> It is a pure function of the dialogue
    /// database - identical for every save and every session - and parsing several
    /// megabytes of JSON inside the hook would spend the budget interception exists
    /// to save. The plugin loads it once at startup.</para>
    /// </remarks>
    public sealed class ArticyIdMap
    {
        /// <summary>
        /// The prefix the Dialogue System puts in front of a conversation's articy id
        /// to name the Lua variable its compressed blob lives under
        /// (PersistentDataManager.cs:782).
        /// </summary>
        public const string ConversationVariablePrefix = "Conversation_SimX_";

        /// <summary>The JSON member holding articy id -&gt; conversation id.</summary>
        private const string ConversationsMemberName = "conversations";

        /// <summary>The JSON member holding articy id -&gt; [conversation id, [entry id, ...]].</summary>
        private const string DialogueEntriesMemberName = "dialogue_entries";

        private readonly ArticyConversation[] _conversations;
        private readonly Dictionary<string, ArticyEntry> _entries;

        private ArticyIdMap(
            ArticyConversation[] conversations,
            Dictionary<string, ArticyEntry> entries,
            int dialogueEntryCount)
        {
            _conversations = conversations;
            _entries = entries;
            DialogueEntryCount = dialogueEntryCount;
        }

        /// <summary>
        /// Every conversation the dialogue database has, in database order. This is
        /// the population the load path iterates: one keyed lookup into the Lua
        /// <c>Variable</c> table apiece, rather than a scan of the whole table.
        /// </summary>
        public IReadOnlyList<ArticyConversation> Conversations => _conversations;

        /// <summary>How many distinct articy ids address dialogue entries.</summary>
        public int EntryArticyIdCount => _entries.Count;

        /// <summary>
        /// How many dialogue entries the database has in total, counting every entry
        /// that shares a duplicated articy id. This is the population a walk of the
        /// master database would visit, and therefore what a decoded pair count is
        /// sanity-checked against.
        /// </summary>
        public int DialogueEntryCount { get; }

        /// <summary>
        /// Resolves one articy id from a blob to the dialogue entry the game would
        /// have written the status into.
        /// </summary>
        /// <param name="articyId">The id as the blob spells it.</param>
        /// <param name="conversationId">
        /// The conversation the map says the id belongs to. Set to 0 when the id is
        /// unknown.
        /// </param>
        /// <param name="dialogueEntryId">
        /// The entry id, which for a duplicated articy id is the last entry carrying
        /// it. Set to 0 when the id is unknown.
        /// </param>
        /// <returns>True if the map knows the id.</returns>
        public bool TryResolveEntry(string articyId, out int conversationId, out int dialogueEntryId)
        {
            if (_entries.TryGetValue(articyId, out ArticyEntry entry))
            {
                conversationId = entry.ConversationId;
                dialogueEntryId = entry.DialogueEntryId;
                return true;
            }

            conversationId = 0;
            dialogueEntryId = 0;
            return false;
        }

        /// <summary>Reads a map from a file.</summary>
        /// <param name="path">The JSON file's path.</param>
        /// <exception cref="IOException">The file could not be read.</exception>
        /// <exception cref="UnauthorizedAccessException">The file could not be read.</exception>
        /// <exception cref="JsonException">The file is not a map.</exception>
        public static ArticyIdMap LoadFromFile(string path) => Load(File.ReadAllBytes(path));

        /// <summary>Reads a map from JSON.</summary>
        /// <param name="utf8Json">The whole document, as UTF-8 bytes.</param>
        /// <exception cref="JsonException">The document is not a map.</exception>
        /// <remarks>
        /// Hand-parsed with <see cref="Utf8JsonReader"/> rather than deserialized into
        /// objects: the file is several megabytes and a deserialized form would
        /// materialize a boxed list per entry only to throw all but one element of it
        /// away. Anything the document gets wrong throws, because a half-understood
        /// map would resolve ids to the wrong entries silently.
        /// </remarks>
        public static ArticyIdMap Load(ReadOnlySpan<byte> utf8Json)
        {
            var reader = new Utf8JsonReader(utf8Json);
            List<ArticyConversation>? conversations = null;
            Dictionary<string, ArticyEntry>? entries = null;
            int dialogueEntryCount = 0;

            ReadExpecting(ref reader, JsonTokenType.StartObject);
            while (Read(ref reader) && reader.TokenType != JsonTokenType.EndObject)
            {
                Expect(reader, JsonTokenType.PropertyName);
                string member = reader.GetString()!;
                switch (member)
                {
                    case ConversationsMemberName:
                        conversations = ReadConversations(ref reader);
                        break;
                    case DialogueEntriesMemberName:
                        entries = ReadDialogueEntries(ref reader, out dialogueEntryCount);
                        break;
                    default:
                        Read(ref reader);
                        reader.Skip();
                        break;
                }
            }

            if (conversations == null)
            {
                throw new JsonException($"The articy id map has no '{ConversationsMemberName}' member.");
            }

            if (entries == null)
            {
                throw new JsonException($"The articy id map has no '{DialogueEntriesMemberName}' member.");
            }

            return new ArticyIdMap(conversations.ToArray(), entries, dialogueEntryCount);
        }

        private static List<ArticyConversation> ReadConversations(ref Utf8JsonReader reader)
        {
            var conversations = new List<ArticyConversation>();
            ReadExpecting(ref reader, JsonTokenType.StartObject);
            while (Read(ref reader) && reader.TokenType != JsonTokenType.EndObject)
            {
                Expect(reader, JsonTokenType.PropertyName);
                string articyId = reader.GetString()!;
                ReadExpecting(ref reader, JsonTokenType.Number);
                conversations.Add(new ArticyConversation(
                    articyId,
                    ConversationVariablePrefix + DialogueLuaTableIndex.Of(articyId),
                    reader.GetInt32()));
            }

            return conversations;
        }

        private static Dictionary<string, ArticyEntry> ReadDialogueEntries(
            ref Utf8JsonReader reader, out int dialogueEntryCount)
        {
            var entries = new Dictionary<string, ArticyEntry>(StringComparer.Ordinal);
            dialogueEntryCount = 0;

            ReadExpecting(ref reader, JsonTokenType.StartObject);
            while (Read(ref reader) && reader.TokenType != JsonTokenType.EndObject)
            {
                Expect(reader, JsonTokenType.PropertyName);
                string articyId = reader.GetString()!;

                ReadExpecting(ref reader, JsonTokenType.StartArray);
                ReadExpecting(ref reader, JsonTokenType.Number);
                int conversationId = reader.GetInt32();

                ReadExpecting(ref reader, JsonTokenType.StartArray);
                int lastEntryId = 0;
                bool sawAnEntry = false;
                while (Read(ref reader) && reader.TokenType != JsonTokenType.EndArray)
                {
                    Expect(reader, JsonTokenType.Number);

                    // Last one wins, matching the game's own Dictionary assignment over
                    // the conversation's entries in order.
                    lastEntryId = reader.GetInt32();
                    sawAnEntry = true;
                    dialogueEntryCount++;
                }

                if (!sawAnEntry)
                {
                    throw new JsonException(
                        $"The articy id map lists no dialogue entry for articy id '{articyId}'.");
                }

                ReadExpecting(ref reader, JsonTokenType.EndArray);
                entries[articyId] = new ArticyEntry(conversationId, lastEntryId);
            }

            return entries;
        }

        private static bool Read(ref Utf8JsonReader reader)
        {
            if (reader.Read())
            {
                return true;
            }

            throw new JsonException("The articy id map ended in the middle of a value.");
        }

        private static void ReadExpecting(ref Utf8JsonReader reader, JsonTokenType expected)
        {
            Read(ref reader);
            Expect(reader, expected);
        }

        private static void Expect(in Utf8JsonReader reader, JsonTokenType expected)
        {
            if (reader.TokenType != expected)
            {
                throw new JsonException(
                    $"The articy id map has a {reader.TokenType} where a {expected} was expected.");
            }
        }

        /// <inheritdoc />
        public override string ToString() =>
            $"ArticyIdMap({_conversations.Length} conversations, {_entries.Count} entry articy ids, "
            + $"{DialogueEntryCount} entries)";

        /// <summary>One articy id's resolution, after last-wins has been applied.</summary>
        private readonly struct ArticyEntry
        {
            public ArticyEntry(int conversationId, int dialogueEntryId)
            {
                ConversationId = conversationId;
                DialogueEntryId = dialogueEntryId;
            }

            public int ConversationId { get; }

            public int DialogueEntryId { get; }
        }
    }
}
