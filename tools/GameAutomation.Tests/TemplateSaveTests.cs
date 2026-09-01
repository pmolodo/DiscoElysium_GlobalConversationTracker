// SPDX-License-Identifier: MIT
using System;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>The committed expanded save the harness packs and stages.</summary>
    public class TemplateSaveTests
    {
        /// <summary>Every game member is prefixed with the expanded save's name.</summary>
        [Fact]
        public void TheExpandedMembersMatchTheDirectoryName()
        {
            string save = FindTemplateSave();
            string expandedName = Path.GetFileName(save);
            string archiveName = expandedName.Substring(0, expandedName.Length - ".ntwtf".Length);

            string[] members = Directory.GetFileSystemEntries(save);
            Assert.NotEmpty(members);
            foreach (string member in members)
            {
                string name = Path.GetFileName(member);
                Assert.True(
                    name.StartsWith(archiveName + ".", StringComparison.Ordinal)
                        || name == expandedName + ".lua.parts",
                    $"expanded member '{name}' does not match '{archiveName}'");
            }
        }

        /// <summary>The thumbnail is named for the save, or the slot shows blank.</summary>
        [Fact]
        public void TheThumbnailIsNamedForTheSave()
        {
            string save = FindTemplateSave();
            string thumbnail = save.Substring(0, save.Length - ".ntwtf".Length) + ".jpg";

            Assert.True(File.Exists(thumbnail), $"expected a thumbnail at {thumbnail}");
        }

        /// <summary>The sparse source has all six files needed to rebuild the Lua blob.</summary>
        [Fact]
        public void TheSaveHoldsItsSparseParts()
        {
            string save = FindTemplateSave();
            string parts = Path.Combine(save, Path.GetFileName(save) + ".lua.parts");

            Assert.True(Directory.Exists(parts), $"expected sparse parts at {parts}");
            Assert.Equal(5, Directory.GetFiles(parts, "*.json").Length);
            Assert.True(File.Exists(Path.Combine(parts, "trailing.bin")));
        }

        private static string FindTemplateSave()
        {
            var directory = new DirectoryInfo(AppDomain.CurrentDomain.BaseDirectory);
            while (directory != null)
            {
                string testing = Path.Combine(directory.FullName, "testing");
                if (Directory.Exists(testing))
                {
                    string[] saves = Directory.GetDirectories(testing, "*.ntwtf");
                    if (saves.Length == 1)
                    {
                        return saves[0];
                    }
                    if (saves.Length > 1)
                    {
                        throw new InvalidOperationException(
                            $"{testing} holds {saves.Length} expanded saves; the harness stages one.");
                    }
                }
                directory = directory.Parent;
            }
            throw new DirectoryNotFoundException(
                "Could not find an expanded template save under testing/.");
        }
    }
}
