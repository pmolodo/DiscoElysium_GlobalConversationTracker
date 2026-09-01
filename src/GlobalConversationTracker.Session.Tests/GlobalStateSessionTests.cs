// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Text;
using GlobalConversationTracker.Persistence;
using Xunit;

namespace GlobalConversationTracker.Session.Tests
{
    /// <summary>
    /// First-access initialization: the three disk outcomes (live file, backup
    /// fallback, nothing usable), the refusal to touch a newer format, the
    /// once-per-session guarantee, and the write-through <c>Record</c> that fills
    /// the state.
    /// </summary>
    public class GlobalStateSessionTests
    {
        private static GlobalConversationState StateWith(params (int Conversation, int Entry, SimStatus Status)[] rows)
        {
            var state = new GlobalConversationState();
            foreach ((int conversation, int entry, SimStatus status) in rows)
            {
                state.Merge(conversation, entry, status);
            }

            return state;
        }

        // -------------------------------------------------------------------
        // Path 1: the live file is present and parses.
        // -------------------------------------------------------------------

        [Fact]
        public void EnsureInitialized_WithValidLiveFile_LoadsIt()
        {
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();
            store.Save(StateWith((3, 17, SimStatus.WasDisplayed), (3, 18, SimStatus.WasOffered)));

            var log = new RecordingLog();
            using var session = new GlobalStateSession(store, log);

            GlobalConversationState state = session.EnsureInitialized();

            Assert.Equal(GlobalStateOrigin.LiveFile, session.Origin);
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(3, 17));
            Assert.Equal(SimStatus.WasOffered, state.GetStatus(3, 18));
            Assert.True(session.CanSave);

            Assert.Empty(log.Errors);
            Assert.Empty(log.Warnings);
            Assert.Contains(log.Info, line => line.Contains(store.LivePath, StringComparison.Ordinal));
        }

        [Fact]
        public void EnsureInitialized_WithPartlyDamagedLiveFile_LoadsWhatItCanAndLogsTheSkippedRows()
        {
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();
            File.WriteAllText(
                store.LivePath,
                "{\"version\":1,\"conversations\":{\"3\":{\"17\":\"WasDisplayed\",\"18\":\"Nonsense\"}}}",
                new UTF8Encoding(encoderShouldEmitUTF8Identifier: false));

            var log = new RecordingLog();
            using var session = new GlobalStateSession(store, log);

            GlobalConversationState state = session.EnsureInitialized();

            Assert.Equal(GlobalStateOrigin.LiveFile, session.Origin);
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(3, 17));
            Assert.Equal(SimStatus.Untouched, state.GetStatus(3, 18));
            Assert.Contains(log.Warnings, line => line.Contains("skipped", StringComparison.OrdinalIgnoreCase));
        }

        // -------------------------------------------------------------------
        // Path 2: the live file is missing or corrupt, so the backup takes over.
        // -------------------------------------------------------------------

        [Fact]
        public void EnsureInitialized_WithCorruptLiveFile_RecoversFromBackupAndSaysSoLoudly()
        {
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();

            // Two saves so a backup generation exists, then damage the live file.
            store.Save(StateWith((3, 17, SimStatus.WasOffered)));
            store.Save(StateWith((3, 17, SimStatus.WasOffered), (4, 1, SimStatus.WasDisplayed)));
            File.WriteAllText(store.LivePath, "{ this is not json");

            var log = new RecordingLog();
            using var session = new GlobalStateSession(store, log);

            GlobalConversationState state = session.EnsureInitialized();

            Assert.Equal(GlobalStateOrigin.BackupFile, session.Origin);
            Assert.Equal(SimStatus.WasOffered, state.GetStatus(3, 17));

            // The second save's extra entry only ever reached the live file, so the
            // fallback genuinely lost it. That is the loss the log has to make visible.
            Assert.Equal(SimStatus.Untouched, state.GetStatus(4, 1));

            Assert.True(log.WarningOrErrorContains("recovered"));
            Assert.True(log.WarningOrErrorContains(store.LivePath));
            Assert.True(log.WarningOrErrorContains(store.BackupPath));
            Assert.True(log.WarningOrErrorContains("lost"));
            Assert.True(session.CanSave);
        }

        [Fact]
        public void EnsureInitialized_WithCorruptLiveFile_KeepsACopyOfTheBadBytes()
        {
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();
            store.Save(StateWith((3, 17, SimStatus.WasOffered)));
            store.Save(StateWith((3, 17, SimStatus.WasOffered)));

            const string badBytes = "{ this is not json";
            File.WriteAllText(store.LivePath, badBytes);

            using var session = new GlobalStateSession(store, new RecordingLog());
            session.EnsureInitialized();

            string[] quarantined = Directory.GetFiles(dir.Path, "*.corrupt-*");
            string kept = Assert.Single(quarantined);
            Assert.Equal(badBytes, File.ReadAllText(kept));
        }

        [Fact]
        public void EnsureInitialized_WithMissingLiveFileButPresentBackup_RecoversAndSaysSoLoudly()
        {
            // The crash window the rotate-then-promote save is exposed to: killed
            // between rotating the live file into the backup slot and promoting the
            // temp file.
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();
            store.Save(StateWith((3, 17, SimStatus.WasDisplayed)));
            File.Move(store.LivePath, store.BackupPath, overwrite: true);

            var log = new RecordingLog();
            using var session = new GlobalStateSession(store, log);

            GlobalConversationState state = session.EnsureInitialized();

            Assert.Equal(GlobalStateOrigin.BackupFile, session.Origin);
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(3, 17));
            Assert.True(log.WarningOrErrorContains("recovered"));
            Assert.True(log.WarningOrErrorContains("missing"));
        }

        [Fact]
        public void EnsureInitialized_WithBothFilesCorrupt_StartsEmptyAndLogsTheLossAsAnError()
        {
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();
            File.WriteAllText(store.LivePath, "{ not json");
            File.WriteAllText(store.BackupPath, "also not json");

            var log = new RecordingLog();
            using var session = new GlobalStateSession(store, log);

            GlobalConversationState state = session.EnsureInitialized();

            Assert.Equal(GlobalStateOrigin.NoStateOnDisk, session.Origin);
            Assert.True(state.IsEmpty);

            // Both bad files named in the log, and the loss called out rather than
            // left to be inferred from an unexpectedly small file later - along with
            // what will put back what the game itself still holds.
            Assert.Contains(log.Errors, line => line.Contains(store.LivePath, StringComparison.Ordinal));
            Assert.Contains(log.Errors, line => line.Contains(store.BackupPath, StringComparison.Ordinal));
            Assert.Contains(log.Errors, line => line.Contains("gone", StringComparison.OrdinalIgnoreCase));
            Assert.Contains(log.Errors, line => line.Contains("savegame is loaded", StringComparison.Ordinal));

            // Both sets of bad bytes preserved.
            Assert.Equal(2, Directory.GetFiles(dir.Path, "*.corrupt-*").Length);
        }

        // -------------------------------------------------------------------
        // Path 3: nothing on disk. The state simply starts empty, and the
        // write-through hook fills it from there.
        // -------------------------------------------------------------------

        [Fact]
        public void EnsureInitialized_WithNoFiles_StartsEmptyAndWritesNothing()
        {
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            using var session = new GlobalStateSession(store, log);

            GlobalConversationState state = session.EnsureInitialized();

            Assert.Equal(GlobalStateOrigin.NoStateOnDisk, session.Origin);
            Assert.True(state.IsEmpty);

            // Initializing is a read. An empty state is not worth a file, so nothing
            // is written until something is actually recorded.
            Assert.False(File.Exists(store.LivePath));
            Assert.Empty(Directory.GetFiles(dir.Path));
            Assert.Empty(log.Errors);
        }

        [Fact]
        public void NewGame_WithNoFileAndNoSavegameLoad_RecordsEverythingThroughTheHook()
        {
            // Starting a brand new game never loads a savegame, so no resync runs.
            // That is correct rather than a gap: a new game's SimStatus table is
            // all-Untouched, so a bulk read would find nothing, and every status from
            // there on arrives as a mark.
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            using var session = new GlobalStateSession(store, log);

            Assert.True(session.Record(3, 17, "WasOffered"));
            Assert.True(session.Record(3, 17, "WasDisplayed"));
            Assert.True(session.Record(4, 1, "WasDisplayed"));

            Assert.Equal(GlobalStateOrigin.NoStateOnDisk, session.Origin);

            Assert.True(session.Flush());
            GlobalConversationState saved = store.Load().RequireState();
            Assert.Equal(SimStatus.WasDisplayed, saved.GetStatus(3, 17));
            Assert.Equal(SimStatus.WasDisplayed, saved.GetStatus(4, 1));
            Assert.Equal(2, saved.EntryCount);
            Assert.Empty(log.Errors);
        }

        // -------------------------------------------------------------------
        // A newer format version is intact history: refuse to touch it.
        // -------------------------------------------------------------------

        [Fact]
        public void EnsureInitialized_WithANewerFormatVersion_RefusesToSaveAndDoesNotFallBack()
        {
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();

            // A good backup exists. Falling back to it would silently revert whatever
            // the newer build recorded, so it must be left alone.
            store.Save(StateWith((3, 17, SimStatus.WasOffered)));
            File.Move(store.LivePath, store.BackupPath, overwrite: true);

            const string newerFile = "{\"version\":99,\"conversations\":{\"3\":{\"17\":\"WasDisplayed\"}}}";
            File.WriteAllText(store.LivePath, newerFile);

            var log = new RecordingLog();
            using var session = new GlobalStateSession(store, log);

            GlobalConversationState state = session.EnsureInitialized();

            Assert.Equal(GlobalStateOrigin.RefusedNewerFormat, session.Origin);
            Assert.False(session.CanSave);

            // Empty rather than the backup's contents: the good backup was left alone.
            Assert.True(state.IsEmpty);

            Assert.Contains(log.Errors, line => line.Contains("newer version", StringComparison.OrdinalIgnoreCase));

            // And the file itself is untouched, before and after a save attempt.
            Assert.False(session.TrySave());
            Assert.Equal(newerFile, File.ReadAllText(store.LivePath));
        }

        [Fact]
        public void EnsureInitialized_WithACorruptLiveFileAndANewerFormatBackup_RefusesToSave()
        {
            // Saving here would rotate the unusable live file over the backup and
            // destroy the only intact history left.
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();
            File.WriteAllText(store.LivePath, "{ not json");
            File.WriteAllText(store.BackupPath, "{\"version\":99,\"conversations\":{}}");

            var log = new RecordingLog();
            using var session = new GlobalStateSession(store, log);

            session.EnsureInitialized();

            Assert.Equal(GlobalStateOrigin.RefusedNewerFormat, session.Origin);
            Assert.False(session.CanSave);
            Assert.NotEmpty(log.Errors);
        }

        // -------------------------------------------------------------------
        // Once per session, and safe to touch while it is happening.
        // -------------------------------------------------------------------

        [Fact]
        public void EnsureInitialized_ReadsTheDiskExactlyOncePerSession()
        {
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();
            store.Save(StateWith((3, 17, SimStatus.WasDisplayed)));

            var log = new RecordingLog();
            using var session = new GlobalStateSession(store, log);

            GlobalConversationState first = session.EnsureInitialized();

            // Change the file behind the session's back. A second read would see it.
            store.Save(StateWith((3, 17, SimStatus.WasDisplayed), (5, 5, SimStatus.WasDisplayed)));

            for (int i = 0; i < 10; i++)
            {
                Assert.Same(first, session.EnsureInitialized());
            }

            Assert.Equal(SimStatus.Untouched, first.GetStatus(5, 5));
            Assert.Single(log.All);
        }

        [Fact]
        public void State_BeforeInitialization_Throws()
        {
            using var dir = new TempDirectory();
            using var session = new GlobalStateSession(dir.CreateStore(), new RecordingLog());

            Assert.Throws<InvalidOperationException>(() => session.State);
            Assert.False(session.IsInitialized);
            Assert.Equal(GlobalStateOrigin.Uninitialized, session.Origin);
        }

        [Fact]
        public void Constructor_DoesNotReadTheDisk()
        {
            // Building the session during BepInEx chainload must not read the disk;
            // that is the once-per-session read EnsureInitialized owns.
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();
            store.Save(StateWith((3, 17, SimStatus.WasDisplayed)));

            var log = new RecordingLog();
            using var session = new GlobalStateSession(store, log);

            Assert.Empty(log.All);
            Assert.False(session.IsInitialized);
        }

        [Fact]
        public void ReloadFrom_ReplacesTheStableStateAndLeavesTheCurrentSaveAlone()
        {
            using var dir = new TempDirectory();
            GlobalStateStore live = dir.CreateStore();
            live.Save(StateWith((1, 1, SimStatus.WasDisplayed)));

            string replacementPath = Path.Combine(dir.Path, "replacement.json");
            GlobalStateStore.AtPath(replacementPath)
                .Save(StateWith((2, 3, SimStatus.WasOffered)));

            using var session = new GlobalStateSession(live, new RecordingLog());
            GlobalConversationState original = session.EnsureInitialized();
            session.Record(9, 9, "WasDisplayed");
            Assert.Equal(1, session.CurrentSaveEntryCount);

            GlobalConversationState reloaded = session.ReloadFrom(replacementPath);

            Assert.Same(original, reloaded);
            Assert.Equal(SimStatus.Untouched, reloaded.GetStatus(1, 1));
            Assert.Equal(SimStatus.Untouched, reloaded.GetStatus(9, 9));
            Assert.Equal(SimStatus.WasOffered, reloaded.GetStatus(2, 3));
            Assert.Equal(1, session.CurrentSaveEntryCount);
            Assert.Equal(
                SimStatus.WasOffered,
                live.Load().RequireState().GetStatus(2, 3));
        }

        [Fact]
        public void ReloadFrom_InvalidCandidateLeavesTheRunningStateUntouched()
        {
            using var dir = new TempDirectory();
            GlobalStateStore live = dir.CreateStore();
            live.Save(StateWith((1, 1, SimStatus.WasDisplayed)));
            string replacementPath = Path.Combine(dir.Path, "replacement.json");
            File.WriteAllText(replacementPath, "not json");

            using var session = new GlobalStateSession(live, new RecordingLog());
            GlobalConversationState original = session.EnsureInitialized();

            Assert.Throws<InvalidDataException>(() => session.ReloadFrom(replacementPath));
            Assert.Same(original, session.State);
            Assert.Equal(SimStatus.WasDisplayed, session.State.GetStatus(1, 1));
        }

        [Fact]
        public void EnsureInitialized_KeepsWhateverAHookMergedBeforeTheFileWasRead()
        {
            // The state object exists from construction, so an early hook can merge
            // into it. Loading the file must not throw that away.
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();
            store.Save(StateWith((3, 17, SimStatus.WasOffered)));

            using var session = new GlobalStateSession(store, new RecordingLog());

            // Reach the state the way a hook would once initialization has happened,
            // then simulate the early write by merging a higher status and re-loading.
            GlobalConversationState state = session.EnsureInitialized();
            state.Merge(3, 17, SimStatus.WasDisplayed);

            Assert.Same(state, session.EnsureInitialized());
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(3, 17));
        }

        [Fact]
        public void TrySave_WritesTheCurrentState()
        {
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();

            using var session = new GlobalStateSession(store, new RecordingLog());
            GlobalConversationState state = session.EnsureInitialized();
            state.Merge(11, 22, SimStatus.WasOffered);

            Assert.True(session.TrySave());
            Assert.Equal(SimStatus.WasOffered, store.Load().RequireState().GetStatus(11, 22));
        }

        [Fact]
        public void TrySave_WhenTheDirectoryCannotBeWritten_LogsAndReturnsFalse()
        {
            using var dir = new TempDirectory();

            // A file where the directory should be: creating the directory fails.
            string blocked = Path.Combine(dir.Path, "blocked");
            File.WriteAllText(blocked, "not a directory");

            var log = new RecordingLog();
            var store = new GlobalStateStore(Path.Combine(blocked, "SaveGames"));
            using var session = new GlobalStateSession(store, log);
            session.EnsureInitialized().Merge(1, 1, SimStatus.WasOffered);

            Assert.False(session.TrySave());
            Assert.Contains(log.Errors, line => line.Contains("Failed to write", StringComparison.Ordinal));
        }

        // -------------------------------------------------------------------
        // Record: the write-through path the MarkDialogueEntry hook drives.
        // -------------------------------------------------------------------

        [Fact]
        public void Record_RaisingAStatus_MergesItAndWritesTheFile()
        {
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            using var session = new GlobalStateSession(store, log);

            Assert.True(session.Record(3, 17, "WasOffered"));
            Assert.True(session.Record(3, 17, "WasDisplayed"));

            Assert.Equal(SimStatus.WasDisplayed, session.State.GetStatus(3, 17));

            // The write is deferred to the background writer, so the file is
            // asserted after waiting for it rather than immediately.
            Assert.True(session.Flush());
            Assert.Equal(SimStatus.WasDisplayed, store.Load().RequireState().GetStatus(3, 17));
            Assert.Empty(log.Errors);
        }

        [Fact]
        public void Record_OnTheFirstCall_InitializesTheSessionFromDisk()
        {
            // The hook is the initialization trigger, so a mark arriving before
            // anything else has to pick up the existing file rather than start empty.
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();
            store.Save(StateWith((3, 17, SimStatus.WasDisplayed)));

            using var session = new GlobalStateSession(store, new RecordingLog());
            Assert.False(session.IsInitialized);

            session.Record(4, 1, "WasOffered");

            Assert.True(session.IsInitialized);
            Assert.Equal(GlobalStateOrigin.LiveFile, session.Origin);
            Assert.Equal(SimStatus.WasDisplayed, session.State.GetStatus(3, 17));

            Assert.True(session.Flush());
            Assert.Equal(SimStatus.WasDisplayed, store.Load().RequireState().GetStatus(3, 17));
        }

        [Fact]
        public void Record_WithNothingNewToSay_WritesNothing()
        {
            // The game re-marks entries constantly, and marks them Untouched outright.
            // Rewriting the file for those would put its whole cost on the common case.
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();

            using var session = new GlobalStateSession(store, new RecordingLog());
            Assert.True(session.Record(3, 17, "WasDisplayed"));
            Assert.True(session.Flush());

            // Delete the file: anything that writes again has to recreate it.
            File.Delete(store.LivePath);

            Assert.False(session.Record(3, 17, "WasDisplayed"));
            Assert.False(session.Record(3, 17, "WasOffered"));
            Assert.False(session.Record(3, 17, "Untouched"));
            Assert.False(session.Record(9, 9, "Untouched"));

            // Flush is not "save": with nothing dirty it has nothing to wait for and
            // nothing to write, so the deleted file stays deleted.
            Assert.True(session.Flush());
            Assert.False(File.Exists(store.LivePath));
            Assert.Equal(SimStatus.WasDisplayed, session.State.GetStatus(3, 17));
        }

        [Fact]
        public void Record_WithAnUnrecognizedStatus_WarnsOncePerStatusAndCarriesOn()
        {
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            using var session = new GlobalStateSession(store, log);

            Assert.False(session.Record(3, 17, "wasdisplayed"));
            Assert.False(session.Record(3, 18, "wasdisplayed"));
            Assert.False(session.Record(3, 19, null));
            Assert.True(session.Record(3, 20, "WasDisplayed"));

            Assert.Equal(1, session.State.EntryCount);
            Assert.Equal(SimStatus.WasDisplayed, session.State.GetStatus(3, 20));

            // One warning per distinct status string, however many entries carry it.
            Assert.Single(log.Warnings, line => line.Contains("'wasdisplayed'", StringComparison.Ordinal));
            Assert.Single(log.Warnings, line => line.Contains("'null'", StringComparison.Ordinal));
            Assert.Empty(log.Errors);
        }

        [Fact]
        public void Record_WhenTheWriteFails_LogsAndDoesNotThrow()
        {
            // A broken global path must cost tracking and nothing else.
            using var dir = new TempDirectory();
            string blocked = Path.Combine(dir.Path, "blocked");
            File.WriteAllText(blocked, "not a directory");

            var log = new RecordingLog();
            var store = new GlobalStateStore(Path.Combine(blocked, "SaveGames"));
            using var session = new GlobalStateSession(store, log);

            Assert.True(session.Record(3, 17, "WasDisplayed"));
            Assert.False(session.Flush());

            Assert.Equal(SimStatus.WasDisplayed, session.State.GetStatus(3, 17));
            Assert.Contains(log.Errors, line => line.Contains("Failed to write", StringComparison.Ordinal));
        }

        [Fact]
        public void Record_AfterRefusingANewerFormat_KeepsMergingButLeavesTheFileAlone()
        {
            using var dir = new TempDirectory();
            GlobalStateStore store = dir.CreateStore();
            const string newerFile = "{\"version\":99,\"conversations\":{\"3\":{\"17\":\"WasDisplayed\"}}}";
            File.WriteAllText(store.LivePath, newerFile);

            var log = new RecordingLog();
            using var session = new GlobalStateSession(store, log);

            Assert.True(session.Record(3, 17, "WasDisplayed"));

            // Refused on the marking side, so the background writer is never even
            // started and there is nothing pending for a flush to land.
            Assert.False(session.Flush());
            Assert.Equal(newerFile, File.ReadAllText(store.LivePath));
            Assert.False(File.Exists(store.BackupPath));
            Assert.Contains(log.Warnings, line => line.Contains("Not saving", StringComparison.Ordinal));
        }
    }
}
