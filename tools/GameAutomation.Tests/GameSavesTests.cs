// SPDX-License-Identifier: MIT
using System;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// Recognising save files and copying one.
    /// </summary>
    /// <remarks>
    /// Only what GameSaves still owns: recognising a save file and copying one with its
    /// thumbnail. Moving folders around is GameProfile's job and is tested there.
    /// </remarks>
    public class GameSavesTests : IDisposable
    {
        private readonly string _root;
        private readonly string _saves;
        public GameSavesTests()
        {
            _root = Path.Combine(Path.GetTempPath(), "gct-saves-" + Guid.NewGuid().ToString("N"));
            _saves = Path.Combine(_root, "SaveGames");
            Directory.CreateDirectory(_saves);

        }

        public void Dispose()
        {
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
        public void ListingIgnoresThumbnails()
        {
            WriteSave(_saves, "one");
            WriteSave(_saves, "two");

            string[] saves = GameSaves.ListSaves(_saves);

            Assert.Equal(2, saves.Length);
            Assert.All(saves, s => Assert.EndsWith(GameSaves.SaveExtension, s));
        }

        [Fact]
        public void CopyingBringsTheThumbnailWithIt()
        {
            string chosen = WriteSave(Path.Combine(_root, "fixtures"), "chosen");
            string destination = Path.Combine(_root, "staged");

            GameSaves.CopyInto(chosen, destination);

            Assert.Single(GameSaves.ListSaves(destination));
            Assert.True(File.Exists(Path.Combine(destination, "chosen.jpg")));
        }

        [Fact]
        public void ListingAFolderThatIsNotThereIsEmptyRatherThanAnError()
        {
            Assert.Empty(GameSaves.ListSaves(Path.Combine(_root, "nowhere")));
        }

        [Fact]
        public void StagingSomethingThatIsNotASaveIsRefused()
        {
            string notASave = Path.Combine(_root, "notes.txt");
            File.WriteAllText(notASave, "hello");

            Assert.Throws<ArgumentException>(
                () => GameSaves.CopyInto(notASave, _root));
            Assert.Throws<FileNotFoundException>(
                () => GameSaves.CopyInto(
                    Path.Combine(_root, "missing" + GameSaves.SaveExtension), _root));
        }

    }
}
