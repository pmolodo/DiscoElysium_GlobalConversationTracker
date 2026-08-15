using UnifiedConversationTracker;

namespace UnifiedStateCheck;

/// <summary>
/// One save being compared: a label for the report and the state read out of it.
/// </summary>
/// <param name="Label">How the save is named in the printed report.</param>
/// <param name="State">The dialogue statuses that save records.</param>
public readonly record struct NamedSave(string Label, UnifiedConversationState State);

/// <summary>
/// The comparison of a unified state file against the saves it should cover.
/// </summary>
/// <remarks>
/// <para>The rule, with the ordering Untouched &lt; WasOffered &lt; WasDisplayed:</para>
/// <code>
/// unified[e] &gt;= max(save1[e], save2[e], ... saveN[e])   for every entry e
/// </code>
/// <para>
/// This is deliberately NOT set equality. The SimStatus a save records is the last
/// value the game wrote into that playthrough's table, while the unified state is
/// merged monotonically across playthroughs, so a unified status sitting strictly
/// higher than every save is correct behaviour and is reported as information, not
/// as a failure. Only an entry the unified state failed to reach - a downgrade or a
/// missing union member - is a failure.
/// </para>
/// <para>
/// Two saves was the original shape (de-omm.10). Three or more matters because an
/// entry that only ever existed in one save is the load-bearing evidence: it can
/// only be in the unified file if the file carried it across, and when that save
/// belongs to a playthrough the current session never touched, the only path is a
/// read off disk.
/// </para>
/// </remarks>
public sealed class UnionReport
{
    /// <summary>How many differing entries <see cref="Write(TextWriter)"/> names.</summary>
    public const int DefaultMaxExamples = 10;

    private UnionReport(IReadOnlyList<ExclusiveCounts> exclusives)
    {
        Exclusives = exclusives;
    }

    /// <summary>Entries where <c>unified &lt; max(saves)</c>. Non-empty means FAIL.</summary>
    public List<Difference> Downgrades { get; } = new();

    /// <summary>Entries where <c>unified &gt; max(saves)</c>. Informational.</summary>
    public List<Difference> AboveAllSaves { get; } = new();

    /// <summary>Per-save counts of entries unique to that save, in the order given.</summary>
    public IReadOnlyList<ExclusiveCounts> Exclusives { get; }

    /// <summary>
    /// Unified entries present in no save. Large counts mean a stale file, so they
    /// are listed rather than only counted: identifying them is how a run tells
    /// genuine later dialogue apart from contamination.
    /// </summary>
    public List<Difference> InUnifiedOnly { get; } = new();

    /// <summary>The union of all saves, i.e. how many entries the rule is checked over.</summary>
    public int UnionEntryCount { get; private set; }

    /// <summary>The whole point: no entry fell below the union.</summary>
    public bool Passed => Downgrades.Count == 0;

    /// <summary>Compares a unified state against two or more saves.</summary>
    /// <exception cref="ArgumentException">Fewer than two saves were given.</exception>
    public static UnionReport Compare(
        UnifiedConversationState unified,
        IReadOnlyList<NamedSave> saves
    )
    {
        ArgumentNullException.ThrowIfNull(unified);
        ArgumentNullException.ThrowIfNull(saves);
        if (saves.Count < 2)
        {
            throw new ArgumentException("At least two saves are required", nameof(saves));
        }

        var exclusives = new ExclusiveCounts[saves.Count];
        for (int i = 0; i < saves.Count; i++)
        {
            exclusives[i] = new ExclusiveCounts(saves[i].Label);
        }

        var report = new UnionReport(exclusives);

        // The union of every save's keys; no state stores Untouched, so every key here
        // is an entry that actually has to be accounted for in the unified file.
        var unionKeys = new HashSet<(int Conversation, int Entry)>();
        foreach (NamedSave save in saves)
        {
            foreach (UnifiedStatusEntry entry in save.State.EnumerateEntries())
            {
                unionKeys.Add((entry.ConversationId, entry.DialogueEntryId));
            }
        }

        report.UnionEntryCount = unionKeys.Count;

        foreach ((int conversationId, int entryId) in unionKeys)
        {
            SimStatus expected = SimStatus.Untouched;
            int holderCount = 0;
            int lastHolder = -1;
            for (int i = 0; i < saves.Count; i++)
            {
                SimStatus status = saves[i].State.GetStatus(conversationId, entryId);
                if (status > expected)
                {
                    expected = status;
                }
                if (status > SimStatus.Untouched)
                {
                    holderCount++;
                    lastHolder = i;
                }
            }

            SimStatus actual = unified.GetStatus(conversationId, entryId);

            if (actual < expected)
            {
                report.Downgrades.Add(new Difference(conversationId, entryId, expected, actual));
            }
            else if (actual > expected)
            {
                report.AboveAllSaves.Add(new Difference(conversationId, entryId, expected, actual));
            }

            if (holderCount == 1)
            {
                exclusives[lastHolder].Total++;
                if (actual >= expected)
                {
                    exclusives[lastHolder].Preserved++;
                }
            }
        }

        foreach (UnifiedStatusEntry entry in unified.EnumerateEntries())
        {
            if (!unionKeys.Contains((entry.ConversationId, entry.DialogueEntryId)))
            {
                report.InUnifiedOnly.Add(
                    new Difference(
                        entry.ConversationId,
                        entry.DialogueEntryId,
                        SimStatus.Untouched,
                        entry.Status
                    )
                );
            }
        }

        return report;
    }

    /// <summary>Writes the human-readable report, ending in a PASS or FAIL line.</summary>
    public void Write(TextWriter writer) => Write(writer, DefaultMaxExamples);

    /// <summary>
    /// Writes the human-readable report, naming at most <paramref name="maxExamples"/>
    /// entries per category.
    /// </summary>
    public void Write(TextWriter writer, int maxExamples)
    {
        ArgumentNullException.ThrowIfNull(writer);

        writer.WriteLine($"Entries in the union of all saves : {UnionEntryCount}");
        writer.WriteLine();
        writer.WriteLine("The evidence that every playthrough survived in one file:");
        foreach (ExclusiveCounts exclusive in Exclusives)
        {
            writer.WriteLine(
                $"  Entries only in {exclusive.Label}, preserved in unified : "
                    + $"{exclusive.Preserved} of {exclusive.Total}"
            );
        }
        writer.WriteLine();
        writer.WriteLine("Informational (not failures):");
        writer.WriteLine($"  Unified above every save : {AboveAllSaves.Count}");
        WriteExamples(writer, AboveAllSaves, maxExamples);
        writer.WriteLine($"  Unified only, in no save : {InUnifiedOnly.Count}");
        WriteExamples(writer, InUnifiedOnly, maxExamples);
        writer.WriteLine();

        if (Passed)
        {
            writer.WriteLine(
                "PASS: every entry in any save is at least as high in the unified state."
            );
            return;
        }

        writer.WriteLine($"Downgraded or missing entries : {Downgrades.Count}");
        WriteExamples(writer, Downgrades, maxExamples);
        writer.WriteLine();
        writer.WriteLine(
            $"FAIL: {Downgrades.Count} entry(s) sit lower in the unified state than in a save."
        );
    }

    private static void WriteExamples(TextWriter writer, List<Difference> differences, int maxExamples)
    {
        foreach (Difference difference in differences.Take(maxExamples))
        {
            writer.WriteLine($"    {difference}");
        }

        if (differences.Count > maxExamples)
        {
            writer.WriteLine($"    ... and {differences.Count - maxExamples} more");
        }
    }

    /// <summary>How many entries are unique to one save, and how many of those survived.</summary>
    public sealed class ExclusiveCounts
    {
        internal ExclusiveCounts(string label)
        {
            Label = label;
        }

        /// <summary>The save these counts belong to.</summary>
        public string Label { get; }

        /// <summary>Entries above Untouched in this save and in no other.</summary>
        public int Total { get; internal set; }

        /// <summary>Of <see cref="Total"/>, how many the unified state preserved.</summary>
        public int Preserved { get; internal set; }
    }

    /// <summary>One entry whose unified status differs from the saves' maximum.</summary>
    public readonly record struct Difference(
        int ConversationId,
        int DialogueEntryId,
        SimStatus ExpectedAtLeast,
        SimStatus Actual
    )
    {
        /// <inheritdoc/>
        public override string ToString() =>
            $"conversation {ConversationId}, entry {DialogueEntryId}: "
            + $"saves say {ExpectedAtLeast}, unified says {Actual}";
    }
}
