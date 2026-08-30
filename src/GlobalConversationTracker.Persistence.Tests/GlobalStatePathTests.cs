// SPDX-License-Identifier: MIT
using System;
using System.IO;
using GlobalConversationTracker.Persistence;
using Xunit;

namespace GlobalConversationTracker.Persistence.Tests
{
    public class GlobalStatePathTests
    {
        private static readonly string SaveGames =
            Path.Combine("C:", "saves", "Disco Elysium", "SaveGames");

        [Theory]
        [InlineData(null)]
        [InlineData("")]
        [InlineData("   ")]
        public void NoOverride_UsesTheDefaultNameInSaveGames(string? value)
        {
            Assert.Equal(
                Path.Combine(SaveGames, GlobalStateStore.FileName),
                GlobalStatePath.Resolve(SaveGames, value));
        }

        /// <summary>
        /// A bare name lands beside the real file rather than in whatever directory the
        /// game was launched from, which is the whole point of resolving it.
        /// </summary>
        [Fact]
        public void BareFileName_ResolvesAgainstSaveGames()
        {
            Assert.Equal(
                Path.Combine(SaveGames, "testing.json"),
                GlobalStatePath.Resolve(SaveGames, "testing.json"));
        }

        [Fact]
        public void RelativePathWithDirectories_ResolvesAgainstSaveGames()
        {
            Assert.Equal(
                Path.Combine(SaveGames, "scratch", "testing.json"),
                GlobalStatePath.Resolve(SaveGames, Path.Combine("scratch", "testing.json")));
        }

        [Fact]
        public void AbsolutePath_IsUsedAsGiven()
        {
            string absolute = Path.Combine("D:", "tmp", "testing.json");
            Assert.Equal(absolute, GlobalStatePath.Resolve(SaveGames, absolute));
        }

        /// <summary>
        /// A trailing separator names a directory, so the default file name goes inside
        /// it rather than the value being read as a file with an empty name.
        /// </summary>
        [Fact]
        public void RelativeDirectory_GetsTheDefaultFileNameInside()
        {
            Assert.Equal(
                Path.Combine(SaveGames, "scratch", GlobalStateStore.FileName),
                GlobalStatePath.Resolve(SaveGames, "scratch" + Path.DirectorySeparatorChar));
        }

        [Fact]
        public void AbsoluteDirectory_GetsTheDefaultFileNameInside()
        {
            string directory = Path.Combine("D:", "tmp");
            Assert.Equal(
                Path.Combine(directory, GlobalStateStore.FileName),
                GlobalStatePath.Resolve(SaveGames, directory + Path.DirectorySeparatorChar));
        }

        [Fact]
        public void SurroundingWhitespace_IsIgnored()
        {
            Assert.Equal(
                Path.Combine(SaveGames, "testing.json"),
                GlobalStatePath.Resolve(SaveGames, "  testing.json  "));
        }

        [Fact]
        public void TrailingSeparatorOnSaveGames_DoesNotDoubleUp()
        {
            Assert.Equal(
                Path.Combine(SaveGames, GlobalStateStore.FileName),
                GlobalStatePath.Resolve(SaveGames + Path.DirectorySeparatorChar, null));
        }

        [Theory]
        [InlineData(null)]
        [InlineData("")]
        [InlineData("   ")]
        public void MissingSaveGamesDirectory_Throws(string? directory)
        {
            Assert.Throws<ArgumentException>(
                () => GlobalStatePath.Resolve(directory!, null));
        }

        /// <summary>
        /// The store must put its backup and temp files beside the redirected live file,
        /// not back in SaveGames - a half-written temp landing next to the real state
        /// would be exactly the corruption the override exists to avoid.
        /// </summary>
        [Fact]
        public void AtPath_KeepsEveryGenerationTogether()
        {
            string path = Path.Combine("D:", "tmp", "testing.json");
            GlobalStateStore store = GlobalStateStore.AtPath(path);

            Assert.Equal(path, store.LivePath);
            Assert.Equal(path + GlobalStateStore.BackupSuffix, store.BackupPath);
            Assert.Equal(path + GlobalStateStore.TempSuffix, store.TempPath);
            Assert.Equal(Path.Combine("D:", "tmp"), store.DirectoryPath);
        }

        [Theory]
        [InlineData(null)]
        [InlineData("")]
        [InlineData("   ")]
        [InlineData("bare-name-with-no-directory.json")]
        public void AtPath_RejectsAPathItCannotWriteInto(string? path)
        {
            Assert.Throws<ArgumentException>(() => GlobalStateStore.AtPath(path!));
        }
    }
}
