// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Numerics;
using GlobalConversationTracker.Persistence;

namespace NtwtfDecode;

/// <summary>
/// A Lua table read out of the raw save blob.
///
/// The C# LuaTable in PixelCrushers.DialogueSystem keeps the array part (List)
/// and the hash part (Dict) separate. This type combines them into a single
/// ordered mapping, using 1-based indices as the keys for the list part - Lua
/// lists are 1-indexed by convention. So given:
///
///     list part = ["a", "b"]
///     dict part = {0: "c", 10: "d"}
///
/// we produce:
///
///     {1: "a", 2: "b", 0: "c", 10: "d"}
///
/// Insertion order is preserved (list part first, then dict part in file
/// order) so the JSON output is stable and comparable across tools.
/// </summary>
public sealed class LuaTable
{
    private readonly List<KeyValuePair<object, object?>> _entries = new();
    private readonly Dictionary<object, int> _indexByKey = new();

    /// <summary>Entries in insertion order. Values are one of: null (nil),
    /// string, int, long, BigInteger, double, bool, or LuaTable.</summary>
    public IReadOnlyList<KeyValuePair<object, object?>> Entries => _entries;

    /// <summary>Number of combined entries in the list and dict parts</summary>
    public int Count => _entries.Count;

    /// <summary>Number of leading entries stored in the table's list part.</summary>
    public int NumListEntries { get; set; }

    /// <summary>
    /// Bytes after the five top-level tables. Set only on the synthetic document root.
    /// </summary>
    public byte[] TrailingBytes { get; set; } = System.Array.Empty<byte>();

    /// <summary>Whether this is the synthetic root containing the five tables.</summary>
    public bool IsDocumentRoot { get; set; }

    /// <summary>Insert a new key/value pair</summary>
    public void Add(object key, object? value)
    {
        if (_indexByKey.ContainsKey(key))
        {
            throw new InvalidDataException(
                $"Duplicate key {LuaKey.ToKeyString(key)} found in Lua table"
            );
        }
        _indexByKey[key] = _entries.Count;
        _entries.Add(new KeyValuePair<object, object?>(key, value));
    }

    /// <summary>Insert a new key/value pair at a position rather than at the end.</summary>
    /// <remarks>
    /// Only the sparse form needs this, to put back an entry it left out; the
    /// decoders otherwise build a table in file order and only ever append.
    /// </remarks>
    public void Insert(int index, object key, object? value)
    {
        if (index < 0 || index > _entries.Count)
        {
            throw new InvalidDataException(
                $"Cannot insert at {index} in a table of {_entries.Count}"
            );
        }
        if (_indexByKey.ContainsKey(key))
        {
            throw new InvalidDataException(
                $"Duplicate key {LuaKey.ToKeyString(key)} found in Lua table"
            );
        }
        _entries.Insert(index, new KeyValuePair<object, object?>(key, value));
        // Everything at or after the insertion point has shifted along.
        _indexByKey.Clear();
        for (int i = 0; i < _entries.Count; i++)
        {
            _indexByKey[_entries[i].Key] = i;
        }
    }

    /// <summary>Analogue of Dictionary.TryGetValue</summary>
    public bool TryGetValue(object key, out object? value)
    {
        if (_indexByKey.TryGetValue(key, out int index))
        {
            value = _entries[index].Value;
            return true;
        }
        value = null;
        return false;
    }
}

/// <summary>How a decoded Lua number is stored in a table.</summary>
public static class LuaNumber
{
    /// <summary>
    /// An Int32-sized whole number as an int, so an id reads as 1 rather than 1.0
    /// and the JSON stays compact; anything else stays the double it already is.
    /// </summary>
    public static object Normalize(double value)
    {
        // Negative zero is integral and it fits, but int 0 would be written back
        // out as +0.0, so it is the one value that has to stay a double.
        if (RawDataParser.TryNumberToInt32(value, out int whole) && !IsNegativeZero(value))
        {
            return whole;
        }
        return value;
    }

    /// <summary>True for -0.0, which ordinary comparison cannot tell from 0.0.</summary>
    private static bool IsNegativeZero(double value) => value == 0 && double.IsNegative(value);
}

/// <summary>Rendering of Lua table keys as JSON object names.</summary>
public static class LuaKey
{
    /// <summary>
    /// The textual form of a table key, matching how Python's json module
    /// coerces non-string dict keys.
    /// </summary>
    public static string ToKeyString(object key) =>
        key switch
        {
            string s => s,
            // NormalizeNumber picks int, long or double by magnitude alone, so a key
            // that is conceptually the same number can arrive as any of the three.
            int i => i.ToString(System.Globalization.CultureInfo.InvariantCulture),
            long l => l.ToString(System.Globalization.CultureInfo.InvariantCulture),
            BigInteger b => b.ToString(System.Globalization.CultureInfo.InvariantCulture),
            double d => d.ToString(System.Globalization.CultureInfo.InvariantCulture),
            bool b => b ? "true" : "false",
            _ => throw new InvalidDataException(
                $"Unsupported Lua table key type {key.GetType().Name}"
            ),
        };
}
