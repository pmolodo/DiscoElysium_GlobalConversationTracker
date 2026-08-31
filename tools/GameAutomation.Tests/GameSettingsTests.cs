// SPDX-License-Identifier: MIT
using System;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// What the shipped test settings say, and the keyboard table.
    /// </summary>
    /// <remarks>
    /// Staging the settings file is GameProfile's job now - the file moves with the rest
    /// of the profile - and is tested there. Reading display values out of a file needs no
    /// installation at all, so nothing here touches a real one.
    /// </remarks>
    public class GameSettingsTests
    {

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
        [Fact]
        public void AnUnknownKeyIsRefusedRatherThanIgnored()
        {
            Assert.False(GameKeyboard.IsKnown("NoSuchKey"));
            Assert.Throws<ArgumentException>(() => GameKeyboard.Press("NoSuchKey"));
            Assert.Throws<ArgumentException>(() => GameKeyboard.Release("NoSuchKey"));
        }

    }
}
