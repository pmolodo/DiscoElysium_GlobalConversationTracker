// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using GlobalConversationTracker.Automation;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// The decisions a raiser makes, without a desktop to make them against.
    /// </summary>
    public class ForegroundRaiserTests
    {
        private static readonly IntPtr Window = new IntPtr(1);

        /// <summary>A window whose foreground state is whatever the test says.</summary>
        private sealed class FakeDesktop
        {
            private readonly Queue<bool> _foreground = new Queue<bool>();

            public FakeDesktop(params bool[] foreground)
            {
                foreach (bool state in foreground)
                {
                    _foreground.Enqueue(state);
                }
            }

            /// <summary>What the last queued answer was, once the queue runs dry.</summary>
            public bool Last { get; private set; }

            public int Raises { get; private set; }

            public List<TimeSpan> Waits { get; } = new List<TimeSpan>();

            public bool IsForeground(IntPtr window)
            {
                if (_foreground.Count > 0)
                {
                    Last = _foreground.Dequeue();
                }

                return Last;
            }

            public bool BringToFront(IntPtr window)
            {
                Raises++;
                return true;
            }

            public void Wait(TimeSpan span) => Waits.Add(span);

            /// <summary>A clock the test moves by hand.</summary>
            public DateTime Now { get; set; } = new DateTime(2026, 1, 1);

            public ForegroundRaiser Raiser(
                int maxAttempts = ForegroundRaiser.DefaultMaxAttempts,
                TimeSpan? retryAfter = null) =>
                new ForegroundRaiser(
                    IsForeground,
                    BringToFront,
                    Wait,
                    maxAttempts,
                    settle: null,
                    now: () => Now,
                    retryAfter: retryAfter);
        }

        [Fact]
        public void AWindowAlreadyInFrontIsNotRaisedAtAll()
        {
            // The common case, and it must not spend an attempt: a run that raises on
            // every poll would use up its budget on a game that was never covered.
            var desktop = new FakeDesktop(true);
            ForegroundRaiser raiser = desktop.Raiser();

            Assert.True(raiser.Ensure(Window));
            Assert.Equal(0, desktop.Raises);
            Assert.Equal(0, raiser.Attempts);
        }

        [Fact]
        public void ARaiseThatWorksIsReportedAsSuccess()
        {
            // Not in front, then in front after the raise settles.
            var desktop = new FakeDesktop(false, true);
            ForegroundRaiser raiser = desktop.Raiser();

            Assert.True(raiser.Ensure(Window));
            Assert.Equal(1, desktop.Raises);
            Assert.Equal(new[] { ForegroundRaiser.DefaultSettle }, desktop.Waits);
        }

        [Fact]
        public void RaisingStopsAfterTheAllowedNumberOfAttempts()
        {
            var desktop = new FakeDesktop(false);
            ForegroundRaiser raiser = desktop.Raiser(maxAttempts: 4);

            for (int i = 0; i < 10; i++)
            {
                Assert.False(raiser.Ensure(Window));
            }

            Assert.Equal(4, desktop.Raises);
            Assert.True(raiser.GaveUp);
        }

        [Fact]
        public void ANewScreenGetsItsOwnAttempts()
        {
            var desktop = new FakeDesktop(false);
            ForegroundRaiser raiser = desktop.Raiser(maxAttempts: 2);

            raiser.Ensure(Window);
            raiser.Ensure(Window);
            Assert.True(raiser.GaveUp);

            raiser.Reset();

            Assert.False(raiser.GaveUp);
            Assert.Equal(0, raiser.Attempts);
            raiser.Ensure(Window);
            Assert.Equal(3, desktop.Raises);
        }

        [Fact]
        public void GivingUpStillAnswersHonestlyIfTheWindowComesForward()
        {
            // Somebody moved it themselves. Having stopped trying is not a reason to
            // report a window that IS in front as one that is not.
            var desktop = new FakeDesktop(false, false, true);
            ForegroundRaiser raiser = desktop.Raiser(maxAttempts: 1);

            Assert.False(raiser.Ensure(Window));
            Assert.True(raiser.GaveUp);
            Assert.True(raiser.Ensure(Window));
            Assert.Equal(1, desktop.Raises);
        }

        [Fact]
        public void ZeroAttemptsMeansNeverRaisingButStillChecking()
        {
            var desktop = new FakeDesktop(false);
            ForegroundRaiser raiser = desktop.Raiser(maxAttempts: 0);

            Assert.False(raiser.Ensure(Window));
            Assert.Equal(0, desktop.Raises);
        }

        [Fact]
        public void ANegativeAllowanceIsRefused()
        {
            var desktop = new FakeDesktop(true);

            Assert.Throws<ArgumentOutOfRangeException>(() => desktop.Raiser(maxAttempts: -1));
        }

        /// <remarks>
        /// The deadlock this prevents. A caller whose only way to see the next screen is
        /// to be in front of it cannot report a screen change, so "resets when the screen
        /// changes" never fires - and during the game's first seconds no raise succeeds
        /// at all. Four attempts spent there used to be the end of it, and a run then sat
        /// out its entire timeout against a game that came forward at second ten.
        /// </remarks>
        [Fact]
        public void TheAllowanceComesBackAfterAWhile()
        {
            var desktop = new FakeDesktop(false);
            ForegroundRaiser raiser = desktop.Raiser(
                maxAttempts: 2, retryAfter: TimeSpan.FromSeconds(5));

            raiser.Ensure(Window);
            raiser.Ensure(Window);
            Assert.True(raiser.GaveUp);

            // Still inside the cooldown: no further raising.
            desktop.Now = desktop.Now.AddSeconds(4);
            Assert.False(raiser.Ensure(Window));
            Assert.Equal(2, desktop.Raises);

            // Past it: it tries again.
            desktop.Now = desktop.Now.AddSeconds(2);
            Assert.False(raiser.Ensure(Window));
            Assert.Equal(3, desktop.Raises);
        }

        [Fact]
        public void TheDescriptionSaysWhetherItIsStillTrying()
        {
            var desktop = new FakeDesktop(false);
            ForegroundRaiser raiser = desktop.Raiser(maxAttempts: 1);

            raiser.Ensure(Window);

            Assert.Contains("trying again in", raiser.Describe());
            raiser.Reset();
            Assert.Contains("raising it", raiser.Describe());
        }
    }
}
