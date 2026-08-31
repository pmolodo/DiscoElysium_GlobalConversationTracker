// SPDX-License-Identifier: MIT
using System;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>Reading Sysinternals Handle's output.</summary>
    /// <remarks>
    /// The parsing is tested against captured output rather than by running Handle, so
    /// these pass on a machine that does not have it installed. Whether it is installed is
    /// a separate question, answered by Find.
    /// </remarks>
    public class SysinternalsHandleTests
    {
        private const string Target = @"C:\Users\someone\AppData\LocalLow\ZAUM Studio\Disco Elysium";

        /// <summary>Real search-mode output, with the columns Handle prints.</summary>
        private const string Sample =
            "explorer.exe       pid: 8256   type: File           1A4: "
            + @"C:\Users\someone\AppData\LocalLow\ZAUM Studio\Disco Elysium" + "\r\n"
            + "Code.exe           pid: 1932   type: File           2C0: "
            + @"C:\Users\someone\AppData\LocalLow\ZAUM Studio\Disco Elysium\SaveGames" + "\r\n";

        [Fact]
        public void AProcessHoldingTheFolderIsFound()
        {
            LockHolder[] holders = SysinternalsHandle.Parse(Sample, Target);

            Assert.Equal(2, holders.Length);
            Assert.Contains(holders, h => h.ProcessId == 8256 && h.Name == "explorer.exe");
            Assert.Contains(holders, h => h.ProcessId == 1932 && h.Name == "Code.exe");
        }

        /// <summary>
        /// Handle matches a FRAGMENT anywhere in a path, so it reports siblings whose
        /// names merely begin the same way. Those are not holding the folder.
        /// </summary>
        [Fact]
        public void ASiblingWithASimilarNameIsRejected()
        {
            string sample =
                "notepad.exe        pid: 4444   type: File           1A4: "
                + Target + " Backup\notes.txt\r\n";

            Assert.Empty(SysinternalsHandle.Parse(sample, Target));
        }

        [Fact]
        public void OneProcessIsReportedOnceHoweverManyHandlesItHolds()
        {
            string sample = Sample + Sample;

            Assert.Equal(2, SysinternalsHandle.Parse(sample, Target).Length);
        }

        [Fact]
        public void NoMatchesReadsAsNoHolders()
        {
            Assert.Empty(SysinternalsHandle.Parse(
                "No matching handles found.\r\n", Target));
            Assert.Empty(SysinternalsHandle.Parse(string.Empty, Target));
        }

        /// <summary>
        /// Absent is a null answer, not an empty one: "no answer available" and "nothing
        /// is holding it" must not look the same, or a missing tool reads as an all-clear.
        /// </summary>
        [Fact]
        public void NotInstalledIsDistinctFromNothingFound()
        {
            string missing = Path.Combine(Path.GetTempPath(), "no-such-handle-tool.exe");

            Assert.Null(SysinternalsHandle.WhoIsHolding(Target, missing));
            Assert.Empty(SysinternalsHandle.Parse(string.Empty, Target));
        }

        [Fact]
        public void TheInstallHintNamesTheToolAndTheNeedForAdmin()
        {
            Assert.Contains("Handle", SysinternalsHandle.InstallHint, StringComparison.Ordinal);
            Assert.Contains("administrator", SysinternalsHandle.InstallHint, StringComparison.Ordinal);
        }
    }
}
