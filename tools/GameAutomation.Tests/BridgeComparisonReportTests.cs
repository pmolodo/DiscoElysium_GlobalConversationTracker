// SPDX-License-Identifier: MIT
using GlobalConversationTracker.Automation;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// Reading the two engines' verdict back out of a BepInEx log.
    /// </summary>
    /// <remarks>
    /// No game needed, for the reason <see cref="NativeEngineReportTests"/> gives. What
    /// needs one is the verdict itself, which is the point of running the game.
    /// </remarks>
    public class BridgeComparisonReportTests
    {
        /// <summary>A run where the two engines said the same thing throughout.</summary>
        private const string Agreeing =
            "[Message:GlobalConversationTracker] Look-ahead bridge: 42 options compared over "
            + "9 menus, 0 disagreed, 3 answered from a cut-short search, 0 the bridge could "
            + "not answer, 17 the managed engine answered without crawling. 12.4 ms per menu "
            + "across the bridge against 8.1 ms per option in the managed engine.\n";

        [Fact]
        public void AnAgreementReportsItsCountsAndItsTimings()
        {
            BridgeComparisonReport report = BridgeComparisonReport.FromText(Agreeing);

            Assert.True(report.Reported);
            Assert.True(report.Agreed);
            Assert.Equal(42, report.Compared);
            Assert.Equal(9, report.Menus);
            Assert.Equal(0, report.Disagreed);
            Assert.Equal(3, report.Incomplete);
            Assert.Equal(0, report.Unanswered);
            Assert.Equal(17, report.NotCrawled);
            Assert.Equal(12.4, report.BridgeMillisecondsPerMenu);
            Assert.Equal(8.1, report.ManagedMillisecondsPerOption);
        }

        /// <summary>A disagreement is not an agreement.</summary>
        [Fact]
        public void ADisagreementIsReadAsOne()
        {
            BridgeComparisonReport report = BridgeComparisonReport.FromText(
                "[Warning:GlobalConversationTracker] Look-ahead bridge: 42 options compared "
                + "over 9 menus, 2 disagreed, 0 answered from a cut-short search, 0 the "
                + "bridge could not answer, 0 the managed engine answered without crawling. "
                + "12.4 ms per menu across the bridge against 8.1 ms per option in the "
                + "managed engine.\n");

            Assert.True(report.Reported);
            Assert.False(report.Agreed);
            Assert.Equal(2, report.Disagreed);
        }

        /// <summary>
        /// A summary that compared nothing is not an agreement either.
        /// </summary>
        /// <remarks>
        /// The failure this exists to catch. A bridge that was never asked - no library, no
        /// index, every menu refused - reports zero disagreements, and zero disagreements
        /// out of zero comparisons is not evidence of anything.
        /// </remarks>
        [Fact]
        public void AComparisonThatComparedNothingIsNotAnAgreement()
        {
            BridgeComparisonReport report = BridgeComparisonReport.FromText(
                "[Message:GlobalConversationTracker] Look-ahead bridge: 0 options compared "
                + "over 0 menus, 0 disagreed, 0 answered from a cut-short search, 12 the "
                + "bridge could not answer, 0 the managed engine answered without crawling. "
                + "no timings.\n");

            Assert.True(report.Reported);
            Assert.False(report.Agreed);
            Assert.Equal(0, report.Compared);
            Assert.Equal(12, report.Unanswered);
            Assert.Equal(-1, report.BridgeMillisecondsPerMenu);
        }

        /// <summary>A log that never mentions it says so.</summary>
        [Fact]
        public void ALogThatNeverMentionsItIsNotAReport()
        {
            BridgeComparisonReport report = BridgeComparisonReport.FromText(
                "[Message:GlobalConversationTracker] GlobalConversationTracker v0.1.0 loaded.\n");

            Assert.False(report.Reported);
            Assert.False(report.Agreed);
            Assert.Null(report.Line);
        }

        /// <summary>The last summary is the one reported.</summary>
        /// <remarks>
        /// A suite run writes one per suite, and a caller asking after the suite that just
        /// finished wants that one.
        /// </remarks>
        [Fact]
        public void TheLastSummaryIsTheOneReported()
        {
            BridgeComparisonReport report = BridgeComparisonReport.FromText(
                Agreeing
                + "[Message:GlobalConversationTracker] Look-ahead bridge: 7 options compared "
                + "over 2 menus, 1 disagreed, 0 answered from a cut-short search, 0 the "
                + "bridge could not answer, 0 the managed engine answered without crawling. "
                + "3.0 ms per menu across the bridge against 2.0 ms per option in the "
                + "managed engine.\n");

            Assert.Equal(7, report.Compared);
            Assert.Equal(1, report.Disagreed);
            Assert.Equal(3.0, report.BridgeMillisecondsPerMenu);
        }
    }
}
