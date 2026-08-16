using System.Diagnostics;
using System.Globalization;

namespace UnifiedConversationTracker.Session
{
    /// <summary>
    /// Where one walk of the game's SimStatus tables spent its time, and the counts
    /// that make those times interpretable, as one loggable line.
    /// </summary>
    /// <remarks>
    /// <para>It lives here rather than in the plugin so that its arithmetic and its
    /// wording can be tested without the game, which is the same reason
    /// <see cref="ISimStatusSource"/> is an interface. Only the real source fills it
    /// in; everything else leaves <see cref="ISimStatusSource.DescribeLastWalk"/>
    /// returning null.</para>
    ///
    /// <para><b>Why it exists</b> (de-p1h). The first in-game resync reported 1744 ms
    /// for 112,940 rows, against 649 ms for the same rows on the walk it replaced,
    /// and no log line could say whether that went on the per-row Lua read, on
    /// resolving one Dialog table per conversation, on the merge that follows the
    /// walk, or on the loading screen the walk now runs inside. The three tick
    /// totals here separate the first three; the fourth is what is left when they
    /// are subtracted from the resync's own total.</para>
    ///
    /// <para>Ticks rather than milliseconds because these are accumulated a row at a
    /// time from <see cref="Stopwatch.GetTimestamp"/>, and rounding 113,000 times
    /// would lose more than it measures.</para>
    /// </remarks>
    public readonly struct SimStatusWalkMeasurement
    {
        /// <summary>Distinguishes a walk that measured nothing from no walk at all.</summary>
        private readonly bool _ran;

        /// <summary>
        /// Records one finished walk. Every tick total comes from differences of
        /// <see cref="Stopwatch.GetTimestamp"/>.
        /// </summary>
        /// <param name="rowCount">Rows yielded.</param>
        /// <param name="conversationCount">Conversations that had an entry list to walk.</param>
        /// <param name="dialogTableResolveCount">
        /// How many times the per-conversation Dialog table actually had to be looked
        /// up, which is how many times the walk's cache of it missed.
        /// </param>
        /// <param name="scanTicks">
        /// Time in the database traversal itself - the conversation and entry list
        /// indexers and the two id field reads - and nothing else.
        /// </param>
        /// <param name="dialogTableResolveTicks">Time in the Dialog table lookups counted by
        /// <paramref name="dialogTableResolveCount"/>.</param>
        /// <param name="statusReadTicks">Time reading the status out of the Lua tables.</param>
        public SimStatusWalkMeasurement(
            long rowCount,
            long conversationCount,
            long dialogTableResolveCount,
            long scanTicks,
            long dialogTableResolveTicks,
            long statusReadTicks)
        {
            _ran = true;
            RowCount = rowCount;
            ConversationCount = conversationCount;
            DialogTableResolveCount = dialogTableResolveCount;
            ScanTicks = scanTicks;
            DialogTableResolveTicks = dialogTableResolveTicks;
            StatusReadTicks = statusReadTicks;
        }

        /// <summary>
        /// A walk that has started but not yet counted anything. Distinct from
        /// <c>default</c>, which describes itself as nothing at all.
        /// </summary>
        public static SimStatusWalkMeasurement Starting() =>
            new SimStatusWalkMeasurement(0, 0, 0, 0, 0, 0);

        /// <summary>Rows yielded.</summary>
        public long RowCount { get; }

        /// <summary>Conversations that had an entry list to walk.</summary>
        public long ConversationCount { get; }

        /// <summary>How many times the per-conversation Dialog table had to be looked up.</summary>
        public long DialogTableResolveCount { get; }

        /// <summary>Ticks spent traversing the dialogue database.</summary>
        public long ScanTicks { get; }

        /// <summary>Ticks spent resolving Dialog tables.</summary>
        public long DialogTableResolveTicks { get; }

        /// <summary>Ticks spent reading statuses out of Lua.</summary>
        public long StatusReadTicks { get; }

        /// <summary>All three measured sections together.</summary>
        public long TotalTicks => ScanTicks + DialogTableResolveTicks + StatusReadTicks;

        /// <summary>
        /// How many times the walk read the clock, so a reader can judge for
        /// themselves how much of the total is the measuring rather than the walk.
        /// Three per row, one more per Dialog table resolved, one to start.
        /// </summary>
        public long ClockReadCount => _ran ? (3 * RowCount) + DialogTableResolveCount + 1 : 0;

        /// <summary>
        /// The single log line, or null if no walk has run.
        /// </summary>
        public string? Describe()
        {
            if (!_ran)
            {
                return null;
            }

            // Both extras are dropped when there are no rows, because a per-row figure
            // over no rows is not a small number, it is no number.
            string resolveRate = RowCount == 0
                ? string.Empty
                : $" ({Ratio(DialogTableResolveCount, RowCount)} per row, one per conversation "
                    + "is the whole point of the cache)";

            string perRow = RowCount == 0
                ? string.Empty
                : $"per row {Us(TotalTicks)} us = scan {Us(ScanTicks)} + resolve "
                    + $"{Us(DialogTableResolveTicks)} + status read {Us(StatusReadTicks)}; ";

            return $"{RowCount} rows over {ConversationCount} conversations; "
                + $"Dialog tables resolved {DialogTableResolveCount} times{resolveRate}; "
                + $"inside the walk {Ms(TotalTicks)} ms = scan {Ms(ScanTicks)} + resolve "
                + $"{Ms(DialogTableResolveTicks)} + status read {Ms(StatusReadTicks)}; "
                + perRow
                + $"{ClockReadCount} clock reads at {Stopwatch.Frequency} ticks/s. "
                + "Whatever the resync total above has beyond 'inside the walk' is the merge.";
        }

        /// <inheritdoc />
        public override string ToString() =>
            $"SimStatusWalkMeasurement({RowCount}, {ConversationCount}, {DialogTableResolveCount}, "
            + $"{ScanTicks}, {DialogTableResolveTicks}, {StatusReadTicks})";

        private static string Ms(long ticks) =>
            (ticks * 1000.0 / Stopwatch.Frequency).ToString("F1", CultureInfo.InvariantCulture);

        private string Us(long ticks) =>
            (ticks * 1000000.0 / Stopwatch.Frequency / RowCount).ToString("F2", CultureInfo.InvariantCulture);

        private static string Ratio(long count, long rowCount) =>
            ((double)count / rowCount).ToString("F4", CultureInfo.InvariantCulture);
    }
}
