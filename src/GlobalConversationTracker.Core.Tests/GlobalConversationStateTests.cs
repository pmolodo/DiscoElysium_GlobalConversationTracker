using System;
using System.Collections;
using System.Collections.Generic;
using System.Linq;
using GlobalConversationTracker;
using Xunit;

namespace GlobalConversationTracker.Tests
{
    public class GlobalConversationStateTests
    {
        private const int ConversationId = 42;
        private const int EntryId = 7;

        private static GlobalConversationState StateWith(SimStatus status)
        {
            var state = new GlobalConversationState();
            state.Merge(ConversationId, EntryId, status);
            Assert.Equal(status, state.GetStatus(ConversationId, EntryId));
            return state;
        }

        // -------------------------------------------------------------------
        // Upgrades
        // -------------------------------------------------------------------

        [Theory]
        [InlineData(SimStatus.Untouched, SimStatus.WasOffered)]
        [InlineData(SimStatus.Untouched, SimStatus.WasDisplayed)]
        [InlineData(SimStatus.WasOffered, SimStatus.WasDisplayed)]
        public void Merge_UpgradesInEveryValidDirection(SimStatus from, SimStatus to)
        {
            GlobalConversationState state = StateWith(from);

            Assert.True(state.Merge(ConversationId, EntryId, to));
            Assert.Equal(to, state.GetStatus(ConversationId, EntryId));
        }

        [Theory]
        [InlineData(SimStatus.WasOffered)]
        [InlineData(SimStatus.WasDisplayed)]
        public void Merge_RecordsAnEntryThatWasNotThereBefore(SimStatus status)
        {
            var state = new GlobalConversationState();

            Assert.True(state.Merge(ConversationId, EntryId, status));
            Assert.Equal(status, state.GetStatus(ConversationId, EntryId));
            Assert.Equal(1, state.ConversationCount);
            Assert.Equal(1, state.EntryCount);
            Assert.False(state.IsEmpty);
        }

        [Fact]
        public void Merge_ReachesWasDisplayedThroughWasOffered()
        {
            // The real sequence from the game: an entry gets offered, then displayed.
            var state = new GlobalConversationState();

            Assert.True(state.Merge(ConversationId, EntryId, SimStatus.WasOffered));
            Assert.True(state.Merge(ConversationId, EntryId, SimStatus.WasDisplayed));
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(ConversationId, EntryId));
            Assert.Equal(1, state.EntryCount);
        }

        // -------------------------------------------------------------------
        // Downgrades: every one of them is a silent no-op
        // -------------------------------------------------------------------

        [Theory]
        [InlineData(SimStatus.WasOffered, SimStatus.Untouched)]
        [InlineData(SimStatus.WasDisplayed, SimStatus.Untouched)]
        [InlineData(SimStatus.WasDisplayed, SimStatus.WasOffered)]
        public void Merge_DowngradeIsASilentNoOp(SimStatus from, SimStatus to)
        {
            GlobalConversationState state = StateWith(from);
            int entryCountBefore = state.EntryCount;

            // No exception, and it reports "nothing changed".
            Assert.False(state.Merge(ConversationId, EntryId, to));

            Assert.Equal(from, state.GetStatus(ConversationId, EntryId));
            Assert.Equal(entryCountBefore, state.EntryCount);
        }

        [Theory]
        [InlineData(SimStatus.Untouched)]
        [InlineData(SimStatus.WasOffered)]
        [InlineData(SimStatus.WasDisplayed)]
        public void Merge_SameStatusReportsNoChange(SimStatus status)
        {
            GlobalConversationState state = StateWith(status);

            Assert.False(state.Merge(ConversationId, EntryId, status));
            Assert.Equal(status, state.GetStatus(ConversationId, EntryId));
        }

        [Fact]
        public void Merge_DowngradeViaGameStringIsASilentNoOp()
        {
            // DialogueLua.MarkDialogueEntryUntouched does exactly this, and a fresh
            // save resets every entry, so the global state has to absorb it.
            GlobalConversationState state = StateWith(SimStatus.WasDisplayed);

            Assert.False(state.Merge(ConversationId, EntryId, SimStatusNames.Untouched));
            Assert.False(state.Merge(ConversationId, EntryId, SimStatusNames.WasOffered));
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(ConversationId, EntryId));
        }

        [Fact]
        public void Merge_SurvivesAFullNewGameResetOfEveryEntry()
        {
            var state = new GlobalConversationState();
            state.Merge(1, 1, SimStatus.WasDisplayed);
            state.Merge(1, 2, SimStatus.WasOffered);
            state.Merge(2, 1, SimStatus.WasDisplayed);

            // New save: the game marks everything Untouched again.
            foreach (GlobalStatusEntry entry in state.EnumerateEntries().ToList())
            {
                Assert.False(state.Merge(entry.ConversationId, entry.DialogueEntryId, SimStatus.Untouched));
            }

            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(1, 1));
            Assert.Equal(SimStatus.WasOffered, state.GetStatus(1, 2));
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(2, 1));
            Assert.Equal(3, state.EntryCount);
        }

        // -------------------------------------------------------------------
        // Untouched is never stored
        // -------------------------------------------------------------------

        [Fact]
        public void Merge_UntouchedOnAnAbsentEntryStoresNothing()
        {
            var state = new GlobalConversationState();

            Assert.False(state.Merge(ConversationId, EntryId, SimStatus.Untouched));

            Assert.True(state.IsEmpty);
            Assert.Equal(0, state.ConversationCount);
            Assert.Equal(0, state.EntryCount);
            Assert.False(state.ContainsConversation(ConversationId));
            Assert.False(state.TryGetStatus(ConversationId, EntryId, out _));
            // ...but it still reads back as Untouched, which is the whole point.
            Assert.Equal(SimStatus.Untouched, state.GetStatus(ConversationId, EntryId));
        }

        // -------------------------------------------------------------------
        // Unknown / missing IDs
        // -------------------------------------------------------------------

        [Fact]
        public void GetStatus_UnknownConversationReadsAsUntouched()
        {
            var state = new GlobalConversationState();

            Assert.Equal(SimStatus.Untouched, state.GetStatus(999, 999));
            Assert.False(state.TryGetStatus(999, 999, out SimStatus status));
            Assert.Equal(SimStatus.Untouched, status);
            Assert.False(state.ContainsConversation(999));
        }

        [Fact]
        public void GetStatus_KnownConversationButUnknownEntryReadsAsUntouched()
        {
            GlobalConversationState state = StateWith(SimStatus.WasDisplayed);

            Assert.True(state.ContainsConversation(ConversationId));
            Assert.Equal(SimStatus.Untouched, state.GetStatus(ConversationId, EntryId + 1));
            Assert.False(state.TryGetStatus(ConversationId, EntryId + 1, out _));
        }

        [Fact]
        public void GetConversationEntries_UnknownConversationYieldsNothing()
        {
            GlobalConversationState state = StateWith(SimStatus.WasDisplayed);

            Assert.Empty(state.GetConversationEntries(999));
            Assert.Single(state.GetConversationEntries(ConversationId));
        }

        [Fact]
        public void Merge_KeepsSameEntryIdInDifferentConversationsIndependent()
        {
            var state = new GlobalConversationState();
            state.Merge(1, EntryId, SimStatus.WasDisplayed);
            state.Merge(2, EntryId, SimStatus.WasOffered);

            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(1, EntryId));
            Assert.Equal(SimStatus.WasOffered, state.GetStatus(2, EntryId));
            Assert.Equal(2, state.ConversationCount);
            Assert.Equal(2, state.EntryCount);
        }

        [Theory]
        [InlineData(0, 0)]
        [InlineData(-1, -5)]
        [InlineData(int.MinValue, int.MaxValue)]
        public void Merge_AcceptsAnyIntegerIds(int conversationId, int dialogueEntryId)
        {
            var state = new GlobalConversationState();

            Assert.True(state.Merge(conversationId, dialogueEntryId, SimStatus.WasOffered));
            Assert.Equal(SimStatus.WasOffered, state.GetStatus(conversationId, dialogueEntryId));
        }

        // -------------------------------------------------------------------
        // Unrecognized status strings arriving from the game
        // -------------------------------------------------------------------

        [Theory]
        [InlineData(null)]
        [InlineData("")]
        [InlineData("Bogus")]
        [InlineData("wasdisplayed")]
        public void Merge_ThrowsOnUnrecognizedStatusStringAndLeavesStateAlone(string? statusName)
        {
            GlobalConversationState state = StateWith(SimStatus.WasOffered);

            Assert.Throws<ArgumentException>(() => state.Merge(ConversationId, EntryId, statusName));

            Assert.Equal(SimStatus.WasOffered, state.GetStatus(ConversationId, EntryId));
            Assert.Equal(1, state.EntryCount);
        }

        [Theory]
        [InlineData(null)]
        [InlineData("")]
        [InlineData("Bogus")]
        [InlineData("wasdisplayed")]
        public void TryMerge_ReportsUnrecognizedStatusStringWithoutThrowing(string? statusName)
        {
            GlobalConversationState state = StateWith(SimStatus.WasOffered);

            Assert.False(state.TryMerge(ConversationId, EntryId, statusName, out bool changed));
            Assert.False(changed);

            Assert.Equal(SimStatus.WasOffered, state.GetStatus(ConversationId, EntryId));
            Assert.Equal(1, state.EntryCount);
        }

        [Fact]
        public void TryMerge_DoesNotInventAnEntryForAnUnrecognizedStatusString()
        {
            var state = new GlobalConversationState();

            Assert.False(state.TryMerge(ConversationId, EntryId, "Bogus", out bool changed));
            Assert.False(changed);
            Assert.True(state.IsEmpty);
        }

        [Fact]
        public void TryMerge_AppliesRecognizedStatusStrings()
        {
            var state = new GlobalConversationState();

            Assert.True(state.TryMerge(ConversationId, EntryId, SimStatusNames.WasOffered, out bool changed));
            Assert.True(changed);

            Assert.True(state.TryMerge(ConversationId, EntryId, SimStatusNames.WasDisplayed, out changed));
            Assert.True(changed);

            // Downgrade: recognized, so true, but nothing changed.
            Assert.True(state.TryMerge(ConversationId, EntryId, SimStatusNames.Untouched, out changed));
            Assert.False(changed);

            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(ConversationId, EntryId));
        }

        [Fact]
        public void Merge_ThrowsOnUndefinedEnumValue()
        {
            var state = new GlobalConversationState();

            // A value outside the enum would break the monotonic ordering, so it is
            // rejected outright rather than stored.
            Assert.Throws<ArgumentOutOfRangeException>(
                () => state.Merge(ConversationId, EntryId, (SimStatus)99));
            Assert.True(state.IsEmpty);
        }

        // -------------------------------------------------------------------
        // Batch merges
        // -------------------------------------------------------------------

        [Fact]
        public void MergeAll_MergesABatchAndCountsOnlyRealChanges()
        {
            var state = new GlobalConversationState();
            state.Merge(1, 1, SimStatus.WasDisplayed);

            int changed = state.MergeAll(new[]
            {
                new GlobalStatusEntry(1, 1, SimStatus.WasOffered),    // downgrade, no-op
                new GlobalStatusEntry(1, 2, SimStatus.WasOffered),    // new
                new GlobalStatusEntry(2, 1, SimStatus.WasDisplayed),  // new
                new GlobalStatusEntry(2, 1, SimStatus.WasDisplayed),  // duplicate, no-op
                new GlobalStatusEntry(3, 1, SimStatus.Untouched),     // never stored
            });

            Assert.Equal(2, changed);
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(1, 1));
            Assert.Equal(SimStatus.WasOffered, state.GetStatus(1, 2));
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(2, 1));
            Assert.False(state.ContainsConversation(3));
            Assert.Equal(3, state.EntryCount);
        }

        [Fact]
        public void MergeAll_MergesAnotherStateWithoutLoweringAnything()
        {
            var target = new GlobalConversationState();
            target.Merge(1, 1, SimStatus.WasDisplayed);
            target.Merge(1, 2, SimStatus.WasOffered);

            var source = new GlobalConversationState();
            source.Merge(1, 1, SimStatus.WasOffered);     // lower than target
            source.Merge(1, 2, SimStatus.WasDisplayed);   // higher than target
            source.Merge(5, 5, SimStatus.WasOffered);     // new

            Assert.Equal(2, target.MergeAll(source));

            Assert.Equal(SimStatus.WasDisplayed, target.GetStatus(1, 1));
            Assert.Equal(SimStatus.WasDisplayed, target.GetStatus(1, 2));
            Assert.Equal(SimStatus.WasOffered, target.GetStatus(5, 5));

            // The source is untouched by the merge.
            Assert.Equal(SimStatus.WasOffered, source.GetStatus(1, 1));
        }

        [Fact]
        public void MergeAll_SelfMergeIsANoOp()
        {
            GlobalConversationState state = StateWith(SimStatus.WasOffered);

            Assert.Equal(0, state.MergeAll(state));
            Assert.Equal(1, state.EntryCount);
        }

        [Fact]
        public void MergeAll_ThrowsOnNull()
        {
            var state = new GlobalConversationState();

            Assert.Throws<ArgumentNullException>(() => state.MergeAll((IEnumerable<GlobalStatusEntry>)null!));
            Assert.Throws<ArgumentNullException>(() => state.MergeAll((GlobalConversationState)null!));
        }

        // -------------------------------------------------------------------
        // Read-only projections (what the serializer reads)
        // -------------------------------------------------------------------

        [Fact]
        public void EnumerateEntries_ReturnsEveryRecordedEntry()
        {
            var state = new GlobalConversationState();
            state.Merge(2, 9, SimStatus.WasDisplayed);
            state.Merge(1, 4, SimStatus.WasOffered);
            state.Merge(1, 1, SimStatus.WasDisplayed);

            Assert.Equal(
                new[]
                {
                    new GlobalStatusEntry(1, 1, SimStatus.WasDisplayed),
                    new GlobalStatusEntry(1, 4, SimStatus.WasOffered),
                    new GlobalStatusEntry(2, 9, SimStatus.WasDisplayed),
                },
                state.EnumerateEntries().OrderBy(e => e.ConversationId).ThenBy(e => e.DialogueEntryId));

            Assert.Equal(new[] { 1, 2 }, state.ConversationIds.OrderBy(id => id));
        }

        [Fact]
        public void EnumerateEntriesInIdOrder_IsSortedAndDeterministic()
        {
            var state = new GlobalConversationState();
            state.Merge(10, 3, SimStatus.WasOffered);
            state.Merge(2, 30, SimStatus.WasDisplayed);
            state.Merge(2, 4, SimStatus.WasOffered);
            state.Merge(-1, 0, SimStatus.WasDisplayed);

            Assert.Equal(
                new[]
                {
                    new GlobalStatusEntry(-1, 0, SimStatus.WasDisplayed),
                    new GlobalStatusEntry(2, 4, SimStatus.WasOffered),
                    new GlobalStatusEntry(2, 30, SimStatus.WasDisplayed),
                    new GlobalStatusEntry(10, 3, SimStatus.WasOffered),
                },
                state.EnumerateEntriesInIdOrder());
        }

        [Fact]
        public void ToNestedDictionary_IsADeepCopy()
        {
            GlobalConversationState state = StateWith(SimStatus.WasOffered);

            Dictionary<int, Dictionary<int, SimStatus>> copy = state.ToNestedDictionary();
            Assert.Equal(SimStatus.WasOffered, copy[ConversationId][EntryId]);

            // Vandalize the copy every way we can.
            copy[ConversationId][EntryId] = SimStatus.Untouched;
            copy[ConversationId].Clear();
            copy.Clear();

            Assert.Equal(SimStatus.WasOffered, state.GetStatus(ConversationId, EntryId));
            Assert.Equal(1, state.EntryCount);
        }

        // -------------------------------------------------------------------
        // Snapshot (what the background writer serializes)
        // -------------------------------------------------------------------

        [Fact]
        public void Snapshot_HoldsExactlyTheSameEntriesAsItsSource()
        {
            // Equality of the enumeration in ID order is the property the on-disk
            // format rests on: the serializer walks exactly this, so two states that
            // enumerate the same way produce byte-identical files.
            var state = new GlobalConversationState();
            state.Merge(10, 3, SimStatus.WasOffered);
            state.Merge(2, 30, SimStatus.WasDisplayed);
            state.Merge(2, 4, SimStatus.WasOffered);
            state.Merge(-1, 0, SimStatus.WasDisplayed);

            GlobalConversationState snapshot = state.Snapshot();

            Assert.Equal(state.EnumerateEntriesInIdOrder(), snapshot.EnumerateEntriesInIdOrder());
            Assert.Equal(state.EntryCount, snapshot.EntryCount);
            Assert.Equal(state.ConversationCount, snapshot.ConversationCount);
        }

        [Fact]
        public void Snapshot_OfAnEmptyState_IsEmpty()
        {
            GlobalConversationState snapshot = new GlobalConversationState().Snapshot();

            Assert.True(snapshot.IsEmpty);
            Assert.Equal(0, snapshot.EntryCount);
            Assert.Equal(0, snapshot.ConversationCount);
            Assert.Empty(snapshot.EnumerateEntriesInIdOrder());
        }

        [Fact]
        public void Snapshot_SharesNothingWithItsSource()
        {
            // The whole reason the snapshot exists: the writer serializes it outside the
            // session lock, so marks keep arriving into the source while it does. If any
            // dictionary were shared, that would be a concurrent read and write.
            GlobalConversationState state = StateWith(SimStatus.WasOffered);
            GlobalConversationState snapshot = state.Snapshot();

            // Raise the existing entry, add one to the same conversation, and add a
            // whole new conversation: every level of the structure is touched.
            Assert.True(state.Merge(ConversationId, EntryId, SimStatus.WasDisplayed));
            Assert.True(state.Merge(ConversationId, EntryId + 1, SimStatus.WasDisplayed));
            Assert.True(state.Merge(ConversationId + 1, EntryId, SimStatus.WasDisplayed));

            Assert.Equal(SimStatus.WasOffered, snapshot.GetStatus(ConversationId, EntryId));
            Assert.Equal(1, snapshot.EntryCount);
            Assert.Equal(1, snapshot.ConversationCount);

            // And the other way round: the snapshot is an ordinary state, so it can be
            // merged into, and doing so must not reach back into the source.
            Assert.True(snapshot.Merge(500, 600, SimStatus.WasDisplayed));
            Assert.Equal(SimStatus.Untouched, state.GetStatus(500, 600));
            Assert.Equal(3, state.EntryCount);
        }

        // -------------------------------------------------------------------
        // The invariant: there is no way in except Merge
        // -------------------------------------------------------------------

        [Fact]
        public void PublicSurface_ExposesNoWayToAssignAStatusDirectly()
        {
            Type type = typeof(GlobalConversationState);

            Assert.Empty(type.GetFields(System.Reflection.BindingFlags.Public
                | System.Reflection.BindingFlags.Instance
                | System.Reflection.BindingFlags.Static));

            foreach (System.Reflection.PropertyInfo property in type.GetProperties())
            {
                Assert.Null(property.GetSetMethod());
            }

            // No public indexer, and every mutating method is a Merge overload.
            string[] mutators = type.GetMethods(System.Reflection.BindingFlags.Public
                    | System.Reflection.BindingFlags.Instance
                    | System.Reflection.BindingFlags.DeclaredOnly)
                .Select(m => m.Name)
                .Where(n => n.StartsWith("Set", StringComparison.Ordinal)
                    || n.StartsWith("Add", StringComparison.Ordinal)
                    || n.StartsWith("Remove", StringComparison.Ordinal)
                    || n.StartsWith("Clear", StringComparison.Ordinal)
                    || n == "get_Item"
                    || n == "set_Item")
                .ToArray();
            Assert.Empty(mutators);
        }

        [Fact]
        public void ReadOnlyProjections_CannotBeCastBackToTheBackingCollections()
        {
            GlobalConversationState state = StateWith(SimStatus.WasOffered);

            // Each of these is a compiler-generated iterator, not the live dictionary,
            // so a caller cannot cast to ICollection and mutate through it.
            Assert.IsNotAssignableFrom<ICollection>(state.ConversationIds);
            Assert.IsNotAssignableFrom<ICollection<int>>(state.ConversationIds);
            Assert.IsNotAssignableFrom<ICollection>(state.GetConversationEntries(ConversationId));
            Assert.IsNotAssignableFrom<ICollection<KeyValuePair<int, SimStatus>>>(
                state.GetConversationEntries(ConversationId));
            Assert.IsNotAssignableFrom<ICollection>(state.EnumerateEntries());
            Assert.IsNotAssignableFrom<ICollection<GlobalStatusEntry>>(state.EnumerateEntries());
        }
    }
}
