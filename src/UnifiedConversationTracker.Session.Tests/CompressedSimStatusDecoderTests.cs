using System.Collections.Generic;
using System.Linq;
using System.Text;
using UnifiedConversationTracker.Core;
using Xunit;

namespace UnifiedConversationTracker.Session.Tests
{
    /// <summary>
    /// The decoder for a savegame's compressed SimStatus blobs.
    /// </summary>
    /// <remarks>
    /// Three things matter more than the rest and each has its own group below: that
    /// the format is read the way the game reads it, that a duplicated articy id
    /// resolves LAST-WINS exactly as the game's own expansion does, and that an entry
    /// the blob does not mention means Untouched rather than unknown. The fourth is
    /// that nothing it cannot make sense of is quietly swallowed, because the caller
    /// refuses the whole interception on those counts.
    /// </remarks>
    public sealed class CompressedSimStatusDecoderTests
    {
        /// <summary>Conversation 9 with three entries, one articy id shared by two of them.</summary>
        private const string SampleMap = @"{
            ""conversations"": { ""conv9"": 9 },
            ""dialogue_entries"": {
                ""solo"": [9, [4]],
                ""shared"": [9, [5, 6]],
                ""other"": [9, [7]],
                ""elsewhere"": [12, [1]]
            }
        }";

        private static CompressedSimStatusDecoder NewDecoder() =>
            new CompressedSimStatusDecoder(ArticyIdMap.Load(Encoding.UTF8.GetBytes(SampleMap)));

        private static Dictionary<int, string?> RowsByEntry(CompressedSimStatusDecoder decoder) =>
            decoder.Rows.ToDictionary(row => row.DialogueEntryId, row => row.StatusName);

        // ---- The format ----

        [Fact]
        public void Decode_ReadsThePairsAndTheGamesThreeStatusCharacters()
        {
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "solo;d;shared;o;other;u");

            Assert.Equal(3, decoder.PairCount);
            Assert.Equal(1, decoder.BlobCount);
            Assert.True(decoder.IsClean);

            Dictionary<int, string?> rows = RowsByEntry(decoder);
            Assert.Equal(SimStatusNames.WasDisplayed, rows[4]);
            Assert.Equal(SimStatusNames.WasOffered, rows[6]);
        }

        [Fact]
        public void Decode_TagsEveryRowWithTheConversationTheBlobBelongsTo()
        {
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "solo;d");

            Assert.Equal(9, Assert.Single(decoder.Rows).ConversationId);
        }

        [Fact]
        public void Decode_AcrossSeveralBlobs_KeepsCountingAndAccumulating()
        {
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "solo;d");
            decoder.Decode(9, "other;o");

            Assert.Equal(2, decoder.BlobCount);
            Assert.Equal(2, decoder.PairCount);
            Assert.Equal(2, decoder.Rows.Count);
        }

        [Fact]
        public void Decode_ForAnEmptyBlob_CountsTheBlobAndNothingElse()
        {
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, string.Empty);

            Assert.Equal(1, decoder.BlobCount);
            Assert.Equal(0, decoder.PairCount);
            Assert.True(decoder.IsClean);
        }

        [Fact]
        public void Reset_ForgetsEverythingSoTheNextLoadStartsClean()
        {
            CompressedSimStatusDecoder decoder = NewDecoder();
            decoder.Decode(9, "solo;d");

            decoder.Reset();

            Assert.Equal(0, decoder.BlobCount);
            Assert.Equal(0, decoder.PairCount);
            Assert.Empty(decoder.Rows);
        }

        // ---- Last-wins duplicate resolution ----

        [Fact]
        public void Decode_ForADuplicatedArticyId_KeepsTheLastOccurrenceAndNotTheFirst()
        {
            // The game resolves both occurrences to the same (last) entry and writes
            // them with LuaTable.AddRaw, which overwrites. So the second status is what
            // survives the load, and reproducing that is what makes this agree with a
            // walk taken afterwards.
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "shared;d;shared;o");

            SimStatusRow row = Assert.Single(decoder.Rows);
            Assert.Equal(6, row.DialogueEntryId);
            Assert.Equal(SimStatusNames.WasOffered, row.StatusName);
        }

        [Fact]
        public void Decode_ForADuplicatedArticyIdWhoseLastOccurrenceIsUntouched_KeepsNothing()
        {
            // The dangerous half of last-wins, and the reason Untouched pairs cannot
            // simply be skipped as they are read: an earlier WasDisplayed really is
            // overwritten by a later Untouched, and the game drops it too.
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "shared;d;shared;u");

            Assert.Empty(decoder.Rows);
            Assert.Equal(2, decoder.PairCount);
            Assert.True(decoder.IsClean);
        }

        [Fact]
        public void Decode_CountsThePairsADuplicateShadowed()
        {
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "shared;d;shared;o;solo;d");

            Assert.Equal(1, decoder.ShadowedPairCount);
            Assert.Equal(3, decoder.PairCount);
        }

        [Fact]
        public void Decode_DoesNotCarryDuplicateResolutionAcrossBlobs()
        {
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "shared;d");
            decoder.Decode(9, "shared;o");

            Assert.Equal(0, decoder.ShadowedPairCount);
            Assert.Equal(2, decoder.Rows.Count);
        }

        // ---- Absence means Untouched ----

        [Fact]
        public void Decode_EmitsNoRowForAnUntouchedPair()
        {
            // The blob is complete rather than a delta - it spells Untouched out
            // explicitly for most of its rows - and the unified state stores nothing
            // for Untouched, so emitting these would be pure cost.
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "solo;u;other;u");

            Assert.Empty(decoder.Rows);
            Assert.Equal(2, decoder.PairCount);
            Assert.True(decoder.IsClean);
        }

        [Fact]
        public void Decode_EmitsNoRowForAnEntryTheBlobNeverMentions()
        {
            // Which is the same thing said the other way round: the game fills an
            // unmentioned entry in as Untouched, so absence means Untouched
            // definitively and never "unknown". Entry 7 is in the map and not in the
            // blob, and must not appear.
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "solo;d");

            Assert.Equal(4, Assert.Single(decoder.Rows).DialogueEntryId);
        }

        // ---- Nothing is swallowed ----

        [Fact]
        public void Decode_ForAnUnknownStatusCharacter_CountsItMalformedRatherThanGuessing()
        {
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "solo;z");

            Assert.Equal(1, decoder.MalformedPairCount);
            Assert.False(decoder.IsClean);
            Assert.Empty(decoder.Rows);
        }

        [Fact]
        public void Decode_ForAnEmptyStatusToken_CountsItMalformed()
        {
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "solo;;other;d");

            Assert.Equal(1, decoder.MalformedPairCount);
            Assert.False(decoder.IsClean);
        }

        [Fact]
        public void Decode_ForATrailingTokenWithNoStatus_CountsItMalformed()
        {
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "solo;d;other");

            Assert.Equal(1, decoder.MalformedPairCount);
            Assert.Single(decoder.Rows);
        }

        [Fact]
        public void Decode_ForAStatusTokenLongerThanOneCharacter_ReadsTheFirstLikeTheGameDoes()
        {
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "solo;dd");

            Assert.Equal(0, decoder.MalformedPairCount);
            Assert.Equal(SimStatusNames.WasDisplayed, Assert.Single(decoder.Rows).StatusName);
        }

        [Fact]
        public void Decode_ForAnArticyIdTheMapDoesNotKnow_CountsItUnresolved()
        {
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "nosuchid;d");

            Assert.Equal(1, decoder.UnresolvedPairCount);
            Assert.False(decoder.IsClean);
            Assert.Empty(decoder.Rows);
        }

        [Fact]
        public void Decode_ForAnArticyIdBelongingToAnotherConversation_DropsItLikeTheGameWould()
        {
            // The game builds its articy-id lookup from the blob's own conversation's
            // entries, so an id from elsewhere is simply not found there. Resolving it
            // globally would write a row the game never wrote.
            CompressedSimStatusDecoder decoder = NewDecoder();

            decoder.Decode(9, "elsewhere;d");

            Assert.Equal(1, decoder.ForeignConversationPairCount);
            Assert.False(decoder.IsClean);
            Assert.Empty(decoder.Rows);
        }
    }
}
