// SPDX-License-Identifier: MIT
using System;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>Finding the folder an executable lives in.</summary>
    public class FilePathsTests
    {
        [Fact]
        public void AnAbsolutePathGivesUpItsFolder()
        {
            Assert.Equal(
                @"C:\games\Disco Elysium",
                FilePaths.FolderOf(@"C:\games\Disco Elysium\disco.exe", "game"));
        }

        [Fact]
        public void ABareFilenameIsRefused()
        {
            // The failure this prevents is not the crash: it is Path.GetDirectoryName
            // answering empty, which turns "that is not an install path" into "nothing
            // is installed there".
            ArgumentException error = Assert.Throws<ArgumentException>(
                () => FilePaths.FolderOf("disco.exe", "game"));

            Assert.Equal("game", error.ParamName);
            Assert.Contains("disco.exe", error.Message);
        }

        [Fact]
        public void ADriveRootIsRefused()
        {
            Assert.Throws<ArgumentException>(() => FilePaths.FolderOf(@"C:\", "game"));
        }

        [Fact]
        public void NothingIsRefused()
        {
            Assert.Throws<ArgumentException>(() => FilePaths.FolderOf(string.Empty, "game"));
        }
    }
}
