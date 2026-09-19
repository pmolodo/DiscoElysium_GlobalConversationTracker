// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;

namespace GlobalConversationTracker.Engine
{
    /// <remarks>
    /// Moved here from the managed look-ahead when that engine was deleted (de-i5xj.6).
    /// The numbers are the same numbers - states, entries, time, why a crawl stopped - and
    /// they now arrive in a <see cref="LookAheadAnswer"/> from the Rust engine rather than
    /// in a result object from a crawl in this process.
    /// </remarks>
    /// <summary>What one conversation's crawls have cost so far.</summary>
    public sealed class ConversationStatistics
    {
        /// <summary>How many crawls started in this conversation.</summary>
        public long Crawls { get; internal set; }

        /// <summary>Their total state count.</summary>
        public long TotalStates { get; internal set; }

        /// <summary>The most states one of them took.</summary>
        public int MaxStates { get; internal set; }

        /// <summary>Their total wall time, in milliseconds.</summary>
        public double TotalMilliseconds { get; internal set; }

        /// <summary>The longest one, in milliseconds.</summary>
        public double MaxMilliseconds { get; internal set; }

        /// <summary>How many of them ran out of budget, by either limit.</summary>
        public long BudgetExhausted { get; internal set; }

        /// <summary>How many of them ran out of time rather than states.</summary>
        public long TimeExhausted { get; internal set; }

        /// <summary>Their mean state count.</summary>
        public double MeanStates => Crawls == 0 ? 0 : (double)TotalStates / Crawls;
    }

    /// <summary>
    /// Running totals for look-ahead crawls, for deciding whether the feature costs what
    /// it is worth.
    /// </summary>
    /// <remarks>
    /// <para>Bounded memory on purpose. A playthrough draws a great many response menus,
    /// so retaining a sample per crawl to compute exact percentiles would grow without
    /// limit; this keeps running totals, extremes, a fixed histogram and a per-conversation
    /// tally instead. The histogram is what makes the tail visible - a mean is useless for
    /// spotting the one menu in a thousand that costs a hundred times the rest.</para>
    ///
    /// <para>Pure accumulation, no IO, so the shape of what gets written is testable
    /// without a game or a filesystem.</para>
    /// </remarks>
    public sealed class LookAheadStatistics
    {
        /// <summary>
        /// The upper bound of each histogram bucket, in states. The last bucket catches
        /// everything above the highest bound.
        /// </summary>
        public static readonly IReadOnlyList<int> BucketBounds =
            new[] { 10, 100, 1_000, 10_000, 100_000 };

        private readonly long[] _buckets = new long[BucketBounds.Count + 1];
        private readonly Dictionary<int, ConversationStatistics> _byConversation =
            new Dictionary<int, ConversationStatistics>();

        /// <summary>How many crawls have been recorded.</summary>
        public long Crawls { get; private set; }

        /// <summary>Their total state count.</summary>
        public long TotalStates { get; private set; }

        /// <summary>The fewest states one crawl took.</summary>
        public int MinStates { get; private set; } = int.MaxValue;

        /// <summary>The most states one crawl took.</summary>
        public int MaxStates { get; private set; }

        /// <summary>The total number of distinct entries reached.</summary>
        public long TotalNodes { get; private set; }

        /// <summary>The most entries one crawl reached.</summary>
        public int MaxNodes { get; private set; }

        /// <summary>Total wall time across all crawls, in milliseconds.</summary>
        public double TotalMilliseconds { get; private set; }

        /// <summary>The longest single crawl, in milliseconds.</summary>
        public double MaxMilliseconds { get; private set; }

        /// <summary>How many crawls ran out of budget, by either limit.</summary>
        public long BudgetExhausted { get; private set; }

        /// <summary>
        /// How many of those ran out of time rather than states.
        /// </summary>
        /// <remarks>
        /// Counted apart because the two mean different things. Running out of states is
        /// a property of the conversation and reproduces; running out of time is a
        /// property of the machine on the day, and a number that moves between runs on
        /// the same save is the signal that the clock, not the graph, is deciding what
        /// gets marked.
        /// </remarks>
        public long TimeExhausted { get; private set; }

        /// <summary>How many crawls found nothing new downstream.</summary>
        public long FoundNothing { get; private set; }

        /// <summary>How many found something unseen in this save but seen elsewhere.</summary>
        public long FoundUnseenThisGame { get; private set; }

        /// <summary>How many found something no save has seen.</summary>
        public long FoundUnseenAnyGame { get; private set; }

        /// <summary>The mean state count per crawl.</summary>
        public double MeanStates => Crawls == 0 ? 0 : (double)TotalStates / Crawls;

        /// <summary>The mean wall time per crawl, in milliseconds.</summary>
        public double MeanMilliseconds => Crawls == 0 ? 0 : TotalMilliseconds / Crawls;

        /// <summary>
        /// How many crawls fell in each state-count bucket, one entry longer than
        /// <see cref="BucketBounds"/> - the extra one is everything above the top bound.
        /// </summary>
        public IReadOnlyList<long> Buckets => _buckets;

        /// <summary>Per-conversation totals, keyed by the conversation a crawl began in.</summary>
        public IReadOnlyDictionary<int, ConversationStatistics> ByConversation => _byConversation;

        /// <summary>Records one crawl.</summary>
        /// <param name="start">The option it began at.</param>
        /// <param name="result">What it found.</param>
        /// <param name="milliseconds">How long it took.</param>
        public void Record(NodeRef start, LookAheadAnswer result, double milliseconds)
        {
            // No null check: an answer is a value now, where the managed engine's result
            // was a reference. There is nothing to guard against.
            Crawls++;
            TotalStates += (int)result.StatesExplored;
            TotalNodes += (int)result.NodesReached;
            TotalMilliseconds += milliseconds;

            if ((int)result.StatesExplored < MinStates)
            {
                MinStates = (int)result.StatesExplored;
            }

            if ((int)result.StatesExplored > MaxStates)
            {
                MaxStates = (int)result.StatesExplored;
            }

            if ((int)result.NodesReached > MaxNodes)
            {
                MaxNodes = (int)result.NodesReached;
            }

            if (milliseconds > MaxMilliseconds)
            {
                MaxMilliseconds = milliseconds;
            }

            if (!result.Complete)
            {
                BudgetExhausted++;
                if (result.StoppedBy == "time")
                {
                    TimeExhausted++;
                }
            }

            switch ((SeenState)result.Best)
            {
                case SeenState.UnseenAnyGame:
                    FoundUnseenAnyGame++;
                    break;
                case SeenState.UnseenThisGame:
                    FoundUnseenThisGame++;
                    break;
                default:
                    FoundNothing++;
                    break;
            }

            _buckets[BucketOf((int)result.StatesExplored)]++;

            if (!_byConversation.TryGetValue(start.Conversation, out ConversationStatistics? row))
            {
                row = new ConversationStatistics();
                _byConversation.Add(start.Conversation, row);
            }

            row.Crawls++;
            row.TotalStates += (int)result.StatesExplored;
            row.TotalMilliseconds += milliseconds;
            if ((int)result.StatesExplored > row.MaxStates)
            {
                row.MaxStates = (int)result.StatesExplored;
            }

            if (milliseconds > row.MaxMilliseconds)
            {
                row.MaxMilliseconds = milliseconds;
            }

            if (!result.Complete)
            {
                row.BudgetExhausted++;
                if (result.StoppedBy == "time")
                {
                    row.TimeExhausted++;
                }
            }
        }

        /// <summary>Which histogram bucket a state count falls in.</summary>
        /// <param name="states">The state count.</param>
        public static int BucketOf(int states)
        {
            for (int i = 0; i < BucketBounds.Count; i++)
            {
                if (states <= BucketBounds[i])
                {
                    return i;
                }
            }

            return BucketBounds.Count;
        }

        /// <summary>A readable label for a bucket, such as <c>101-1000</c>.</summary>
        /// <param name="index">The bucket index.</param>
        /// <exception cref="ArgumentOutOfRangeException">No such bucket.</exception>
        public static string BucketLabel(int index)
        {
            if (index < 0 || index > BucketBounds.Count)
            {
                throw new ArgumentOutOfRangeException(nameof(index));
            }

            if (index == BucketBounds.Count)
            {
                return (BucketBounds[BucketBounds.Count - 1] + 1) + "+";
            }

            int low = index == 0 ? 0 : BucketBounds[index - 1] + 1;
            return low + "-" + BucketBounds[index];
        }

        /// <summary>The fewest states any crawl took, or zero before the first one.</summary>
        public int MinStatesOrZero => Crawls == 0 ? 0 : MinStates;
    }
}
