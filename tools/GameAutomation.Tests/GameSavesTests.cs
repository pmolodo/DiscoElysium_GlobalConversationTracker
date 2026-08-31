// SPDX-License-Identifier: MIT
using System;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// Moving the player's saves aside and putting them back.
    /// </summary>
    /// <remarks>
    /// Every test runs against a scratch folder through DISCO_ELYSIUM_GCT_SAVES_DIR, never
    /// a real installation. This is the most destructive thing the harness does - the
    /// folder is gigabytes of somebody's playthroughs and is Steam-Cloud-synced - so the
    /// tests care mostly about what happens when something goes wrong partway.
    /// </remarks>
    public class GameSavesTests : IDisposable
    {
        private const string RedirectVariable = "DISCO_ELYSIUM_GCT_SAVES_DIR";

        private readonly string _root;
        private readonly string _saves;
        private readonly string? _previous;

        public GameSavesTests()
        {
            _root = Path.Combine(Path.GetTempPath(), "gct-saves-" + Guid.NewGuid().ToString("N"));
            _saves = Path.Combine(_root, "SaveGames");
            Directory.CreateDirectory(_saves);

            _previous = Environment.GetEnvironmentVariable(RedirectVariable);
            Environment.SetEnvironmentVariable(RedirectVariable, _saves);
        }

        public void Dispose()
        {
            Environment.SetEnvironmentVariable(RedirectVariable, _previous);
            if (Directory.Exists(_root))
            {
                Directory.Delete(_root, recursive: true);
            }
        }

        private string WriteSave(string folder, string name, string content = "save")
        {
            Directory.CreateDirectory(folder);
            string path = Path.Combine(folder, name + GameSaves.SaveExtension);
            File.WriteAllText(path, content);
            File.WriteAllText(Path.Combine(folder, name + ".jpg"), "thumbnail");
            return path;
        }

        [Fact]
        public void TheRedirectIsHonoured()
        {
            Assert.Equal(_saves, GameSaves.SavesPath);
        }

        [Fact]
        public void ListingIgnoresThumbnails()
        {
            WriteSave(_saves, "one");
            WriteSave(_saves, "two");

            string[] saves = GameSaves.ListSaves(_saves);

            Assert.Equal(2, saves.Length);
            Assert.All(saves, s => Assert.EndsWith(GameSaves.SaveExtension, s));
        }

        [Fact]
        public void BackupMovesThePlayersSavesAndLeavesAnEmptyFolder()
        {
            WriteSave(_saves, "playthrough");
            string movedTo = Path.Combine(_root, "moved");

            SavesBackup backup = GameSaves.Backup(movedTo);

            Assert.Equal(2, backup.FileCount);
            Assert.True(Directory.Exists(_saves), "the game still needs a folder to exist");
            Assert.Empty(Directory.GetFiles(_saves));
            Assert.Single(GameSaves.ListSaves(movedTo));
        }

        /// <summary>The whole point: one save, so Continue cannot load the wrong one.</summary>
        [Fact]
        public void StagingLeavesExactlyOneSave()
        {
            WriteSave(_saves, "real one");
            WriteSave(_saves, "real two");
            string chosen = WriteSave(Path.Combine(_root, "fixtures"), "chosen");

            GameSaves.Backup(Path.Combine(_root, "moved"));
            GameSaves.Install(chosen);

            string[] staged = GameSaves.ListSaves(_saves);
            Assert.Single(staged);
            Assert.Contains("chosen", staged[0]);
        }

        [Fact]
        public void StagingBringsTheThumbnailWithIt()
        {
            string chosen = WriteSave(Path.Combine(_root, "fixtures"), "chosen");
            GameSaves.Backup(Path.Combine(_root, "moved"));

            GameSaves.Install(chosen);

            Assert.True(File.Exists(Path.Combine(_saves, "chosen.jpg")));
        }

        [Fact]
        public void RestorePutsEverythingBackAndRemovesTheStagedSave()
        {
            WriteSave(_saves, "real one");
            WriteSave(_saves, "real two");
            string chosen = WriteSave(Path.Combine(_root, "fixtures"), "chosen");

            SavesBackup backup = GameSaves.Backup(Path.Combine(_root, "moved"));
            GameSaves.Install(chosen);
            GameSaves.Restore(backup);

            string[] saves = GameSaves.ListSaves(_saves);
            Assert.Equal(2, saves.Length);
            Assert.DoesNotContain(saves, s => s.Contains("chosen"));
            Assert.False(Directory.Exists(Path.Combine(_root, "moved")));
        }

        [Fact]
        public void RestoreKeepsTheFileContents()
        {
            WriteSave(_saves, "precious", "the actual playthrough");
            SavesBackup backup = GameSaves.Backup(Path.Combine(_root, "moved"));

            GameSaves.Restore(backup);

            Assert.Equal(
                "the actual playthrough",
                File.ReadAllText(Path.Combine(_saves, "precious" + GameSaves.SaveExtension)));
        }

        /// <summary>
        /// Restoring when the backup has vanished must not quietly succeed and leave the
        /// player with a folder holding one test save.
        /// </summary>
        [Fact]
        public void ALostBackupIsReportedRatherThanIgnored()
        {
            WriteSave(_saves, "real");
            SavesBackup backup = GameSaves.Backup(Path.Combine(_root, "moved"));
            Directory.Delete(Path.Combine(_root, "moved"), recursive: true);

            InvalidOperationException error = Assert.Throws<InvalidOperationException>(
                () => GameSaves.Restore(backup));

            Assert.Contains("do not launch the game", error.Message);
        }

        [Fact]
        public void BackingUpOntoAnExistingFolderIsRefused()
        {
            WriteSave(_saves, "real");
            string movedTo = Path.Combine(_root, "moved");
            Directory.CreateDirectory(movedTo);

            Assert.Throws<InvalidOperationException>(() => GameSaves.Backup(movedTo));
        }

        [Fact]
        public void NoSavesFolderIsNotAnError()
        {
            Directory.Delete(_saves, recursive: true);

            SavesBackup backup = GameSaves.Backup(Path.Combine(_root, "moved"));

            Assert.Null(backup.MovedTo);
            GameSaves.Restore(backup);
        }

        [Fact]
        public void StagingSomethingThatIsNotASaveIsRefused()
        {
            string notASave = Path.Combine(_root, "notes.txt");
            File.WriteAllText(notASave, "hello");

            Assert.Throws<ArgumentException>(() => GameSaves.Install(notASave));
            Assert.Throws<FileNotFoundException>(
                () => GameSaves.Install(Path.Combine(_root, "missing" + GameSaves.SaveExtension)));
        }

        [Fact]
        public void RestoringNullIsRefused()
        {
            Assert.Throws<ArgumentNullException>(() => GameSaves.Restore(null!));
        }
    }
}
