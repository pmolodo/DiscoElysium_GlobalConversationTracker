// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using GlobalConversationTracker.Engine;
using Xunit;
using Xunit.Abstractions;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// Does the engine process start, stop, and stay stopped?
    /// </summary>
    /// <remarks>
    /// <para>The questions that only exist because the engine is a PROCESS now - de-bnjy.1.
    /// <see cref="BridgeTests"/> asks whether the answers are right; this asks whether the
    /// thing that produced them is where it should be, and gone when it should be.</para>
    ///
    /// <para>WHAT IS NOT TESTED HERE, and cannot be from inside the parent: that a child is
    /// killed when the parent is killed rather than closed. That is what the job object in
    /// <see cref="ProcessJob"/> buys, and proving it needs a parent that dies - which a test
    /// cannot be and survive to assert. What is tested is the half a test can see: that a
    /// child ends when it is asked to, and that a host whose child is gone says so instead
    /// of waiting.</para>
    /// </remarks>
    public class EngineProcessTests
    {
        private readonly ITestOutputHelper _output;

        public EngineProcessTests(ITestOutputHelper output)
        {
            _output = output;
            NativeLookAhead.Install();
        }

        /// <summary>Disposing the engine ends the process, rather than leaving it.</summary>
        /// <remarks>
        /// The ordinary ending, and the one that happens hundreds of times in this test run
        /// alone. An engine left behind per open would be a machine full of them by the end
        /// of an afternoon.
        /// </remarks>
        [Fact]
        public void ClosingTheEngineEndsItsProcess()
        {
            string? index = NativeLookAhead.Index;
            if (NativeLookAhead.Engine == null || index == null)
            {
                _output.WriteLine("the engine or the index is missing; skipping.");
                return;
            }

            int id;
            using (LookAheadLibrary engine = LookAheadLibrary.Open(index))
            {
                id = engine.ProcessId;
                Assert.True(id > 0, "an opened engine should name its process");
                _output.WriteLine($"the engine is process {id}");

                // Alive, and it is the one this test started: asked something, and it
                // answered.
                Assert.True(engine.ConversationCount > 0);
            }

            Assert.True(HasEnded(id), $"process {id} was still running after Dispose");
        }

        /// <summary>
        /// A closed engine refuses the next question rather than blocking on a dead pipe.
        /// </summary>
        /// <remarks>
        /// The failure this replaces would have been a hang, which in the game is a frozen
        /// frame with no log line - the exact shape of failure that moving out of process
        /// is meant to abolish.
        /// </remarks>
        [Fact]
        public void AskingAClosedEngineIsAnErrorAndNotAWait()
        {
            string? index = NativeLookAhead.Index;
            if (NativeLookAhead.Engine == null || index == null)
            {
                _output.WriteLine("the engine or the index is missing; skipping.");
                return;
            }

            LookAheadLibrary engine = LookAheadLibrary.Open(index);
            engine.Dispose();

            InvalidOperationException refused = Assert.Throws<InvalidOperationException>(
                () => engine.EntryCount(631));
            _output.WriteLine(refused.Message);
        }

        /// <summary>Disposing twice is not an error, and does not need a live child.</summary>
        /// <remarks>
        /// A <c>using</c> around an engine that something else already closed is an
        /// ordinary shape, and the second close must be as quiet as the first.
        /// </remarks>
        [Fact]
        public void ClosingTwiceIsHarmless()
        {
            string? index = NativeLookAhead.Index;
            if (NativeLookAhead.Engine == null || index == null)
            {
                _output.WriteLine("the engine or the index is missing; skipping.");
                return;
            }

            LookAheadLibrary engine = LookAheadLibrary.Open(index);
            engine.Dispose();
            engine.Dispose();
        }

        /// <summary>
        /// The deadline is a knob with a value, and the default is the documented one.
        /// </summary>
        /// <remarks>
        /// Thin, deliberately. What it guards is a default quietly changing to something
        /// that would fire on a legitimate slow search - the deadline exists for a child
        /// that will never answer, and a value near a real search's cost would turn a slow
        /// answer into a killed engine.
        /// </remarks>
        [Fact]
        public void TheDeadlineIsGenerousByDefault()
        {
            Assert.Equal(30_000, LookAheadLibrary.DeadlineMs);
        }

        /// <summary>Whether a process id is no longer running, allowing it a moment.</summary>
        /// <remarks>
        /// Dispose waits for the child itself, so this should be true immediately; the
        /// retry is for the case where the operating system has not yet reaped it, which
        /// would otherwise be a test that fails once a fortnight.
        /// </remarks>
        private static bool HasEnded(int id)
        {
            for (int attempt = 0; attempt < 20; attempt++)
            {
                try
                {
                    using Process found = Process.GetProcessById(id);
                    if (found.HasExited)
                    {
                        return true;
                    }
                }
                catch (ArgumentException)
                {
                    // No such process, which is the answer this is looking for.
                    return true;
                }

                System.Threading.Thread.Sleep(50);
            }

            return false;
        }
    }
}
