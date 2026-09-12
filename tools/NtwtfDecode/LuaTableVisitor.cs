// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using GlobalConversationTracker.Persistence;

namespace NtwtfDecode;

/// <summary>
/// Builds the whole blob as nested <see cref="LuaTable"/>s - the decode
/// <c>NtwtfDecode</c> wants, where nothing is known in
/// advance about which parts matter.
///
/// The result is a single table whose five entries are the top-level tables, keyed
/// by the names in <see cref="RawDataParser.TableNames"/> in file order.
/// </summary>
public sealed class LuaTableVisitor : IRawDataVisitor
{
    /// <summary>Reads the five top-level tables, keyed by name in file order.</summary>
    public static LuaTable ReadAllTables(ReadOnlySpan<byte> data, out int trailingBytes)
    {
        LuaTableVisitor visitor = new();
        RawDataParser parser = new(data, visitor);
        parser.Parse();
        trailingBytes = parser.Remaining;
        visitor.Root.IsDocumentRoot = true;
        visitor.Root.TrailingBytes = data[^trailingBytes..].ToArray();
        return visitor.Root;
    }

    private readonly LuaTable _root = new();
    private readonly Stack<LuaTable> _open = new();

    // The key the next value will be filed under. Set by a dict key, by a list
    // index, or by the top-level table's name, and consumed by whatever value
    // follows - including a table, which is added to its parent as soon as it
    // opens, so entries keep file order.
    private object _pendingKey = 0;
    private bool _expectingKey;

    /// <summary>The five top-level tables, keyed by name in file order.</summary>
    public LuaTable Root => _root;

    private LuaTable Current => _open.Count > 0 ? _open.Peek() : _root;

    /// <inheritdoc />
    public void BeginTopLevelValue(ref RawDataParser parser, int index, string name)
    {
        _pendingKey = name;
        _expectingKey = false;
    }

    /// <inheritdoc />
    public void VisitDictKey(ref RawDataParser parser) => _expectingKey = true;

    /// <inheritdoc />
    public void VisitDictValue(ref RawDataParser parser) => _expectingKey = false;

    /// <inheritdoc />
    public void VisitListIndex(ref RawDataParser parser, int index)
    {
        _pendingKey = index;
        _expectingKey = false;
    }

    /// <inheritdoc />
    public void OpenList(ref RawDataParser parser, int count) => Current.NumListEntries = count;

    /// <inheritdoc />
    public void VisitString(ref RawDataParser parser, int length) =>
        Take(parser.PeekString(length));

    /// <inheritdoc />
    public void VisitNumber(ref RawDataParser parser) =>
        Take(LuaNumber.Normalize(parser.PeekDouble()));

    /// <inheritdoc />
    public void VisitBoolean(ref RawDataParser parser) => Take(parser.PeekBoolean());

    /// <inheritdoc />
    public void VisitNil(ref RawDataParser parser)
    {
        // The parser rejects a nil key before the value is parsed, so this can only
        // be a value.
        Current.Add(_pendingKey, null);
    }

    /// <inheritdoc />
    public void OpenTable(ref RawDataParser parser)
    {
        LuaTable table = new();
        Current.Add(_pendingKey, table);
        _open.Push(table);
        // Whatever this table's own keys turn out to be, the parent's is spent.
        _expectingKey = false;
    }

    /// <inheritdoc />
    public void CloseTable(ref RawDataParser parser) => _open.Pop();

    /// <summary>
    /// Files a decoded scalar: as the pending key if one is being read, otherwise as
    /// the value that pending key names.
    /// </summary>
    private void Take(object value)
    {
        if (_expectingKey)
        {
            _pendingKey = value;
        }
        else
        {
            Current.Add(_pendingKey, value);
        }
    }
}
