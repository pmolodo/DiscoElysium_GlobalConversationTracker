// SPDX-License-Identifier: MIT
using Xunit;

namespace GlobalConversationTracker.DialogueAsset.Tests
{
    /// <summary>What the articy id maps hold, what they refuse, and what they are written as.</summary>
    public class ArticyIdTests
    {
        private const string ArticyIdField = ArticyIdIndex.ArticyIdField;

        private static readonly string FixtureAsset =
            Path.Combine(AppContext.BaseDirectory, "Fixtures", "articy-database.asset");

        private static readonly string ExpectedJson =
            Path.Combine(AppContext.BaseDirectory, "Fixtures", "articy-database.expected.json");

        private static readonly string IndexFixtureAsset =
            Path.Combine(AppContext.BaseDirectory, "Fixtures", "mini-database.asset");

        [Fact]
        public void WritesTheFileTheOldExtractorWrote()
        {
            ArticyIdIndex index = ArticyIdIndex.Build(FixtureAsset);

            // The expected file was written by the Python this replaced, so the format is
            // pinned to a record of the old behavior. Compared line by line because the
            // repository's end-of-file hook has since put a line break after its last
            // brace, and the real file has none - which this comparison still holds the
            // output to, the joined lines ending at the brace.
            Assert.Equal(string.Join(Environment.NewLine, File.ReadAllLines(ExpectedJson)),
                ArticyIdFile.ToJson(index));
        }

        [Fact]
        public void GroupsTheEntriesSharingOneArticyIdUnderTheirConversation()
        {
            ArticyIdIndex index = ArticyIdIndex.Build(FixtureAsset);

            Assert.Equal(new[] { "0x0000000000000000", "1owioef_a1f7_r3z" }, index.Conversations.Keys);
            Assert.Equal(1, index.Conversations["0x0000000000000000"]);
            ArticyEntryIds shared = index.DialogueEntries["0x0000000000000000"];
            Assert.Equal(1, shared.ConversationId);
            Assert.Equal(new[] { 0, 3 }, shared.EntryIds);
        }

        [Fact]
        public void CanonicalisesABareHexIdAndLeavesEveryOtherAlone()
        {
            // What the YAML loader the Python used did, which was to read the bare hex as a
            // number and render it back in one form.
            Assert.Equal("0x010000040000032B", ArticyIdIndex.Canonicalize("0x010000040000032b"));
            Assert.Equal("0x0000000000000002-START", ArticyIdIndex.Canonicalize("0x0000000000000002-START"));
            Assert.Equal("xic5nycau3n8v6nt", ArticyIdIndex.Canonicalize("xic5nycau3n8v6nt"));
            // Not a hexadecimal literal to that loader either: the prefix is lower case there.
            Assert.Equal("0X000000000000000a", ArticyIdIndex.Canonicalize("0X000000000000000a"));
        }

        [Fact]
        public void ReadsTheConversationEachEntrySaysItIsIn()
        {
            List<ConversationRecord> conversations =
                ConversationIndexExtractor.Extract(IndexFixtureAsset).ToList();

            // Not the originConversationID inside an outgoing link, which sits two spaces
            // deeper and says the same thing.
            Assert.Equal(new int?[] { 1, 1 }, conversations[0].Entries.Select(entry => entry.ConversationId));
            Assert.Equal(2, conversations[1].Entries[0].ConversationId);
        }

        [Fact]
        public void KeepsEveryConversationField()
        {
            ConversationRecord conversation = ConversationIndexExtractor.Extract(IndexFixtureAsset).First();

            Assert.Equal(new[] { "Title", "Actor", "Conversant", "Description", ArticyIdField },
                conversation.Fields.Keys);
            Assert.Equal("0x0000000000000001", conversation.Fields[ArticyIdField]);
            Assert.Equal(conversation.Fields["Title"], conversation.Title);
        }

        [Fact]
        public void RefusesAConversationWithNoArticyId()
        {
            InvalidDataException failure = Build(Conversation(4, null, Entry(0, 4, "0x0000000000000001")));

            Assert.Contains("Conversation 4 is missing an Articy Id", failure.Message);
        }

        [Fact]
        public void RefusesAnEntryWithNoArticyId()
        {
            // An empty Articy Id is no Articy Id, as it was to the Python.
            InvalidDataException failure = Build(Conversation(4, "0x0000000000000001", Entry(9, 4, string.Empty)));

            Assert.Contains("Dialogue entry 4:9 is missing an Articy Id", failure.Message);
        }

        [Fact]
        public void RefusesAnEntryThatDisagreesWithItsParent()
        {
            InvalidDataException failure = Build(
                Conversation(4, "0x0000000000000001", Entry(9, 5, "0x0000000000000002")));

            Assert.Contains("Dialogue entry 9 has conversationID 5", failure.Message);
            Assert.Contains("does not match parent conversation id 4", failure.Message);
        }

        [Fact]
        public void RefusesTwoConversationsOnOneArticyId()
        {
            InvalidDataException failure = Build(
                Conversation(4, "0x0000000000000001"),
                Conversation(5, "0x0000000000000001"));

            Assert.Contains("Duplicate Articy Id 0x0000000000000001 for conversation 5", failure.Message);
        }

        [Fact]
        public void RefusesOneArticyIdSharedAcrossConversations()
        {
            InvalidDataException failure = Build(
                Conversation(4, "0x0000000000000001", Entry(0, 4, "0x0000000000000009")),
                Conversation(5, "0x0000000000000002", Entry(0, 5, "0x0000000000000009")));

            Assert.Contains("shared by dialogue entries in different conversations (4 and 5)", failure.Message);
        }

        [Fact]
        public void RefusesARecordWhoseIdWillNotParse()
        {
            // The other half of the Python's "is missing an id" check: a record exists only
            // because its id line did, and an id that will not parse never becomes one.
            string asset = string.Join(Environment.NewLine, "  conversations:", "  - id: ", "  syncInfo:");

            Assert.Throws<FormatException>(
                () => ConversationIndexExtractor.Extract(new StringReader(asset)).ToList());
        }

        private static InvalidDataException Build(params ConversationRecord[] conversations)
        {
            return Assert.Throws<InvalidDataException>(() => ArticyIdIndex.Build(conversations));
        }

        private static ConversationRecord Conversation(int id, string? articyId, params EntryRecord[] entries)
        {
            var conversation = new ConversationRecord { Id = id, Entries = entries.ToList() };
            if (articyId != null)
            {
                conversation.Fields[ArticyIdField] = articyId;
            }

            return conversation;
        }

        private static EntryRecord Entry(int id, int? conversationId, string? articyId)
        {
            var entry = new EntryRecord { Id = id, ConversationId = conversationId };
            if (articyId != null)
            {
                entry.Fields[ArticyIdField] = articyId;
            }

            return entry;
        }
    }
}
