// SPDX-License-Identifier: MIT
using GlobalConversationTracker.Automation;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// Reading the plugin's snapshot-agreement line back out of a BepInEx log.
    /// </summary>
    /// <remarks>
    /// No game needed, for the reason <see cref="NativeEngineReportTests"/> gives: the
    /// parsing is the part that can be got wrong quietly. What cannot be checked here is
    /// whether the two worlds actually agree, which is what the in-game run is for.
    /// </remarks>
    public class SnapshotAgreementReportTests
    {
        /// <summary>What the plugin writes when the two worlds say the same thing.</summary>
        private const string Agreeing =
            "[Message:GlobalConversationTracker] Snapshot agreement: conversation 631: "
            + "money and clock agree, 306 variables (0 differ), 8 CheckItem (0 differ), "
            + "3 IsTHCPresent (0 differ), 430 checks (0 differ), "
            + "4514 entries (0 differ), 13 queries (13 answered)\n";

        [Fact]
        public void AnAgreementIsReadAsOne()
        {
            SnapshotAgreementReport report = SnapshotAgreementReport.FromText(Agreeing);

            Assert.True(report.Reported);
            Assert.True(report.Agreed);
            Assert.Null(report.Differences);
            Assert.Equal((306, 0), report.Counts["variables"]);
            Assert.Equal((430, 0), report.Counts["checks"]);
            Assert.Equal((4514, 0), report.Counts["entries"]);
            Assert.Equal((8, 0), report.Counts["CheckItem"]);
            Assert.Equal(13, report.QueriesAsked);
            Assert.Equal(13, report.QueriesAnswered);
        }

        /// <summary>
        /// A disagreement is read as one, and keeps what the plugin said differed.
        /// </summary>
        [Fact]
        public void ADisagreementKeepsWhatDiffered()
        {
            SnapshotAgreementReport report = SnapshotAgreementReport.FromText(
                "[Warning:GlobalConversationTracker] Snapshot agreement: conversation 631: "
                + "money and clock agree, 306 variables (2 differ), 430 checks (0 differ), "
                + "4514 entries (0 differ), 13 queries (13 answered) FIRST DIFFERENCES: "
                + "variable jam.asked: managed unknown, snapshot false\n");

            Assert.True(report.Reported);
            Assert.False(report.Agreed);
            Assert.Equal((306, 2), report.Counts["variables"]);
            Assert.Contains("jam.asked", report.Differences!);
        }

        /// <summary>
        /// A count inside a NAMED DIFFERENCE is not mistaken for a summary count.
        /// </summary>
        /// <remarks>
        /// The differences carry entry ids and values, and an entry named
        /// <c>631:12</c> beside a count would otherwise be read as one - which would turn a
        /// real disagreement into a report that looked fine.
        /// </remarks>
        [Fact]
        public void ACountInsideADifferenceIsNotReadAsASummary()
        {
            SnapshotAgreementReport report = SnapshotAgreementReport.FromText(
                "[Warning:GlobalConversationTracker] Snapshot agreement: conversation 631: "
                + "306 variables (1 differ) FIRST DIFFERENCES: "
                + "check 631:12: managed 99 entries (0 differ)\n");

            Assert.Equal((306, 1), report.Counts["variables"]);
            Assert.False(report.Counts.ContainsKey("entries"));
        }

        /// <summary>
        /// A log that never mentions the comparison says so, rather than reading as
        /// agreement.
        /// </summary>
        /// <remarks>
        /// The failure worth guarding against: a check that silently did not run must not
        /// look like a check that passed.
        /// </remarks>
        [Fact]
        public void ALogThatNeverMentionsItIsNotAnAgreement()
        {
            SnapshotAgreementReport report = SnapshotAgreementReport.FromText(
                "[Message:GlobalConversationTracker] GlobalConversationTracker v0.1.0 loaded.\n");

            Assert.False(report.Reported);
            Assert.False(report.Agreed);
            Assert.Null(report.Line);
        }

        /// <summary>A comparison that could not run at all is reported, not silent.</summary>
        [Fact]
        public void AComparisonThatCouldNotRunIsReportedWithoutCounts()
        {
            SnapshotAgreementReport report = SnapshotAgreementReport.FromText(
                "[Warning:GlobalConversationTracker] Snapshot agreement: conversation 631 "
                + "could not be compared (InvalidOperationException: the look-ahead library "
                + "would not open).\n");

            Assert.True(report.Reported);
            Assert.Empty(report.Counts);
            Assert.Equal(-1, report.QueriesAnswered);
            Assert.Contains("could not be compared", report.Line!);
        }

        /// <summary>The last comparison in a log is the one reported.</summary>
        /// <remarks>
        /// A run may compare several groups, and a harness asking after the newest command
        /// wants the newest answer.
        /// </remarks>
        [Fact]
        public void TheLastComparisonIsTheOneReported()
        {
            SnapshotAgreementReport report = SnapshotAgreementReport.FromText(
                Agreeing
                + "[Message:GlobalConversationTracker] Snapshot agreement: conversation 368: "
                + "money and clock agree, 12 variables (0 differ), 1 queries (1 answered)\n");

            Assert.Contains("conversation 368", report.Line!);
            Assert.Equal((12, 0), report.Counts["variables"]);
        }
    }
}
