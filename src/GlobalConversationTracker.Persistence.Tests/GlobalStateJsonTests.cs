// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Text;
using Xunit;

namespace GlobalConversationTracker.Persistence.Tests
{
    /// <summary>
    /// Format-level tests: the exact bytes written, and every way a file can be
    /// wrong.
    /// </summary>
    public class GlobalStateJsonTests
    {
        private const string TestSource = "test";

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

        /// <summary>
        /// Reads through the migration-only path, which is the only thing that still
        /// understands the per-entry shape.
        /// </summary>
        /// <remarks>
        /// Every test below that passes a version 1 or 2 document uses this. They are not
        /// describing what the game does on load any more - it refuses those files, which
        /// is what the version-gate tests cover - but what the converter has to cope with
        /// on the way to producing a version 3 file, which is still worth pinning down:
        /// the profiles being migrated are real ones, damage and all.
        /// </remarks>
        private static GlobalStateLoadResult ParseLegacy(string json) =>
            GlobalStateJson.DeserializeLegacy(Encoding.UTF8.GetBytes(json), TestSource);

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
                "{\"version\":3,\"conversations\":{\"WasOffered\":{\"3\":[18]},"
                + "\"WasDisplayed\":{\"3\":[17]}},\"orbs\":[]}",
                GlobalStateJson.Serialize(state));
        }

        [Fact]
        public void Serialize_EmptyState_WritesAnEmptyConversationMap()
        {
            Assert.Equal(
                "{\"version\":3,\"conversations\":{},\"orbs\":[]}",
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
                "{\"version\":3,\"conversations\":{\"WasOffered\":{\"1\":[2]}},\"orbs\":[]}", json);
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
                "{\"version\":3,\"conversations\":{\"WasOffered\":{\"2\":[9,100],\"10\":[1]}},"
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
        [InlineData("{\"version\":3,\"conversations\":{\"3\":{\"17\":\"WasDis")]
        [InlineData("{\"version\":3,\"conversations\":{}} trailing garbage")]
        [InlineData("[]")]
        [InlineData("\"a string\"")]
        [InlineData("null")]
        [InlineData("{\"conversations\":{}}")]
        [InlineData("{\"version\":\"1\",\"conversations\":{}}")]
        [InlineData("{\"version\":1.5,\"conversations\":{}}")]
        [InlineData("{\"version\":3}")]
        [InlineData("{\"version\":3,\"conversations\":[]}")]
        [InlineData("{\"version\":3,\"conversations\":null}")]
        public void Deserialize_StructurallyBrokenFile_IsCorrupt(string json)
        {
            GlobalStateLoadResult result = Parse(json);

            Assert.Equal(GlobalStateLoadOutcome.Corrupt, result.Outcome);
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
        // Versions: newer is refused, older is read
        // -------------------------------------------------------------------

        [Theory]
        [InlineData(GlobalStateJson.FormatVersion + 1)]
        [InlineData(99)]
        public void Deserialize_NewerFormatVersion_IsUnsupportedVersion(int version)
        {
            GlobalStateLoadResult result = Parse(
                $"{{\"version\":{version},\"conversations\":{{\"1\":{{\"2\":\"WasOffered\"}}}}}}");

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

            Assert.Equal(GlobalStateLoadOutcome.UnsupportedVersion, result.Outcome);
            Assert.Null(result.State);
            Assert.Contains(version.ToString(), result.ErrorMessage!, StringComparison.Ordinal);
        }

        [Fact]
        public void Deserialize_VersionOneFile_LoadsWithNoOrbsAndNoWarnings()
        {
            // The exact shape every file on disk had before orbs existed.
            GlobalStateLoadResult result = ParseLegacy(
                "{\"version\":1,\"conversations\":{\"3\":{\"17\":\"WasDisplayed\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(0, result.SkippedRowCount);
            Assert.Empty(result.Warnings);
            Assert.Equal(0, result.State!.OrbCount);
            Assert.Equal(SimStatus.WasDisplayed, result.State.GetStatus(3, 17));
        }

        // -------------------------------------------------------------------
        // The grouped shape, and the per-entry one it replaced
        // -------------------------------------------------------------------

        [Fact]
        public void Deserialize_GroupedFormat_ReadsEveryStatusGroup()
        {
            GlobalStateLoadResult result = Parse(
                "{\"version\":3,\"conversations\":{\"WasDisplayed\":{\"3\":[17,19],\"8\":[1]},"
                + "\"WasOffered\":{\"3\":[18]}},\"orbs\":[]}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(0, result.SkippedRowCount);
            Assert.Empty(result.Warnings);
            Assert.Equal(SimStatus.WasDisplayed, result.State!.GetStatus(3, 17));
            Assert.Equal(SimStatus.WasDisplayed, result.State.GetStatus(3, 19));
            Assert.Equal(SimStatus.WasDisplayed, result.State.GetStatus(8, 1));
            Assert.Equal(SimStatus.WasOffered, result.State.GetStatus(3, 18));
        }

        [Fact]
        public void Deserialize_VersionTwoFile_StillReadsThePerEntryShape()
        {
            // The shape every file on disk had before the grouping. Read for as long as
            // GlobalStateJson.LegacyPerEntryFormatVersion says so, and no longer.
            GlobalStateLoadResult result = ParseLegacy(
                "{\"version\":2,\"conversations\":{\"3\":{\"17\":\"WasDisplayed\",\"18\":\"WasOffered\"}},"
                + "\"orbs\":[\"COAST ORB / floatice\"]}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(0, result.SkippedRowCount);
            Assert.Empty(result.Warnings);
            Assert.Equal(SimStatus.WasDisplayed, result.State!.GetStatus(3, 17));
            Assert.Equal(SimStatus.WasOffered, result.State.GetStatus(3, 18));
            Assert.Equal(1, result.State.OrbCount);
        }

        [Theory]
        [InlineData(1)]
        [InlineData(2)]
        public void Deserialize_LegacyVersion_IsRefusedRatherThanRead(int version)
        {
            // Refused as UnsupportedVersion, NOT Corrupt. The distinction is the whole
            // point: a caller that sees Corrupt may overwrite the file from a backup, and
            // this file is full of real history. It has to stop instead.
            GlobalStateLoadResult result = Parse(
                "{\"version\":" + version + ",\"conversations\":{\"3\":{\"17\":\"WasDisplayed\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.UnsupportedVersion, result.Outcome);
            Assert.Null(result.State);
            Assert.Contains(
                "GlobalStateConvert", result.ErrorMessage!, StringComparison.Ordinal);
            Assert.Throws<InvalidOperationException>(() => result.RequireState());
        }

        [Fact]
        public void Deserialize_LegacyVersion_IsStillReadableForMigration()
        {
            // The converter has to keep reading what the runtime now refuses, or there is
            // no way off the old format.
            const string legacy =
                "{\"version\":2,\"conversations\":{\"3\":{\"17\":\"WasDisplayed\"}},\"orbs\":[]}";

            Assert.Equal(GlobalStateLoadOutcome.UnsupportedVersion, Parse(legacy).Outcome);
            Assert.Equal(GlobalStateLoadOutcome.Loaded, ParseLegacy(legacy).Outcome);
        }

        [Fact]
        public void Deserialize_TheCurrentVersion_IsTheOldestAccepted()
        {
            // Says out loud what the gate is, so that lowering it needs a deliberate edit
            // here rather than passing unnoticed.
            Assert.Equal(
                GlobalStateJson.FormatVersion,
                GlobalStateJson.MinimumReadableFormatVersion);
            Assert.True(
                GlobalStateJson.LegacyPerEntryFormatVersion
                    < GlobalStateJson.MinimumReadableFormatVersion,
                "the per-entry shape must sit below the minimum the runtime reads");
        }

        [Fact]
        public void Deserialize_BothShapes_DescribeTheSameState()
        {
            // The claim the migration rests on: the two files say the same thing, so a
            // profile that has not been rewritten yet loses nothing by waiting.
            GlobalStateLoadResult grouped = Parse(
                "{\"version\":3,\"conversations\":{\"WasOffered\":{\"3\":[18]},"
                + "\"WasDisplayed\":{\"3\":[17]}},\"orbs\":[]}");
            GlobalStateLoadResult perEntry = ParseLegacy(
                "{\"version\":2,\"conversations\":{\"3\":{\"17\":\"WasDisplayed\","
                + "\"18\":\"WasOffered\"}},\"orbs\":[]}");

            Assert.Equal(
                GlobalStateJson.Serialize(grouped.State!),
                GlobalStateJson.Serialize(perEntry.State!));
        }

        [Fact]
        public void Deserialize_GroupedFormat_SkipsAnUnrecognizedStatusBlockWhole()
        {
            // One warning for the block rather than one per row: there are only ever a
            // few blocks, and repeating the same fact would push every other complaint
            // out of the warning budget.
            GlobalStateLoadResult result = Parse(
                "{\"version\":3,\"conversations\":{\"WasChewed\":{\"3\":[17,18,19]},"
                + "\"WasOffered\":{\"3\":[20]}}}");

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
                "{\"version\":3,\"conversations\":{\"WasOffered\":{"
                + "\"not-a-number\":[1,2],"
                + "\"4\":{\"5\":\"WasOffered\"},"
                + "\"6\":[7,\"eight\",9]}}}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            // Two under the unreadable key, one for the object where an array belongs,
            // one for the entry ID that is not a number.
            Assert.Equal(4, result.SkippedRowCount);
            Assert.Equal(SimStatus.WasOffered, result.State!.GetStatus(6, 7));
            Assert.Equal(SimStatus.WasOffered, result.State.GetStatus(6, 9));
            Assert.Equal(SimStatus.Untouched, result.State.GetStatus(4, 5));
        }

        [Fact]
        public void Deserialize_GroupedFormat_CannotLowerAStatus()
        {
            // The same guarantee the per-entry reader gives: every row goes through
            // TryMerge, so a file cannot pull a status back down.
            GlobalStateLoadResult result = Parse(
                "{\"version\":3,\"conversations\":{\"WasDisplayed\":{\"3\":[17]},"
                + "\"WasOffered\":{\"3\":[17]}}}");

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
                "{\"version\":3,\"conversations\":{},\"orbs\":["
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
                "{\"version\":3,\"conversations\":{\"WasOffered\":{\"1\":[2]}},\"orbs\":\"nope\"}");

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
                "{\"version\":3,\"conversations\":{},\"orbs\":[\"COAST ORB / seagull\","
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
                "{\"version\":3,\"conversations\":{},\"orbs\":["
                + "\"COAST ORB / seagull\",\"COAST ORB / seagull\"]}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(0, result.SkippedRowCount);
            Assert.Equal(1, result.State!.OrbCount);
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
            GlobalStateLoadResult result = ParseLegacy(
                "{\"version\":1,\"conversations\":{\"1\":{\"2\":" + statusLiteral + ",\"3\":\"WasOffered\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(1, result.SkippedRowCount);
            Assert.Single(result.Warnings);
            GlobalConversationState state = result.RequireState();
            Assert.Equal(SimStatus.Untouched, state.GetStatus(1, 2));
            Assert.Equal(SimStatus.WasOffered, state.GetStatus(1, 3));
        }

        [Fact]
        public void Deserialize_NonIntegerConversationKey_SkipsTheWholeConversation()
        {
            GlobalStateLoadResult result = ParseLegacy(
                "{\"version\":1,\"conversations\":{\"oops\":{\"1\":\"WasOffered\",\"2\":\"WasOffered\"},"
                + "\"5\":{\"6\":\"WasDisplayed\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(2, result.SkippedRowCount);
            Assert.Equal(1, result.RequireState().EntryCount);
            Assert.Equal(SimStatus.WasDisplayed, result.RequireState().GetStatus(5, 6));
        }

        [Fact]
        public void Deserialize_ConversationValueNotAnObject_SkipsIt()
        {
            GlobalStateLoadResult result = ParseLegacy(
                "{\"version\":1,\"conversations\":{\"1\":\"WasOffered\",\"2\":{\"3\":\"WasOffered\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(1, result.SkippedRowCount);
            Assert.Equal(1, result.RequireState().EntryCount);
        }

        [Fact]
        public void Deserialize_NonIntegerEntryKey_SkipsThatRowOnly()
        {
            GlobalStateLoadResult result = ParseLegacy(
                "{\"version\":1,\"conversations\":{\"1\":{\"x\":\"WasOffered\",\"2\":\"WasOffered\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
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
            GlobalStateLoadResult result = ParseLegacy(
                "{\"version\":1,\"conversations\":{\"1\":{\"2\":" + statusLiteral + ",\"3\":\"WasOffered\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(1, result.SkippedRowCount);
            Assert.Equal(1, result.RequireState().EntryCount);
        }

        [Fact]
        public void Deserialize_UntouchedInFile_IsRecognizedButStoresNothing()
        {
            // A hand-written or older file may spell Untouched out. It is a known
            // status, so it is not a skipped row, but it must not create an entry.
            GlobalStateLoadResult result = ParseLegacy(
                "{\"version\":1,\"conversations\":{\"1\":{\"2\":\"Untouched\"}}}");

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(0, result.SkippedRowCount);
            Assert.True(result.RequireState().IsEmpty);
        }

        [Fact]
        public void Deserialize_WarningsAreCappedButTheCountIsNot()
        {
            var json = new StringBuilder("{\"version\":1,\"conversations\":{\"1\":{");
            const int badRowCount = GlobalStateJson.MaxWarnings + 15;
            for (int i = 0; i < badRowCount; i++)
            {
                json.Append(i == 0 ? string.Empty : ",").Append('"').Append(i).Append("\":\"Nonsense\"");
            }

            json.Append("}}}");

            GlobalStateLoadResult result = ParseLegacy(json.ToString());

            Assert.Equal(badRowCount, result.SkippedRowCount);
            Assert.Equal(GlobalStateJson.MaxWarnings, result.Warnings.Count);
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
            GlobalStateLoadResult displayedFirst = ParseLegacy(
                "{\"version\":1,\"conversations\":{\"1\":{\"2\":\"WasDisplayed\",\"2\":\"WasOffered\"}}}");
            GlobalStateLoadResult offeredFirst = ParseLegacy(
                "{\"version\":1,\"conversations\":{\"1\":{\"2\":\"WasOffered\",\"2\":\"WasDisplayed\"}}}");

            Assert.Equal(SimStatus.WasDisplayed, displayedFirst.RequireState().GetStatus(1, 2));
            Assert.Equal(SimStatus.WasDisplayed, offeredFirst.RequireState().GetStatus(1, 2));
        }

        [Fact]
        public void Deserialize_DuplicateConversationBlocks_AreMergedNotReplaced()
        {
            GlobalStateLoadResult result = ParseLegacy(
                "{\"version\":1,\"conversations\":{\"1\":{\"2\":\"WasDisplayed\"},\"1\":{\"3\":\"WasOffered\"}}}");

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
                Parse("{\"version\":3,\"conversations\":{}}"),
                Parse("garbage"),
                Parse("{\"version\":7,\"conversations\":{}}"),
            };

            Assert.All(results, result => Assert.Equal(TestSource, result.SourcePath));
        }
    }
}
