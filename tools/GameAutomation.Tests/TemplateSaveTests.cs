// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.IO.Compression;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>The committed save the harness stages.</summary>
    public class TemplateSaveTests
    {
        /// <summary>
        /// Every entry in a save is prefixed with the save's own filename, and the game
        /// needs them to agree.
        /// </summary>
        /// <remarks>
        /// Renaming the archive to save_template.ntwtf.zip while its entries still said
        /// MARTINAISE produced a main menu with no Continue and Load Game greyed out: the
        /// game had found no saves at all. Nothing failed, nothing was logged, and the
        /// only symptom was a menu that no longer matched its reference.
        /// </remarks>
        [Fact]
        public void TheArchivesEntriesMatchItsFileName()
        {
            string save = FindTemplateSave();
            string expected = Path.GetFileName(save);
            expected = expected.Substring(0, expected.Length - GameSaves.SaveExtension.Length);

            using ZipArchive archive = ZipFile.OpenRead(save);

            Assert.NotEmpty(archive.Entries);
            foreach (ZipArchiveEntry entry in archive.Entries)
            {
                Assert.StartsWith(expected + ".", entry.FullName, StringComparison.Ordinal);
            }
        }

        /// <summary>The thumbnail is named for the save, or the slot shows blank.</summary>
        [Fact]
        public void TheThumbnailIsNamedForTheSave()
        {
            string save = FindTemplateSave();
            string thumbnail =
                save.Substring(0, save.Length - GameSaves.SaveExtension.Length) + ".jpg";

            Assert.True(File.Exists(thumbnail), $"expected a thumbnail at {thumbnail}");
        }

        /// <summary>It has to be a save, holding the parts a save holds.</summary>
        [Fact]
        public void TheSaveHoldsItsParts()
        {
            using ZipArchive archive = ZipFile.OpenRead(FindTemplateSave());

            Assert.Contains(archive.Entries, e => e.FullName.EndsWith(".ntwtf.lua", StringComparison.Ordinal));
            Assert.Contains(archive.Entries, e => e.FullName.EndsWith(".states.lua", StringComparison.Ordinal));
        }

        private static string FindTemplateSave()
        {
            var directory = new DirectoryInfo(AppDomain.CurrentDomain.BaseDirectory);
            while (directory != null)
            {
                string testing = Path.Combine(directory.FullName, "testing");
                if (Directory.Exists(testing))
                {
                    string[] saves = GameSaves.ListSaves(testing);
                    if (saves.Length == 1)
                    {
                        return saves[0];
                    }

                    if (saves.Length > 1)
                    {
                        throw new InvalidOperationException(
                            $"{testing} holds {saves.Length} saves; the harness stages one.");
                    }
                }

                directory = directory.Parent;
            }

            throw new FileNotFoundException("Could not find a template save under testing/.");
        }
    }
}
