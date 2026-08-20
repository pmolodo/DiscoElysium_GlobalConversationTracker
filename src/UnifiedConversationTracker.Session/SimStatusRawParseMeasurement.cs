using System.Diagnostics;
using System.Globalization;
using UnifiedConversationTracker.Persistence;

namespace UnifiedConversationTracker.Session
{
    /// <summary>
    /// What one parse of a savegame's raw .ntwtf.lua bytes cost, and the counts that
    /// make that cost interpretable, as one loggable line.
    /// </summary>
    /// <remarks>
    /// <para>The third of the same family as <see cref="SimStatusWalkMeasurement"/>
    /// and <see cref="SimStatusInterceptionMeasurement"/>, so all three routes into a
    /// resync report themselves the same way and can be read against each other in
    /// one log. It lives here rather than in the plugin for the same reason they do:
    /// its arithmetic and its wording can then be tested without the game.</para>
    ///
    /// <para><b>Why the parse needs a clock of its own.</b> The raw-bytes route parses
    /// the blob into a list and only then calls the merge, so the resync total that
    /// the merge times covers the merge alone - across four in-game runs it read
    /// 8-12 ms while the load envelope around the whole of ApplyRawData moved from
    /// 1706 ms to 1076 ms. Neither figure is wrong; between them they simply left the
    /// parse unmeasured, which is what this line reports.</para>
    ///
    /// <para>Milliseconds from a single <see cref="Stopwatch"/> rather than the
    /// accumulated ticks the other two use: this is one straight-line call with one
    /// start and one stop, so there is no per-row rounding to lose.</para>
    /// </remarks>
    public readonly struct SimStatusRawParseMeasurement
    {
        /// <summary>How many times the parse read the clock: one start, one stop.</summary>
        private const long ClockReads = 2;

        /// <summary>Distinguishes a parse that measured nothing from no parse at all.</summary>
        private readonly bool _ran;

        /// <summary>Records one finished parse.</summary>
        /// <param name="byteCount">Size of the blob handed to the parse.</param>
        /// <param name="rowCount">Rows the parse produced.</param>
        /// <param name="counts">What the parse walked over to produce them.</param>
        /// <param name="parseTicks">
        /// Time in the parse itself, from <see cref="Stopwatch"/>, covering nothing
        /// but the call that produced the rows.
        /// </param>
        public SimStatusRawParseMeasurement(
            long byteCount,
            long rowCount,
            SimStatusParseCounts counts,
            long parseTicks)
        {
            _ran = true;
            ByteCount = byteCount;
            RowCount = rowCount;
            Counts = counts;
            ParseTicks = parseTicks;
        }

        /// <summary>Size of the blob handed to the parse.</summary>
        public long ByteCount { get; }

        /// <summary>Rows the parse produced.</summary>
        public long RowCount { get; }

        /// <summary>What the parse walked over.</summary>
        public SimStatusParseCounts Counts { get; }

        /// <summary>Ticks spent inside the parse.</summary>
        public long ParseTicks { get; }

        /// <summary>The single log line, or null if no parse has run.</summary>
        public string? Describe()
        {
            if (!_ran)
            {
                return null;
            }

            // Dropped when there are no rows, because a per-row figure over no rows is
            // not a small number, it is no number.
            string perRow = RowCount == 0
                ? string.Empty
                : $"per row {Us(ParseTicks)} us; ";

            return $"{RowCount} rows over {Counts.ConversationCount} conversations; "
                + $"{ByteCount} bytes parsed ({Counts.TrailingByteCount} trailing bytes not read); "
                + $"\n{Counts.ValueCount} values in {Counts.TableCount} tables; "
                + $"\nTiming (GetSimStatusRows only reads minimum needed):"
                + $"\n    inside the parse {Ms(ParseTicks)} ms ({perRow}); "
                + $"\n    {ClockReads} clock reads at {Stopwatch.Frequency} ticks/s. "
                + "\nThe resync total above is the merge over rows this parse had already "
                + "produced, so the two lines add up rather than overlap.";
        }

        /// <inheritdoc />
        public override string ToString() =>
            $"SimStatusRawParseMeasurement({ByteCount}, {RowCount}, {Counts}, {ParseTicks})";

        private static string Ms(long ticks) =>
            (ticks * 1000.0 / Stopwatch.Frequency).ToString("F1", CultureInfo.InvariantCulture);

        private string Us(long ticks) =>
            (ticks * 1000000.0 / Stopwatch.Frequency / RowCount).ToString(
                "F2", CultureInfo.InvariantCulture);
    }
}
