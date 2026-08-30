// SPDX-License-Identifier: MIT
using System.Collections.Generic;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>How many distinct states one entry was reached in.</summary>
    /// <remarks>
    /// The number that identifies a blow-up. A search that exhausts its budget has
    /// almost always found one entry it can arrive at in hundreds of different states -
    /// usually a hub inside a cycle whose guards read something the path keeps changing -
    /// and naming that entry is the difference between a diagnosable report and a count.
    /// </remarks>
    public sealed class NodeStateCount
    {
        /// <summary>Creates a count.</summary>
        /// <param name="node">Which entry.</param>
        /// <param name="states">How many distinct states it was reached in.</param>
        public NodeStateCount(DialogueNodeId node, int states)
        {
            Node = node;
            States = states;
        }

        /// <summary>Which entry.</summary>
        public DialogueNodeId Node { get; }

        /// <summary>How many distinct states it was reached in.</summary>
        public int States { get; }

        /// <inheritdoc/>
        public override string ToString()
        {
            return $"{Node.ConversationId}:{Node.EntryId} x{States}";
        }
    }

    /// <summary>
    /// What one crawl did, in enough detail to explain a budget overflow after the fact.
    /// </summary>
    /// <remarks>
    /// Only built when asked for, via <see cref="LookAheadOptions.CollectTrace"/>. The
    /// per-entry tally it carries costs a dictionary write per state, which is precisely
    /// the inner loop, so the normal path does not pay for it.
    /// </remarks>
    public sealed class LookAheadTrace
    {
        /// <summary>Creates a trace.</summary>
        /// <param name="start">The option the crawl began at.</param>
        /// <param name="graphNodeCount">How many entries the loaded group holds.</param>
        /// <param name="trackedSlots">How many state slots the group interned.</param>
        /// <param name="money">The balance the crawl started with, in centimes.</param>
        /// <param name="dayMinutes">The clock the crawl started with.</param>
        /// <param name="dayCounter">The story day the crawl started with.</param>
        /// <param name="clockLocked">Whether the clock was locked.</param>
        /// <param name="hottestNodes">The entries reached in the most distinct states.</param>
        public LookAheadTrace(
            DialogueNodeId start,
            int graphNodeCount,
            int trackedSlots,
            int money,
            int dayMinutes,
            int dayCounter,
            bool clockLocked,
            IReadOnlyList<NodeStateCount> hottestNodes)
        {
            Start = start;
            GraphNodeCount = graphNodeCount;
            TrackedSlots = trackedSlots;
            Money = money;
            DayMinutes = dayMinutes;
            DayCounter = dayCounter;
            ClockLocked = clockLocked;
            HottestNodes = hottestNodes;
        }

        /// <summary>The option the crawl began at.</summary>
        public DialogueNodeId Start { get; }

        /// <summary>How many entries the loaded conversation group holds.</summary>
        public int GraphNodeCount { get; }

        /// <summary>
        /// How many state slots the group interned - variables, items, tasks and markers.
        /// A wide state vector is what makes many states per entry possible.
        /// </summary>
        public int TrackedSlots { get; }

        /// <summary>The balance the crawl started with, in centimes.</summary>
        public int Money { get; }

        /// <summary>The clock the crawl started with, in minutes since midnight.</summary>
        public int DayMinutes { get; }

        /// <summary>The story day the crawl started with.</summary>
        public int DayCounter { get; }

        /// <summary>Whether the clock was locked, making PassTime a no-op.</summary>
        public bool ClockLocked { get; }

        /// <summary>
        /// The entries reached in the most distinct states, worst first. Where a
        /// blow-up lives.
        /// </summary>
        public IReadOnlyList<NodeStateCount> HottestNodes { get; }
    }
}
