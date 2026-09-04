// SPDX-License-Identifier: MIT
using GlobalConversationTracker.Automation;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// Reading the plugin's index-cache lines back out of a BepInEx log.
    /// </summary>
    /// <remarks>
    /// No game needed, for the reason <see cref="NativeEngineReportTests"/> gives. What
    /// cannot be checked here is what the numbers turn out to be, which is the whole point
    /// of asking a running game.
    /// </remarks>
    public class IndexCacheReportTests
    {
        /// <summary>A launch where the shipped index turned out to be right.</summary>
        private const string Matching =
            "[Message:GlobalConversationTracker] Look-ahead index: opened "
            + "GlobalConversationTracker.Index.jsonl, 1501 conversations, format 1.\n"
            + "[Message:GlobalConversationTracker] Look-ahead index: group "
            + "631,632,635,636,637,1249 matches the loaded database (43 ms).\n";

        [Fact]
        public void AMatchingIndexReportsWhatItOpenedAndWhatTheCheckCost()
        {
            IndexCacheReport report = IndexCacheReport.FromText(Matching);

            Assert.True(report.Reported);
            Assert.Equal("GlobalConversationTracker.Index.jsonl", report.Opened);
            Assert.Equal(1501, report.Conversations);
            Assert.Equal(1, report.Format);
            Assert.Equal("631,632,635,636,637,1249", report.Group);
            Assert.Equal(43, report.CheckMilliseconds);
            Assert.False(report.Rebuilt);
        }

        /// <summary>A rebuild is reported as one.</summary>
        /// <remarks>
        /// On an unmodified install this must not happen: it would mean the extractor's
        /// canonicalisation and the plugin's disagree about a database neither of them
        /// changed, which is exactly the drift the shared routine exists to prevent.
        /// </remarks>
        [Fact]
        public void ARebuildIsReported()
        {
            IndexCacheReport report = IndexCacheReport.FromText(
                "[Warning:GlobalConversationTracker] Look-ahead index: conversation 631 is "
                + "not what the index says it is (52 ms). The game's dialogue database has "
                + "changed; rebuilding.\n"
                + "[Message:GlobalConversationTracker] Look-ahead index: rebuilt 1,501 "
                + "conversations from the loaded database in 4,210 ms, at C:\\saves\\x.jsonl. "
                + "It will be used from now on, and found on the next launch.\n");

            Assert.True(report.Rebuilt);
            Assert.Contains("rebuilt", report.Line!);
        }

        /// <summary>
        /// An index with no header reports format 0, which is a real answer.
        /// </summary>
        /// <remarks>
        /// It means the full index was deployed - a build intermediate with no hashes - so
        /// nothing about it can be checked. Different from "no index opened", and only one
        /// of the two is a problem.
        /// </remarks>
        [Fact]
        public void AnIndexWithNoHeaderReportsFormatZero()
        {
            IndexCacheReport report = IndexCacheReport.FromText(
                "[Message:GlobalConversationTracker] Look-ahead index: opened "
                + "GlobalConversationTracker.Index.jsonl, 1501 conversations, format 0.\n");

            Assert.Equal(0, report.Format);
            Assert.Equal(-1, report.CheckMilliseconds);
        }

        /// <summary>A log that never mentions the index says so.</summary>
        /// <remarks>
        /// A check that silently did not run must not look like a check that passed.
        /// </remarks>
        [Fact]
        public void ALogThatNeverMentionsItIsNotAReport()
        {
            IndexCacheReport report = IndexCacheReport.FromText(
                "[Message:GlobalConversationTracker] GlobalConversationTracker v0.1.0 loaded.\n");

            Assert.False(report.Reported);
            Assert.Null(report.Opened);
            Assert.Equal(-1, report.Conversations);
        }

        /// <summary>An index that would not open at all is reported without counts.</summary>
        [Fact]
        public void AnIndexThatWouldNotOpenIsReported()
        {
            IndexCacheReport report = IndexCacheReport.FromText(
                "[Warning:GlobalConversationTracker] Look-ahead index: no index could be "
                + "opened; the look-ahead has no graph.\n");

            Assert.True(report.Reported);
            Assert.Null(report.Opened);
            Assert.Contains("no index could be opened", report.ToString());
        }
    }
}
