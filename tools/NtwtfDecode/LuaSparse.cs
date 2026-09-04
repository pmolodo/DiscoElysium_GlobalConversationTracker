// SPDX-License-Identifier: MIT
using System.Globalization;

using GlobalConversationTracker.Core;

namespace NtwtfDecode;

/// <summary>
/// Converts between a Lua table and the sparse JSON representation of it.
/// </summary>
/// <remarks>
/// <para>
/// The dense form in <see cref="LuaJson"/> mirrors the blob's layout entry for
/// entry, so a save converts back byte for byte. The sparse form keeps the data
/// and drops the layout. It restructures whole tables where the shape is worth
/// exploiting, and it does not record two things the dense form does: how a table
/// was split between its Lua list and dictionary parts, and what order its entries
/// came in. Reading back picks one canonical answer to both.
/// </para>
/// <para>
/// So a sparse round trip is not byte for byte, and the blob it produces is not the
/// blob it read. What it does preserve is every key and every value. The bet is that
/// the game does not care where the boundary fell or what order a table's entries
/// were stored in, which is what Lua semantics say and what an actual load has to
/// confirm.
/// </para>
/// <para>
/// Only tables named by <see cref="LuaSparseManifest"/> are restructured, and only
/// when they really have the shape the manifest claims; everything else is written
/// out entry by entry.
/// </para>
/// </remarks>
public static class LuaSparse
{
    /// <summary>Marks a grouped table and lists the keys it stands for.</summary>
    public const string KeysName = "_keys";

    /// <summary>
    /// The dense form's list boundary. The sparse writer never emits it - the
    /// reader works the boundary out - but the reader still honours it, so that one
    /// reader handles both forms and a hand-written file can pin the split if it
    /// has a reason to.
    /// </summary>
    public const string ListCountName = "_num_list_entries";

    /// <summary>
    /// A name an earlier version of this format wrote to pin a table's entry order.
    /// Nothing writes it now, and it is refused rather than ignored so that a file
    /// from then fails loudly instead of quietly losing the order it recorded.
    /// </summary>
    private const string RetiredReorderName = "_reorder";

    /// <summary>Property names that are bookkeeping rather than a table's own entry.</summary>
    private static readonly string[] ReservedNames =
    {
        KeysName,
        RetiredReorderName,
        LuaJson.FormatName,
    };

    /// <summary>The table whose variables mirror another table's data.</summary>
    public const string VariableTableName = "Variable";

    /// <summary>The table those variables mirror.</summary>
    public const string ConversationTableName = "Conversation";

    /// <summary>Turns one table into its sparse tree.</summary>
    /// <param name="conversations">
    /// The save's Conversation table, when the caller has it. Only the Variable
    /// table uses it, to leave out the variables that merely repeat it.
    /// </param>
    public static SparseMap Encode(LuaTable table, string path, LuaTable? conversations = null)
    {
        if (path == VariableTableName && conversations is not null)
        {
            return EncodeVariables(table, path, conversations);
        }
        LuaValueGrouping? grouping = LuaSparseManifest.GroupingFor(path);
        if (grouping is not null && TryEncodeGrouped(table, path, grouping, out SparseMap? grouped))
        {
            return grouped;
        }
        return EncodeDense(table, path);
    }

    /// <summary>Turns a sparse tree back into the table it came from.</summary>
    /// <param name="conversations">As for <see cref="Encode"/>.</param>
    public static LuaTable Decode(object? node, string path, LuaTable? conversations = null)
    {
        if (node is not SparseMap map)
        {
            throw new InvalidDataException($"{path} must be a JSON object");
        }
        LuaValueGrouping? grouping = LuaSparseManifest.GroupingFor(path);
        if (grouping is not null && map.Has(KeysName))
        {
            return DecodeGrouped(map, path, grouping);
        }
        LuaTable table = DecodeDense(map, path);
        if (map.Find(LuaSimX.HeaderName) is SparseMap header)
        {
            if (conversations is null)
            {
                throw new InvalidDataException(
                    $"{path} leaves out variables that only the {ConversationTableName} "
                        + "table can rebuild, and it was not supplied"
                );
            }
            LuaSimX.Restore(
                table,
                header,
                conversations,
                SimXOrders.TryLoad()
                    ?? throw new InvalidDataException(
                        $"{path} needs {SimXOrders.ArticyIdsFileName}, which is not in "
                            + "this checkout"
                    ),
                path
            );
        }
        return table;
    }

    /// <summary>
    /// The Variable table, without the variables that only repeat the Conversation
    /// table. What was left out is named by a header, so the reader can put it back
    /// where it was.
    /// </summary>
    private static SparseMap EncodeVariables(LuaTable table, string path, LuaTable conversations)
    {
        SimXOrders? orders = SimXOrders.TryLoad();
        Dictionary<int, string> derived = LuaSimX.Derivable(table, conversations, orders);
        if (derived.Count == 0)
        {
            return EncodeDense(table, path);
        }

        var map = new SparseMap();
        map.Add(LuaSimX.HeaderName, LuaSimX.Header(derived, orders!));
        for (int i = 0; i < table.Count; i++)
        {
            if (derived.ContainsKey(i))
            {
                continue;
            }
            string name = LuaKey.ToKeyString(table.Entries[i].Key);
            object? value = table.Entries[i].Value;
            map.Add(name, value is LuaTable child ? Encode(child, path + "/" + name) : value);
        }
        return map;
    }

    private static SparseMap EncodeDense(LuaTable table, string path)
    {
        var map = new SparseMap();
        for (int i = 0; i < table.Count; i++)
        {
            string name = LuaKey.ToKeyString(table.Entries[i].Key);
            object? value = table.Entries[i].Value;
            map.Add(
                name,
                value is LuaTable child ? Encode(child, path + "/" + name) : value
            );
        }
        return map;
    }

    private static LuaTable DecodeDense(SparseMap map, string path)
    {
        var table = new LuaTable();
        var used = new HashSet<string>(StringComparer.Ordinal);
        int listCount = 0;
        bool stillList = true;
        bool boundaryGiven = false;
        foreach (KeyValuePair<string, object?> entry in map.Entries)
        {
            if (entry.Key == LuaJson.FormatName && table.Count == 0)
            {
                // Which representation this is; the caller has already acted on it.
                continue;
            }
            if (entry.Key == LuaSimX.HeaderName)
            {
                // Put back by the caller, once the whole table has been read.
                continue;
            }
            if (entry.Key == ListCountName && table.Count == 0)
            {
                listCount = AsCount(entry.Value, path);
                boundaryGiven = true;
                continue;
            }
            if (Array.IndexOf(ReservedNames, entry.Key) >= 0)
            {
                throw new InvalidDataException(
                    $"Table '{path}' cannot name an entry '{entry.Key}'; that name is reserved"
                );
            }
            if (!used.Add(entry.Key))
            {
                throw new InvalidDataException($"Table '{path}' has two '{entry.Key}' properties");
            }

            int index = table.Count + 1;
            bool isListEntry = boundaryGiven
                ? table.Count < listCount
                : stillList && entry.Key == index.ToString(CultureInfo.InvariantCulture);
            stillList = isListEntry;
            object key = isListEntry
                ? LuaKeys.ListIndex(entry.Key, path, index)
                : LuaJson.ParseDictionaryKey(
                    entry.Key,
                    LuaKeyTypeManifest.ExpectedType(path),
                    path
                );
            if (isListEntry && !boundaryGiven)
            {
                listCount = index;
            }
            table.Add(
                key,
                entry.Value is SparseMap child ? Decode(child, path + "/" + entry.Key) : entry.Value
            );
        }
        table.NumListEntries = listCount;
        return table;
    }

    /// <summary>
    /// Writes a table of same-shaped children as one key list per distinct value.
    /// Returns false when the table does not have that shape after all, which
    /// leaves the caller to write it densely.
    /// </summary>
    private static bool TryEncodeGrouped(
        LuaTable table,
        string path,
        LuaValueGrouping grouping,
        out SparseMap grouped
    )
    {
        grouped = null!;
        var keys = new List<long>(table.Count);
        var byValue = new List<KeyValuePair<string, List<long>>>();
        foreach (KeyValuePair<object, object?> entry in table.Entries)
        {
            if (
                entry.Key is not (int or long)
                || entry.Value is not LuaTable child
                || child.Count != 1
                || child.NumListEntries != 0
                || !Equals(child.Entries[0].Key, grouping.ChildKey)
                || child.Entries[0].Value is LuaTable
            )
            {
                return false;
            }
            long key = Convert.ToInt64(entry.Key, CultureInfo.InvariantCulture);
            keys.Add(key);
            object? value = child.Entries[0].Value;
            if (Equals(value, grouping.DefaultValue))
            {
                continue;
            }
            // The value becomes a property name, so it has to be one.
            if (value is not string name || name.Length == 0)
            {
                return false;
            }
            List<long>? bucket = null;
            foreach (KeyValuePair<string, List<long>> candidate in byValue)
            {
                if (candidate.Key == name)
                {
                    bucket = candidate.Value;
                    break;
                }
            }
            if (bucket is null)
            {
                bucket = new List<long>();
                byValue.Add(new KeyValuePair<string, List<long>>(name, bucket));
            }
            bucket.Add(key);
        }

        grouped = new SparseMap();
        var sorted = new List<long>(keys);
        sorted.Sort();
        grouped.Add(KeysName, SparseOrder.PackRange(sorted));
        foreach (KeyValuePair<string, List<long>> bucket in byValue)
        {
            bucket.Value.Sort();
            grouped.Add(bucket.Key, SparseOrder.PackRange(bucket.Value));
        }
        return true;
    }

    private static LuaTable DecodeGrouped(SparseMap map, string path, LuaValueGrouping grouping)
    {
        string? range = null;
        var byKey = new Dictionary<long, string>();
        foreach (KeyValuePair<string, object?> entry in map.Entries)
        {
            switch (entry.Key)
            {
                case KeysName:
                    range = entry.Value as string
                        ?? throw new InvalidDataException($"{path}.{KeysName} must be a string");
                    break;
                default:
                    if (entry.Value is not string keyRange)
                    {
                        throw new InvalidDataException(
                            $"{path}.{entry.Key} must be a key range"
                        );
                    }
                    foreach (long key in SparseOrder.UnpackRange(keyRange, path))
                    {
                        if (!byKey.TryAdd(key, entry.Key))
                        {
                            throw new InvalidDataException(
                                $"{path} lists key {key} under both '{byKey[key]}' and "
                                    + $"'{entry.Key}'"
                            );
                        }
                    }
                    break;
            }
        }

        List<long> sorted = SparseOrder.UnpackRange(range!, path);
        List<long> order = CanonicalOrder(sorted, out int listCount);
        foreach (long key in byKey.Keys)
        {
            if (!sorted.Contains(key))
            {
                throw new InvalidDataException($"{path} groups key {key}, which {KeysName} omits");
            }
        }

        var table = new LuaTable { NumListEntries = listCount };
        foreach (long key in order)
        {
            var child = new LuaTable();
            child.Add(
                grouping.ChildKey,
                byKey.TryGetValue(key, out string? value) ? value : grouping.DefaultValue
            );
            table.Add(Narrow(key), child);
        }
        return table;
    }

    private static int AsCount(object? value, string path) =>
        value switch
        {
            int i when i >= 0 => i,
            _ => throw new InvalidDataException(
                $"{path}.{ListCountName} must be a non-negative whole number"
            ),
        };

    /// <summary>
    /// The order a grouped table's keys are written back in: the run 1, 2, 3, ...
    /// as the list part, then whatever is left, ascending.
    /// </summary>
    /// <remarks>
    /// The save this came from may well have had them in some other order, and its
    /// own split between the list and dictionary parts. Neither is recorded, and
    /// neither is reproduced - see the note on the class.
    /// </remarks>
    private static List<long> CanonicalOrder(IReadOnlyList<long> ascending, out int listCount)
    {
        listCount = 0;
        while (listCount < ascending.Count && ascending[listCount] == listCount + 1)
        {
            listCount++;
        }
        var order = new List<long>(ascending.Count);
        for (int i = 0; i < listCount; i++)
        {
            order.Add(ascending[i]);
        }
        foreach (long key in ascending)
        {
            if (key < 1 || key > listCount)
            {
                order.Add(key);
            }
        }
        return order;
    }

    /// <summary>An Int32-sized key as an int, matching what the decoder produces.</summary>
    /// <remarks>
    /// Written out rather than as a conditional expression: <c>cond ? (int)key : key</c>
    /// would unify int and long to long and box every key as a long, which no lookup
    /// by int would then find.
    /// </remarks>
    private static object Narrow(long key)
    {
        if (key is >= int.MinValue and <= int.MaxValue)
        {
            return (int)key;
        }
        return key;
    }

}

/// <summary>Reading a list entry's property name.</summary>
internal static class LuaKeys
{
    /// <summary>
    /// The key a list entry's property name stands for, which is its own 1-based
    /// index. A name that is not that index means the file disagrees with itself
    /// about where the list part ends.
    /// </summary>
    public static object ListIndex(string name, string path, int index)
    {
        string expected = index.ToString(CultureInfo.InvariantCulture);
        if (name != expected)
        {
            throw new InvalidDataException(
                $"{path} list entry {index} must be named '{expected}', not '{name}'"
            );
        }
        return index;
    }
}
