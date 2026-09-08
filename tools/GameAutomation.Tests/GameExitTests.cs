// SPDX-License-Identifier: MIT
using System.Diagnostics;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// Telling a game that was closed from a game that crashed.
    /// </summary>
    /// <remarks>
    /// The distinction the whole class exists for. A run that finds the game gone used to
    /// say only that, and the two stories behind it - something asked it to close, or its
    /// own code took it down - want opposite investigations.
    /// </remarks>
    public class GameExitTests
    {
        [Fact]
        public void AnOrdinaryCloseIsNamedAsOne()
        {
            string said = GameExit.Explain(GameExit.Clean);

            Assert.Contains("0", said);
            Assert.Contains("ordinary close", said);
            Assert.DoesNotContain("the game crashed", said);
        }

        [Fact]
        public void AnAccessViolationIsNamedACrash()
        {
            // What a NullReferenceException escaping into native code leaves, and what
            // the Player.log of the run in de-wncd.4 ended on. In decimal it is
            // -1073741819, which nobody recognises.
            string said = GameExit.Explain(unchecked((int)0xC0000005));

            Assert.Contains("-1073741819", said);
            Assert.Contains("0xC0000005", said);
            Assert.Contains("crashed", said);
        }

        [Fact]
        public void AKilledProcessIsNotReportedAsACrash()
        {
            // Process.Kill asks TerminateProcess for -1, so the harness's own hard close
            // lands here. Reading that as a crash would send the reader to a Player.log
            // that says nothing.
            string said = GameExit.Explain(-1);

            Assert.Contains("killing", said);
            Assert.DoesNotContain("the game crashed", said);
        }

        [Fact]
        public void AnUnrecognisedCodeStillGivesTheNumberAndSaysItIsNotAClose()
        {
            string said = GameExit.Explain(3);

            Assert.Contains("3", said);
            Assert.Contains("0x00000003", said);
            Assert.Contains("not an ordinary close", said);
        }

        [Fact]
        public void AProcessThatHasNotBeenStartedIsNotGuessedAt()
        {
            // The one thing worse than not knowing how the game ended is inventing it,
            // since this is only ever read while diagnosing a failure.
            Assert.Equal(GameExit.Unknown, GameExit.Describe(null));
            using var never = new Process();
            Assert.Equal(GameExit.Unknown, GameExit.Describe(never));
        }

        [Fact]
        public void ARunningProcessIsSaidToBeRunning()
        {
            using Process child = Process.Start(new ProcessStartInfo(
                "cmd.exe", "/c ping -n 30 127.0.0.1 > nul")
            {
                UseShellExecute = false,
                CreateNoWindow = true,
            })!;

            try
            {
                Assert.Equal("it is still running", GameExit.Describe(child));
            }
            finally
            {
                child.Kill();
                child.WaitForExit(10_000);
            }
        }

        [Fact]
        public void AProcessThatExitedCleanlyIsDescribedFromItsOwnCode()
        {
            using Process child = Process.Start(new ProcessStartInfo("cmd.exe", "/c exit 0")
            {
                UseShellExecute = false,
                CreateNoWindow = true,
            })!;

            child.WaitForExit(10_000);

            string said = GameExit.Describe(child);
            Assert.Contains("exited with code 0", said);
            Assert.Contains("ordinary close", said);
        }
    }
}
