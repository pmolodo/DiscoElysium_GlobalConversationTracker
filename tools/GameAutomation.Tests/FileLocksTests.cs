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
        /// Describe never throws: it runs inside an error path, and an exception there
        /// would replace the real failure with its own.
        /// </summary>
        [Fact]
        public void DescribeSurvivesAPathThatIsNotThere()
        {
            string missing = Path.Combine(_root, "no-such-thing");

            string description = FileLocks.Describe(missing, askHandleTool: false);

            Assert.False(string.IsNullOrWhiteSpace(description));
        }

        /// <summary>
        /// The message for "found nothing" must not read as "nothing is holding it".
        /// A folder open in Explorer holds no file handle and shows up in no list.
        /// </summary>
        [Fact]
        public void FindingNothingSaysWhatThatDoesAndDoesNotMean()
        {
            // askHandleTool off: it shells out to a system-wide handle scan that takes
            // tens of seconds, which is right in an error path and wrong in a unit test.
            string description = FileLocks.Describe(_root, askHandleTool: false);

            // The wording is free to change; what must not is that it says the folder
            // itself may be held and that finding no file handle is not an all-clear.
            Assert.Contains("FOLDER", description, StringComparison.Ordinal);
            Assert.Contains("blocks a move", description, StringComparison.Ordinal);
        }
    }
}
