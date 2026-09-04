// SPDX-License-Identifier: MIT
using Xunit;

namespace GlobalConversationTracker.DialogueAsset.Tests
{
    /// <summary>The state built from an index, and the bytes written for it.</summary>
    public class WorstCaseGlobalStateTests
    {
        [Fact]
        public void OrdersConversationsByNumberRatherThanByText()
        {
            // Keyed on the string a conversation id is written as, 10 would come first.
            string json = WorstCaseGlobalState.ToJson(
                WorstCaseGlobalState.Build(new[] { Conversation(10, 0), Conversation(9, 0) }));

            Assert.Equal("{\"version\":4,\"conversations\":{\"WasDisplayed\":{\"9\":\"0\",\"10\":\"0\"}}}", json);
        }

        [Fact]
        public void SortsAndDeduplicatesEntryIds()
        {
            SortedDictionary<int, List<int>> displayed =
                WorstCaseGlobalState.Build(new[] { Conversation(1, 5, 2, 5, 0) });

            Assert.Equal(new[] { 0, 2, 5 }, displayed[1]);
        }

        [Fact]
        public void SkipsConversationsWithNoEntries()
        {
            SortedDictionary<int, List<int>> displayed =
                WorstCaseGlobalState.Build(new[] { Conversation(1), Conversation(2, 0) });

            Assert.Equal(new[] { 2 }, displayed.Keys);
        }

        [Fact]
        public void RefusesAnIndexThatDescribesNoConversations()
        {
            // An index with nothing in it, or one holding only empty conversations, would
            // build a state that measures nothing while looking like it ran.
            Assert.Throws<InvalidDataException>(
                () => WorstCaseGlobalState.Build(Array.Empty<ConversationRecord>()));
            Assert.Throws<InvalidDataException>(
                () => WorstCaseGlobalState.Build(new[] { Conversation(1) }));
        }

        [Fact]
        public void WritesUtf8WithNoBomAndOneTrailingNewline()
        {
            string path = Path.Combine(Path.GetTempPath(), Path.GetRandomFileName());
            try
            {
                WorstCaseGlobalState.Write(path, new[] { Conversation(1, 0) });

                Assert.Equal(
                    "{\"version\":4,\"conversations\":{\"WasDisplayed\":{\"1\":\"0\"}}}\n",
                    File.ReadAllText(path));
                Assert.NotEqual(0xEF, File.ReadAllBytes(path)[0]);
            }
            finally
            {
                File.Delete(path);
            }
        }

        private static ConversationRecord Conversation(int id, params int[] entryIds)
        {
            return new ConversationRecord
            {
                Id = id,
                Entries = entryIds.Select(entryId => new EntryRecord { Id = entryId }).ToList(),
            };
        }
    }
}
