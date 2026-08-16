using System;
using System.Diagnostics;
using Xunit;

namespace UnifiedConversationTracker.Session.Tests
{
    /// <summary>
    /// The per-walk timing line (de-p1h). The plugin fills this in from the running
    /// game, which no test can do, so what is pinned here is the part that does not
    /// need the game: that the arithmetic turns ticks into the units it claims, that
    /// no walk is distinguishable from an empty walk, and that the counts a reader
    /// would use to judge the numbers are all present in the line.
    /// </summary>
    public sealed class SimStatusWalkMeasurementTests
    {
        /// <summary>One second, in whatever units this machine's Stopwatch counts.</summary>
        private static readonly long OneSecond = Stopwatch.Frequency;

        [Fact]
        public void Describe_WithNoWalkAtAll_SaysNothingRatherThanZero()
        {
            // The default is what every source that does not measure itself reports,
            // and a line of zeroes from one of those would be a lie.
            Assert.Null(default(SimStatusWalkMeasurement).Describe());
        }

        [Fact]
        public void Describe_AfterAWalkThatCountedNothing_StillProducesALine()
        {
            // A walk that found no conversations is a real finding, and one worth
            // seeing in the log rather than silently looking like no walk.
            string? line = SimStatusWalkMeasurement.Starting().Describe();

            Assert.NotNull(line);
            Assert.Contains("0 rows", line, StringComparison.Ordinal);
        }

        [Fact]
        public void Describe_WithNoRows_OmitsThePerRowBreakdownRatherThanDividingByZero()
        {
            string line = SimStatusWalkMeasurement.Starting().Describe()!;

            Assert.DoesNotContain("per row", line, StringComparison.Ordinal);
            Assert.DoesNotContain("NaN", line, StringComparison.Ordinal);
        }

        [Fact]
        public void Describe_TurnsTicksIntoMillisecondsAndMicrosecondsPerRow()
        {
            // 1000 rows taking one second between them: 1000.0 ms in total and
            // 1000.00 us a row, whatever this machine's tick rate happens to be.
            var walk = new SimStatusWalkMeasurement(
                rowCount: 1000,
                conversationCount: 10,
                dialogTableResolveCount: 10,
                scanTicks: OneSecond / 4,
                dialogTableResolveTicks: OneSecond / 4,
                statusReadTicks: OneSecond / 2);

            string line = walk.Describe()!;

            Assert.Contains("inside the walk 1000.0 ms", line, StringComparison.Ordinal);
            Assert.Contains("status read 500.0", line, StringComparison.Ordinal);
            Assert.Contains("per row 1000.00 us", line, StringComparison.Ordinal);
            Assert.Contains("status read 500.00", line, StringComparison.Ordinal);
        }

        [Fact]
        public void Describe_ReportsTheCountsThatSayWhetherTheCacheIsWorking()
        {
            // The walk resolves one Dialog table per conversation only if entries
            // arrive grouped by conversation. If they interleave, this count runs
            // towards the row count instead, and that is the whole reason it is
            // logged.
            var walk = new SimStatusWalkMeasurement(
                rowCount: 112940,
                conversationCount: 1501,
                dialogTableResolveCount: 1501,
                scanTicks: OneSecond,
                dialogTableResolveTicks: OneSecond,
                statusReadTicks: OneSecond);

            string line = walk.Describe()!;

            Assert.Contains("112940 rows over 1501 conversations", line, StringComparison.Ordinal);
            Assert.Contains("Dialog tables resolved 1501 times", line, StringComparison.Ordinal);
            Assert.Contains("0.0133 per row", line, StringComparison.Ordinal);
        }

        [Fact]
        public void ClockReadCount_IsThreePerRowPlusOnePerResolvePlusOne()
        {
            // Reported so a reader can decide for themselves whether the measuring
            // is distorting the measurement, instead of taking anyone's word for it.
            var walk = new SimStatusWalkMeasurement(
                rowCount: 112940,
                conversationCount: 1501,
                dialogTableResolveCount: 1501,
                scanTicks: 0,
                dialogTableResolveTicks: 0,
                statusReadTicks: 0);

            Assert.Equal((3 * 112940) + 1501 + 1, walk.ClockReadCount);
            Assert.Contains(
                walk.ClockReadCount.ToString() + " clock reads",
                walk.Describe()!,
                StringComparison.Ordinal);
        }

        [Fact]
        public void ClockReadCount_WithNoWalk_IsZero()
        {
            Assert.Equal(0, default(SimStatusWalkMeasurement).ClockReadCount);
        }

        [Fact]
        public void TotalTicks_IsTheThreeMeasuredSectionsAndNothingElse()
        {
            var walk = new SimStatusWalkMeasurement(5, 1, 1, 100, 20, 3);

            Assert.Equal(123, walk.TotalTicks);
        }
    }
}
