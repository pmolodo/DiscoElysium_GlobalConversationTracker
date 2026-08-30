// SPDX-License-Identifier: MIT
using System;
using GlobalConversationTracker.LookAhead;
using Xunit;

namespace GlobalConversationTracker.LookAhead.Tests
{
    public class LookAheadStatisticsTests
    {
        private static LookAheadResult Result(
            int states, int nodes = 1, bool exhausted = false,
            Novelty best = Novelty.SeenThisGame)
        {
            return new LookAheadResult(best, states, nodes, exhausted);
        }

        [Fact]
        public void EmptyStatistics_ReportZeroesRatherThanNonsense()
        {
            var statistics = new LookAheadStatistics();

            Assert.Equal(0, statistics.Crawls);
            Assert.Equal(0d, statistics.MeanStates);
            Assert.Equal(0d, statistics.MeanMilliseconds);

            // MinStates starts at int.MaxValue so the first sample wins; the reported
            // value must not leak that sentinel into a file someone reads.
            Assert.Equal(0, statistics.MinStatesOrZero);
        }

        [Fact]
        public void Record_TracksExtremesAndMeans()
        {
            var statistics = new LookAheadStatistics();
            var node = GraphBuilder.Node(1);

            statistics.Record(node, Result(10, nodes: 4), 1.0);
            statistics.Record(node, Result(30, nodes: 9), 3.0);
            statistics.Record(node, Result(20, nodes: 6), 2.0);

            Assert.Equal(3, statistics.Crawls);
            Assert.Equal(60, statistics.TotalStates);
            Assert.Equal(10, statistics.MinStatesOrZero);
            Assert.Equal(30, statistics.MaxStates);
            Assert.Equal(20d, statistics.MeanStates);
            Assert.Equal(9, statistics.MaxNodes);
            Assert.Equal(19, statistics.TotalNodes);
            Assert.Equal(3.0, statistics.MaxMilliseconds);
            Assert.Equal(2.0, statistics.MeanMilliseconds);
        }

        [Fact]
        public void Record_CountsBudgetOverflows()
        {
            var statistics = new LookAheadStatistics();
            var node = GraphBuilder.Node(1);

            statistics.Record(node, Result(5), 1.0);
            statistics.Record(node, Result(200_000, exhausted: true), 900.0);

            Assert.Equal(1, statistics.BudgetExhausted);
        }

        [Fact]
        public void Record_TalliesWhatWasFound()
        {
            var statistics = new LookAheadStatistics();
            var node = GraphBuilder.Node(1);

            statistics.Record(node, Result(1, best: Novelty.UnseenAnyGame), 1.0);
            statistics.Record(node, Result(1, best: Novelty.UnseenAnyGame), 1.0);
            statistics.Record(node, Result(1, best: Novelty.UnseenThisGame), 1.0);
            statistics.Record(node, Result(1, best: Novelty.SeenThisGame), 1.0);

            Assert.Equal(2, statistics.FoundUnseenAnyGame);
            Assert.Equal(1, statistics.FoundUnseenThisGame);
            Assert.Equal(1, statistics.FoundNothing);
        }

        [Theory]
        [InlineData(0, 0)]
        [InlineData(10, 0)]
        [InlineData(11, 1)]
        [InlineData(100, 1)]
        [InlineData(101, 2)]
        [InlineData(100_000, 4)]
        [InlineData(100_001, 5)]
        public void BucketOf_PlacesCountsInTheRightDecade(int states, int expected)
        {
            Assert.Equal(expected, LookAheadStatistics.BucketOf(states));
        }

        [Fact]
        public void BucketLabel_ReadsAsARange()
        {
            Assert.Equal("0-10", LookAheadStatistics.BucketLabel(0));
            Assert.Equal("11-100", LookAheadStatistics.BucketLabel(1));
            Assert.Equal("100001+", LookAheadStatistics.BucketLabel(5));
            Assert.Throws<ArgumentOutOfRangeException>(
                () => LookAheadStatistics.BucketLabel(6));
        }

        /// <summary>
        /// The histogram is the point of keeping statistics at all: a mean hides the one
        /// menu in a thousand that costs a hundred times the rest.
        /// </summary>
        [Fact]
        public void Buckets_SeparateTheTailFromTheBulk()
        {
            var statistics = new LookAheadStatistics();
            var node = GraphBuilder.Node(1);

            for (int i = 0; i < 99; i++)
            {
                statistics.Record(node, Result(5), 0.1);
            }

            statistics.Record(node, Result(50_000), 500.0);

            Assert.Equal(99, statistics.Buckets[0]);
            Assert.Equal(1, statistics.Buckets[4]);
            Assert.Equal(50_000, statistics.MaxStates);
        }

        [Fact]
        public void ByConversation_SeparatesTheExpensiveOnes()
        {
            var statistics = new LookAheadStatistics();

            statistics.Record(new DialogueNodeId(825, 1), Result(10), 1.0);
            statistics.Record(new DialogueNodeId(825, 2), Result(30), 2.0);
            statistics.Record(new DialogueNodeId(28, 1), Result(5), 0.5);

            Assert.Equal(2, statistics.ByConversation.Count);

            ConversationStatistics lena = statistics.ByConversation[825];
            Assert.Equal(2, lena.Crawls);
            Assert.Equal(30, lena.MaxStates);
            Assert.Equal(20d, lena.MeanStates);
            Assert.Equal(2.0, lena.MaxMilliseconds);

            ConversationStatistics garte = statistics.ByConversation[28];
            Assert.Equal(1, garte.Crawls);
            Assert.Equal(5, garte.MaxStates);
        }

        [Fact]
        public void Record_RejectsANullResult()
        {
            var statistics = new LookAheadStatistics();
            Assert.Throws<ArgumentNullException>(
                () => statistics.Record(GraphBuilder.Node(1), null!, 1.0));
        }
    }
}
