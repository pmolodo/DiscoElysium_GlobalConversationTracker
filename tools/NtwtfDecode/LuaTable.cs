// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.IO;
using System.Numerics;

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
    /// string, long, BigInteger, double, bool, or LuaTable.</summary>
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
