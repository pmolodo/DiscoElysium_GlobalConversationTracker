using System;
using System.Diagnostics;
using Xunit;

namespace UnifiedConversationTracker.Session.Tests
{
    /// <summary>
    /// The session's half of interception: when the compressed-blob route is used
    /// instead of the walk, what happens when it declines, and how loudly it says so.
    /// </summary>
    /// <remarks>
    /// The route that fires them - a Harmony prefix and postfix on
    /// <c>PersistentDataManager.ExpandCompressedSimStatusData</c> - needs the game, so
    /// what is pinned here is the contract the prefix relies on: a true return means
    /// the caller must not walk, a false return means it must, and every false return
    /// but the ones the session has already explained is an ERROR that names what
    /// failed.
    /// </remarks>
    public sealed class InterceptedResyncTests
    {
        [Fact]
        public void TryResyncFromInterception_MergesTheRowsAndSaysTheCallerNeedNotWalk()
        {
            using var directory = new TempDirectory();
            var log = new RecordingLog();
            var walk = new FakeSimStatusSource();
            var interceptor = new FakeSimStatusInterceptor()
                .Add(3, 7, SimStatusNames.WasDisplayed);
            using var session = new UnifiedStateSession(
                directory.CreateStore(), walk, interceptor, log);

            Assert.True(session.TryResyncFromInterception());

            Assert.Equal(SimStatus.WasDisplayed, session.State.GetStatus(3, 7));
            Assert.Equal(1, session.InterceptedResyncCount);
            Assert.Equal(1, session.ResyncCount);
            Assert.Equal(0, walk.EnumerationCount);
        }

        [Fact]
        public void TryResyncFromInterception_WithNothingNew_StillCountsAsHandled()
        {
            // An empty interception is the common case - a save the mod already tracked
            // raises nothing - and it still means the load has been read, so walking
            // afterwards would be pure cost.
            using var directory = new TempDirectory();
            var log = new RecordingLog();
            using var session = new UnifiedStateSession(
                directory.CreateStore(), new FakeSimStatusSource(), new FakeSimStatusInterceptor(), log);

            Assert.True(session.TryResyncFromInterception());
            Assert.True(log.AnyContains(log.Info, "nothing new in 0 rows"));
        }

        [Fact]
        public void TryResyncFromInterception_LogsTheDetailLineUnderItsOwnNameSoTheTwoRoutesAreTellableApart()
        {
            using var directory = new TempDirectory();
            var log = new RecordingLog();
            var interceptor = new FakeSimStatusInterceptor
            {
                Measurement = new SimStatusInterceptionMeasurement(
                    conversationCount: 1501,
                    blobCount: 1494,
                    pairCount: 112940,
                    shadowedPairCount: 19814,
                    variableCount: 12144,
                    rowCount: 1473,
                    convertTicks: Stopwatch.Frequency / 100,
                    readTicks: Stopwatch.Frequency / 100,
                    decodeTicks: Stopwatch.Frequency / 100),
            };
            interceptor.Add(3, 7, SimStatusNames.WasOffered);
            using var session = new UnifiedStateSession(
                directory.CreateStore(), new FakeSimStatusSource(), interceptor, log);

            session.TryResyncFromInterception();

            Assert.True(log.AnyContains(log.Info, "Resync intercept detail"));
            Assert.True(log.AnyContains(log.Info, "1494 blobs read for 1501 conversations"));
            Assert.False(log.AnyContains(log.Info, "Resync walk detail"));
        }

        [Fact]
        public void TryResyncFromInterception_WithNoInterceptorAtAll_SaysSoAsAnErrorAndDeclines()
        {
            // The state the mod ships in until the articy id map has been audited
            // (de-0m0.21). It is loud on purpose: a silent decline is indistinguishable
            // from interception working, which is exactly what an in-game measurement
            // has to be able to tell apart.
            using var directory = new TempDirectory();
            var log = new RecordingLog();
            using var session = new UnifiedStateSession(
                directory.CreateStore(), new FakeSimStatusSource(), log);

            Assert.False(session.TryResyncFromInterception());

            Assert.True(log.AnyContains(log.Errors, "FAILED to intercept"));
            Assert.True(log.AnyContains(log.Errors, "articy id map"));
            Assert.Equal(0, session.InterceptedResyncCount);
        }

        [Fact]
        public void TryResyncFromInterception_WhenTheInterceptorDeclines_ReportsItsReasonVerbatimAsAnError()
        {
            using var directory = new TempDirectory();
            var log = new RecordingLog();
            var interceptor = new FakeSimStatusInterceptor
            {
                UnavailableReason = "the row count did not look right.",
            };
            using var session = new UnifiedStateSession(
                directory.CreateStore(), new FakeSimStatusSource(), interceptor, log);

            Assert.False(session.TryResyncFromInterception());

            Assert.True(log.AnyContains(log.Errors, "the row count did not look right."));
            Assert.Empty(log.Warnings);
        }

        [Fact]
        public void TryResyncFromInterception_WhenTheInterceptorThrows_ReportsItAndLeavesTheWalkAlive()
        {
            // A failure here costs speed, not tracking, so it must not disable the
            // resync the way a failing merge does.
            using var directory = new TempDirectory();
            var log = new RecordingLog();
            var walk = new FakeSimStatusSource().Add(3, 7, SimStatusNames.WasDisplayed);
            var interceptor = new FakeSimStatusInterceptor
            {
                ThrowOnIntercept = new InvalidOperationException("the Lua table went away"),
            };
            using var session = new UnifiedStateSession(
                directory.CreateStore(), walk, interceptor, log);

            Assert.False(session.TryResyncFromInterception());
            Assert.Equal(1, session.ResyncFromGame());

            Assert.True(log.AnyContains(log.Errors, "the Lua table went away"));
            Assert.Equal(SimStatus.WasDisplayed, session.State.GetStatus(3, 7));
        }

        [Fact]
        public void TryResyncFromInterception_MergesMonotonicallyLikeEveryOtherPath()
        {
            // Interception changes where the rows come from and nothing about what they
            // mean: absence still means Untouched, and the merge still only ever raises.
            using var directory = new TempDirectory();
            var log = new RecordingLog();
            var interceptor = new FakeSimStatusInterceptor()
                .Add(3, 7, SimStatusNames.WasDisplayed);
            using var session = new UnifiedStateSession(
                directory.CreateStore(), new FakeSimStatusSource(), interceptor, log);
            session.Record(3, 7, SimStatusNames.WasDisplayed);

            var lowering = new FakeSimStatusInterceptor().Add(3, 7, SimStatusNames.Untouched);
            using var second = new UnifiedStateSession(
                directory.CreateStore(), new FakeSimStatusSource(), lowering, log);
            second.Record(3, 7, SimStatusNames.WasDisplayed);
            second.TryResyncFromInterception();

            Assert.Equal(SimStatus.WasDisplayed, second.State.GetStatus(3, 7));
        }

        [Fact]
        public void TryResyncFromInterception_WhenSavingIsDisabled_DeclinesWithoutCallingTheGame()
        {
            // Not an interception failure, so not an interception error: the walk would
            // be refused for the same reason and the session has already said why.
            using var directory = new TempDirectory();
            var log = new RecordingLog();
            var interceptor = new FakeSimStatusInterceptor();
            var store = directory.CreateStore();
            System.IO.File.WriteAllText(
                store.LivePath, "{\"version\":99,\"conversations\":{\"3\":{\"17\":\"WasDisplayed\"}}}");
            using var session = new UnifiedStateSession(store, new FakeSimStatusSource(), interceptor, log);

            Assert.False(session.TryResyncFromInterception());

            Assert.Equal(0, interceptor.InterceptCount);
            Assert.False(log.AnyContains(log.Errors, "FAILED to intercept"));
        }
    }
}
