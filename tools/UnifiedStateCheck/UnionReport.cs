using UnifiedConversationTracker;

namespace UnifiedStateCheck;

/// <summary>
/// The comparison of a unified state file against the two saves it should cover.
/// </summary>
/// <remarks>
/// <para>The rule, with the ordering Untouched &lt; WasOffered &lt; WasDisplayed:</para>
/// <code>
/// unified[e] &gt;= max(saveA[e], saveB[e])   for every entry e
/// </code>
/// <para>
/// This is deliberately NOT set equality. The SimStatus a save records is the last
/// value the game wrote into that playthrough's table, while the unified state is
/// merged monotonically across playthroughs, so a unified status sitting strictly
/// higher than both saves is correct behaviour and is reported as information, not
/// as a failure. Only an entry the unified state failed to reach - a downgrade or a
/// missing union member - is a failure.
/// </para>
/// </remarks>
public sealed class UnionReport
{
    /// <summary>How many differing entries to name in the printed output.</summary>
    public const int MaxExamples = 10;

    private UnionReport() { }

    /// <summary>Entries where <c>unified &lt; max(saveA, saveB)</c>. Non-empty means FAIL.</summary>
    public List<Difference> Downgrades { get; } = new();

    /// <summary>Entries where <c>unified &gt; max(saveA, saveB)</c>. Informational.</summary>
    public List<Difference> AboveBothSaves { get; } = new();

    /// <summary>Entries above Untouched in save A but not in save B.</summary>
    public int OnlyInA { get; private set; }

    /// <summary>Of <see cref="OnlyInA"/>, how many the unified state preserved.</summary>
    public int OnlyInAPreserved { get; private set; }

    /// <summary>Entries above Untouched in save B but not in save A.</summary>
    public int OnlyInB { get; private set; }

    /// <summary>Of <see cref="OnlyInB"/>, how many the unified state preserved.</summary>
    public int OnlyInBPreserved { get; private set; }

    /// <summary>Unified entries present in neither save. Large counts mean a stale file.</summary>
    public int InUnifiedOnly { get; private set; }

    /// <summary>The union of both saves, i.e. how many entries the rule is checked over.</summary>
    public int UnionEntryCount { get; private set; }

    /// <summary>The whole point: no entry fell below the union.</summary>
    public bool Passed => Downgrades.Count == 0;

    /// <summary>Compares a unified state against two saves.</summary>
    public static UnionReport Compare(
        UnifiedConversationState unified,
        UnifiedConversationState saveA,
        UnifiedConversationState saveB
    )
    {
        ArgumentNullException.ThrowIfNull(unified);
        ArgumentNullException.ThrowIfNull(saveA);
        ArgumentNullException.ThrowIfNull(saveB);

        var report = new UnionReport();

        // The union of both saves' keys; neither state stores Untouched, so every key
        // here is an entry that actually has to be accounted for in the unified file.
        var unionKeys = new HashSet<(int Conversation, int Entry)>();
        foreach (UnifiedStatusEntry entry in saveA.EnumerateEntries())
        {
            unionKeys.Add((entry.ConversationId, entry.DialogueEntryId));
        }
        foreach (UnifiedStatusEntry entry in saveB.EnumerateEntries())
        {
            unionKeys.Add((entry.ConversationId, entry.DialogueEntryId));
        }

        report.UnionEntryCount = unionKeys.Count;

        foreach ((int conversationId, int entryId) in unionKeys)
        {
            SimStatus a = saveA.GetStatus(conversationId, entryId);
            SimStatus b = saveB.GetStatus(conversationId, entryId);
            SimStatus expected = a > b ? a : b;
            SimStatus actual = unified.GetStatus(conversationId, entryId);

            if (actual < expected)
            {
                report.Downgrades.Add(new Difference(conversationId, entryId, expected, actual));
            }
            else if (actual > expected)
            {
                report.AboveBothSaves.Add(new Difference(conversationId, entryId, expected, actual));
            }

            bool inA = a > SimStatus.Untouched;
            bool inB = b > SimStatus.Untouched;
            bool preserved = actual >= expected;
            if (inA && !inB)
            {
                report.OnlyInA++;
                if (preserved)
                {
                    report.OnlyInAPreserved++;
                }
            }
            else if (inB && !inA)
            {
                report.OnlyInB++;
                if (preserved)
                {
                    report.OnlyInBPreserved++;
                }
            }
        }

        foreach (UnifiedStatusEntry entry in unified.EnumerateEntries())
        {
            if (!unionKeys.Contains((entry.ConversationId, entry.DialogueEntryId)))
            {
                report.InUnifiedOnly++;
            }
        }

        return report;
    }

    /// <summary>Writes the human-readable report, ending in a PASS or FAIL line.</summary>
    public void Write(TextWriter writer)
    {
        ArgumentNullException.ThrowIfNull(writer);

        writer.WriteLine($"Entries in the union of both saves : {UnionEntryCount}");
        writer.WriteLine();
        writer.WriteLine("The evidence that both playthroughs survived in one file:");
        writer.WriteLine(
            $"  Save-A-only entries preserved in unified : {OnlyInAPreserved} of {OnlyInA}"
        );
        writer.WriteLine(
            $"  Save-B-only entries preserved in unified : {OnlyInBPreserved} of {OnlyInB}"
        );
        writer.WriteLine();
        writer.WriteLine("Informational (not failures):");
        writer.WriteLine($"  Unified above both saves : {AboveBothSaves.Count}");
        writer.WriteLine($"  Unified only, in neither save : {InUnifiedOnly}");
        WriteExamples(writer, AboveBothSaves);
        writer.WriteLine();

        if (Passed)
        {
            writer.WriteLine(
                "PASS: every entry in either save is at least as high in the unified state."
            );
            return;
        }

        writer.WriteLine($"Downgraded or missing entries : {Downgrades.Count}");
        WriteExamples(writer, Downgrades);
        writer.WriteLine();
        writer.WriteLine(
            $"FAIL: {Downgrades.Count} entry(s) sit lower in the unified state than in a save."
        );
    }

    private static void WriteExamples(TextWriter writer, List<Difference> differences)
    {
        foreach (Difference difference in differences.Take(MaxExamples))
        {
            writer.WriteLine($"    {difference}");
        }

        if (differences.Count > MaxExamples)
        {
            writer.WriteLine($"    ... and {differences.Count - MaxExamples} more");
        }
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
