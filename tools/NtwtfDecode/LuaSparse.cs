// SPDX-License-Identifier: MIT
using System.Globalization;

namespace NtwtfDecode;

/// <summary>
/// Converts between a Lua table and the sparse JSON representation of it.
/// </summary>
/// <remarks>
/// <para>
/// The dense form in <see cref="LuaJson"/> mirrors the blob's layout entry for
/// entry. The sparse form gives that up: it restructures whole tables where the
/// data has a shape worth exploiting, and keeps only enough bookkeeping to put
/// the original layout back. What it does not give up is exactness - every save
/// still converts back byte for byte.
/// </para>
/// <para>
/// Only tables named by <see cref="LuaSparseManifest"/> are restructured, and
/// only when they really have the shape the manifest claims. Everything else is
/// written exactly as the dense form writes it, so the two are the same format
/// wherever no rule applies.
/// </para>
/// </remarks>
public static class LuaSparse
{
    /// <summary>Marks a grouped table and lists the keys it stands for.</summary>
    public const string KeysName = "_keys";

    /// <summary>Moves that restore a grouped table's original entry order.</summary>
    public const string ReorderName = "_reorder";

    /// <summary>The list boundary, spelled as the dense form spells it.</summary>
    public const string ListCountName = "_num_list_entries";

    /// <summary>Property names that are bookkeeping rather than a table's own entry.</summary>
    private static readonly string[] ReservedNames = { ListCountName, KeysName, ReorderName };

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
        if (table.NumListEntries > 0)
        {
            map.Add(ListCountName, table.NumListEntries);
        }
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
        if (table.NumListEntries > 0)
        {
            map.Add(ListCountName, table.NumListEntries);
        }
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
        int listCount = 0;
        var table = new LuaTable();
        var used = new HashSet<string>(StringComparer.Ordinal);
        foreach (KeyValuePair<string, object?> entry in map.Entries)
        {
            if (entry.Key == ListCountName && table.Count == 0)
            {
                listCount = AsCount(entry.Value, path);
                continue;
            }
            if (entry.Key == LuaSimX.HeaderName)
            {
                // Put back by the caller, once the whole table has been read.
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
            object key = LuaKeys.Parse(entry.Key, path, table.Count < listCount, table.Count + 1);
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

        List<long> canonical = CanonicalOrder(keys, table.NumListEntries);
        if (canonical.Count != keys.Count)
        {
            // Duplicate keys cannot happen in a LuaTable, so this would be a bug.
            throw new InvalidDataException($"Table '{path}' has a key more than once");
        }

        grouped = new SparseMap();
        if (table.NumListEntries > 0)
        {
            grouped.Add(ListCountName, table.NumListEntries);
        }
        var sorted = new List<long>(keys);
        sorted.Sort();
        grouped.Add(KeysName, SparseOrder.PackRange(sorted));
        List<(int From, int To)> moves = SparseOrder.Moves(canonical, keys);
        if (moves.Count > 0)
        {
            grouped.Add(ReorderName, SparseOrder.PackMoves(moves));
        }
        foreach (KeyValuePair<string, List<long>> bucket in byValue)
        {
            bucket.Value.Sort();
            grouped.Add(bucket.Key, SparseOrder.PackRange(bucket.Value));
        }
        return true;
    }

    private static LuaTable DecodeGrouped(SparseMap map, string path, LuaValueGrouping grouping)
    {
        int listCount = 0;
        string? range = null;
        object? reorder = null;
        var byKey = new Dictionary<long, string>();
        foreach (KeyValuePair<string, object?> entry in map.Entries)
        {
            switch (entry.Key)
            {
                case ListCountName:
                    listCount = AsCount(entry.Value, path);
                    break;
                case KeysName:
                    range = entry.Value as string
                        ?? throw new InvalidDataException($"{path}.{KeysName} must be a string");
                    break;
                case ReorderName:
                    reorder = entry.Value;
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
        List<long> order = SparseOrder.ApplyMoves(
            CanonicalOrder(sorted, listCount),
            SparseOrder.UnpackMoves(reorder, path)
        );
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

    /// <summary>
    /// The order a grouped table's keys are assumed to be in: the list part, which
    /// is always 1..n, and then the rest ascending. Where the real order differs -
    /// about a third of the template's dialogue maps - the difference is a move or
    /// two, which is what makes recording it cheap.
    /// </summary>
    private static List<long> CanonicalOrder(IReadOnlyList<long> keys, int listCount)
    {
        var rest = new List<long>();
        var order = new List<long>(keys.Count);
        var inList = new HashSet<long>();
        for (int i = 1; i <= listCount; i++)
        {
            inList.Add(i);
            order.Add(i);
        }
        foreach (long key in keys)
        {
            if (!inList.Contains(key))
            {
                rest.Add(key);
            }
        }
        rest.Sort();
        order.AddRange(rest);
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

    private static int AsCount(object? value, string path) =>
        value switch
        {
            int i when i >= 0 => i,
            _ => throw new InvalidDataException(
                $"{path}.{ListCountName} must be a non-negative whole number"
            ),
        };

}

/// <summary>Turning a sparse property name back into the Lua key it stands for.</summary>
internal static class LuaKeys
{
    /// <summary>
    /// The key a property name stands for. A list entry is its own 1-based index;
    /// a dictionary key takes its type from <see cref="LuaKeyTypeManifest"/>, the
    /// same way the dense form reads it.
    /// </summary>
    public static object Parse(string name, string path, bool inListPart, int index)
    {
        if (inListPart)
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
        return LuaJson.ParseDictionaryKey(name, LuaKeyTypeManifest.ExpectedType(path), path);
    }
}
