using System.Linq;
using UnifiedConversationTracker.Persistence;
using UnifiedConversationTracker.Session;
using Xunit;

namespace UnifiedConversationTracker.Session.Tests
{
    /// <summary>
    /// The count for the save being played, as opposed to the across-all-saves state
    /// beside it. The two are asserted together throughout: the point of this feature
    /// is that they diverge, and a test that only checked one could not tell a working
    /// per-save count from a second copy of the unified one.
    /// </summary>
    public class CurrentSaveCountTests
    {
        [Fact]
        public void NewSession_CountsNothingForTheCurrentSave()
        {
            // Never blank, never a leftover: a session that has loaded no save and
            // seen no mark has reached nothing in the save being played.
            using var dir = new TempDirectory();
            using var session = new UnifiedStateSession(dir.CreateStore(), new RecordingLog());

            Assert.Equal(0, session.CurrentSaveEntryCount);
        }

        [Fact]
        public void Record_CountsAnEntryOnceForTheCurrentSave()
        {
            using var dir = new TempDirectory();
            using var session = new UnifiedStateSession(dir.CreateStore(), new RecordingLog());

            session.Record(3, 17, "WasOffered");
            Assert.Equal(1, session.CurrentSaveEntryCount);

            // The game offers and re-offers the same line; the promotion is a change
            // to the unified state but not to how many entries have been reached.
            session.Record(3, 17, "WasDisplayed");
            session.Record(3, 17, "WasOffered");
            Assert.Equal(1, session.CurrentSaveEntryCount);

            session.Record(4, 1, "WasDisplayed");
            Assert.Equal(2, session.CurrentSaveEntryCount);
            Assert.Equal(2, session.State.EntryCount);
        }

        [Fact]
        public void Record_Untouched_LowersTheCurrentSaveCountButNotTheUnifiedOne()
        {
            // The whole reason the two counts are separate objects. History cannot go
            // down; this save can.
            using var dir = new TempDirectory();
            using var session = new UnifiedStateSession(dir.CreateStore(), new RecordingLog());

            session.Record(3, 17, "WasDisplayed");
            session.Record(4, 1, "WasDisplayed");
            Assert.Equal(2, session.CurrentSaveEntryCount);

            Assert.False(session.Record(3, 17, "Untouched"));
            Assert.Equal(1, session.CurrentSaveEntryCount);
            Assert.Equal(2, session.State.EntryCount);
            Assert.Equal(SimStatus.WasDisplayed, session.State.GetStatus(3, 17));

            // And clearing it twice does not run the count below what is really there.
            session.Record(3, 17, "Untouched");
            session.Record(9, 9, "Untouched");
            Assert.Equal(1, session.CurrentSaveEntryCount);
        }

        [Fact]
        public void Record_WithAnUnrecognizedStatus_LeavesTheCurrentSaveCountAlone()
        {
            using var dir = new TempDirectory();
            using var session = new UnifiedStateSession(dir.CreateStore(), new RecordingLog());

            session.Record(3, 17, "WasDisplayed");

            Assert.False(session.Record(3, 17, "wasdisplayed"));
            Assert.False(session.Record(3, 18, null));
            Assert.Equal(1, session.CurrentSaveEntryCount);
        }

        [Fact]
        public void ResetCurrentSave_DropsTheCurrentSaveCountAndKeepsTheUnifiedOne()
        {
            // What a new game does: the game throws its own SimStatus table away, and
            // the across-all-saves history is exactly what has to survive that.
            using var dir = new TempDirectory();
            var log = new RecordingLog();
            using var session = new UnifiedStateSession(dir.CreateStore(), log);

            session.Record(3, 17, "WasDisplayed");
            session.Record(4, 1, "WasOffered");

            Assert.Equal(2, session.ResetCurrentSave("a test"));
            Assert.Equal(0, session.CurrentSaveEntryCount);
            Assert.Equal(2, session.State.EntryCount);
            Assert.Contains(log.Info, m => m.Contains("reset by a test") && m.Contains("dropped 2 entries"));

            // A second reset has nothing left to drop, and says so rather than failing.
            Assert.Equal(0, session.ResetCurrentSave("a test"));
        }

        [Fact]
        public void ResetCurrentSave_LeavesTheCountLiveForTheNewGame()
        {
            using var dir = new TempDirectory();
            using var session = new UnifiedStateSession(dir.CreateStore(), new RecordingLog());

            session.Record(3, 17, "WasDisplayed");
            session.ResetCurrentSave("a test");

            session.Record(3, 17, "WasDisplayed");
            Assert.Equal(1, session.CurrentSaveEntryCount);
        }

        [Fact]
        public void ResetCurrentSave_BeforeAnyResync_SaysSoRatherThanReportingATime()
        {
            using var dir = new TempDirectory();
            var log = new RecordingLog();
            using var session = new UnifiedStateSession(dir.CreateStore(), log);

            session.ResetCurrentSave("a test");

            Assert.Contains(
                log.Info,
                m => m.Contains("no savegame has been resynced this session"));
        }

        [Fact]
        public void ResetCurrentSave_WithNoTrigger_StillNamesSomethingInTheLog()
        {
            using var dir = new TempDirectory();
            var log = new RecordingLog();
            using var session = new UnifiedStateSession(dir.CreateStore(), log);

            session.ResetCurrentSave(null);

            Assert.Contains(log.Info, m => m.Contains("an unnamed trigger"));
        }
    }
}
