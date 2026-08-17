using System.Diagnostics;
using System.Globalization;

namespace UnifiedConversationTracker.Session
{
    /// <summary>
    /// Where one interception of a savegame's compressed SimStatus blobs spent its
    /// time, and the counts that make those times interpretable, as one loggable line.
    /// </summary>
    /// <remarks>
    /// <para>The deliberate counterpart to <see cref="SimStatusWalkMeasurement"/>, so
    /// the two paths can be read against each other in the same log. The walk splits
    /// into scan / resolve / status read; this splits into the Lua reads that fetch
    /// the blobs and the managed decode that turns them into rows, which are the same
    /// two halves - what the game is asked for, and what is done with the answer.</para>
    ///
    /// <para><b>Only one of these two numbers has ever been observed.</b> The decode
    /// half was measured offline, over a real savegame's blobs on a desktop CPU, at
    /// 35-53 ms (see <see cref="CompressedSimStatusDecoder"/>). The read half - ~1,500
    /// keyed lookups across the IL2CPP boundary - has never run at all, and neither has
    /// the pair of them inside a game: no prefix had executed anywhere when this was
    /// written. de-0m0.19 reasoned order 20-50 ms for the whole thing against the
    /// walk's measured 1,892 ms, from de-0m0.17's measured per-crossing cost. This line
    /// exists precisely so that stops being a reasoned figure.</para>
    ///
    /// <para>Ticks rather than milliseconds for the same reason as the walk's: they
    /// are accumulated a conversation at a time, and rounding ~1,500 times would lose
    /// more than it measures.</para>
    /// </remarks>
    public readonly struct SimStatusInterceptionMeasurement
    {
        /// <summary>Distinguishes an interception that measured nothing from none at all.</summary>
        private readonly bool _ran;

        /// <summary>Records one finished interception.</summary>
        /// <param name="conversationCount">Conversations looked for, i.e. keyed lookups made.</param>
        /// <param name="blobCount">Conversations that actually had a blob.</param>
        /// <param name="pairCount">Id/status pairs the blobs held, Untouched included.</param>
        /// <param name="shadowedPairCount">Pairs overwritten by a later duplicate articy id.</param>
        /// <param name="rowCount">Non-Untouched rows handed to the merge.</param>
        /// <param name="readTicks">Time fetching blobs out of the Lua Variable table.</param>
        /// <param name="decodeTicks">Time decoding and resolving them, in managed code.</param>
        public SimStatusInterceptionMeasurement(
            long conversationCount,
            long blobCount,
            long pairCount,
            long shadowedPairCount,
            long rowCount,
            long readTicks,
            long decodeTicks)
        {
            _ran = true;
            ConversationCount = conversationCount;
            BlobCount = blobCount;
            PairCount = pairCount;
            ShadowedPairCount = shadowedPairCount;
            RowCount = rowCount;
            ReadTicks = readTicks;
            DecodeTicks = decodeTicks;
        }

        /// <summary>Conversations the articy id map knows, each of them one keyed lookup.</summary>
        public long ConversationCount { get; }

        /// <summary>Conversations that had a compressed blob to read.</summary>
        public long BlobCount { get; }

        /// <summary>Id/status pairs decoded, Untouched ones included.</summary>
        public long PairCount { get; }

        /// <summary>Pairs a later pair for the same dialogue entry overwrote.</summary>
        public long ShadowedPairCount { get; }

        /// <summary>Non-Untouched rows handed to the merge.</summary>
        public long RowCount { get; }

        /// <summary>Ticks spent fetching blobs out of Lua.</summary>
        public long ReadTicks { get; }

        /// <summary>Ticks spent decoding blobs into rows.</summary>
        public long DecodeTicks { get; }

        /// <summary>Both measured sections together.</summary>
        public long TotalTicks => ReadTicks + DecodeTicks;

        /// <summary>
        /// Conversations the map knew that had no blob. Expected to be small and
        /// non-zero: a conversation with no dialogue entries gets no blob, and a real
        /// save had exactly 7 of those out of 1,501 (de-0m0.19).
        /// </summary>
        public long MissingBlobCount => ConversationCount - BlobCount;

        /// <summary>
        /// How many times the interception read the clock, so a reader can judge how
        /// much of the total is the measuring. Two per conversation, one to start.
        /// </summary>
        public long ClockReadCount => _ran ? (2 * ConversationCount) + 1 : 0;

        /// <summary>The single log line, or null if no interception has run.</summary>
        public string? Describe()
        {
            if (!_ran)
            {
                return null;
            }

            return $"{BlobCount} blobs read for {ConversationCount} conversations"
                + $" ({MissingBlobCount} with no blob)"
                + $"\n{PairCount} pairs, {ShadowedPairCount} with duplicate articy id,"
                + $" {RowCount} non-Untouched rows merged"
                + "\nTiming (Get-Value per conversation-variable):"
                + $"\n    inside the interception (total): {Ms(TotalTicks)} ms"
                + $"\n        read: {Ms(ReadTicks)} ms"
                + $"\n        decode: {Ms(DecodeTicks)}"
                + $"\n    {ClockReadCount} clock reads at {Stopwatch.Frequency} ticks/s";
        }

        /// <inheritdoc />
        public override string ToString() =>
            $"SimStatusInterceptionMeasurement({ConversationCount}, {BlobCount}, {PairCount}, "
            + $"{ShadowedPairCount}, {RowCount}, {ReadTicks}, {DecodeTicks})";

        private static string Ms(long ticks) =>
            (ticks * 1000.0 / Stopwatch.Frequency).ToString("F1", CultureInfo.InvariantCulture);

        private string Us(long ticks) =>
            (ticks * 1000000.0 / Stopwatch.Frequency / PairCount).ToString("F2", CultureInfo.InvariantCulture);
    }
}
