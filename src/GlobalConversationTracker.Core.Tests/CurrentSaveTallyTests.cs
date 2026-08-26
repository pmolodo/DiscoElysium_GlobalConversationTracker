// SPDX-License-Identifier: MIT
using System;
using GlobalConversationTracker;
using Xunit;

namespace GlobalConversationTracker.Tests
{
    public class CurrentSaveTallyTests
    {
        [Fact]
        public void NewTally_IsEmpty()
        {
            var tally = new CurrentSaveTally();

            Assert.Equal(0, tally.Count);
            Assert.True(tally.IsEmpty);
        }

        [Theory]
        [InlineData(SimStatus.WasOffered)]
        [InlineData(SimStatus.WasDisplayed)]
        public void Set_CountsAnEntryAboveUntouched(SimStatus status)
        {
            var tally = new CurrentSaveTally();

            Assert.True(tally.Set(3, 7, status));
            Assert.Equal(1, tally.Count);
            Assert.True(tally.Contains(3, 7));
        }

        [Fact]
        public void Set_Untouched_DoesNotCount()
        {
            var tally = new CurrentSaveTally();

            Assert.False(tally.Set(3, 7, SimStatus.Untouched));
            Assert.Equal(0, tally.Count);
            Assert.False(tally.Contains(3, 7));
        }

        [Fact]
        public void Set_ReMarkingTheSameEntry_DoesNotCountItTwice()
        {
            var tally = new CurrentSaveTally();
            tally.Set(3, 7, SimStatus.WasOffered);

            // The game offers the same line over and over; every re-mark at a status
            // the entry already holds or has passed is a no-op for the total.
            Assert.False(tally.Set(3, 7, SimStatus.WasOffered));

            // ... but reaching WasDisplayed is a real change, worth the other half.
            Assert.True(tally.Set(3, 7, SimStatus.WasDisplayed));
            Assert.False(tally.Set(3, 7, SimStatus.WasDisplayed));

            // Being offered again afterwards is not the player unseeing it.
            Assert.False(tally.Set(3, 7, SimStatus.WasOffered));
            Assert.Equal(1, tally.Count);
            Assert.Equal(1d, tally.Score);
            Assert.Equal(SimStatus.WasDisplayed, tally.GetStatus(3, 7));
        }

        [Fact]
        public void Score_WeighsOfferedAtAHalfAndDisplayedAtOne()
        {
            var tally = new CurrentSaveTally();

            tally.Set(1, 1, SimStatus.WasOffered);
            Assert.Equal(0.5d, tally.Score);

            tally.Set(1, 2, SimStatus.WasDisplayed);
            Assert.Equal(1.5d, tally.Score);

            tally.Set(1, 3, SimStatus.WasOffered);
            Assert.Equal(2d, tally.Score);

            Assert.Equal(2, tally.OfferedCount);
            Assert.Equal(1, tally.DisplayedCount);
            Assert.Equal(3, tally.Count);
        }

        [Fact]
        public void Set_Displayed_MovesTheEntryOutOfTheOfferedSet()
        {
            var tally = new CurrentSaveTally();
            tally.Set(3, 7, SimStatus.WasOffered);

            tally.Set(3, 7, SimStatus.WasDisplayed);

            // Counted once, at its higher value, rather than once in each set.
            Assert.Equal(0, tally.OfferedCount);
            Assert.Equal(1, tally.DisplayedCount);
            Assert.Equal(1d, tally.Score);
        }

        [Fact]
        public void Set_Untouched_ClearsAnEntryFromEitherSet()
        {
            var tally = new CurrentSaveTally();
            tally.Set(1, 1, SimStatus.WasOffered);
            tally.Set(2, 2, SimStatus.WasDisplayed);

            Assert.True(tally.Set(1, 1, SimStatus.Untouched));
            Assert.True(tally.Set(2, 2, SimStatus.Untouched));
            Assert.Equal(0d, tally.Score);
            Assert.True(tally.IsEmpty);
        }

        [Fact]
        public void GetStatus_ReportsWhichSetAnEntryIsIn()
        {
            var tally = new CurrentSaveTally();
            tally.Set(1, 1, SimStatus.WasOffered);
            tally.Set(2, 2, SimStatus.WasDisplayed);

            Assert.Equal(SimStatus.WasOffered, tally.GetStatus(1, 1));
            Assert.Equal(SimStatus.WasDisplayed, tally.GetStatus(2, 2));
            Assert.Equal(SimStatus.Untouched, tally.GetStatus(3, 3));
        }

        [Fact]
        public void Set_UndefinedStatus_Throws()
        {
            var tally = new CurrentSaveTally();

            Assert.Throws<ArgumentOutOfRangeException>(
                () => tally.Set(1, 1, (SimStatus)99));
        }

        [Fact]
        public void Set_Untouched_TakesAnEntryBackOut()
        {
            var tally = new CurrentSaveTally();
            tally.Set(3, 7, SimStatus.WasDisplayed);

            Assert.True(tally.Set(3, 7, SimStatus.Untouched));
            Assert.Equal(0, tally.Count);
            Assert.False(tally.Contains(3, 7));

            // ... and clearing it again changes nothing, rather than going negative.
            Assert.False(tally.Set(3, 7, SimStatus.Untouched));
            Assert.Equal(0, tally.Count);
        }

        [Fact]
        public void Set_KeepsConversationsAndEntriesApart()
        {
            var tally = new CurrentSaveTally();

            tally.Set(1, 2, SimStatus.WasDisplayed);
            tally.Set(2, 1, SimStatus.WasDisplayed);
            tally.Set(1, 3, SimStatus.WasDisplayed);

            Assert.Equal(3, tally.Count);
            Assert.True(tally.Contains(1, 2));
            Assert.True(tally.Contains(2, 1));
            Assert.False(tally.Contains(2, 2));
        }

        [Fact]
        public void Set_HandlesNegativeIds()
        {
            // The IDs are the game's own ints and the key packs both into one long;
            // a negative one must not collide with anything or lose its sign.
            var tally = new CurrentSaveTally();

            tally.Set(-1, -1, SimStatus.WasDisplayed);
            tally.Set(-1, 1, SimStatus.WasDisplayed);
            tally.Set(1, -1, SimStatus.WasDisplayed);

            Assert.Equal(3, tally.Count);
            Assert.True(tally.Contains(-1, -1));
            Assert.False(tally.Contains(0, 0));
        }

        [Theory]
        [InlineData("WasOffered", true, 1)]
        [InlineData("WasDisplayed", true, 1)]
        [InlineData("Untouched", true, 0)]
        [InlineData("Bogus", false, 0)]
        [InlineData(null, false, 0)]
        public void TrySet_FollowsTheGameStrings(string? statusName, bool recognized, int expectedCount)
        {
            var tally = new CurrentSaveTally();

            Assert.Equal(recognized, tally.TrySet(3, 7, statusName, out bool changed));
            Assert.Equal(expectedCount == 1, changed);
            Assert.Equal(expectedCount, tally.Count);
        }

        [Fact]
        public void TrySet_UnrecognizedStatus_LeavesACountedEntryCounted()
        {
            // The dangerous direction: a status the mod does not understand must not
            // be treated as "not above Untouched" and drop the entry.
            var tally = new CurrentSaveTally();
            tally.Set(3, 7, SimStatus.WasDisplayed);

            Assert.False(tally.TrySet(3, 7, "Bogus", out bool changed));
            Assert.False(changed);
            Assert.Equal(1, tally.Count);
            Assert.True(tally.Contains(3, 7));
        }

        [Fact]
        public void Clear_EmptiesTheTallyAndReportsWhatItDropped()
        {
            var tally = new CurrentSaveTally();
            tally.Set(1, 1, SimStatus.WasDisplayed);
            tally.Set(1, 2, SimStatus.WasOffered);

            Assert.Equal(2, tally.Clear());
            Assert.Equal(0, tally.Count);
            Assert.True(tally.IsEmpty);
            Assert.Equal(0, tally.Clear());
        }

        [Fact]
        public void Clear_LeavesTheTallyUsable()
        {
            var tally = new CurrentSaveTally();
            tally.Set(1, 1, SimStatus.WasDisplayed);
            tally.Clear();

            Assert.True(tally.Set(1, 1, SimStatus.WasDisplayed));
            Assert.Equal(1, tally.Count);
        }

        [Fact]
        public void ToString_SaysTheScoreAndWhatItIsMadeOf()
        {
            var tally = new CurrentSaveTally();
            tally.Set(1, 1, SimStatus.WasDisplayed);
            tally.Set(1, 2, SimStatus.WasOffered);

            Assert.Equal(
                "CurrentSaveTally(1.5 from 1 displayed and 1 offered)", tally.ToString());
        }
    }
}
