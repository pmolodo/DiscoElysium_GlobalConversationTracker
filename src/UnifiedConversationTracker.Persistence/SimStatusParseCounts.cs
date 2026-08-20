namespace UnifiedConversationTracker.Persistence;

/// <summary>
/// How much of a save blob one parse had to step over, as counted by the parse
/// itself. Produced by <see cref="RawDataParser.GetSimStatuses(System.ReadOnlySpan{byte}, out SimStatusParseCounts)"/>.
/// </summary>
/// <remarks>
/// <para>These are the numbers that make a parse time interpretable: the rows it
/// produced say what the parse was for, and these say how much it had to walk to
/// find them. Every one of them is either already known when the parse ends or is a
/// single increment on a path the parse already runs, so counting them costs
/// nothing measurable - which matters, because this runs inside a savegame load
/// that is itself being timed.</para>
///
/// <para>It carries counts only, never times: the parse is one straight-line walk
/// with no sections to attribute, so the clock belongs to whoever chose to start
/// one around it.</para>
/// </remarks>
public readonly struct SimStatusParseCounts
{
    /// <summary>Records what one finished parse walked over.</summary>
    /// <param name="conversationCount">Conversations in the Conversation table.</param>
    /// <param name="tableCount">Tables opened, at every depth, in all five top-level tables.</param>
    /// <param name="valueCount">Values stepped over, table keys included.</param>
    /// <param name="trailingByteCount">
    /// Bytes left after the five top-level tables - the blob's uninterpreted "extra
    /// data", which the parse does not read.
    /// </param>
    public SimStatusParseCounts(
        long conversationCount,
        long tableCount,
        long valueCount,
        long trailingByteCount)
    {
        ConversationCount = conversationCount;
        TableCount = tableCount;
        ValueCount = valueCount;
        TrailingByteCount = trailingByteCount;
    }

    /// <summary>Conversations in the Conversation table.</summary>
    public long ConversationCount { get; }

    /// <summary>Tables opened, at every depth, across all five top-level tables.</summary>
    public long TableCount { get; }

    /// <summary>Values stepped over, table keys included.</summary>
    public long ValueCount { get; }

    /// <summary>Bytes after the five tables that the parse never interprets.</summary>
    public long TrailingByteCount { get; }

    /// <inheritdoc />
    public override string ToString() =>
        $"SimStatusParseCounts({ConversationCount}, {TableCount}, {ValueCount}, "
        + $"{TrailingByteCount})";
}
