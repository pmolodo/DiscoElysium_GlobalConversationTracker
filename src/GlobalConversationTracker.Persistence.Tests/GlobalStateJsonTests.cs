// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Text;
using GlobalConversationTracker.Core;
using Xunit;

namespace GlobalConversationTracker.Persistence.Tests
{
    /// <summary>
    /// Format-level tests: the exact bytes written, and every way a file can be
    /// wrong.
    /// </summary>
    /// <remarks>
    /// The format itself is native - see <c>GlobalStateNative</c> - so what these exercise
    /// is the library and the marshalling together, which is the pair a caller actually
    /// gets. There is no C# reader left for them to be testing instead.
    /// </remarks>
    public class GlobalStateJsonTests
    {
        private const string TestSource = "test";

        /// <remarks>
        /// A STATUS CROSSES AS A NUMBER, and the two sides number them the same way. That
        /// is checked rather than assumed: a silent disagreement would not fail anything
        /// loudly, it would rewrite every status in every file the next time one was saved.
        /// </remarks>
        [Theory]
        [InlineData(SimStatus.Untouched, 0)]
        [InlineData(SimStatus.WasOffered, 1)]
        [InlineData(SimStatus.WasDisplayed, 2)]
        public void AStatusIsTheSameNumberOnBothSidesOfTheBoundary(SimStatus status, int crossing)
        {
            Assert.Equal(crossing, (int)status);

            // And the one that matters end to end: written out and read back, a status is
            // the status it was.
            GlobalConversationState state = StateWith((3, 7, status));
            GlobalStateLoadResult read = Parse(GlobalStateJson.Serialize(state));

            Assert.Equal(GlobalStateLoadOutcome.Loaded, read.Outcome);
            read.RequireState().TryGetStatus(3, 7, out SimStatus back);
            Assert.Equal(status == SimStatus.Untouched ? SimStatus.Untouched : status, back);
        }

        private static GlobalConversationState StateWith(params (int Conversation, int Entry, SimStatus Status)[] rows)
        {
            var state = new GlobalConversationState();
            foreach ((int conversation, int entry, SimStatus status) in rows)
            {
                state.Merge(conversation, entry, status);
            }

            return state;
        }

        private static GlobalStateLoadResult Parse(string json) => GlobalStateJson.Deserialize(json, TestSource);

        // -------------------------------------------------------------------
        // Writing
        // -------------------------------------------------------------------

        [Fact]
        public void Serialize_ProducesTheDocumentedShape()
        {
            GlobalConversationState state = StateWith(
                (3, 17, SimStatus.WasDisplayed),
                (3, 18, SimStatus.WasOffered));

            // Grouped by status, then conversation, then a plain array of entry IDs, so
            // a status string is written once per conversation instead of once per entry.
            Assert.Equal(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"WasOffered\":{\"3\":\"18\"},"
                + "\"WasDisplayed\":{\"3\":\"17\"}},\"orbs\":[]}",
                GlobalStateJson.Serialize(state));
        }

        [Fact]
        public void Serialize_EmptyState_WritesAnEmptyConversationMap()
        {
            Assert.Equal(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{},\"orbs\":[]}",
                GlobalStateJson.Serialize(new GlobalConversationState()));
        }

        [Fact]
        public void Serialize_WritesUtf8WithoutABom()
        {
            byte[] bytes = GlobalStateJson.SerializeToUtf8Bytes(new GlobalConversationState());

            Assert.Equal((byte)'{', bytes[0]);
            Assert.False(bytes.Take(3).SequenceEqual(Encoding.UTF8.GetPreamble()));
        }

        [Fact]
        public void Serialize_NeverWritesUntouched()
        {
            // Untouched is not storable in the first place, so this is really a check
            // that an attempt to record it leaves no trace in the file either.
            GlobalConversationState state = StateWith(
                (1, 1, SimStatus.Untouched),
                (1, 2, SimStatus.WasOffered));

            string json = GlobalStateJson.Serialize(state);

            Assert.DoesNotContain(SimStatusNames.Untouched, json, StringComparison.Ordinal);
            Assert.Equal(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"WasOffered\":{\"1\":\"2\"}},\"orbs\":[]}", json);
        }

        [Fact]
        public void Serialize_IsDeterministic_WhateverTheInsertionOrder()
        {
            GlobalConversationState ascending = StateWith(
                (1, 1, SimStatus.WasOffered),
                (1, 2, SimStatus.WasDisplayed),
                (2, 1, SimStatus.WasOffered),
                (10, 5, SimStatus.WasDisplayed));

            GlobalConversationState scrambled = StateWith(
                (10, 5, SimStatus.WasDisplayed),
                (2, 1, SimStatus.WasOffered),
                (1, 2, SimStatus.WasDisplayed),
                (1, 1, SimStatus.WasOffered));

            Assert.Equal(GlobalStateJson.Serialize(ascending), GlobalStateJson.Serialize(scrambled));
        }

        [Fact]
        public void Serialize_SortsNumerically_NotAsStrings()
        {
            GlobalConversationState state = StateWith(
                (2, 100, SimStatus.WasOffered),
                (2, 9, SimStatus.WasOffered),
                (10, 1, SimStatus.WasOffered));

            string json = GlobalStateJson.Serialize(state);

            Assert.Equal(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"WasOffered\":{\"2\":\"9,100\",\"10\":\"1\"}},"
                + "\"orbs\":[]}",
                json);
        }

        [Fact]
        public void Serialize_OfASnapshot_IsByteIdenticalToSerializingItsSource()
        {
            // The background writer serializes a snapshot rather than the live state,
            // so the file's determinism depends on the snapshot being
            // indistinguishable from its source to this serializer.
            GlobalConversationState state = StateWith(
                (10, 5, SimStatus.WasDisplayed),
                (2, 1, SimStatus.WasOffered),
                (1, 2, SimStatus.WasDisplayed),
                (1, 1, SimStatus.WasOffered));

            Assert.Equal(
                GlobalStateJson.SerializeToUtf8Bytes(state),
                GlobalStateJson.SerializeToUtf8Bytes(state.Snapshot()));

            Assert.Equal(
                GlobalStateJson.SerializeToUtf8Bytes(new GlobalConversationState()),
                GlobalStateJson.SerializeToUtf8Bytes(new GlobalConversationState().Snapshot()));
        }

        // -------------------------------------------------------------------
        // Round trip
        // -------------------------------------------------------------------

        [Fact]
        public void RoundTrip_PopulatedState_PreservesEveryEntry()
        {
            GlobalConversationState original = StateWith(
                (1, 10, SimStatus.WasDisplayed),
                (1, 11, SimStatus.WasOffered),
                (2, 20, SimStatus.WasOffered),
                (-5, -6, SimStatus.WasDisplayed),
                (int.MaxValue, int.MinValue, SimStatus.WasDisplayed));

            GlobalStateLoadResult result = Parse(GlobalStateJson.Serialize(original));

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(0, result.SkippedRowCount);
            Assert.Equal(
                original.EnumerateEntriesInIdOrder().ToList(),
                result.RequireState().EnumerateEntriesInIdOrder().ToList());
        }

        [Fact]
        public void RoundTrip_IsByteStable_AcrossASecondSerialize()
        {
            GlobalConversationState original = StateWith(
                (7, 1, SimStatus.WasDisplayed),
                (7, 2, SimStatus.WasOffered),
                (8, 3, SimStatus.WasDisplayed));

            string first = GlobalStateJson.Serialize(original);
            string second = GlobalStateJson.Serialize(Parse(first).RequireState());

            Assert.Equal(first, second);
        }

        [Fact]
        public void RoundTrip_AtRealisticScale_IsLinearAndCorrect()
        {
            // A real save runs to something like 1,500 rows. This runs 50,000 to leave
            // headroom and to make anything accidentally quadratic blow the time
            // budget: 50k squared would be 2.5 billion operations, far past ten
            // seconds.
            const int conversationCount = 500;
            const int entriesPerConversation = 100;
            var original = new GlobalConversationState();
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
            GlobalStateLoadResult result = Parse(GlobalStateJson.Serialize(original));
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
        [InlineData("{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"3\":{\"17\":\"WasDis")]
        [InlineData("{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{}} trailing garbage")]
        [InlineData("[]")]
        [InlineData("\"a string\"")]
        [InlineData("null")]
        [InlineData("{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":[]}")]
        [InlineData("{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":null}")]
        public void Deserialize_StructurallyBrokenFile_IsCorrupt(string json)
        {
            GlobalStateLoadResult result = Parse(json);

            Assert.Equal(GlobalStateLoadOutcome.Corrupt, result.Outcome);
            Assert.Null(result.State);
            Assert.False(string.IsNullOrEmpty(result.ErrorMessage));
            Assert.Throws<InvalidOperationException>(() => result.RequireState());
        }

        /// <remarks>
        /// NOT CORRUPT, and the difference decides what a caller may do about it. A file
        /// that does not name this format is one this build cannot read - an older one, or
        /// something else entirely - and it is full of real history either way, so the
        /// caller must stop and convert rather than overwrite it from a backup. Damage
        /// INSIDE a file that does name itself is the corrupt case above.
        /// </remarks>
        [Theory]
        [InlineData("{\"conversations\":{}}")]
        [InlineData("{\"version\":4}")]
        [InlineData("{\"version\":4,\"conversations\":{}}")]
        public void Deserialize_FileThatSaysNothingAboutItself_NamesTheConverter(string json)
        {
            GlobalStateLoadResult result = Parse(json);

            Assert.Equal(GlobalStateLoadOutcome.OutdatedVersion, result.Outcome);
            Assert.Null(result.State);
            Assert.Contains(
                FormatStamp.Converter, result.ErrorMessage!, StringComparison.Ordinal);
        }

        /// <remarks>
        /// ALSO UNSUPPORTED AND NOT CORRUPT, for the same reason, but the message does not
        /// offer the converter: this file is not an older state, it is a different document
        /// altogether, and converting it would produce a newer version of that. What helps
        /// here is being told what the file actually says it is.
        /// </remarks>
        [Fact]
        public void Deserialize_FileThatNamesAnotherFormat_SaysWhichOne()
        {
            GlobalStateLoadResult result = Parse(
                "{\"_format\":\"json-diff\",\"_formatVersion\":1,\"conversations\":{}}");

            Assert.Equal(GlobalStateLoadOutcome.UnsupportedVersion, result.Outcome);
            Assert.Null(result.State);
            Assert.Contains("json-diff", result.ErrorMessage!, StringComparison.Ordinal);
        }

        [Fact]
        public void Deserialize_TruncatedAtEveryLength_IsNeverSilentlyPartial()
        {
            // The torn-write family, exhaustively: every prefix of a real file must be
            // either rejected or exactly equal to the whole thing, never a half state
            // that looks fine.
            string complete = GlobalStateJson.Serialize(StateWith(
                (3, 17, SimStatus.WasDisplayed),
                (3, 18, SimStatus.WasOffered),
                (4, 1, SimStatus.WasDisplayed)));

            for (int length = 0; length < complete.Length; length++)
            {
                GlobalStateLoadResult result = Parse(complete.Substring(0, length));
                Assert.Equal(GlobalStateLoadOutcome.Corrupt, result.Outcome);
            }

            Assert.Equal(GlobalStateLoadOutcome.Loaded, Parse(complete).Outcome);
        }

        // -------------------------------------------------------------------
        // Versions: anything but the current one is refused
        // -------------------------------------------------------------------

        [Theory]
        [InlineData(GlobalStateJson.FormatVersion + 1)]
        [InlineData(99)]
        public void Deserialize_NewerFormatVersion_IsUnsupportedVersion(int version)
        {
            GlobalStateLoadResult result = Parse(
                $"{{\"_format\":\"global-state\",\"_formatVersion\":{version},"
                + "\"conversations\":{\"WasOffered\":{\"1\":\"2\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.UnsupportedVersion, result.Outcome);
            Assert.Null(result.State);
            Assert.Contains(version.ToString(), result.ErrorMessage!);
        }

        [Theory]
        [InlineData(0)]
        [InlineData(-1)]
        [InlineData(1)]
        public void Deserialize_OlderFormatVersion_IsRefusedNotRead(int version)
        {
            // This used to load. It loaded because refusing would have made the caller
            // treat the file as unwritable and stop tracking, which was the greater harm
            // while old files were still in use. Now that a converter exists and the
            // profiles in use have been migrated, the balance is the other way: a file
            // this old should be refused loudly rather than read by a path nothing else
            // exercises. Refused, not Corrupt - the file is real history, so the caller
            // must stop rather than overwrite it.
            GlobalStateLoadResult result = Parse(
                $"{{\"version\":{version},\"conversations\":{{\"1\":{{\"2\":\"WasOffered\"}}}}}}");

            // It does not even say what format it is: naming one is what version 5 added,
            // so every older file is refused for that before its version is looked at.
            Assert.Equal(GlobalStateLoadOutcome.OutdatedVersion, result.Outcome);
            Assert.Null(result.State);
            Assert.Contains(
                FormatStamp.Converter, result.ErrorMessage!, StringComparison.Ordinal);
        }

        // -------------------------------------------------------------------
        // The shape this reads, and the damage it survives
        // -------------------------------------------------------------------

        [Fact]
        public void Deserialize_GroupedFormat_ReadsEveryStatusGroup()
        {
            GlobalStateLoadResult result = Parse(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"WasDisplayed\":{\"3\":\"17,19\",\"8\":\"1\"},"
                + "\"WasOffered\":{\"3\":\"18\"}},\"orbs\":[]}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(0, result.SkippedRowCount);
            Assert.Empty(result.Warnings);
            Assert.Equal(SimStatus.WasDisplayed, result.State!.GetStatus(3, 17));
            Assert.Equal(SimStatus.WasDisplayed, result.State.GetStatus(3, 19));
            Assert.Equal(SimStatus.WasDisplayed, result.State.GetStatus(8, 1));
            Assert.Equal(SimStatus.WasOffered, result.State.GetStatus(3, 18));
        }

        [Theory]
        [InlineData(1)]
        [InlineData(2)]
        public void Deserialize_LegacyVersion_IsRefusedRatherThanRead(int version)
        {
            // Refused as OutdatedVersion, NOT Corrupt. The distinction is the whole
            // point: a caller that sees Corrupt may overwrite the file from a backup, and
            // this file is full of real history. It has to stop instead.
            GlobalStateLoadResult result = Parse(
                "{\"version\":" + version + ",\"conversations\":{\"3\":{\"17\":\"WasDisplayed\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.OutdatedVersion, result.Outcome);
            Assert.Null(result.State);
            Assert.Contains(
                FormatStamp.Converter, result.ErrorMessage!, StringComparison.Ordinal);
            Assert.Throws<InvalidOperationException>(() => result.RequireState());
        }

        [Fact]
        public void Deserialize_TheCurrentVersionIsTheOnlyOneRead()
        {
            // Says out loud what the gate is, so that lowering it needs a deliberate edit
            // here rather than passing unnoticed. What an older file looks like is
            // the engine host's convert verb's business now, and nothing here reads one.
            Assert.Equal(
                GlobalStateJson.FormatVersion,
                GlobalStateJson.MinimumReadableFormatVersion);
        }

        [Fact]
        public void Deserialize_GroupedFormat_SkipsAnUnrecognizedStatusBlockWhole()
        {
            // One warning for the block rather than one per row: there are only ever a
            // few blocks, and repeating the same fact would push every other complaint
            // out of the warning budget.
            GlobalStateLoadResult result = Parse(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"WasChewed\":{\"3\":\"17-19\"},"
                + "\"WasOffered\":{\"3\":\"20\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(3, result.SkippedRowCount);
            Assert.Single(result.Warnings);
            Assert.Contains("WasChewed", result.Warnings[0]);
            Assert.Equal(SimStatus.Untouched, result.State!.GetStatus(3, 17));
            Assert.Equal(SimStatus.WasOffered, result.State.GetStatus(3, 20));
        }

        [Fact]
        public void Deserialize_GroupedFormat_SkipsMalformedRowsAndKeepsTheRest()
        {
            GlobalStateLoadResult result = Parse(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"WasOffered\":{"
                + "\"not-a-number\":\"1-2\","
                + "\"4\":{\"5\":\"WasOffered\"},"
                + "\"6\":\"7,eight,9\","
                + "\"8\":[3],"
                + "\"10\":\"1,3-5\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);

            // Two under the unreadable key, whose run does expand and so can be counted;
            // then ONE EACH for the three that cannot be read at all - an object, a
            // malformed run, and a format 3 array. None of those three has a number of
            // rows to report: a run that will not parse is not a list with a bad element
            // in it, and counting what it might have held would be inventing a figure.
            Assert.Equal(5, result.SkippedRowCount);

            // The well-formed run beside them still loads, which is the "keeps the rest"
            // half - including its range, so a skipped neighbour does not cost a run.
            Assert.Equal(SimStatus.WasOffered, result.State!.GetStatus(10, 1));
            Assert.Equal(SimStatus.WasOffered, result.State.GetStatus(10, 3));
            Assert.Equal(SimStatus.WasOffered, result.State.GetStatus(10, 5));
            Assert.Equal(SimStatus.Untouched, result.State.GetStatus(10, 2));

            Assert.Equal(SimStatus.Untouched, result.State.GetStatus(4, 5));
            Assert.Equal(SimStatus.Untouched, result.State.GetStatus(6, 7));
            Assert.Equal(SimStatus.Untouched, result.State.GetStatus(8, 3));
        }

        [Fact]
        public void Deserialize_GroupedFormat_CannotLowerAStatus()
        {
            // The same guarantee the per-entry reader gives: every row goes through
            // TryMerge, so a file cannot pull a status back down.
            GlobalStateLoadResult result = Parse(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"WasDisplayed\":{\"3\":\"17\"},"
                + "\"WasOffered\":{\"3\":\"17\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(SimStatus.WasDisplayed, result.State!.GetStatus(3, 17));
        }

        // -------------------------------------------------------------------
        // Orbs
        // -------------------------------------------------------------------

        [Fact]
        public void Serialize_WritesOrbsSortedOrdinally()
        {
            var state = new GlobalConversationState();
            state.MergeOrb("PLAZA ORB / seagull");
            state.MergeOrb("COAST ORB / floatice");
            state.MergeOrb("COAST ORB / drawbridge");

            Assert.Equal(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{},\"orbs\":["
                + "\"COAST ORB / drawbridge\",\"COAST ORB / floatice\",\"PLAZA ORB / seagull\"]}",
                GlobalStateJson.Serialize(state));
        }

        [Fact]
        public void Orbs_SurviveARoundTrip()
        {
            var state = new GlobalConversationState();
            state.Merge(3, 17, SimStatus.WasDisplayed);
            state.MergeOrb("LANDS END / DEPOT DOOR");
            state.MergeOrb("WHIRLING F1 ORB / spilled rum");

            GlobalStateLoadResult result = Parse(GlobalStateJson.Serialize(state));

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(2, result.State!.OrbCount);
            Assert.True(result.State.ContainsOrb("LANDS END / DEPOT DOOR"));
            Assert.True(result.State.ContainsOrb("WHIRLING F1 ORB / spilled rum"));
            Assert.Equal(state.Score, result.State.Score);
        }

        [Fact]
        public void Deserialize_OrbsNotAnArray_SkipsThemAndLoadsTheRest()
        {
            GlobalStateLoadResult result = Parse(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"WasOffered\":{\"1\":\"2\"}},\"orbs\":\"nope\"}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(1, result.SkippedRowCount);
            Assert.Equal(0, result.State!.OrbCount);
            Assert.Equal(SimStatus.WasOffered, result.State.GetStatus(1, 2));
        }

        [Theory]
        [InlineData("17")]
        [InlineData("null")]
        [InlineData("[]")]
        [InlineData("\"\"")]
        public void Deserialize_BadOrbElement_SkipsThatElementOnly(string element)
        {
            GlobalStateLoadResult result = Parse(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{},\"orbs\":[\"COAST ORB / seagull\","
                + element + "]}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(1, result.SkippedRowCount);
            Assert.Equal(1, result.State!.OrbCount);
            Assert.True(result.State.ContainsOrb("COAST ORB / seagull"));
        }

        [Fact]
        public void Deserialize_DuplicateOrbs_AreCountedOnce()
        {
            GlobalStateLoadResult result = Parse(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{},\"orbs\":["
                + "\"COAST ORB / seagull\",\"COAST ORB / seagull\"]}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(0, result.SkippedRowCount);
            Assert.Equal(1, result.State!.OrbCount);
        }

        // -------------------------------------------------------------------
        // Row-level damage: skipped and reported, load still succeeds
        // -------------------------------------------------------------------

        [Fact]
        public void Deserialize_NonIntegerConversationKey_SkipsTheWholeConversation()
        {
            GlobalStateLoadResult result = Parse(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"WasOffered\":{\"oops\":\"1,2\",\"5\":\"6\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(2, result.SkippedRowCount);
            Assert.Equal(1, result.RequireState().EntryCount);
            Assert.Equal(SimStatus.WasOffered, result.RequireState().GetStatus(5, 6));
        }

        [Fact]
        public void Deserialize_UntouchedInFile_IsRecognizedButStoresNothing()
        {
            // A hand-written file may spell Untouched out. It is a known status, so it is
            // not a skipped row, but it must not create an entry.
            GlobalStateLoadResult result = Parse(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"Untouched\":{\"1\":\"2\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(0, result.SkippedRowCount);
            Assert.True(result.RequireState().IsEmpty);
        }

        [Fact]
        public void Deserialize_WarningsAreCappedButTheCountIsNot()
        {
            var json = new StringBuilder("{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"WasOffered\":{");
            const int badRowCount = GlobalStateJson.MaxWarnings + 15;
            for (int i = 0; i < badRowCount; i++)
            {
                json.Append(i == 0 ? string.Empty : ",")
                    .Append("\"not-a-conversation-")
                    .Append(i)
                    .Append("\":\"1\"");
            }

            json.Append("}}}");

            GlobalStateLoadResult result = Parse(json.ToString());

            Assert.Equal(badRowCount, result.SkippedRowCount);
            Assert.Equal(GlobalStateJson.MaxWarnings, result.Warnings.Count);
        }

        // -------------------------------------------------------------------
        // The load path cannot lower a status
        // -------------------------------------------------------------------

        [Fact]
        public void Deserialize_DuplicateConversationBlocks_AreMergedNotReplaced()
        {
            // A hand-edited file can name the same conversation twice under two statuses.
            // Loading goes through Merge rather than assignment, so both rows land and
            // neither pulls the other down.
            GlobalStateLoadResult result = Parse(
                "{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{\"WasDisplayed\":{\"1\":\"2\"},"
                + "\"WasOffered\":{\"1\":\"3\"}}}");

            GlobalConversationState state = result.RequireState();
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(1, 2));
            Assert.Equal(SimStatus.WasOffered, state.GetStatus(1, 3));
        }

        // -------------------------------------------------------------------
        // Argument checking
        // -------------------------------------------------------------------

        [Fact]
        public void Serialize_NullState_Throws()
        {
            Assert.Throws<ArgumentNullException>(() => GlobalStateJson.SerializeToUtf8Bytes(null!));
        }

        [Fact]
        public void Deserialize_NullInput_Throws()
        {
            Assert.Throws<ArgumentNullException>(() => GlobalStateJson.Deserialize((string)null!, TestSource));
            Assert.Throws<ArgumentNullException>(() => GlobalStateJson.Deserialize((byte[])null!, TestSource));
        }

        [Fact]
        public void LoadResult_RecordsWhereItCameFrom()
        {
            IReadOnlyList<GlobalStateLoadResult> results = new[]
            {
                Parse("{\"_format\":\"global-state\",\"_formatVersion\":5,\"conversations\":{}}"),
                Parse("garbage"),
                Parse("{\"version\":7,\"conversations\":{}}"),
            };

            Assert.All(results, result => Assert.Equal(TestSource, result.SourcePath));
        }
    }
}
