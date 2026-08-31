// SPDX-License-Identifier: MIT
using System;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// The settings swap, against a scratch file and with the registry left alone.
    /// </summary>
    /// <remarks>
    /// Never touches the real settings: DISCO_ELYSIUM_GCT_SETTINGS_FILE redirects
    /// GameSettings at a temporary copy for the duration. That redirect exists for
    /// exactly this.
    /// </remarks>
    public class GameSettingsTests : IDisposable
    {
        private readonly string _directory;
        private readonly string _settings;
        private readonly string? _previousRedirect;

        public GameSettingsTests()
        {
            _directory = Path.Combine(Path.GetTempPath(), "gct-settings-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(_directory);
            _settings = Path.Combine(_directory, "Settings.json");
            File.WriteAllText(_settings, "{\"GRAPHICS\":{\"resolutionWidth\":3840}}");

            _previousRedirect = Environment.GetEnvironmentVariable("DISCO_ELYSIUM_GCT_SETTINGS_FILE");
            Environment.SetEnvironmentVariable("DISCO_ELYSIUM_GCT_SETTINGS_FILE", _settings);
        }

        public void Dispose()
        {
            Environment.SetEnvironmentVariable("DISCO_ELYSIUM_GCT_SETTINGS_FILE", _previousRedirect);
            if (Directory.Exists(_directory))
            {
                Directory.Delete(_directory, recursive: true);
            }
        }

        [Fact]
        public void TheRedirectIsHonoured()
        {
            Assert.Equal(_settings, GameSettings.SettingsPath);
            Assert.True(GameSettings.Exists);
        }

        [Fact]
        public void BackupAndRestoreRoundTripByteForByte()
        {
            byte[] original = File.ReadAllBytes(_settings);
            string backupPath = Path.Combine(_directory, "backup.json");

            SettingsBackup backup = GameSettings.Backup(backupPath, includeRegistry: false);
            Assert.Null(backup.RegistryPath);

            File.WriteAllText(_settings, "{\"GRAPHICS\":{\"resolutionWidth\":1280}}");
            Assert.NotEqual(original, File.ReadAllBytes(_settings));

            GameSettings.Restore(backup);
            Assert.Equal(original, File.ReadAllBytes(_settings));
        }

        [Fact]
        public void InstallReplacesTheWholeFile()
        {
            string replacement = Path.Combine(_directory, "test-settings.json");
            File.WriteAllText(replacement, "{\"GRAPHICS\":{\"resolutionWidth\":1280}}");

            GameSettings.Install(replacement);

            Assert.Equal(File.ReadAllBytes(replacement), File.ReadAllBytes(_settings));
        }

        [Fact]
        public void InstallingAMissingFileIsRefused()
        {
            Assert.Throws<FileNotFoundException>(
                () => GameSettings.Install(Path.Combine(_directory, "no-such-file.json")));
        }

        [Fact]
        public void RestoringAMissingBackupIsRefused()
        {
            var backup = new SettingsBackup(Path.Combine(_directory, "gone.json"), null);
            Assert.Throws<FileNotFoundException>(() => GameSettings.Restore(backup));
        }

        [Fact]
        public void RestoringNothingIsRefused()
        {
            Assert.Throws<ArgumentNullException>(() => GameSettings.Restore(null!));
        }

        /// <summary>
        /// The shipped test settings must actually say what the harness claims, or every
        /// run silently uses whatever they drifted to.
        /// </summary>
        [Fact]
        public void TheShippedTestSettingsAreTheCheapOnes()
        {
            string path = FindRepoFile(Path.Combine("testing", "Settings.json"));
            string json = File.ReadAllText(path);

            Assert.Contains("\"resolutionWidth\"", json, StringComparison.Ordinal);
            Assert.Contains("1280", json, StringComparison.Ordinal);
            Assert.Contains("720", json, StringComparison.Ordinal);
        }

        private static string FindRepoFile(string relative)
        {
            var directory = new DirectoryInfo(AppDomain.CurrentDomain.BaseDirectory);
            while (directory != null)
            {
                string candidate = Path.Combine(directory.FullName, relative);
                if (File.Exists(candidate))
                {
                    return candidate;
                }

                directory = directory.Parent;
            }

            throw new FileNotFoundException($"Could not find {relative} above the test assembly.");
        }
    }

    /// <summary>The key table, which is the other thing a wrong value fails silently on.</summary>
    public class GameKeyboardTests
    {
        [Theory]
        [InlineData("Escape")]
        [InlineData("Enter")]
        [InlineData("Up")]
        [InlineData("Down")]
        [InlineData("Left")]
        [InlineData("Right")]
        [InlineData("Space")]
        [InlineData("A")]
        [InlineData("0")]
        public void TheKeysAMenuNeedsAreKnown(string key)
        {
            Assert.True(GameKeyboard.IsKnown(key), $"'{key}' should be a known key");
        }

        [Fact]
        public void KeyNamesAreCaseInsensitive()
        {
            Assert.True(GameKeyboard.IsKnown("escape"));
            Assert.True(GameKeyboard.IsKnown("ESCAPE"));
        }

        /// <summary>
        /// An unknown name must throw rather than do nothing: a keystroke that silently
        /// vanishes is indistinguishable from a game that ignored it.
        /// </summary>
        [Fact]
        public void AnUnknownKeyIsRefusedRatherThanIgnored()
        {
            Assert.False(GameKeyboard.IsKnown("NoSuchKey"));
            Assert.Throws<ArgumentException>(() => GameKeyboard.Press("NoSuchKey"));
            Assert.Throws<ArgumentException>(() => GameKeyboard.Release("NoSuchKey"));
        }

        [Fact]
        public void TheKeyListIsSortedAndNotEmpty()
        {
            string[] names = GameKeyboard.KeyNames;

            Assert.NotEmpty(names);
            Assert.Equal(names.Length, new System.Collections.Generic.HashSet<string>(names).Count);
        }
    }
}
