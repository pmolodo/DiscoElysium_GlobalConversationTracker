// SPDX-License-Identifier: MIT
using System;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// Moving the player's whole profile aside and putting it back.
    /// </summary>
    /// <remarks>
    /// Every test runs against a scratch folder through the redirect
    /// <see cref="GameProfile.RedirectVariable"/> names, never a real installation. This is
    /// the most destructive thing the harness does - the folder is gigabytes of somebody's
    /// playthroughs and is Steam-Cloud-synced - so most of these are about what happens when
    /// something goes wrong partway.
    /// </remarks>
    public class GameProfileTests : IDisposable
    {
        /// <summary>The redirect, qualified: this sets it, so it needs the full name.</summary>
        private static readonly string RedirectVariable =
            DegctEnv.Qualified(GameProfile.RedirectVariable);

        private readonly string _root;
        private readonly string _profile;
        private readonly string? _previous;

        public GameProfileTests()
        {
            _root = Path.Combine(Path.GetTempPath(), "gct-profile-" + Guid.NewGuid().ToString("N"));
            _profile = Path.Combine(_root, "Disco Elysium");
            Directory.CreateDirectory(_profile);

            _previous = Environment.GetEnvironmentVariable(RedirectVariable);
            Environment.SetEnvironmentVariable(RedirectVariable, _profile);
        }

        public void Dispose()
        {
            Environment.SetEnvironmentVariable(RedirectVariable, _previous);
            if (Directory.Exists(_root))
            {
                Directory.Delete(_root, recursive: true);
            }
        }

        /// <summary>A profile shaped like a real one: settings, saves, logs, collages.</summary>
        private void WriteRealProfile()
        {
            Directory.CreateDirectory(Path.Combine(_profile, "Settings"));
            File.WriteAllText(
                Path.Combine(_profile, "Settings", "Settings.json"), "the player's settings");

            Directory.CreateDirectory(Path.Combine(_profile, "SaveGames"));
            WriteSave(Path.Combine(_profile, "SaveGames"), "playthrough one");
            WriteSave(Path.Combine(_profile, "SaveGames"), "playthrough two");
            File.WriteAllText(
                Path.Combine(_profile, "SaveGames", "global-conversation-state.json"), "{}");

            Directory.CreateDirectory(Path.Combine(_profile, "SaveCollages"));
            File.WriteAllText(Path.Combine(_profile, "Player.log"), "log");
        }

        private static string WriteSave(string folder, string name, string content = "save")
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
            Assert.Equal(_profile, GameProfile.ProfilePath);
            Assert.Equal(
                Path.Combine(_profile, "Settings", "Settings.json"), GameProfile.SettingsFile);
            Assert.Equal(Path.Combine(_profile, "SaveGames"), GameProfile.SavesFolder);
        }

        [Fact]
        public void BackupMovesTheWholeProfileAway()
        {
            WriteRealProfile();
            string movedTo = Path.Combine(_root, "moved");

            ProfileBackup backup = GameProfile.Backup(movedTo);

            Assert.Equal(4, backup.EntryCount);
            Assert.False(Directory.Exists(_profile), "the profile should be gone, not emptied");
            Assert.True(File.Exists(Path.Combine(movedTo, "Player.log")));
            Assert.Equal(2, GameSaves.ListSaves(Path.Combine(movedTo, "SaveGames")).Length);
        }

        /// <summary>The point of staging: one save, so Continue cannot load the wrong one.</summary>
        [Fact]
        public void AStagedProfileHasOnlyTheStagedSave()
        {
            WriteRealProfile();
            string settings = Path.Combine(_root, "test-settings.json");
            File.WriteAllText(settings, "the test settings");
            string save = WriteSave(Path.Combine(_root, "fixtures"), "chosen");

            GameProfile.Backup(Path.Combine(_root, "moved"));
            GameProfile.Stage(settings, save);

            string[] staged = GameSaves.ListSaves(GameProfile.SavesFolder);
            Assert.Single(staged);
            Assert.Contains("chosen", staged[0]);
            Assert.Equal("the test settings", File.ReadAllText(GameProfile.SettingsFile));
        }

        /// <summary>
        /// A staged profile carries nothing over from the player: no logs, no collages,
        /// and no leftover global state from a previous run.
        /// </summary>
        [Fact]
        public void AStagedProfileCarriesNothingOver()
        {
            WriteRealProfile();
            string settings = Path.Combine(_root, "test-settings.json");
            File.WriteAllText(settings, "the test settings");

            GameProfile.Backup(Path.Combine(_root, "moved"));
            GameProfile.Stage(settings, saveFile: null);

            Assert.False(File.Exists(Path.Combine(_profile, "Player.log")));
            Assert.False(Directory.Exists(Path.Combine(_profile, "SaveCollages")));
            Assert.False(File.Exists(
                Path.Combine(GameProfile.SavesFolder, "global-conversation-state.json")));
        }

        [Fact]
        public void RestorePutsEverythingBackAndRemovesTheStagedProfile()
        {
            WriteRealProfile();
            string settings = Path.Combine(_root, "test-settings.json");
            File.WriteAllText(settings, "the test settings");
            string save = WriteSave(Path.Combine(_root, "fixtures"), "chosen");
            string movedTo = Path.Combine(_root, "moved");

            ProfileBackup backup = GameProfile.Backup(movedTo);
            GameProfile.Stage(settings, save);
            GameProfile.Restore(backup);

            Assert.Equal("the player's settings", File.ReadAllText(GameProfile.SettingsFile));
            Assert.Equal(2, GameSaves.ListSaves(GameProfile.SavesFolder).Length);
            Assert.DoesNotContain(
                GameSaves.ListSaves(GameProfile.SavesFolder), s => s.Contains("chosen"));
            Assert.True(File.Exists(Path.Combine(_profile, "Player.log")));
            Assert.False(Directory.Exists(movedTo));
        }

        [Fact]
        public void RestoreKeepsSaveContents()
        {
            WriteSave(Path.Combine(_profile, "SaveGames"), "precious", "the actual playthrough");
            ProfileBackup backup = GameProfile.Backup(Path.Combine(_root, "moved"));
            GameProfile.Stage(settingsFile: null, saveFile: null);

            GameProfile.Restore(backup);

            Assert.Equal(
                "the actual playthrough",
                File.ReadAllText(Path.Combine(
                    GameProfile.SavesFolder, "precious" + GameSaves.SaveExtension)));
        }

        /// <summary>
        /// A lost backup must not quietly succeed and leave the player with a profile
        /// holding one test save - it is cloud-synced, so that can propagate.
        /// </summary>
        [Fact]
        public void ALostBackupIsReportedRatherThanIgnored()
        {
            WriteRealProfile();
            ProfileBackup backup = GameProfile.Backup(Path.Combine(_root, "moved"));
            GameProfile.Stage(settingsFile: null, saveFile: null);
            Directory.Delete(Path.Combine(_root, "moved"), recursive: true);

            InvalidOperationException error = Assert.Throws<InvalidOperationException>(
                () => GameProfile.Restore(backup));

            Assert.Contains("do not launch the game", error.Message);
        }

        [Fact]
        public void BackingUpOntoAnExistingFolderIsRefused()
        {
            WriteRealProfile();
            string movedTo = Path.Combine(_root, "moved");
            Directory.CreateDirectory(movedTo);

            Assert.Throws<InvalidOperationException>(() => GameProfile.Backup(movedTo));
        }

        /// <summary>
        /// A first run on a machine that has never launched the game: there is nothing to
        /// move aside, and restoring must remove what was staged rather than leave it.
        /// </summary>
        [Fact]
        public void NoProfileMeansTheStagedOneIsRemovedOnRestore()
        {
            Directory.Delete(_profile, recursive: true);

            ProfileBackup backup = GameProfile.Backup(Path.Combine(_root, "moved"));
            GameProfile.Stage(settingsFile: null, saveFile: null);
            Assert.True(Directory.Exists(_profile));

            GameProfile.Restore(backup);

            Assert.Null(backup.MovedTo);
            Assert.False(Directory.Exists(_profile));
        }

        [Fact]
        public void StagingSomethingThatIsNotASaveIsRefused()
        {
            string notASave = Path.Combine(_root, "notes.txt");
            File.WriteAllText(notASave, "hello");

            Assert.Throws<ArgumentException>(
                () => GameProfile.Stage(settingsFile: null, saveFile: notASave));
            Assert.Throws<FileNotFoundException>(
                () => GameProfile.Stage(Path.Combine(_root, "missing.json"), saveFile: null));
        }

        /// <summary>
        /// The backup goes beside the profile, not into the temp directory.
        /// </summary>
        /// <remarks>
        /// Directory.Move cannot cross volumes, and there is no guarantee the temp
        /// directory is on the same one - a redirected AppData or a TEMP set elsewhere and
        /// the move fails outright. It did, on the first real exercise of this.
        /// </remarks>
        [Fact]
        public void TheBackupGoesBesideTheProfile()
        {
            string backup = GameProfile.DefaultBackupPath("20260831-120000");

            Assert.Equal(
                Path.GetDirectoryName(_profile), Path.GetDirectoryName(backup));
            Assert.Contains("20260831-120000", backup);
            Assert.NotEqual(_profile, backup);
        }

        /// <summary>Two runs must not collide on the same backup folder.</summary>
        [Fact]
        public void DifferentStampsGiveDifferentBackups()
        {
            Assert.NotEqual(
                GameProfile.DefaultBackupPath("one"), GameProfile.DefaultBackupPath("two"));
        }

        [Fact]
        public void RestoringNullIsRefused()
        {
            Assert.Throws<ArgumentNullException>(() => GameProfile.Restore(null!));
        }
    }
}
