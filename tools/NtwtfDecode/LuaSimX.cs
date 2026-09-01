// SPDX-License-Identifier: MIT
using System.Text;

namespace NtwtfDecode;

/// <summary>
/// Leaves the <c>Conversation_SimX_*</c> variables out of the sparse form and
/// rebuilds them from the Conversation table.
/// </summary>
/// <remarks>
/// <para>
/// Every one of those variables is a second copy of a conversation's dialogue
/// statuses: <c>"&lt;dialogue articy id&gt;;&lt;code&gt;;..."</c>, one pair per
/// dialogue entry, where the code is the same SimStatus the Conversation table's
/// Dialog map holds. In the template save that is 1,494 strings and 2.44 MB - a
/// quarter of the whole expansion - carrying nothing the Dialog maps do not.
/// </para>
/// <para>
/// Dropping it costs the Variable table its independence: it can no longer be read
/// without the Conversation table beside it, and editing a status in one place now
/// changes both. What it does not cost is the data. Every string is rebuilt during
/// encoding and compared against the original pair for pair; only a string that
/// matches is dropped, so one that does not follow the pattern is kept verbatim
/// rather than lost.
/// </para>
/// <para>
/// The comparison ignores the order the pairs came in, and a rebuilt string uses
/// one fixed order instead. A save's own order is the game's Lua table order, which
/// is not stable even between saves of a single playthrough, and it carries nothing:
/// an id that appears more than once always carries the same status, so the string
/// says only which status each dialogue entry has.
/// </para>
/// </remarks>
public static class LuaSimX
{
    /// <summary>Prefix of every variable this handles.</summary>
    public const string VariablePrefix = "Conversation_SimX_";

    /// <summary>The property that stands in for the variables left out.</summary>
    public const string HeaderName = "_derived_simx";

    /// <summary>Conversations whose variable was derived, in the order they appeared.</summary>
    public const string ConversationsName = "_conversations";

    /// <summary>Where those variables sit among the table's entries.</summary>
    public const string PositionsName = "_at";

    /// <summary>The field of a conversation holding the id its variable is named for.</summary>
    public const string ArticyIdKey = "Articy_Id";

    /// <summary>The key of a dialogue entry's status.</summary>
    public const string SimStatusKey = LuaSparseManifest.SimStatusKey;

    private const char PairSeparator = ';';

    /// <summary>The one-letter code each status is written as.</summary>
    private static readonly (string Status, char Code)[] StatusCodes =
    {
        (LuaSparseManifest.UntouchedStatus, 'u'),
        ("WasDisplayed", 'd'),
        ("WasOffered", 'o'),
    };

    /// <summary>
    /// The variables that can be rebuilt from <paramref name="conversations"/>, as a
    /// map from entry position to the variable's name. Empty when the constants are
    /// unavailable or nothing matched.
    /// </summary>
    public static Dictionary<int, string> Derivable(
        LuaTable variables,
        LuaTable? conversations,
        SimXOrders? orders
    )
    {
        var derived = new Dictionary<int, string>();
        if (conversations is null || orders is null)
        {
            return derived;
        }
        for (int i = 0; i < variables.Count; i++)
        {
            if (
                variables.Entries[i].Key is not string name
                || !name.StartsWith(VariablePrefix, StringComparison.Ordinal)
                || variables.Entries[i].Value is not string actual
            )
            {
                continue;
            }
            int? conversation = orders.ConversationIndex(name[VariablePrefix.Length..]);
            if (
                conversation is int index
                && Rebuild(index, conversations, orders) is string rebuilt
                && SamePairs(rebuilt, actual)
            )
            {
                derived[i] = name;
            }
        }
        return derived;
    }

    /// <summary>The header naming what was left out, in the order it was left out.</summary>
    public static SparseMap Header(
        IReadOnlyDictionary<int, string> derived,
        SimXOrders orders
    )
    {
        var positions = new List<long>();
        var conversations = new List<long>();
        foreach (int position in derived.Keys.Order())
        {
            positions.Add(position);
            conversations.Add(
                orders.ConversationIndex(derived[position][VariablePrefix.Length..])!.Value
            );
        }
        var header = new SparseMap();
        header.Add(ConversationsName, SparseOrder.PackRange(conversations));
        header.Add(PositionsName, SparseOrder.PackRange(positions));
        return header;
    }

    /// <summary>
    /// Puts the left-out variables back, given the entries that were written and the
    /// header describing what is missing.
    /// </summary>
    public static void Restore(
        LuaTable variables,
        SparseMap header,
        LuaTable conversations,
        SimXOrders orders,
        string context
    )
    {
        List<long> owners = SparseOrder.UnpackRange(
            AsText(header.Find(ConversationsName), context, ConversationsName),
            context
        );
        List<long> positions = SparseOrder.UnpackRange(
            AsText(header.Find(PositionsName), context, PositionsName),
            context
        );
        if (owners.Count != positions.Count)
        {
            throw new InvalidDataException(
                $"{context}.{HeaderName} names {owners.Count} conversations but "
                    + $"{positions.Count} positions"
            );
        }

        for (int i = 0; i < positions.Count; i++)
        {
            int conversation = (int)owners[i];
            string articyId = ArticyIdOf(conversation, conversations, context);
            string rebuilt =
                Rebuild(conversation, conversations, orders)
                ?? throw new InvalidDataException(
                    $"{context}.{HeaderName} names conversation {conversation}, whose "
                        + $"dialogue entries {SimXOrders.ArticyIdsFileName} does not list"
                );
            variables.Insert((int)positions[i], VariablePrefix + articyId, rebuilt);
        }
    }

    /// <summary>
    /// The SimX string a conversation's dialogue statuses spell out, or null when
    /// the conversation or its dialogue order is not known.
    /// </summary>
    private static string? Rebuild(int conversation, LuaTable conversations, SimXOrders orders)
    {
        IReadOnlyList<(string ArticyId, int DialogueIndex)>? sequence = orders.SequenceFor(
            conversation
        );
        if (
            sequence is null
            || !conversations.TryGetValue(conversation, out object? value)
            || value is not LuaTable table
            || !table.TryGetValue("Dialog", out object? dialogValue)
            || dialogValue is not LuaTable dialog
        )
        {
            return null;
        }

        var text = new StringBuilder();
        foreach ((string articyId, int dialogueIndex) in sequence)
        {
            if (
                !dialog.TryGetValue(dialogueIndex, out object? entryValue)
                || entryValue is not LuaTable entry
                || !entry.TryGetValue(SimStatusKey, out object? status)
                || status is not string statusText
                || CodeFor(statusText) is not char code
            )
            {
                return null;
            }
            if (text.Length > 0)
            {
                text.Append(PairSeparator);
            }
            text.Append(articyId).Append(PairSeparator).Append(code);
        }
        return text.ToString();
    }

    /// <summary>
    /// Whether two SimX strings say the same thing: the same id and status pairs,
    /// in whatever order each happens to list them.
    /// </summary>
    private static bool SamePairs(string rebuilt, string actual)
    {
        string[] left = rebuilt.Split(PairSeparator);
        string[] right = actual.Split(PairSeparator);
        if (left.Length != right.Length)
        {
            return false;
        }
        var leftPairs = new List<string>(left.Length / 2);
        var rightPairs = new List<string>(right.Length / 2);
        for (int i = 0; i + 1 < left.Length; i += 2)
        {
            leftPairs.Add(left[i] + PairSeparator + left[i + 1]);
            rightPairs.Add(right[i] + PairSeparator + right[i + 1]);
        }
        leftPairs.Sort(StringComparer.Ordinal);
        rightPairs.Sort(StringComparer.Ordinal);
        return leftPairs.SequenceEqual(rightPairs, StringComparer.Ordinal);
    }

    private static char? CodeFor(string status)
    {
        foreach ((string known, char code) in StatusCodes)
        {
            if (known == status)
            {
                return code;
            }
        }
        return null;
    }

    private static string ArticyIdOf(int conversation, LuaTable conversations, string context)
    {
        if (
            conversations.TryGetValue(conversation, out object? value)
            && value is LuaTable table
            && table.TryGetValue(ArticyIdKey, out object? articyId)
            && articyId is string text
        )
        {
            return text;
        }
        throw new InvalidDataException(
            $"{context} needs conversation {conversation}'s {ArticyIdKey}, which the "
                + "Conversation table does not have"
        );
    }

    private static string AsText(object? value, string context, string name) =>
        value as string
        ?? throw new InvalidDataException($"{context}.{HeaderName}.{name} must be a string");
}
