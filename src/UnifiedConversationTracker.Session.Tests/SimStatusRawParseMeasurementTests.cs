using System;
using System.Diagnostics;
using UnifiedConversationTracker.Persistence;
using Xunit;

namespace UnifiedConversationTracker.Session.Tests
{
    /// <summary>
    /// The per-parse timing line for the raw-bytes route (de-5kv.14). Only the game
    /// can produce a real blob, so what is pinned here is the part that does not need
    /// one: that the arithmetic turns ticks into the units it claims, that no parse is
    /// distinguishable from a parse of nothing, and that the counts a reader would use
    /// to judge the number are all in the line.
    /// </summary>
    public sealed class SimStatusRawParseMeasurementTests
    {
        /// <summary>One second, in whatever units this machine's Stopwatch counts.</summary>
        private static readonly long OneSecond = Stopwatch.Frequency;

        /// <summary>The shape of a real save's Conversation table, roughly.</summary>
        private static readonly SimStatusParseCounts SampleCounts = new(
            conversationCount: 1501,
            tableCount: 114441,
            valueCount: 500000,
            trailingByteCount: 64);

        [Fact]
        public void Describe_WithNoParseAtAll_SaysNothingRatherThanZero()
        {
            // The default is what a route that does not measure itself would report,
            // and a line of zeroes from one of those would be a lie.
            Assert.Null(default(SimStatusRawParseMeasurement).Describe());
        }

        [Fact]
        public void Describe_AfterAParseThatFoundNothing_StillProducesALine()
        {
            // A blob with no SimStatus in it is a real finding, and one worth seeing
            // rather than silently looking like no parse at all.
            string? line = Measure(rowCount: 0, counts: default, parseTicks: 0).Describe();

            Assert.NotNull(line);
            Assert.Contains("0 rows", line, StringComparison.Ordinal);
        }

        [Fact]
        public void Describe_WithNoRows_OmitsThePerRowFigureRatherThanDividingByZero()
        {
            string line = Measure(rowCount: 0, counts: default, parseTicks: OneSecond).Describe()!;

            Assert.DoesNotContain("per row", line, StringComparison.Ordinal);
            Assert.DoesNotContain("NaN", line, StringComparison.Ordinal);
        }

        [Fact]
        public void Describe_TurnsTicksIntoMillisecondsAndMicrosecondsPerRow()
        {
            // 1000 rows taking one second between them: 1000.0 ms in total and
            // 1000.00 us a row, whatever this machine's tick rate happens to be.
            string line = Measure(
                rowCount: 1000, counts: SampleCounts, parseTicks: OneSecond).Describe()!;

            Assert.Contains("inside the parse 1000.0 ms", line, StringComparison.Ordinal);
            Assert.Contains("per row 1000.00 us", line, StringComparison.Ordinal);
        }

        [Fact]
        public void Describe_ReportsHowMuchOfTheBlobWasWalkedForThoseRows()
        {
            // A parse time means nothing without the size of what was parsed: these
            // are what separate a slow parser from a big save.
            string line = Measure(
                rowCount: 112940, counts: SampleCounts, parseTicks: OneSecond).Describe()!;

            Assert.Contains("112940 rows over 1501 conversations", line, StringComparison.Ordinal);
            Assert.Contains("6543210 bytes parsed", line, StringComparison.Ordinal);
            Assert.Contains("64 trailing bytes not read", line, StringComparison.Ordinal);
            Assert.Contains("500000 values in 114441 tables", line, StringComparison.Ordinal);
        }

        [Fact]
        public void Describe_SaysHowTheParseTimeRelatesToTheResyncTotalAboveIt()
        {
            // The two lines are read together, and a reader who assumed the resync
            // total contained this one would double-count the rows.
            string line = Measure(
                rowCount: 112940, counts: SampleCounts, parseTicks: OneSecond).Describe()!;

            Assert.Contains("2 clock reads at ", line, StringComparison.Ordinal);
            Assert.Contains("add up rather than overlap", line, StringComparison.Ordinal);
        }

        private static SimStatusRawParseMeasurement Measure(
            long rowCount,
            SimStatusParseCounts counts,
            long parseTicks) =>
            new SimStatusRawParseMeasurement(
                byteCount: 6543210,
                rowCount: rowCount,
                counts: counts,
                parseTicks: parseTicks);
    }
}
