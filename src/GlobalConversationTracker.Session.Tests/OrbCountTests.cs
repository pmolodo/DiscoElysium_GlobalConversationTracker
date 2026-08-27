// SPDX-License-Identifier: MIT
using System;
using GlobalConversationTracker.Persistence;
using GlobalConversationTracker.Session;
using Xunit;

namespace GlobalConversationTracker.Session.Tests
{
    /// <summary>
    /// Orbs, from the write-through hook down to the file.
    /// </summary>
    /// <remarks>
    /// The current save and the across-all-saves state are asserted together for the
    /// same reason the entry tests do it: the two are supposed to diverge, and checking
    /// only one could not tell a working per-save orb count from a second copy of the
    /// global one.
    /// </remarks>
    public class OrbCountTests
    {
        private const string DepotDoor = "LANDS END / DEPOT DOOR";
        private const string SpilledRum = "WHIRLING F1 ORB / spilled rum";

        [Fact]
        public void NewSession_CountsNoOrbs()
        {
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            Assert.Equal(0, session.CurrentSaveOrbCount);
            Assert.Equal(0, session.EnsureInitialized().OrbCount);
        }

        [Fact]
        public void RecordOrb_CountsAnOrbOnceHoweverOftenItIsOpened()
        {
            // SenseOrb.SetShown runs on every click, not only the first - only the Lua
            // write inside it is guarded - so the repeat is the normal case, not an edge.
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            Assert.True(session.RecordOrb(DepotDoor));
            Assert.False(session.RecordOrb(DepotDoor));
            Assert.False(session.RecordOrb(DepotDoor));

            Assert.Equal(1, session.CurrentSaveOrbCount);
            Assert.Equal(1, session.State.OrbCount);
        }

        [Fact]
        public void RecordOrb_AddsAWholePointToBothScores()
        {
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            session.RecordOrb(DepotDoor);
            session.RecordOrb(SpilledRum);

            Assert.Equal(2 * DialogueScore.Orb, session.CurrentSaveScore);
            Assert.Equal(2 * DialogueScore.Orb, session.State.Score);
        }

        [Fact]
        public void RecordOrb_AddsToDialogueEntriesRatherThanReplacingThem()
        {
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            session.Record(3, 17, "WasOffered");
            session.RecordOrb(DepotDoor);

            Assert.Equal(DialogueScore.Offered + DialogueScore.Orb, session.CurrentSaveScore);
            Assert.Equal(1, session.CurrentSaveEntryCount);
            Assert.Equal(1, session.CurrentSaveOrbCount);
        }

        [Theory]
        [InlineData(null)]
        [InlineData("")]
        public void RecordOrb_WithoutATitle_Throws(string? title)
        {
            // The hook filters these out itself, because an orb with no conversation -
            // every thought orb - is one the game declines to record too. Throwing here
            // makes a caller that forgets to filter obvious rather than silent.
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            Assert.Throws<ArgumentException>(() => session.RecordOrb(title!));
        }

        [Fact]
        public void RecordOrb_ReachesTheFile()
        {
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();

            using (var session = new GlobalStateSession(store, new RecordingLog()))
            {
                session.RecordOrb(DepotDoor);
                Assert.True(session.Flush());
            }

            GlobalStateLoadResult result = store.Load();

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.True(result.State!.ContainsOrb(DepotDoor));
        }

        [Fact]
        public void OrbsFromTheFile_AreAlreadyGlobalButNotYetInThisSave()
        {
            // What a second playthrough looks like on startup: the global state carries
            // the orb, the save being played has not reached it.
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();

            using (var first = new GlobalStateSession(store, new RecordingLog()))
            {
                first.RecordOrb(DepotDoor);
                Assert.True(first.Flush());
            }

            using var second = new GlobalStateSession(store, new RecordingLog());

            Assert.Equal(1, second.EnsureInitialized().OrbCount);
            Assert.Equal(0, second.CurrentSaveOrbCount);
        }

        [Fact]
        public void ResetCurrentSave_DropsTheSavesOrbsAndKeepsTheGlobalOnes()
        {
            // A new game does reset ShownOrbs - GenericLuaFunctions.InitTables assigns a
            // fresh empty table - so the per-save count starts over while the history
            // the mod exists to keep does not.
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            session.Record(3, 17, "WasDisplayed");
            session.RecordOrb(DepotDoor);

            Assert.Equal(2, session.ResetCurrentSave("a test"));

            Assert.Equal(0, session.CurrentSaveOrbCount);
            Assert.Equal(0, session.CurrentSaveEntryCount);
            Assert.Equal(1, session.State.OrbCount);
            Assert.Equal(1, session.State.EntryCount);
        }
    }
}
