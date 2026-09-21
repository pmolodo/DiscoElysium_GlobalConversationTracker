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
            using (LookAheadLibrary engine = LookAheadLibrary.Open(index, NativeLookAhead.Declared))
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

            LookAheadLibrary engine = LookAheadLibrary.Open(index, NativeLookAhead.Declared);
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

            LookAheadLibrary engine = LookAheadLibrary.Open(index, NativeLookAhead.Declared);
            engine.Dispose();
            engine.Dispose();
        }

        /// <summary>
        /// An engine killed under the mod's feet is reported as a death, not as a hiccup.
        /// </summary>
        /// <remarks>
        /// The case de-bnjy.1.2 turns on. A call that could not be served comes back as an
        /// error inside a response and leaves the engine standing; this is the engine
        /// itself going, and the mod has to be able to tell the two apart because only one
        /// of them ends the feature for the session.
        ///
        /// Killed from outside rather than provoked from within, which is the honest
        /// simulation: whatever actually kills an engine - an abort, the machine, a player
        /// with a task manager - reaches this side as a pipe that stopped.
        /// </remarks>
        [Fact]
        public void AnEngineKilledUnderneathIsReportedAsADeath()
        {
            string? index = NativeLookAhead.Index;
            if (NativeLookAhead.Engine == null || index == null)
            {
                _output.WriteLine("the engine or the index is missing; skipping.");
                return;
            }

            using LookAheadLibrary engine = LookAheadLibrary.Open(index, NativeLookAhead.Declared);
            using (Process child = Process.GetProcessById(engine.ProcessId))
            {
                child.Kill();
                child.WaitForExit();
            }

            EngineDiedException died = Assert.Throws<EngineDiedException>(
                () => engine.EntryCount(631));
            _output.WriteLine($"{died.Death}: {died.Message}");

            // Crashed rather than OutOfMemory, because the engine said nothing about an
            // allocation - which is the point of the default: a reason is claimed only on
            // the engine's own words.
            Assert.Equal(EngineDeath.Crashed, died.Death);

            // AND IT KEEPS SAYING SO. A second ask must not degrade into "the pipe is
            // closed", which would be a worse account of the same event - and the mod asks
            // again, because a menu is drawn again.
            EngineDiedException again = Assert.Throws<EngineDiedException>(
                () => engine.ConversationCount);
            Assert.Equal(died.Death, again.Death);
            Assert.Equal(died.Message, again.Message);
        }

        /// <summary>A killed engine is reported at once, not at the deadline.</summary>
        /// <remarks>
        /// <para>de-wncd.4. The deadline is thirty seconds because a child that is still
        /// thinking and one that will never answer look the same to a blocking read - but a
        /// child that has EXITED is not thinking, and the thing waiting on that read is a
        /// response menu being drawn. Measured in game 2026-09-07: the same suite drew its
        /// menu instantly on one run and reported `advance-to-menu after 68.5s` on the
        /// next.</para>
        ///
        /// <para>WHAT THIS DOES AND DOES NOT PROVE, because the difference matters. The
        /// common path is that killing the child breaks the pipe and the read fails
        /// immediately, and this test takes that path most of the time - so a pass is not
        /// by itself evidence that the exit check works. What it guards is the REGRESSION:
        /// with the deadline set far above any patience a person has, a killed engine must
        /// still be reported in moments. If the exit check were removed, the run that
        /// happens to block would take a minute here and fail.</para>
        ///
        /// <para>Provoking the blocking path deterministically would need a child that
        /// holds its pipe open and stops answering, which is a fake engine rather than this
        /// one; that is why this is a bound rather than a demonstration.</para>
        /// </remarks>
        [Fact]
        public void AKilledEngineIsReportedWithoutWaitingOutTheDeadline()
        {
            string? index = NativeLookAhead.Index;
            if (NativeLookAhead.Engine == null || index == null)
            {
                _output.WriteLine("the engine or the index is missing; skipping.");
                return;
            }

            int wasDeadline = LookAheadLibrary.DeadlineMs;
            LookAheadLibrary.DeadlineMs = PatienceMs;
            try
            {
                using LookAheadLibrary engine = LookAheadLibrary.Open(index, NativeLookAhead.Declared);
                using (Process child = Process.GetProcessById(engine.ProcessId))
                {
                    child.Kill();
                    child.WaitForExit();
                }

                var clock = Stopwatch.StartNew();
                Assert.Throws<EngineDiedException>(() => engine.EntryCount(631));
                clock.Stop();

                _output.WriteLine($"the death was reported after {clock.ElapsedMilliseconds} ms");
                Assert.True(
                    clock.ElapsedMilliseconds < ReportedWithinMs,
                    $"a killed engine took {clock.ElapsedMilliseconds} ms to be reported, "
                    + $"against a deadline of {PatienceMs} ms. A menu waits on this.");
            }
            finally
            {
                LookAheadLibrary.DeadlineMs = wasDeadline;
            }
        }

        /// <summary>A deadline nobody would sit through, so the bound below means something.</summary>
        private const int PatienceMs = 60_000;

        /// <summary>What "at once" is allowed to mean, generously.</summary>
        /// <remarks>
        /// Far above the exit check's own interval and drain - a tenth of a second and a
        /// quarter - and far below the deadline, so the test says which of the two happened
        /// without turning on how busy the machine is.
        /// </remarks>
        private const int ReportedWithinMs = 5_000;

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
