using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Text;
using Xunit;

namespace UnifiedConversationTracker.Persistence.Tests
{
    /// <summary>
    /// Format-level tests: the exact bytes written, and every way a file can be
    /// wrong.
    /// </summary>
    public class UnifiedStateJsonTests
    {
        private const string TestSource = "test";

        private static UnifiedConversationState StateWith(params (int Conversation, int Entry, SimStatus Status)[] rows)
        {
            var state = new UnifiedConversationState();
            foreach ((int conversation, int entry, SimStatus status) in rows)
            {
                state.Merge(conversation, entry, status);
            }

            return state;
        }

        private static UnifiedStateLoadResult Parse(string json) => UnifiedStateJson.Deserialize(json, TestSource);

        // -------------------------------------------------------------------
        // Writing
        // -------------------------------------------------------------------

        [Fact]
        public void Serialize_ProducesTheDocumentedShape()
        {
            UnifiedConversationState state = StateWith(
                (3, 17, SimStatus.WasDisplayed),
                (3, 18, SimStatus.WasOffered));

            Assert.Equal(
                "{\"version\":1,\"conversations\":{\"3\":{\"17\":\"WasDisplayed\",\"18\":\"WasOffered\"}}}",
                UnifiedStateJson.Serialize(state));
        }

        [Fact]
        public void Serialize_EmptyState_WritesAnEmptyConversationMap()
        {
            Assert.Equal(
                "{\"version\":1,\"conversations\":{}}",
                UnifiedStateJson.Serialize(new UnifiedConversationState()));
        }

        [Fact]
        public void Serialize_WritesUtf8WithoutABom()
        {
            byte[] bytes = UnifiedStateJson.SerializeToUtf8Bytes(new UnifiedConversationState());

            Assert.Equal((byte)'{', bytes[0]);
            Assert.False(bytes.Take(3).SequenceEqual(Encoding.UTF8.GetPreamble()));
        }

        [Fact]
        public void Serialize_NeverWritesUntouched()
        {
            // Untouched is not storable in the first place, so this is really a check
            // that an attempt to record it leaves no trace in the file either.
            UnifiedConversationState state = StateWith(
                (1, 1, SimStatus.Untouched),
                (1, 2, SimStatus.WasOffered));

            string json = UnifiedStateJson.Serialize(state);

            Assert.DoesNotContain(SimStatusNames.Untouched, json, StringComparison.Ordinal);
            Assert.Equal("{\"version\":1,\"conversations\":{\"1\":{\"2\":\"WasOffered\"}}}", json);
        }

        [Fact]
        public void Serialize_IsDeterministic_WhateverTheInsertionOrder()
        {
            UnifiedConversationState ascending = StateWith(
                (1, 1, SimStatus.WasOffered),
                (1, 2, SimStatus.WasDisplayed),
                (2, 1, SimStatus.WasOffered),
                (10, 5, SimStatus.WasDisplayed));

            UnifiedConversationState scrambled = StateWith(
                (10, 5, SimStatus.WasDisplayed),
                (2, 1, SimStatus.WasOffered),
                (1, 2, SimStatus.WasDisplayed),
                (1, 1, SimStatus.WasOffered));

            Assert.Equal(UnifiedStateJson.Serialize(ascending), UnifiedStateJson.Serialize(scrambled));
        }

        [Fact]
        public void Serialize_SortsNumerically_NotAsStrings()
        {
            UnifiedConversationState state = StateWith(
                (2, 100, SimStatus.WasOffered),
                (2, 9, SimStatus.WasOffered),
                (10, 1, SimStatus.WasOffered));

            string json = UnifiedStateJson.Serialize(state);

            Assert.Equal(
                "{\"version\":1,\"conversations\":{\"2\":{\"9\":\"WasOffered\",\"100\":\"WasOffered\"},"
                + "\"10\":{\"1\":\"WasOffered\"}}}",
                json);
        }

        [Fact]
        public void Serialize_OfASnapshot_IsByteIdenticalToSerializingItsSource()
        {
            // Since de-0m0.5 the background writer serializes a snapshot rather than the
            // live state, so the file's determinism now depends on the snapshot being
            // indistinguishable from its source to this serializer.
            UnifiedConversationState state = StateWith(
                (10, 5, SimStatus.WasDisplayed),
                (2, 1, SimStatus.WasOffered),
                (1, 2, SimStatus.WasDisplayed),
                (1, 1, SimStatus.WasOffered));

            Assert.Equal(
                UnifiedStateJson.SerializeToUtf8Bytes(state),
                UnifiedStateJson.SerializeToUtf8Bytes(state.Snapshot()));

            Assert.Equal(
                UnifiedStateJson.SerializeToUtf8Bytes(new UnifiedConversationState()),
                UnifiedStateJson.SerializeToUtf8Bytes(new UnifiedConversationState().Snapshot()));
        }

        // -------------------------------------------------------------------
        // Round trip
        // -------------------------------------------------------------------

        [Fact]
        public void RoundTrip_PopulatedState_PreservesEveryEntry()
        {
            UnifiedConversationState original = StateWith(
                (1, 10, SimStatus.WasDisplayed),
                (1, 11, SimStatus.WasOffered),
                (2, 20, SimStatus.WasOffered),
                (-5, -6, SimStatus.WasDisplayed),
                (int.MaxValue, int.MinValue, SimStatus.WasDisplayed));

            UnifiedStateLoadResult result = Parse(UnifiedStateJson.Serialize(original));

            Assert.Equal(UnifiedStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(0, result.SkippedRowCount);
            Assert.Equal(
                original.EnumerateEntriesInIdOrder().ToList(),
                result.RequireState().EnumerateEntriesInIdOrder().ToList());
        }

        [Fact]
        public void RoundTrip_IsByteStable_AcrossASecondSerialize()
        {
            UnifiedConversationState original = StateWith(
                (7, 1, SimStatus.WasDisplayed),
                (7, 2, SimStatus.WasOffered),
                (8, 3, SimStatus.WasDisplayed));

            string first = UnifiedStateJson.Serialize(original);
            string second = UnifiedStateJson.Serialize(Parse(first).RequireState());

            Assert.Equal(first, second);
        }

        [Fact]
        public void RoundTrip_AtRealisticScale_IsLinearAndCorrect()
        {
            // de-omm.9 measured a real save at 1,331 WasDisplayed plus 142 WasOffered,
            // so ~1,500 rows is the realistic file. This runs 50,000 to leave headroom
            // and to make anything accidentally quadratic blow the time budget: 50k
            // squared would be 2.5 billion operations, far past ten seconds.
            const int conversationCount = 500;
            const int entriesPerConversation = 100;
            var original = new UnifiedConversationState();
            for (int conversationId = 0; conversationId < conversationCount; conversationId++)
            {
                for (int entryId = 0; entryId < entriesPerConversation; entryId++)
                {
                    original.Merge(
                        conversationId,
                        entryId,
                        entryId % 2 == 0 ? SimStatus.WasDisplayed : SimStatus.WasOffered);
                }
            }

            var stopwatch = Stopwatch.StartNew();
            UnifiedStateLoadResult result = Parse(UnifiedStateJson.Serialize(original));
            stopwatch.Stop();

            Assert.Equal(conversationCount * entriesPerConversation, result.RequireState().EntryCount);
            Assert.True(
                stopwatch.Elapsed < TimeSpan.FromSeconds(10),
                $"Round trip of {original.EntryCount} entries took {stopwatch.Elapsed}.");
        }

        // -------------------------------------------------------------------
        // Structurally broken files: Corrupt
        // -------------------------------------------------------------------

        [Theory]
        [InlineData("")]
        [InlineData("   ")]
        [InlineData("not json at all")]
        [InlineData("{")]
        [InlineData("{\"version\":1,\"conversations\":{\"3\":{\"17\":\"WasDis")]
        [InlineData("{\"version\":1,\"conversations\":{}} trailing garbage")]
        [InlineData("[]")]
        [InlineData("\"a string\"")]
        [InlineData("null")]
        [InlineData("{\"conversations\":{}}")]
        [InlineData("{\"version\":\"1\",\"conversations\":{}}")]
        [InlineData("{\"version\":1.5,\"conversations\":{}}")]
        [InlineData("{\"version\":1}")]
        [InlineData("{\"version\":1,\"conversations\":[]}")]
        [InlineData("{\"version\":1,\"conversations\":null}")]
        public void Deserialize_StructurallyBrokenFile_IsCorrupt(string json)
        {
            UnifiedStateLoadResult result = Parse(json);

            Assert.Equal(UnifiedStateLoadOutcome.Corrupt, result.Outcome);
            Assert.Null(result.State);
            Assert.False(string.IsNullOrEmpty(result.ErrorMessage));
            Assert.Throws<InvalidOperationException>(() => result.RequireState());
        }

        [Fact]
        public void Deserialize_TruncatedAtEveryLength_IsNeverSilentlyPartial()
        {
            // The torn-write family, exhaustively: every prefix of a real file must be
            // either rejected or exactly equal to the whole thing, never a half state
            // that looks fine.
            string complete = UnifiedStateJson.Serialize(StateWith(
                (3, 17, SimStatus.WasDisplayed),
                (3, 18, SimStatus.WasOffered),
                (4, 1, SimStatus.WasDisplayed)));

            for (int length = 0; length < complete.Length; length++)
            {
                UnifiedStateLoadResult result = Parse(complete.Substring(0, length));
                Assert.Equal(UnifiedStateLoadOutcome.Corrupt, result.Outcome);
            }

            Assert.Equal(UnifiedStateLoadOutcome.Loaded, Parse(complete).Outcome);
        }

        // -------------------------------------------------------------------
        // Unknown version: distinct from Corrupt
        // -------------------------------------------------------------------

        [Theory]
        [InlineData(0)]
        [InlineData(2)]
        [InlineData(99)]
        [InlineData(-1)]
        public void Deserialize_UnknownFormatVersion_IsUnsupportedVersion(int version)
        {
            UnifiedStateLoadResult result = Parse(
                $"{{\"version\":{version},\"conversations\":{{\"1\":{{\"2\":\"WasOffered\"}}}}}}");

            Assert.Equal(UnifiedStateLoadOutcome.UnsupportedVersion, result.Outcome);
            Assert.Null(result.State);
            Assert.Contains(version.ToString(), result.ErrorMessage!);
        }

        // -------------------------------------------------------------------
        // Row-level damage: skipped and reported, load still succeeds
        // -------------------------------------------------------------------

        [Theory]
        [InlineData("\"Untouched \"")]
        [InlineData("\"wasdisplayed\"")]
        [InlineData("\"WASDISPLAYED\"")]
        [InlineData("\"Seen\"")]
        [InlineData("\"\"")]
        public void Deserialize_UnrecognizedStatusString_SkipsThatRowOnly(string statusLiteral)
        {
            UnifiedStateLoadResult result = Parse(
                "{\"version\":1,\"conversations\":{\"1\":{\"2\":" + statusLiteral + ",\"3\":\"WasOffered\"}}}");

            Assert.Equal(UnifiedStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(1, result.SkippedRowCount);
            Assert.Single(result.Warnings);
            UnifiedConversationState state = result.RequireState();
            Assert.Equal(SimStatus.Untouched, state.GetStatus(1, 2));
            Assert.Equal(SimStatus.WasOffered, state.GetStatus(1, 3));
        }

        [Fact]
        public void Deserialize_NonIntegerConversationKey_SkipsTheWholeConversation()
        {
            UnifiedStateLoadResult result = Parse(
                "{\"version\":1,\"conversations\":{\"oops\":{\"1\":\"WasOffered\",\"2\":\"WasOffered\"},"
                + "\"5\":{\"6\":\"WasDisplayed\"}}}");

            Assert.Equal(UnifiedStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(2, result.SkippedRowCount);
            Assert.Equal(1, result.RequireState().EntryCount);
            Assert.Equal(SimStatus.WasDisplayed, result.RequireState().GetStatus(5, 6));
        }

        [Fact]
        public void Deserialize_ConversationValueNotAnObject_SkipsIt()
        {
            UnifiedStateLoadResult result = Parse(
                "{\"version\":1,\"conversations\":{\"1\":\"WasOffered\",\"2\":{\"3\":\"WasOffered\"}}}");

            Assert.Equal(UnifiedStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(1, result.SkippedRowCount);
            Assert.Equal(1, result.RequireState().EntryCount);
        }

        [Fact]
        public void Deserialize_NonIntegerEntryKey_SkipsThatRowOnly()
        {
            UnifiedStateLoadResult result = Parse(
                "{\"version\":1,\"conversations\":{\"1\":{\"x\":\"WasOffered\",\"2\":\"WasOffered\"}}}");

            Assert.Equal(UnifiedStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(1, result.SkippedRowCount);
            Assert.Equal(SimStatus.WasOffered, result.RequireState().GetStatus(1, 2));
        }

        [Theory]
        [InlineData("2")]
        [InlineData("null")]
        [InlineData("true")]
        [InlineData("{}")]
        [InlineData("[\"WasOffered\"]")]
        public void Deserialize_StatusNotAString_SkipsThatRowOnly(string statusLiteral)
        {
            UnifiedStateLoadResult result = Parse(
                "{\"version\":1,\"conversations\":{\"1\":{\"2\":" + statusLiteral + ",\"3\":\"WasOffered\"}}}");

            Assert.Equal(UnifiedStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(1, result.SkippedRowCount);
            Assert.Equal(1, result.RequireState().EntryCount);
        }

        [Fact]
        public void Deserialize_UntouchedInFile_IsRecognizedButStoresNothing()
        {
            // A hand-written or older file may spell Untouched out. It is a known
            // status, so it is not a skipped row, but it must not create an entry.
            UnifiedStateLoadResult result = Parse(
                "{\"version\":1,\"conversations\":{\"1\":{\"2\":\"Untouched\"}}}");

            Assert.Equal(UnifiedStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(0, result.SkippedRowCount);
            Assert.True(result.RequireState().IsEmpty);
        }

        [Fact]
        public void Deserialize_WarningsAreCappedButTheCountIsNot()
        {
            var json = new StringBuilder("{\"version\":1,\"conversations\":{\"1\":{");
            const int badRowCount = UnifiedStateJson.MaxWarnings + 15;
            for (int i = 0; i < badRowCount; i++)
            {
                json.Append(i == 0 ? string.Empty : ",").Append('"').Append(i).Append("\":\"Nonsense\"");
            }

            json.Append("}}}");

            UnifiedStateLoadResult result = Parse(json.ToString());

            Assert.Equal(badRowCount, result.SkippedRowCount);
            Assert.Equal(UnifiedStateJson.MaxWarnings, result.Warnings.Count);
        }

        // -------------------------------------------------------------------
        // The load path cannot lower a status
        // -------------------------------------------------------------------

        [Fact]
        public void Deserialize_DuplicateKeyWithALowerStatus_CannotPullTheEntryDown()
        {
            // A hand-edited file can contain the same entry twice. Because loading goes
            // through Merge rather than assignment, the higher status wins whichever
            // order the rows appear in.
            UnifiedStateLoadResult displayedFirst = Parse(
                "{\"version\":1,\"conversations\":{\"1\":{\"2\":\"WasDisplayed\",\"2\":\"WasOffered\"}}}");
            UnifiedStateLoadResult offeredFirst = Parse(
                "{\"version\":1,\"conversations\":{\"1\":{\"2\":\"WasOffered\",\"2\":\"WasDisplayed\"}}}");

            Assert.Equal(SimStatus.WasDisplayed, displayedFirst.RequireState().GetStatus(1, 2));
            Assert.Equal(SimStatus.WasDisplayed, offeredFirst.RequireState().GetStatus(1, 2));
        }

        [Fact]
        public void Deserialize_DuplicateConversationBlocks_AreMergedNotReplaced()
        {
            UnifiedStateLoadResult result = Parse(
                "{\"version\":1,\"conversations\":{\"1\":{\"2\":\"WasDisplayed\"},\"1\":{\"3\":\"WasOffered\"}}}");

            UnifiedConversationState state = result.RequireState();
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(1, 2));
            Assert.Equal(SimStatus.WasOffered, state.GetStatus(1, 3));
        }

        // -------------------------------------------------------------------
        // Argument checking
        // -------------------------------------------------------------------

        [Fact]
        public void Serialize_NullState_Throws()
        {
            Assert.Throws<ArgumentNullException>(() => UnifiedStateJson.SerializeToUtf8Bytes(null!));
        }

        [Fact]
        public void Deserialize_NullInput_Throws()
        {
            Assert.Throws<ArgumentNullException>(() => UnifiedStateJson.Deserialize((string)null!, TestSource));
            Assert.Throws<ArgumentNullException>(() => UnifiedStateJson.Deserialize((byte[])null!, TestSource));
        }

        [Fact]
        public void LoadResult_RecordsWhereItCameFrom()
        {
            IReadOnlyList<UnifiedStateLoadResult> results = new[]
            {
                Parse("{\"version\":1,\"conversations\":{}}"),
                Parse("garbage"),
                Parse("{\"version\":7,\"conversations\":{}}"),
            };

            Assert.All(results, result => Assert.Equal(TestSource, result.SourcePath));
        }
    }
}
