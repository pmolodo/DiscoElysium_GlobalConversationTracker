using System;
using System.IO;
using System.Text;
using UnifiedConversationTracker.Persistence;
using Xunit;

namespace UnifiedConversationTracker.Session.Tests
{
    /// <summary>
    /// First-access initialization: the three recovery paths (live file, backup
    /// fallback, seed from the running game), the two refusals (newer format,
    /// failed game read), and the once-per-session guarantee.
    /// </summary>
    public class UnifiedStateSessionTests
    {
        private static UnifiedConversationState StateWith(params (int Conversation, int Entry, SimStatus Status)[] rows)
        {
            var state = new UnifiedConversationState();
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
            UnifiedStateStore store = dir.CreateStore();
            store.Save(StateWith((3, 17, SimStatus.WasDisplayed), (3, 18, SimStatus.WasOffered)));

            var log = new RecordingLog();
            var source = new FakeSimStatusSource().Add(99, 1, "WasDisplayed");
            var session = new UnifiedStateSession(store, source, log);

            UnifiedConversationState state = session.EnsureInitialized();

            Assert.Equal(UnifiedStateOrigin.LiveFile, session.Origin);
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(3, 17));
            Assert.Equal(SimStatus.WasOffered, state.GetStatus(3, 18));
            Assert.True(session.CanSave);

            // The game is never read when the file is usable.
            Assert.Equal(0, source.EnumerationCount);
            Assert.Equal(SimStatus.Untouched, state.GetStatus(99, 1));

            Assert.Empty(log.Errors);
            Assert.Empty(log.Warnings);
            Assert.Contains(log.Info, line => line.Contains(store.LivePath, StringComparison.Ordinal));
        }

        [Fact]
        public void EnsureInitialized_WithPartlyDamagedLiveFile_LoadsWhatItCanAndLogsTheSkippedRows()
        {
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();
            File.WriteAllText(
                store.LivePath,
                "{\"version\":1,\"conversations\":{\"3\":{\"17\":\"WasDisplayed\",\"18\":\"Nonsense\"}}}",
                new UTF8Encoding(encoderShouldEmitUTF8Identifier: false));

            var log = new RecordingLog();
            var session = new UnifiedStateSession(store, new FakeSimStatusSource(), log);

            UnifiedConversationState state = session.EnsureInitialized();

            Assert.Equal(UnifiedStateOrigin.LiveFile, session.Origin);
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
            UnifiedStateStore store = dir.CreateStore();

            // Two saves so a backup generation exists, then damage the live file.
            store.Save(StateWith((3, 17, SimStatus.WasOffered)));
            store.Save(StateWith((3, 17, SimStatus.WasOffered), (4, 1, SimStatus.WasDisplayed)));
            File.WriteAllText(store.LivePath, "{ this is not json");

            var log = new RecordingLog();
            var source = new FakeSimStatusSource().Add(99, 1, "WasDisplayed");
            var session = new UnifiedStateSession(store, source, log);

            UnifiedConversationState state = session.EnsureInitialized();

            Assert.Equal(UnifiedStateOrigin.BackupFile, session.Origin);
            Assert.Equal(SimStatus.WasOffered, state.GetStatus(3, 17));

            // The second save's extra entry only ever reached the live file, so the
            // fallback genuinely lost it. That is the loss the log has to make visible.
            Assert.Equal(SimStatus.Untouched, state.GetStatus(4, 1));

            // Never seeded over a recovery.
            Assert.Equal(0, source.EnumerationCount);

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
            UnifiedStateStore store = dir.CreateStore();
            store.Save(StateWith((3, 17, SimStatus.WasOffered)));
            store.Save(StateWith((3, 17, SimStatus.WasOffered)));

            const string badBytes = "{ this is not json";
            File.WriteAllText(store.LivePath, badBytes);

            var session = new UnifiedStateSession(store, new FakeSimStatusSource(), new RecordingLog());
            session.EnsureInitialized();

            string[] quarantined = Directory.GetFiles(dir.Path, "*.corrupt-*");
            string kept = Assert.Single(quarantined);
            Assert.Equal(badBytes, File.ReadAllText(kept));
        }

        [Fact]
        public void EnsureInitialized_WithMissingLiveFileButPresentBackup_RecoversAndSaysSoLoudly()
        {
            // The crash window de-omm.6 designed for: killed between rotating the live
            // file into the backup slot and promoting the temp file.
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();
            store.Save(StateWith((3, 17, SimStatus.WasDisplayed)));
            File.Move(store.LivePath, store.BackupPath, overwrite: true);

            var log = new RecordingLog();
            var source = new FakeSimStatusSource().Add(99, 1, "WasDisplayed");
            var session = new UnifiedStateSession(store, source, log);

            UnifiedConversationState state = session.EnsureInitialized();

            Assert.Equal(UnifiedStateOrigin.BackupFile, session.Origin);
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(3, 17));
            Assert.Equal(0, source.EnumerationCount);
            Assert.True(log.WarningOrErrorContains("recovered"));
            Assert.True(log.WarningOrErrorContains("missing"));
        }

        [Fact]
        public void EnsureInitialized_WithBothFilesCorrupt_SeedsAndLogsTheLossAsAnError()
        {
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();
            File.WriteAllText(store.LivePath, "{ not json");
            File.WriteAllText(store.BackupPath, "also not json");

            var log = new RecordingLog();
            var source = new FakeSimStatusSource().Add(7, 2, "WasDisplayed");
            var session = new UnifiedStateSession(store, source, log);

            UnifiedConversationState state = session.EnsureInitialized();

            Assert.Equal(UnifiedStateOrigin.SeededFromGame, session.Origin);
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(7, 2));

            // Both bad files named in the log, and the loss called out rather than
            // left to be inferred from an unexpectedly small file later.
            Assert.Contains(log.Errors, line => line.Contains(store.LivePath, StringComparison.Ordinal));
            Assert.Contains(log.Errors, line => line.Contains(store.BackupPath, StringComparison.Ordinal));
            Assert.Contains(log.Errors, line => line.Contains("gone", StringComparison.OrdinalIgnoreCase));

            // Both sets of bad bytes preserved.
            Assert.Equal(2, Directory.GetFiles(dir.Path, "*.corrupt-*").Length);
        }

        // -------------------------------------------------------------------
        // Path 3: nothing on disk, so seed from the running game.
        // -------------------------------------------------------------------

        [Fact]
        public void EnsureInitialized_WithNoFiles_SeedsFromTheGameAndWritesTheResult()
        {
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            var source = new FakeSimStatusSource()
                .Add(3, 17, "WasDisplayed")
                .Add(3, 18, "WasOffered")
                .Add(3, 19, "Untouched")
                .Add(4, 1, "WasDisplayed");
            var session = new UnifiedStateSession(store, source, log);

            UnifiedConversationState state = session.EnsureInitialized();

            Assert.Equal(UnifiedStateOrigin.SeededFromGame, session.Origin);
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(3, 17));
            Assert.Equal(SimStatus.WasOffered, state.GetStatus(3, 18));
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(4, 1));

            // Untouched is the bottom of the ordering and is never stored.
            Assert.Equal(3, state.EntryCount);

            // The seed reached disk, and reads back identically.
            Assert.True(File.Exists(store.LivePath));
            UnifiedStateLoadResult reloaded = store.Load();
            Assert.True(reloaded.IsLoaded);
            Assert.Equal(3, reloaded.RequireState().EntryCount);

            Assert.Empty(log.Errors);
        }

        [Fact]
        public void EnsureInitialized_SeedingAnEmptyGame_WritesNoFile()
        {
            // A brand new game has nothing above Untouched. Writing a file for that
            // would stop a later session from seeding a save made before the mod was
            // installed, because the file's existence is what suppresses seeding.
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            var source = new FakeSimStatusSource()
                .Add(3, 17, "Untouched")
                .Add(3, 18, "Untouched");
            var session = new UnifiedStateSession(store, source, log);

            UnifiedConversationState state = session.EnsureInitialized();

            Assert.Equal(UnifiedStateOrigin.SeededFromGame, session.Origin);
            Assert.True(state.IsEmpty);
            Assert.False(File.Exists(store.LivePath));
            Assert.Empty(Directory.GetFiles(dir.Path));
        }

        [Fact]
        public void EnsureInitialized_WithUnrecognizedStatusStrings_SkipsThemAndLogsThem()
        {
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            var source = new FakeSimStatusSource()
                .Add(3, 17, "WasDisplayed")
                .Add(3, 18, "wasdisplayed")
                .Add(3, 19, null);
            var session = new UnifiedStateSession(store, source, log);

            UnifiedConversationState state = session.EnsureInitialized();

            Assert.Equal(1, state.EntryCount);
            Assert.Contains(log.Warnings, line => line.Contains("wasdisplayed", StringComparison.Ordinal));
            Assert.Contains(log.Warnings, line => line.Contains("null", StringComparison.Ordinal));
        }

        // -------------------------------------------------------------------
        // Seeding timing: deferred while the game is not ready, retried after.
        // -------------------------------------------------------------------

        [Fact]
        public void EnsureInitialized_WhenTheGameIsNotReady_DefersTheSeedAndRetriesLater()
        {
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            var source = new FakeSimStatusSource { IsReady = false }.Add(3, 17, "WasDisplayed");
            var session = new UnifiedStateSession(store, source, log);

            UnifiedConversationState first = session.EnsureInitialized();
            Assert.Equal(UnifiedStateOrigin.AwaitingGame, session.Origin);
            Assert.True(session.IsSeedPending);
            Assert.True(first.IsEmpty);
            Assert.Equal(0, source.EnumerationCount);

            // Still not ready: no walk, no file, and no repeat of the deferral line.
            session.EnsureInitialized();
            Assert.Equal(0, source.EnumerationCount);
            Assert.False(File.Exists(store.LivePath));
            Assert.Single(log.Info, line => line.Contains("deferred", StringComparison.OrdinalIgnoreCase));

            // The save loads and the game becomes readable.
            source.IsReady = true;
            UnifiedConversationState after = session.EnsureInitialized();

            Assert.Same(first, after);
            Assert.Equal(UnifiedStateOrigin.SeededFromGame, session.Origin);
            Assert.Equal(SimStatus.WasDisplayed, after.GetStatus(3, 17));
            Assert.False(session.IsSeedPending);
            Assert.Equal(1, source.EnumerationCount);
        }

        [Fact]
        public void EnsureInitialized_AfterASeed_NeverWalksTheGameAgain()
        {
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var source = new FakeSimStatusSource().Add(3, 17, "WasDisplayed");
            var session = new UnifiedStateSession(store, source, new RecordingLog());

            for (int i = 0; i < 25; i++)
            {
                session.EnsureInitialized();
            }

            Assert.Equal(1, source.EnumerationCount);
            Assert.Equal(1, session.SeedAttemptCount);
        }

        [Fact]
        public void EnsureInitialized_WhenTheGameWalkThrows_LogsItAndDoesNotRetry()
        {
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var log = new RecordingLog();
            var source = new FakeSimStatusSource { ThrowOnEnumerate = new InvalidOperationException("no database") };
            var session = new UnifiedStateSession(store, source, log);

            session.EnsureInitialized();
            session.EnsureInitialized();
            session.EnsureInitialized();

            Assert.Equal(UnifiedStateOrigin.SeedFailed, session.Origin);
            Assert.Equal(1, source.EnumerationCount);
            Assert.Contains(log.Errors, line => line.Contains("no database", StringComparison.Ordinal));
        }

        // -------------------------------------------------------------------
        // A newer format version is intact history: refuse to touch it.
        // -------------------------------------------------------------------

        [Fact]
        public void EnsureInitialized_WithANewerFormatVersion_RefusesToSaveAndDoesNotFallBack()
        {
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            // A good backup exists. Falling back to it would silently revert whatever
            // the newer build recorded, so it must be left alone.
            store.Save(StateWith((3, 17, SimStatus.WasOffered)));
            File.Move(store.LivePath, store.BackupPath, overwrite: true);

            const string newerFile = "{\"version\":99,\"conversations\":{\"3\":{\"17\":\"WasDisplayed\"}}}";
            File.WriteAllText(store.LivePath, newerFile);

            var log = new RecordingLog();
            var source = new FakeSimStatusSource().Add(7, 2, "WasDisplayed");
            var session = new UnifiedStateSession(store, source, log);

            UnifiedConversationState state = session.EnsureInitialized();

            Assert.Equal(UnifiedStateOrigin.RefusedNewerFormat, session.Origin);
            Assert.False(session.CanSave);
            Assert.True(state.IsEmpty);

            // Neither seeded over nor recovered from the backup.
            Assert.Equal(0, source.EnumerationCount);
            Assert.False(session.IsSeedPending);

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
            UnifiedStateStore store = dir.CreateStore();
            File.WriteAllText(store.LivePath, "{ not json");
            File.WriteAllText(store.BackupPath, "{\"version\":99,\"conversations\":{}}");

            var log = new RecordingLog();
            var source = new FakeSimStatusSource().Add(7, 2, "WasDisplayed");
            var session = new UnifiedStateSession(store, source, log);

            session.EnsureInitialized();

            Assert.Equal(UnifiedStateOrigin.RefusedNewerFormat, session.Origin);
            Assert.False(session.CanSave);
            Assert.Equal(0, source.EnumerationCount);
            Assert.NotEmpty(log.Errors);
        }

        // -------------------------------------------------------------------
        // Once per session, and safe to touch while it is happening.
        // -------------------------------------------------------------------

        [Fact]
        public void EnsureInitialized_ReadsTheDiskExactlyOncePerSession()
        {
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();
            store.Save(StateWith((3, 17, SimStatus.WasDisplayed)));

            var log = new RecordingLog();
            var session = new UnifiedStateSession(store, new FakeSimStatusSource(), log);

            UnifiedConversationState first = session.EnsureInitialized();

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
            var session = new UnifiedStateSession(
                dir.CreateStore(), new FakeSimStatusSource(), new RecordingLog());

            Assert.Throws<InvalidOperationException>(() => session.State);
            Assert.False(session.IsInitialized);
            Assert.Equal(UnifiedStateOrigin.Uninitialized, session.Origin);
        }

        [Fact]
        public void Constructor_TouchesNeitherDiskNorGame()
        {
            // Building the session during BepInEx chainload must not read the game,
            // which is not up yet, or the disk, which is the once-per-session read.
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();
            store.Save(StateWith((3, 17, SimStatus.WasDisplayed)));

            var log = new RecordingLog();
            var source = new FakeSimStatusSource();
            var session = new UnifiedStateSession(store, source, log);

            Assert.Empty(log.All);
            Assert.Equal(0, source.EnumerationCount);
            Assert.Equal(0, source.ReadinessCheckCount);
            Assert.False(session.IsInitialized);
        }

        [Fact]
        public void EnsureInitialized_KeepsWhateverAHookMergedBeforeTheFileWasRead()
        {
            // The state object exists from construction, so an early hook can merge
            // into it. Loading the file must not throw that away.
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();
            store.Save(StateWith((3, 17, SimStatus.WasOffered)));

            var session = new UnifiedStateSession(store, new FakeSimStatusSource(), new RecordingLog());

            // Reach the state the way a hook would once initialization has happened,
            // then simulate the early write by merging a higher status and re-loading.
            UnifiedConversationState state = session.EnsureInitialized();
            state.Merge(3, 17, SimStatus.WasDisplayed);

            Assert.Same(state, session.EnsureInitialized());
            Assert.Equal(SimStatus.WasDisplayed, state.GetStatus(3, 17));
        }

        [Fact]
        public void TrySave_WritesTheCurrentState()
        {
            using var dir = new TempDirectory();
            UnifiedStateStore store = dir.CreateStore();

            var session = new UnifiedStateSession(store, new FakeSimStatusSource(), new RecordingLog());
            UnifiedConversationState state = session.EnsureInitialized();
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
            var store = new UnifiedStateStore(Path.Combine(blocked, "SaveGames"));
            var session = new UnifiedStateSession(store, new FakeSimStatusSource(), log);
            session.EnsureInitialized().Merge(1, 1, SimStatus.WasOffered);

            Assert.False(session.TrySave());
            Assert.Contains(log.Errors, line => line.Contains("Failed to write", StringComparison.Ordinal));
        }
    }
}
