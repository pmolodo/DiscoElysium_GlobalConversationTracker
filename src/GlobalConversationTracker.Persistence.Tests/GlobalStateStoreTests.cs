// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Xunit;

namespace GlobalConversationTracker.Persistence.Tests
{
    /// <summary>
    /// File-level tests: path handling, the save sequence, backup rotation, and the
    /// missing / corrupt / unsupported distinction.
    /// </summary>
    public class GlobalStateStoreTests
    {
        private sealed class SimulatedCrashException : Exception
        {
            public SimulatedCrashException(GlobalStateSaveStep step)
                : base($"Simulated crash at {step}.")
            {
            }
        }

        private static GlobalConversationState OldGeneration()
        {
            var state = new GlobalConversationState();
            state.Merge(1, 10, SimStatus.WasOffered);
            return state;
        }

        private static GlobalConversationState NewGeneration()
        {
            var state = new GlobalConversationState();
            state.Merge(1, 10, SimStatus.WasDisplayed);
            state.Merge(2, 20, SimStatus.WasOffered);
            state.Merge(3, 30, SimStatus.WasDisplayed);
            return state;
        }

        private static void AssertSameEntries(GlobalConversationState expected, GlobalConversationState actual)
        {
            Assert.Equal(
                expected.EnumerateEntriesInIdOrder().ToList(),
                actual.EnumerateEntriesInIdOrder().ToList());
        }

        // -------------------------------------------------------------------
        // Paths
        // -------------------------------------------------------------------

        [Fact]
        public void Constructor_TrailingSeparator_IsNormalizedAway()
        {
            // GetSaveGameDirectoryPath() returns ".../SaveGames/", trailing slash and
            // all, so this is the shape the plugin will actually hand over.
            using var temp = new TempDirectory();

            var withSlash = new GlobalStateStore(temp.PathWithTrailingSeparator);
            var withoutSlash = new GlobalStateStore(temp.Path);

            Assert.Equal(temp.Path, withSlash.DirectoryPath);
            Assert.Equal(withoutSlash.LivePath, withSlash.LivePath);
            Assert.DoesNotContain(
                new string(Path.DirectorySeparatorChar, 2),
                withSlash.LivePath.Substring(1),
                StringComparison.Ordinal);
        }

        [Fact]
        public void Constructor_ForwardSlashDirectory_IsNormalizedAway()
        {
            // Application.persistentDataPath uses forward slashes even on Windows, and
            // the game appends "/SaveGames/" to it.
            var store = new GlobalStateStore("C:/Users/someone/AppData/LocalLow/ZAUM Studio/Disco Elysium/SaveGames/");

            Assert.EndsWith("SaveGames", store.DirectoryPath, StringComparison.Ordinal);
            Assert.EndsWith(GlobalStateStore.FileName, store.LivePath, StringComparison.Ordinal);
        }

        [Theory]
        [InlineData(null)]
        [InlineData("")]
        [InlineData("   ")]
        public void Constructor_EmptyDirectory_Throws(string? directory)
        {
            Assert.Throws<ArgumentException>(() => new GlobalStateStore(directory!));
        }

        [Fact]
        public void Paths_AreThreeDistinctSiblingsOfTheFixedFileName()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();

            Assert.Equal(Path.Combine(temp.Path, GlobalStateStore.FileName), store.LivePath);
            Assert.Equal(store.LivePath + GlobalStateStore.BackupSuffix, store.BackupPath);
            Assert.Equal(store.LivePath + GlobalStateStore.TempSuffix, store.TempPath);
            Assert.Equal(temp.Path, Path.GetDirectoryName(store.BackupPath));
            Assert.Equal(temp.Path, Path.GetDirectoryName(store.TempPath));
        }

        // -------------------------------------------------------------------
        // Round trip
        // -------------------------------------------------------------------

        [Fact]
        public void SaveThenLoad_RoundTripsAPopulatedState()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();
            GlobalConversationState original = NewGeneration();

            store.Save(original);
            GlobalStateLoadResult result = store.Load();

            Assert.Equal(GlobalStateLoadOutcome.Loaded, result.Outcome);
            Assert.Equal(store.LivePath, result.SourcePath);
            AssertSameEntries(original, result.RequireState());
        }

        [Fact]
        public void Save_CreatesTheDirectoryIfItIsNotThere()
        {
            using var temp = new TempDirectory();
            string nested = Path.Combine(temp.Path, "SaveGames");
            var store = new GlobalStateStore(nested);

            store.Save(NewGeneration());

            Assert.True(File.Exists(store.LivePath));
        }

        [Fact]
        public void Save_LeavesNoTempFileBehind()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();

            store.Save(NewGeneration());
            store.Save(NewGeneration());

            Assert.False(File.Exists(store.TempPath));
        }

        [Fact]
        public void Save_OfEqualStates_ProducesByteIdenticalFiles()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();

            store.Save(NewGeneration());
            byte[] first = File.ReadAllBytes(store.LivePath);
            store.Save(NewGeneration());
            byte[] second = File.ReadAllBytes(store.LivePath);

            Assert.Equal(first, second);
        }

        [Fact]
        public void Save_NullState_Throws()
        {
            using var temp = new TempDirectory();
            Assert.Throws<ArgumentNullException>(() => temp.CreateStore().Save(null!));
        }

        [Fact]
        public void Save_DoesNotReadTheExistingLiveFile()
        {
            // ProjectGoal.md: the on-disk copy is never read before overwriting. A live
            // file full of garbage must therefore not affect the save at all.
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();
            File.WriteAllText(store.LivePath, "this is not JSON and never was");

            store.Save(NewGeneration());

            AssertSameEntries(NewGeneration(), store.Load().RequireState());
        }

        // -------------------------------------------------------------------
        // Backup rotation
        // -------------------------------------------------------------------

        [Fact]
        public void FirstSave_WritesNoBackup()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();

            store.Save(OldGeneration());

            Assert.True(File.Exists(store.LivePath));
            Assert.False(File.Exists(store.BackupPath));
        }

        [Fact]
        public void SecondSave_MovesThePreviousGenerationIntoTheBackupSlot()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();

            store.Save(OldGeneration());
            store.Save(NewGeneration());

            AssertSameEntries(NewGeneration(), store.Load().RequireState());
            AssertSameEntries(OldGeneration(), store.LoadBackup().RequireState());
        }

        [Fact]
        public void ThirdSave_KeepsExactlyOneGenerationOfHistory()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();
            var third = new GlobalConversationState();
            third.Merge(9, 9, SimStatus.WasDisplayed);

            store.Save(OldGeneration());
            store.Save(NewGeneration());
            store.Save(third);

            AssertSameEntries(third, store.Load().RequireState());
            AssertSameEntries(NewGeneration(), store.LoadBackup().RequireState());
            Assert.Equal(
                new[] { GlobalStateStore.FileName, GlobalStateStore.FileName + GlobalStateStore.BackupSuffix },
                Directory.GetFiles(temp.Path).Select(path => Path.GetFileName(path)!).OrderBy(name => name).ToArray());
        }

        // -------------------------------------------------------------------
        // Missing, corrupt, unsupported
        // -------------------------------------------------------------------

        [Fact]
        public void Load_WithNothingOnDisk_IsMissingNotAnError()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();

            Assert.Equal(GlobalStateLoadOutcome.Missing, store.Load().Outcome);
            Assert.Equal(GlobalStateLoadOutcome.Missing, store.LoadBackup().Outcome);
        }

        [Fact]
        public void Load_WithNoDirectoryAtAll_IsMissing()
        {
            using var temp = new TempDirectory();
            var store = new GlobalStateStore(Path.Combine(temp.Path, "does-not-exist"));

            Assert.Equal(GlobalStateLoadOutcome.Missing, store.Load().Outcome);
        }

        [Fact]
        public void Load_WithACorruptLiveFile_IsCorruptNotMissing()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();
            File.WriteAllText(store.LivePath, "{\"version\":1,\"conversa");

            GlobalStateLoadResult result = store.Load();

            Assert.Equal(GlobalStateLoadOutcome.Corrupt, result.Outcome);
            Assert.Equal(store.LivePath, result.SourcePath);
        }

        [Fact]
        public void Load_WithAnEmptyLiveFile_IsCorruptNotMissing()
        {
            // A zero-byte file is what a naive in-place overwrite would leave behind
            // after a crash. It must not be mistaken for "no history yet".
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();
            File.WriteAllBytes(store.LivePath, Array.Empty<byte>());

            Assert.Equal(GlobalStateLoadOutcome.Corrupt, store.Load().Outcome);
        }

        // -------------------------------------------------------------------
        // Recovery
        // -------------------------------------------------------------------

        [Fact]
        public void LoadWithBackupFallback_HealthyLiveFile_DoesNotConsultTheBackup()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();
            store.Save(OldGeneration());
            store.Save(NewGeneration());

            GlobalStateRecovery recovery = store.LoadWithBackupFallback();

            Assert.False(recovery.RecoveredFromBackup);
            Assert.Null(recovery.Backup);
            AssertSameEntries(NewGeneration(), recovery.Effective.RequireState());
        }

        [Fact]
        public void LoadWithBackupFallback_CorruptLiveFile_RecoversThePreviousGeneration()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();
            store.Save(OldGeneration());
            store.Save(NewGeneration());
            File.WriteAllText(store.LivePath, "corrupted after the fact");

            GlobalStateRecovery recovery = store.LoadWithBackupFallback();

            Assert.True(recovery.RecoveredFromBackup);
            Assert.Equal(GlobalStateLoadOutcome.Corrupt, recovery.Live.Outcome);
            Assert.Equal(GlobalStateLoadOutcome.Loaded, recovery.Outcome);
            AssertSameEntries(OldGeneration(), recovery.Effective.RequireState());
        }

        [Fact]
        public void LoadWithBackupFallback_MissingLiveFile_RecoversThePreviousGeneration()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();
            store.Save(OldGeneration());
            store.Save(NewGeneration());
            File.Delete(store.LivePath);

            GlobalStateRecovery recovery = store.LoadWithBackupFallback();

            Assert.True(recovery.RecoveredFromBackup);
            Assert.Equal(GlobalStateLoadOutcome.Missing, recovery.Live.Outcome);

            // The backup slot holds the generation before the one that went missing,
            // which is the whole point: a lost live file costs one save, not everything.
            AssertSameEntries(OldGeneration(), recovery.Effective.RequireState());
        }

        [Fact]
        public void LoadWithBackupFallback_NothingOnDiskAtAll_ReportsMissing()
        {
            using var temp = new TempDirectory();

            GlobalStateRecovery recovery = temp.CreateStore().LoadWithBackupFallback();

            Assert.Equal(GlobalStateLoadOutcome.Missing, recovery.Outcome);
            Assert.False(recovery.RecoveredFromBackup);
            Assert.Null(recovery.State);
        }

        [Fact]
        public void LoadWithBackupFallback_BothFilesCorrupt_ReportsTheLiveFailure()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();
            File.WriteAllText(store.LivePath, "junk");
            File.WriteAllText(store.BackupPath, "also junk");

            GlobalStateRecovery recovery = store.LoadWithBackupFallback();

            Assert.False(recovery.RecoveredFromBackup);
            Assert.Equal(GlobalStateLoadOutcome.Corrupt, recovery.Outcome);
            Assert.Equal(store.LivePath, recovery.Effective.SourcePath);
            Assert.NotNull(recovery.Backup);
        }

        [Fact]
        public void LoadWithBackupFallback_UnsupportedVersion_RefusesToFallBack()
        {
            // A file from a newer build is intact history, not damage. Quietly reverting
            // to an older backup would be the same as deleting whatever the newer build
            // recorded.
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();
            store.Save(OldGeneration());
            store.Save(NewGeneration());
            File.WriteAllText(
                store.LivePath,
                $"{{\"_format\":\"global-state\",\"_formatVersion\":{GlobalStateJson.FormatVersion + 1},"
                + "\"conversations\":{}}");

            GlobalStateRecovery recovery = store.LoadWithBackupFallback();

            Assert.Equal(GlobalStateLoadOutcome.UnsupportedVersion, recovery.Outcome);
            Assert.Null(recovery.Backup);
            Assert.False(recovery.RecoveredFromBackup);
        }

        [Fact]
        public void LoadWithBackupFallback_OutdatedVersion_RefusesToFallBack()
        {
            // An older file is intact history too. Reverting to the backup would drop it,
            // where converting it keeps everything.
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();
            store.Save(OldGeneration());
            store.Save(NewGeneration());
            File.WriteAllText(store.LivePath, "{\"version\":4,\"conversations\":{}}");

            GlobalStateRecovery recovery = store.LoadWithBackupFallback();

            Assert.Equal(GlobalStateLoadOutcome.OutdatedVersion, recovery.Outcome);
            Assert.Null(recovery.Backup);
            Assert.False(recovery.RecoveredFromBackup);
        }

        // -------------------------------------------------------------------
        // Crash points, simulated in-process by aborting mid-save.
        //
        // Save has no try/catch and no cleanup, so an exception thrown from the step
        // hook leaves the directory in the same state a killed process would. The
        // out-of-process versions of these live in CrashPointTests.
        // -------------------------------------------------------------------

        private static GlobalStateStore CrashingStore(TempDirectory temp, GlobalStateSaveStep crashAt)
        {
            GlobalStateStore store = temp.CreateStore();

            // Small chunks so the write takes several passes and the mid-write crash
            // point is reachable.
            store.WriteChunkSize = 16;
            store.SaveStepHook = step =>
            {
                if (step == crashAt)
                {
                    throw new SimulatedCrashException(step);
                }
            };
            return store;
        }

        private static void AssertSomeCompleteFileSurvives(
            GlobalStateStore store,
            GlobalConversationState expected)
        {
            GlobalStateRecovery recovery = store.LoadWithBackupFallback();
            Assert.True(
                recovery.IsLoaded,
                $"No complete file survived the crash: {recovery}. "
                + "Directory contents: "
                + string.Join(", ", Directory.GetFiles(store.DirectoryPath).Select(path => Path.GetFileName(path))));
            AssertSameEntries(expected, recovery.Effective.RequireState());
        }

        [Fact]
        public void CrashDuringTheTempWrite_LeavesThePreviousGenerationLive()
        {
            using var temp = new TempDirectory();
            temp.CreateStore().Save(OldGeneration());

            GlobalStateStore crashing = CrashingStore(temp, GlobalStateSaveStep.DuringTempWrite);
            Assert.Throws<SimulatedCrashException>(() => crashing.Save(NewGeneration()));

            // The live file was never opened for writing, so it is untouched, and the
            // truncated temp file is never consulted.
            Assert.Equal(GlobalStateLoadOutcome.Loaded, crashing.Load().Outcome);
            AssertSomeCompleteFileSurvives(crashing, OldGeneration());
        }

        [Fact]
        public void CrashAfterTheTempFlush_LeavesThePreviousGenerationLive()
        {
            using var temp = new TempDirectory();
            temp.CreateStore().Save(OldGeneration());

            GlobalStateStore crashing = CrashingStore(temp, GlobalStateSaveStep.AfterTempFlushed);
            Assert.Throws<SimulatedCrashException>(() => crashing.Save(NewGeneration()));

            Assert.True(File.Exists(crashing.TempPath));
            AssertSomeCompleteFileSurvives(crashing, OldGeneration());
        }

        [Fact]
        public void CrashAfterTheRotation_LeavesThePreviousGenerationInTheBackupSlot()
        {
            // The one window with no live file. This is exactly what the backup
            // generation is for.
            using var temp = new TempDirectory();
            temp.CreateStore().Save(OldGeneration());

            GlobalStateStore crashing = CrashingStore(temp, GlobalStateSaveStep.AfterLiveRotatedToBackup);
            Assert.Throws<SimulatedCrashException>(() => crashing.Save(NewGeneration()));

            Assert.False(File.Exists(crashing.LivePath));
            Assert.True(File.Exists(crashing.BackupPath));
            AssertSomeCompleteFileSurvives(crashing, OldGeneration());
        }

        [Fact]
        public void CrashOnTheVeryFirstSave_LeavesNothingToLoseAndNoJunkLiveFile()
        {
            using var temp = new TempDirectory();

            GlobalStateStore crashing = CrashingStore(temp, GlobalStateSaveStep.DuringTempWrite);
            Assert.Throws<SimulatedCrashException>(() => crashing.Save(NewGeneration()));

            // Missing, not Corrupt: a half-written temp file must never be mistaken for
            // the live state, or a first run would look like damaged history and be
            // refused a save over it.
            Assert.Equal(GlobalStateLoadOutcome.Missing, crashing.Load().Outcome);
            Assert.Equal(GlobalStateLoadOutcome.Missing, crashing.LoadWithBackupFallback().Outcome);
        }

        [Fact]
        public void SaveAfterACrashInTheRotationWindow_RecoversWithoutLosingTheBackup()
        {
            // Crash between the rotation and the promotion, then save again. The second
            // save must not destroy the surviving backup while there is no live file.
            using var temp = new TempDirectory();
            temp.CreateStore().Save(OldGeneration());

            GlobalStateStore crashing = CrashingStore(temp, GlobalStateSaveStep.AfterLiveRotatedToBackup);
            Assert.Throws<SimulatedCrashException>(() => crashing.Save(NewGeneration()));

            var third = new GlobalConversationState();
            third.Merge(4, 40, SimStatus.WasDisplayed);
            GlobalStateStore healthy = temp.CreateStore();
            healthy.Save(third);

            AssertSameEntries(third, healthy.Load().RequireState());
            AssertSameEntries(OldGeneration(), healthy.LoadBackup().RequireState());
        }

        [Fact]
        public void StaleTempFileFromAnEarlierCrash_IsTruncatedNotAppendedTo()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();
            File.WriteAllText(store.TempPath, new string('x', 10_000));

            store.Save(NewGeneration());

            AssertSameEntries(NewGeneration(), store.Load().RequireState());
        }

        // -------------------------------------------------------------------
        // Retrying a lock that clears (de-zexr).
        // -------------------------------------------------------------------

        /// <summary>
        /// A save whose live file is briefly held by something else still lands.
        /// </summary>
        /// <remarks>
        /// <para>The real failure, produced with a real lock rather than an injected
        /// exception. A save is three filesystem operations on the same directory, and on
        /// Windows any of them can come back as a sharing violation because a virus
        /// scanner, the search indexer or a backup agent held the file for an instant.
        /// Before this, one such moment put an ERROR in the player's log and made
        /// <c>TrySave</c> report failure for a save that was never going to be wrong -
        /// which is also what made the deferred-write soak fail intermittently, twice, five
        /// months apart (de-wqs).</para>
        ///
        /// <para>The lock is released on a timer well inside the retry budget, so the test
        /// turns on the retry existing rather than on how fast the machine is: ten attempts
        /// fifty milliseconds apart against a hold of one hundred.</para>
        /// </remarks>
        [Fact]
        public async Task SaveWhoseFileIsBrieflyHeldElsewhere_RetriesAndLands()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();

            // A first generation, so the save under test has a live file to rotate - which
            // is the step the lock below blocks.
            store.Save(OldGeneration());

            using var locked = new ManualResetEventSlim(false);
            var holder = Task.Run(() =>
            {
                using var hold = new FileStream(
                    store.LivePath, FileMode.Open, FileAccess.Read, FileShare.None);
                locked.Set();
                Thread.Sleep(100);
            });

            Assert.True(locked.Wait(TimeSpan.FromSeconds(5)), "the holder never took the file");

            // Would throw without the retry: the rotation cannot move a file another
            // handle holds with FileShare.None.
            store.Save(NewGeneration());
            await holder;

            AssertSameEntries(NewGeneration(), store.Load().RequireState());
        }

        /// <summary>
        /// The generation the rotation moved aside survives a retry of the promotion.
        /// </summary>
        /// <remarks>
        /// The reason the retry is per STEP rather than around the whole save. The rotation
        /// - moving the live file onto the backup - is not idempotent: running the sequence
        /// again after it had already succeeded would rotate the new backup away as well,
        /// and a transient lock would have become real data loss. Retrying only the step
        /// that failed cannot do that, and this says so out loud.
        /// </remarks>
        [Fact]
        public async Task RetryAfterTheRotation_DoesNotRotateASecondTime()
        {
            using var temp = new TempDirectory();
            GlobalStateStore store = temp.CreateStore();

            store.Save(OldGeneration());

            using var locked = new ManualResetEventSlim(false);
            var holder = Task.Run(() =>
            {
                using var hold = new FileStream(
                    store.LivePath, FileMode.Open, FileAccess.Read, FileShare.None);
                locked.Set();
                Thread.Sleep(100);
            });

            Assert.True(locked.Wait(TimeSpan.FromSeconds(5)), "the holder never took the file");
            store.Save(NewGeneration());
            await holder;

            // One rotation happened, so the backup holds the generation that was live -
            // not the one before it, and not the new one.
            Assert.True(File.Exists(store.BackupPath), "the previous generation should be kept");
            AssertSameEntries(
                OldGeneration(),
                GlobalStateJson.Deserialize(
                    File.ReadAllBytes(store.BackupPath), store.BackupPath).RequireState());
        }

        /// <summary>Only a lock that clears is retried; everything else is reported.</summary>
        /// <remarks>
        /// Retrying a permission error would turn an immediate honest failure into a slow
        /// one, and retrying a crash-point test's exception would quietly complete a save
        /// that test exists to stop.
        /// </remarks>
        [Theory]
        [InlineData(32, true)]   // ERROR_SHARING_VIOLATION
        [InlineData(33, true)]   // ERROR_LOCK_VIOLATION
        [InlineData(5, false)]   // ERROR_ACCESS_DENIED
        [InlineData(112, false)] // ERROR_DISK_FULL
        [InlineData(0, false)]
        public void OnlyASharingOrLockViolationIsRetried(int win32Code, bool expected)
        {
            var error = new IOException("something") { HResult = unchecked((int)0x80070000) | win32Code };
            Assert.Equal(expected, GlobalStateStore.IsTransient(error));
        }

        /// <summary>A failure that is not IO at all is never retried.</summary>
        /// <remarks>
        /// The crash-point tests throw their own exception type, and the mid-write hook is
        /// the one hook inside a retried step - so "not an IOException" is what keeps a
        /// simulated crash from being retried into a completed save.
        /// </remarks>
        [Fact]
        public void AFailureThatIsNotIoIsNeverRetried()
        {
            Assert.False(GlobalStateStore.IsTransient(new InvalidOperationException("no")));
            Assert.False(GlobalStateStore.IsTransient(
                new UnauthorizedAccessException("denied")));
        }
    }
}
