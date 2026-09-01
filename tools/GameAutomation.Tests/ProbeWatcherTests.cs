// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Text.Json;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>Waiting for the in-game probe to report something.</summary>
    public class ProbeWatcherTests
    {
        private static readonly TimeSpan Instant = TimeSpan.FromMilliseconds(1);
        private static readonly TimeSpan Poll = TimeSpan.FromMilliseconds(1);

        private static ProbeEvent Event(string json)
        {
            using JsonDocument document = JsonDocument.Parse(json);
            return new ProbeEvent(document.RootElement.Clone());
        }

        private static ProbeEvent Named(string name)
        {
            return Event($"{{\"event\":\"{name}\"}}");
        }

        private static ProbeEvent Menu(int conversation)
        {
            return Event($"{{\"event\":\"menu\",\"conversation\":{conversation}}}");
        }

        /// <summary>A log that grows one step per read, as a running game's would.</summary>
        private sealed class GrowingLog
        {
            private readonly List<ProbeEvent[]> _steps = new List<ProbeEvent[]>();
            private int _read;

            public GrowingLog(params ProbeEvent[][] steps)
            {
                _steps.AddRange(steps);
            }

            public int Reads => _read;

            public ProbeEvent[] Next()
            {
                ProbeEvent[] step = _steps[Math.Min(_read, _steps.Count - 1)];
                _read++;
                return step;
            }
        }

        [Fact]
        public void AnEventAlreadyThereIsReturnedAtOnce()
        {
            var watcher = new ProbeWatcher(() => new[] { Named("world-ready") }, Poll);

            Assert.Equal("world-ready", watcher.WaitForEvent("world-ready", Instant).Name);
        }

        [Fact]
        public void AnEventThatArrivesLaterIsWaitedFor()
        {
            var log = new GrowingLog(
                new ProbeEvent[0],
                new[] { Named("ready") },
                new[] { Named("ready"), Named("world-ready") });
            var watcher = new ProbeWatcher(log.Next, Poll);

            ProbeEvent found = watcher.WaitForEvent("world-ready", TimeSpan.FromSeconds(5));

            Assert.Equal("world-ready", found.Name);
            Assert.True(log.Reads >= 3, $"only read {log.Reads} times");
        }

        [Fact]
        public void MarkMakesEarlierEventsUnwaitable()
        {
            // The mistake this prevents: a wait answered by the previous scenario's
            // menu, so a test reports on something it never caused.
            ProbeEvent[] existing = { Menu(451), Named("conversation-end") };
            var watcher = new ProbeWatcher(() => existing, Poll);

            Assert.Equal(2, watcher.Mark());
            Assert.Throws<TimeoutException>(() => watcher.WaitForMenu(451, Instant));
        }

        [Fact]
        public void OnlyEventsAfterTheMarkAreReturned()
        {
            var log = new GrowingLog(
                new[] { Named("ready") },
                new[] { Named("ready"), Named("world-ready"), Menu(451) });
            var watcher = new ProbeWatcher(log.Next, Poll);

            watcher.Mark();

            Assert.Equal(
                new[] { "world-ready", "menu" },
                Array.ConvertAll(watcher.Since(), e => e.Name));
        }

        [Fact]
        public void SinceIsEmptyWhenNothingNewHasArrived()
        {
            ProbeEvent[] existing = { Named("ready") };
            var watcher = new ProbeWatcher(() => existing, Poll);

            watcher.Mark();

            Assert.Empty(watcher.Since());
        }

        [Fact]
        public void AMenuForAnotherConversationDoesNotSatisfyTheWait()
        {
            var watcher = new ProbeWatcher(() => new[] { Menu(9), Menu(13) }, Poll);

            Assert.Throws<TimeoutException>(() => watcher.WaitForMenu(451, Instant));
        }

        [Fact]
        public void TheRightMenuIsPickedOutFromAmongOthers()
        {
            var watcher = new ProbeWatcher(
                () => new[] { Menu(9), Menu(451), Menu(13) }, Poll);

            Assert.Equal(451, watcher.WaitForMenu(451, Instant).Number("conversation"));
        }

        [Fact]
        public void ConsecutiveWaitsAdvanceRatherThanRepeat()
        {
            var watcher = new ProbeWatcher(
                () => new[] { Menu(451), Named("conversation-end"), Menu(451) }, Poll);

            watcher.WaitForMenu(451, Instant);
            watcher.WaitForEvent("conversation-end", Instant);
            watcher.WaitForMenu(451, Instant);

            // A fourth would have nothing left to find.
            Assert.Throws<TimeoutException>(() => watcher.WaitForMenu(451, Instant));
        }

        [Fact]
        public void ATimeoutNamesWhatDidArrive()
        {
            var watcher = new ProbeWatcher(
                () => new[] { Named("ready"), Named("world-ready") }, Poll);

            TimeoutException error = Assert.Throws<TimeoutException>(
                () => watcher.WaitForMenu(451, Instant));

            Assert.Contains("conversation 451", error.Message);
            Assert.Contains("ready", error.Message);
            Assert.Contains("world-ready", error.Message);
        }

        [Fact]
        public void ATimeoutWithNoEventsAtAllSaysSo()
        {
            var watcher = new ProbeWatcher(Array.Empty<ProbeEvent>, Poll);

            TimeoutException error = Assert.Throws<TimeoutException>(
                () => watcher.WaitForEvent("world-ready", Instant));

            Assert.Contains("nothing at all", error.Message);
            Assert.Contains("installed", error.Message);
        }

        [Fact]
        public void NoReaderIsRefused()
        {
            Assert.Throws<ArgumentNullException>(() => new ProbeWatcher((Func<ProbeEvent[]>)null!));
            Assert.Throws<ArgumentNullException>(() => new ProbeWatcher((string)null!));
        }

        [Fact]
        public void NoPredicateIsRefused()
        {
            var watcher = new ProbeWatcher(Array.Empty<ProbeEvent>, Poll);

            Assert.Throws<ArgumentNullException>(() => watcher.WaitFor(null!, Instant, "anything"));
        }
    }
}
