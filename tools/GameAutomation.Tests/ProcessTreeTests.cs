// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using Xunit;
using Xunit.Abstractions;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// Walking from a windowless process up to the one that owns the window.
    /// </summary>
    public class ProcessTreeTests
    {
        private readonly ITestOutputHelper _output;

        public ProcessTreeTests(ITestOutputHelper output)
        {
            _output = output;
        }

        /// <summary>A child started here must report this process as its parent.</summary>
        [Fact]
        public void TheParentOfAChildIsThisProcess()
        {
            using Process child = Process.Start(new ProcessStartInfo(
                "cmd.exe", "/c ping -n 20 127.0.0.1 > nul")
            {
                UseShellExecute = false,
                CreateNoWindow = true,
            })!;

            try
            {
                Assert.Equal(
                    Process.GetCurrentProcess().Id, ProcessTree.ParentOf(child.Id));
            }
            finally
            {
                try
                {
                    child.Kill();
                }
                catch (Exception)
                {
                    // Already gone.
                }
            }
        }

        [Fact]
        public void AProcessThatDoesNotExistHasNoParent()
        {
            Assert.Equal(0, ProcessTree.ParentOf(int.MaxValue));
        }

        /// <summary>
        /// Nothing is returned when no ancestor is acceptable, however many have windows.
        /// Walking up must not arrive somewhere the caller never agreed to touch.
        /// </summary>
        [Fact]
        public void AnUnacceptableAncestorStopsTheWalk()
        {
            using Process child = Process.Start(new ProcessStartInfo(
                "cmd.exe", "/c ping -n 20 127.0.0.1 > nul")
            {
                UseShellExecute = false,
                CreateNoWindow = true,
            })!;

            try
            {
                Assert.Equal(0, ProcessTree.NearestWithWindow(child.Id, _ => false));
            }
            finally
            {
                try
                {
                    child.Kill();
                }
                catch (Exception)
                {
                    // Already gone.
                }
            }
        }

        [Fact]
        public void ANullPredicateIsRefused()
        {
            Assert.Throws<ArgumentNullException>(
                () => ProcessTree.NearestWithWindow(Process.GetCurrentProcess().Id, null!));
        }

        /// <summary>The bound exists because recycled ids can make the chain appear to loop.</summary>
        [Fact]
        public void ThereIsADepthBound()
        {
            Assert.InRange(ProcessTree.MaxDepth, 1, 32);
        }

        /// <summary>
        /// Explorer always has a window, so it is a reliable subject for the windowed
        /// lookup on any desktop this runs on.
        /// </summary>
        [Fact]
        public void ProcessesWithWindowsAreFound()
        {
            int[] found = ProcessTree.WithWindowsNamed("explorer");

            foreach (int id in found)
            {
                _output.WriteLine($"explorer with a window: {id}");
            }

            // Not asserted as non-empty: a machine running this headless has no shell.
            foreach (int id in found)
            {
                using Process process = Process.GetProcessById(id);
                Assert.NotEqual(IntPtr.Zero, process.MainWindowHandle);
            }
        }

        [Fact]
        public void NothingNamedThatMeansNoWindows()
        {
            Assert.Empty(ProcessTree.WithWindowsNamed("no-such-process-anywhere"));
        }
    }
}
