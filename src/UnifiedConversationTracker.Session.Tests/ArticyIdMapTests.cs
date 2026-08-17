using System;
using System.Text;
using System.Text.Json;
using Xunit;

namespace UnifiedConversationTracker.Session.Tests
{
    /// <summary>
    /// The articy id map: the translation from the ids a savegame's compressed
    /// SimStatus blobs are keyed by to the ids the unified state stores.
    /// </summary>
    /// <remarks>
    /// The real map is a several-megabyte file built from the dialogue database, is
    /// not in the repo, and has not had its provenance audited (de-0m0.21). What is
    /// pinned here is everything that does not need it: the shape of the document, the
    /// last-wins rule that decides which of several entries a duplicated articy id
    /// addresses, the Lua variable name a conversation implies, and that a document
    /// this does not fully understand throws rather than resolving ids wrongly.
    /// </remarks>
    public sealed class ArticyIdMapTests
    {
        /// <summary>
        /// A map with one conversation, one unique entry id and one id shared by three
        /// entries - which is the case the game resolves last-wins.
        /// </summary>
        private const string SampleMap = @"{
            ""conversations"": {
                ""0x0100000400000281"": 9,
                ""-dashed-id"": 12
            },
            ""dialogue_entries"": {
                ""aaaa"": [9, [0]],
                ""bbbb"": [9, [3, 7, 11]],
                ""cccc"": [12, [1]]
            }
        }";

        private static ArticyIdMap Load(string json) => ArticyIdMap.Load(Encoding.UTF8.GetBytes(json));

        [Fact]
        public void Load_ReadsEveryConversationInTheOrderTheDatabaseHasThem()
        {
            ArticyIdMap map = Load(SampleMap);

            Assert.Equal(2, map.Conversations.Count);
            Assert.Equal("0x0100000400000281", map.Conversations[0].ArticyId);
            Assert.Equal(9, map.Conversations[0].ConversationId);
            Assert.Equal(12, map.Conversations[1].ConversationId);
        }

        [Fact]
        public void Load_BuildsTheLuaVariableNameTheGameWouldHaveWritten()
        {
            ArticyIdMap map = Load(SampleMap);

            Assert.Equal(
                "Conversation_SimX_0x0100000400000281", map.Conversations[0].VariableName);
        }

        [Fact]
        public void Load_PutsTheConversationsArticyIdThroughTheGamesTableIndexRule()
        {
            // The variable name is StringToTableIndex(articyId), not the articy id, so
            // an id the rule rewrites has to be rewritten here too or its blob is
            // simply never found.
            ArticyIdMap map = Load(SampleMap);

            Assert.Equal("Conversation_SimX__dashed_id", map.Conversations[1].VariableName);
        }

        [Fact]
        public void TryResolveEntry_ReturnsTheConversationAndEntryForAUniqueArticyId()
        {
            ArticyIdMap map = Load(SampleMap);

            Assert.True(map.TryResolveEntry("aaaa", out int conversationId, out int entryId));
            Assert.Equal(9, conversationId);
            Assert.Equal(0, entryId);
        }

        [Fact]
        public void TryResolveEntry_ForADuplicatedArticyId_ReturnsTheLastEntryAndNotTheFirst()
        {
            // The game builds Dictionary<string, int> over the conversation's entries in
            // order and assigns dictionary[articyId] = entry.id, so the last one wins.
            // Matching that exactly is what makes interception agree with a walk.
            ArticyIdMap map = Load(SampleMap);

            Assert.True(map.TryResolveEntry("bbbb", out _, out int entryId));
            Assert.Equal(11, entryId);
        }

        [Fact]
        public void TryResolveEntry_ForAnUnknownArticyId_SaysSoRatherThanGuessing()
        {
            ArticyIdMap map = Load(SampleMap);

            Assert.False(map.TryResolveEntry("nosuchid", out int conversationId, out int entryId));
            Assert.Equal(0, conversationId);
            Assert.Equal(0, entryId);
        }

        [Fact]
        public void TryResolveEntry_MatchesOrdinally()
        {
            // Articy ids are hex or short alphanumeric tokens and the game compares them
            // ordinally; a culture- or case-insensitive match here would resolve two
            // different entries to one.
            ArticyIdMap map = Load(SampleMap);

            Assert.False(map.TryResolveEntry("AAAA", out _, out _));
        }

        [Fact]
        public void DialogueEntryCount_CountsEveryEntryIncludingTheDuplicateShadowedOnes()
        {
            // This is the population a walk of the master database visits, so it is what
            // a decoded pair count is checked against. Counting distinct articy ids
            // instead would under-count by exactly the duplicates.
            ArticyIdMap map = Load(SampleMap);

            Assert.Equal(5, map.DialogueEntryCount);
            Assert.Equal(3, map.EntryArticyIdCount);
        }

        [Fact]
        public void Load_WithNoConversationsMember_Throws()
        {
            Assert.Throws<JsonException>(
                () => Load(@"{ ""dialogue_entries"": { ""a"": [1, [0]] } }"));
        }

        [Fact]
        public void Load_WithNoDialogueEntriesMember_Throws()
        {
            Assert.Throws<JsonException>(() => Load(@"{ ""conversations"": { ""a"": 1 } }"));
        }

        [Fact]
        public void Load_WithAnEntryThatListsNoEntryIds_Throws()
        {
            // An articy id with nothing behind it cannot resolve to anything, and
            // silently dropping it would look identical to a map that does not cover
            // this database.
            Assert.Throws<JsonException>(
                () => Load(@"{ ""conversations"": {}, ""dialogue_entries"": { ""a"": [1, []] } }"));
        }

        [Fact]
        public void Load_WithAConversationIdThatIsNotANumber_Throws()
        {
            Assert.Throws<JsonException>(
                () => Load(@"{ ""conversations"": { ""a"": ""1"" }, ""dialogue_entries"": {} }"));
        }

        [Fact]
        public void Load_IgnoresMembersItDoesNotKnow()
        {
            // So that a map gaining a field does not stop the mod reading the two
            // objects it actually uses.
            ArticyIdMap map = Load(@"{
                ""built_from"": { ""anything"": [1, 2, 3] },
                ""conversations"": { ""a"": 1 },
                ""dialogue_entries"": { ""b"": [1, [2]] }
            }");

            Assert.Single(map.Conversations);
            Assert.True(map.TryResolveEntry("b", out _, out int entryId));
            Assert.Equal(2, entryId);
        }

        [Fact]
        public void LoadFromFile_ForAMissingFile_ThrowsRatherThanReturningAnEmptyMap()
        {
            // An empty map would resolve nothing and look exactly like a map for the
            // wrong database, so the caller has to be able to tell the two apart.
            Assert.ThrowsAny<Exception>(
                () => ArticyIdMap.LoadFromFile(
                    System.IO.Path.Combine(
                        System.IO.Path.GetTempPath(), Guid.NewGuid().ToString("N") + ".json")));
        }
    }
}
