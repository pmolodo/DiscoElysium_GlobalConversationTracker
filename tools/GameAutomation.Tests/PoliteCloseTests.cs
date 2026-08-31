// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>Asking a process to close, and accepting a refusal.</summary>
    public class PoliteCloseTests
    {
        private static readonly string[] Askable = { "Code" };

        [Theory]
        [InlineData("Code")]
        [InlineData("Code.exe")]
        [InlineData("code.exe")]
        [InlineData("Code - Insiders")]
        public void TheEditorIsAskable(string name)
        {
            Assert.True(PoliteClose.IsAskable(name, Askable));
        }

        /// <summary>
        /// Only what is named may be asked. Everything else holding the folder is reported
        /// and left running - this must never widen into "close whatever is in the way".
        /// </summary>
        [Theory]
        [InlineData("explorer.exe")]
        [InlineData("chrome.exe")]
        [InlineData("disco.exe")]
        [InlineData("csrss.exe")]
        public void EverythingElseIsNot(string name)
        {
            Assert.False(PoliteClose.IsAskable(name, Askable));
        }

        /// <summary>A prefix, not a substring: VSCodium is not VS Code.</summary>
        [Fact]
        public void MatchingIsAPrefixNotASubstring()
        {
            Assert.False(PoliteClose.IsAskable("VSCodium", Askable));
        }

        /// <summary>
        /// A name no real process has. These tests must never reach the fallback that
        /// asks every window of an application to close - on a developer's machine that
        /// would be their actual editor.
        /// </summary>
        private static readonly string[] Imaginary = { "no-such-editor" };

        [Fact]
        public void AProcessThatIsGoneIsNotAskedAnything()
        {
            // A pid that cannot exist, standing in for one that exited between being seen
            // holding the folder and being asked about it.
            var holder = new LockHolder(int.MaxValue, "no-such-editor.exe", string.Empty);

            CloseAttempt[] attempts = PoliteClose.AskToClose(
                new[] { holder }, Imaginary, TimeSpan.FromSeconds(1));

            // Nothing to ask: it has no window, no ancestor to walk to, and nothing of
            // that name is running.
            Assert.Empty(attempts);
        }

        [Fact]
        public void NothingAskableMeansNothingAttempted()
        {
            var holders = new[]
            {
                new LockHolder(1234, "explorer.exe", string.Empty),
                new LockHolder(5678, "chrome.exe", string.Empty),
            };

            Assert.Empty(PoliteClose.AskToClose(holders, Askable, TimeSpan.FromSeconds(1)));
        }

        /// <summary>
        /// A windowless process with nothing above it to ask is left running.
        /// </summary>
        /// <remarks>
        /// The holder of a folder is usually a windowless helper, and the walk up the
        /// process tree is what finds something that can be asked. When there is nothing -
        /// no window, no acceptable ancestor, nothing of that name running - the answer is
        /// to do nothing, never to kill the process that was found.
        /// </remarks>
        [Fact]
        public void AWindowlessProcessWithNoWindowedParentIsLeftRunning()
        {
            using Process helper = Process.Start(new ProcessStartInfo(
                "cmd.exe", "/c ping -n 30 127.0.0.1 > nul")
            {
                UseShellExecute = false,
                CreateNoWindow = true,
            })!;

            try
            {
                var holder = new LockHolder(helper.Id, "no-such-editor.exe", string.Empty);

                CloseAttempt[] attempts = PoliteClose.AskToClose(
                    new[] { holder }, Imaginary, TimeSpan.FromSeconds(2));

                Assert.Empty(attempts);

                // The point: it is still running.
                Assert.False(helper.HasExited);
            }
            finally
            {
                try
                {
                    helper.Kill();
                }
                catch (Exception)
                {
                    // Already gone.
                }
            }
        }

        [Fact]
        public void NullArgumentsAreRefused()
        {
            Assert.Throws<ArgumentNullException>(
                () => PoliteClose.AskToClose(null!, Askable));
            Assert.Throws<ArgumentNullException>(
                () => PoliteClose.AskToClose(Enumerable.Empty<LockHolder>(), null!));
        }

        /// <summary>The deadline exists so an unattended run is not held up forever.</summary>
        [Fact]
        public void ThereIsADeadlineAndItIsNotAbsurd()
        {
            Assert.InRange(PoliteClose.DefaultDeadline.TotalSeconds, 1, 60);
        }
    }
}
