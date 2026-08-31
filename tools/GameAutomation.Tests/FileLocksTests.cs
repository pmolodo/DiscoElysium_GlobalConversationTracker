// SPDX-License-Identifier: MIT
using System;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// Naming the process that is holding a file, when Windows will only say "another
    /// process".
    /// </summary>
    public class FileLocksTests : IDisposable
    {
        private readonly string _root;

        public FileLocksTests()
        {
            _root = Path.Combine(Path.GetTempPath(), "gct-locks-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(_root);
        }

        public void Dispose()
        {
            if (Directory.Exists(_root))
            {
                Directory.Delete(_root, recursive: true);
            }
        }

        /// <summary>
        /// The test process holds the file, so it must be the answer. Anything less and
        /// this reports "nothing found" for every real lock too.
        /// </summary>
        [Fact]
        public void AHeldFileNamesTheProcessHoldingIt()
        {
            string file = Path.Combine(_root, "held.txt");
            File.WriteAllText(file, "content");

            using var held = new FileStream(file, FileMode.Open, FileAccess.ReadWrite, FileShare.None);

            LockHolder[] holders = FileLocks.WhoIsUsing(file);

            Assert.NotEmpty(holders);
            int mine = System.Diagnostics.Process.GetCurrentProcess().Id;
            Assert.Contains(holders, h => h.ProcessId == mine);
        }

        /// <summary>A file inside a folder is found when the folder is asked about.</summary>
        [Fact]
        public void AHeldFileIsFoundThroughItsFolder()
        {
            string file = Path.Combine(_root, "inside.txt");
            File.WriteAllText(file, "content");

            using var held = new FileStream(file, FileMode.Open, FileAccess.ReadWrite, FileShare.None);

            LockHolder[] holders = FileLocks.WhoIsUsing(_root);

            int mine = System.Diagnostics.Process.GetCurrentProcess().Id;
            Assert.Contains(holders, h => h.ProcessId == mine);
        }

        [Fact]
        public void AnUnheldFileHasNoHolders()
        {
            string file = Path.Combine(_root, "free.txt");
            File.WriteAllText(file, "content");

            Assert.Empty(FileLocks.WhoIsUsing(file));
        }

        /// <summary>
        /// Describe never throws: it runs inside an error path, and an exception there
        /// would replace the real failure with its own.
        /// </summary>
        [Fact]
        public void DescribeSurvivesAPathThatIsNotThere()
        {
            string missing = Path.Combine(_root, "no-such-thing");

            string description = FileLocks.Describe(missing);

            Assert.False(string.IsNullOrWhiteSpace(description));
        }

        /// <summary>
        /// The message for "found nothing" must not read as "nothing is holding it".
        /// A folder open in Explorer holds no file handle and shows up in no list.
        /// </summary>
        [Fact]
        public void FindingNothingSaysWhatThatDoesAndDoesNotMean()
        {
            string description = FileLocks.Describe(_root);

            Assert.Contains("FOLDER open", description, StringComparison.Ordinal);
        }
    }
}
