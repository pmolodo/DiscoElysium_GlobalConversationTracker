// SPDX-License-Identifier: MIT
using System.Text.Json;
using Xunit;

namespace GlobalConversationTracker.DialogueAsset.Tests
{
    /// <summary>What the extractor reads out of a database, and what it writes for it.</summary>
    public class ConversationIndexTests
    {
        private static readonly string FixtureAsset =
            Path.Combine(AppContext.BaseDirectory, "Fixtures", "mini-database.asset");

        private static readonly string ExpectedIndex =
            Path.Combine(AppContext.BaseDirectory, "Fixtures", "mini-database.expected.jsonl");

        [Fact]
        public void ReadsOnlyTheConversationsSection()
        {
            List<ConversationRecord> conversations = Extract();

            // The actor above the section and the conversation below syncInfo are both
            // shaped like things the scanner reads, and neither is one.
            Assert.Equal(new[] { 1, 2 }, conversations.Select(conversation => conversation.Id));
        }

        [Fact]
        public void ReadsTheConversationFieldsItWants()
        {
            ConversationRecord conversation = Extract()[0];

            Assert.Equal("Kim's notebook: page 2", conversation.Title);
            Assert.Equal(12, conversation.Actor);
            // "not-a-number" is not one, and an unreadable number is no number.
            Assert.Null(conversation.Conversant);
        }

        [Fact]
        public void ReadsGroupsGuardsAndScripts()
        {
            List<EntryRecord> entries = Extract()[0].Entries;

            Assert.False(entries[0].Group);
            // An empty conditionsString is written with nothing after the colon, so the
            // line does not match the prefix at all and the guard stays empty.
            Assert.Equal(string.Empty, entries[0].Guard);
            Assert.Equal(string.Empty, entries[0].Script);

            Assert.True(entries[1].Group);
            Assert.Equal("Variable[\"kim.trust\"] > 2 and Variable[\"money\"] == true", entries[1].Guard);
            Assert.Equal("Money = Money - 50", entries[1].Script);
        }

        [Fact]
        public void ReadsLinksWithinAndAcrossConversations()
        {
            List<EntryRecord> entries = Extract()[0].Entries;

            Assert.Equal(new[] { 1 }, entries[0].To);
            Assert.Equal(new[] { 1 }, entries[0].ToConversation);

            Assert.Equal(new[] { 5, 0 }, entries[1].To);
            Assert.Equal(new[] { 2, 1 }, entries[1].ToConversation);

            // No links at all means no key, not an empty one.
            Assert.Empty(Extract()[1].Entries[0].To);
            Assert.Null(Extract()[1].Entries[0].ToConversation);
        }

        [Fact]
        public void KeepsEveryFieldAndDecodesItsScalar()
        {
            EntryRecord entry = Extract()[0].Entries[1];

            Assert.Equal(new[] { "Title", "Sequence", "DifficultyPass", "Note" }, entry.Fields.Keys);
            Assert.Equal("Say(\"hi\") \\ done", entry.Fields["Sequence"]);
            // Escaped rather than written out, so this file stays ASCII: the fixture is
            // where the accented letter and the astral character actually live.
            Assert.Equal("Sur la lune - caf\u00e9 \U0001F600", entry.Fields["Note"]);
        }

        [Fact]
        public void KeepsEntryActorOnlyInFields()
        {
            using JsonDocument document = JsonDocument.Parse(ConversationIndexFile.ToJson(Extract()[0]));
            JsonElement entry = document.RootElement.GetProperty("entries")[0];

            Assert.False(entry.TryGetProperty("actor", out _));
            Assert.Equal("12", entry.GetProperty("fields").GetProperty("Actor").GetString());
        }

        [Fact]
        public void LetsARepeatedFieldWinWithoutMovingIt()
        {
            EntryRecord entry = Extract()[0].Entries[1];

            // Two Title fields: the later value, in the earlier one's place.
            Assert.Equal("The later Title wins, in the earlier Title's place", entry.Fields["Title"]);
            Assert.Equal("Title", entry.Fields.Keys.First());
            Assert.Equal(entry.Fields["Title"], entry.Title);
        }

        [Fact]
        public void WritesTheExpectedSchema()
        {
            string[] expected = File.ReadAllLines(ExpectedIndex);
            List<ConversationRecord> conversations = Extract();

            Assert.Equal(expected, conversations.Select(ConversationIndexFile.ToJson));
        }

        [Fact]
        public void EndsEveryRecordWithALineBreak()
        {
            var writer = new StringWriter();

            int written = ConversationIndexFile.Write(writer, Extract());

            Assert.Equal(2, written);
            string expected = string.Join(string.Empty,
                File.ReadAllLines(ExpectedIndex).Select(line => line + Environment.NewLine));
            Assert.Equal(expected, writer.ToString());
        }

        [Fact]
        public void ReadsBackEverythingItWrote()
        {
            var writer = new StringWriter();
            ConversationIndexFile.Write(writer, Extract());

            List<ConversationRecord> read = ConversationIndexFile.Read(new StringReader(writer.ToString())).ToList();

            Assert.Equal(Extract().Select(ConversationIndexFile.ToJson), read.Select(ConversationIndexFile.ToJson));
            // Round-tripping has to preserve field order, or the next index written from a
            // read one would differ from the index it came from.
            Assert.Equal(new[] { "Title", "Sequence", "DifficultyPass", "Note" }, read[0].Entries[1].Fields.Keys);
        }

        private static List<ConversationRecord> Extract()
        {
            return ConversationIndexExtractor.Extract(FixtureAsset).ToList();
        }
    }
}
