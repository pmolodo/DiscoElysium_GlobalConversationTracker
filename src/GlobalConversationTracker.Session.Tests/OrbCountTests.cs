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

        // -------------------------------------------------------------------
        // Savegame load
        // -------------------------------------------------------------------

        [Fact]
        public void ResyncOrbs_CountsTheLoadedSavesOrbsForBothStates()
        {
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            Assert.Equal(2, session.ResyncOrbs(new[] { DepotDoor, SpilledRum }));

            Assert.Equal(2, session.CurrentSaveOrbCount);
            Assert.Equal(2, session.State.OrbCount);
        }

        [Fact]
        public void ResyncOrbs_ReplacesTheCurrentSaveButOnlyAddsToTheGlobalState()
        {
            // Loading save B must not leave save A's orbs on screen, while the history
            // the mod exists to keep must survive the switch.
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            session.ResyncOrbs(new[] { DepotDoor });
            session.ResyncOrbs(new[] { SpilledRum });

            // The save on screen is B only...
            Assert.Equal(1, session.CurrentSaveOrbCount);
            Assert.True(session.CurrentSaveOrbCount == 1 && session.State.OrbCount == 2);

            // ...while the global state has kept A as well.
            Assert.True(session.State.ContainsOrb(DepotDoor));
            Assert.True(session.State.ContainsOrb(SpilledRum));
        }

        [Fact]
        public void ResyncOrbs_LeavesDialogueEntriesAlone()
        {
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            session.Record(3, 17, "WasDisplayed");
            session.ResyncOrbs(new[] { DepotDoor });

            Assert.Equal(1, session.CurrentSaveEntryCount);
            Assert.Equal(DialogueScore.Displayed + DialogueScore.Orb, session.CurrentSaveScore);
        }

        [Fact]
        public void ResyncOrbs_OfASaveWithNoOrbs_EmptiesTheCurrentSaveCount()
        {
            // A genuinely fresh save has no orbs, and loading one has to be able to say
            // so rather than inheriting the previous save's figure.
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            session.RecordOrb(DepotDoor);
            Assert.Equal(0, session.ResyncOrbs(Array.Empty<string>()));

            Assert.Equal(0, session.CurrentSaveOrbCount);
            Assert.Equal(1, session.State.OrbCount);
        }

        [Fact]
        public void ResyncOrbs_SkipsEmptyTitlesRatherThanThrowing()
        {
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            Assert.Equal(1, session.ResyncOrbs(new string?[] { null, "", DepotDoor }));

            Assert.Equal(1, session.CurrentSaveOrbCount);
        }

        [Fact]
        public void ResyncOrbs_DuplicateTitles_AreCountedOnce()
        {
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            Assert.Equal(1, session.ResyncOrbs(new[] { DepotDoor, DepotDoor }));

            Assert.Equal(1, session.CurrentSaveOrbCount);
        }

        [Fact]
        public void ResyncOrbs_Null_Throws()
        {
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            Assert.Throws<ArgumentNullException>(() => session.ResyncOrbs(null!));
        }

        [Fact]
        public void ResyncOrbs_ReachesTheFile()
        {
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();

            using (var session = new GlobalStateSession(store, new RecordingLog()))
            {
                session.ResyncOrbs(new[] { DepotDoor });
                Assert.True(session.Flush());
            }

            Assert.True(store.Load().State!.ContainsOrb(DepotDoor));
        }

        [Fact]
        public void ResyncOrbs_WhenNothingIsNew_WritesNothingButStillFixesTheSaveCount()
        {
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            session.RecordOrb(DepotDoor);
            session.RecordOrb(SpilledRum);

            // The loaded save holds only one of the two the global state knows.
            Assert.Equal(0, session.ResyncOrbs(new[] { DepotDoor }));

            Assert.Equal(1, session.CurrentSaveOrbCount);
            Assert.Equal(2, session.State.OrbCount);
        }
    }
}
