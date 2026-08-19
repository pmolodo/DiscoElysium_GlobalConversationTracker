using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.IO;
using System.Numerics;
using System.Text;
using Il2CppInterop.Runtime.InteropTypes.Arrays;
using Microsoft.VisualBasic;
using UnifiedConversationTracker.Core;
using UnifiedConversationTracker.Persistence.Interop;

namespace UnifiedConversationTracker.Persistence;

/// <summary>
/// Reads the binary "raw data" blob stored in a {save}.ntwtf.lua file.
///
/// This is a port of PixelCrushers.DialogueSystem.PersistentDataManager's
/// ApplyRawData / ReadValue / ReadTable, which use a .NET BinaryReader over a
/// MemoryStream. The blob is five consecutive Lua tables - Actor, Item,
/// Location, Variable, Conversation - optionally followed by "extra data"
/// (length-prefixed Lua source strings) that this tool does not interpret.
///
/// Binary format notes (.NET BinaryReader semantics):
///   - Int32 / Double are little-endian.
///   - Strings are prefixed with their byte length, encoded 7 bits at a time
///     (LEB128-style), followed by UTF-8 bytes.
///   - The type-code markers ('T', 'S', 'N', 'B', 'X') are read with
///     ReadChar / PeekChar; they are ASCII, so one byte each.
/// </summary>

// Implemented as ref struct so it can hold a ReadOnlySpan<byte>
public ref struct RawDataReader
{
    /// <summary>
    /// Source of this data - used like ISimStatusSource.Description and
    /// ISimStatusInterceptor.Description
    /// </summary>
    public const string Description = "raw bytes of .ntwtf.lua save file";

    // Type-code markers (as produced by reader.ReadChar / PeekChar).
    private const char TableMarker = 'T';
    private const char StringCode = 'S';
    private const char NumberCode = 'N';
    private const char BooleanCode = 'B';
    private const char NilCode = 'X';

    /// <summary>Names of the five top-level tables, in file order.</summary>
    public static readonly string[] TableNames =
    {
        "Actor",
        "Item",
        "Location",
        "Variable",
        "Conversation",
    };

    // Decode strictly: silently substituting U+FFFD would hide format errors.
    private static readonly UTF8Encoding Utf8 = new(
        encoderShouldEmitUTF8Identifier: false,
        throwOnInvalidBytes: true
    );

    private readonly ReadOnlySpan<byte> _data;
    private int _pos;

    /// <summary>Constructor - takes bytes from .ntwtf.lua file.</summary>
    public RawDataReader(ReadOnlySpan<byte> data)
    {
        _data = data;
    }

    /// <summary>
    /// Constructor - takes bytes from ApplyRawData prefix hook.
    ///
    /// The span aliases the IL2CPP array's element vector in place - nothing is copied - so the
    /// caller must keep <paramref name="data"/> alive (GC.KeepAlive) until this reader is done.
    /// Note that the implicit Il2CppStructArray-to-byte[] conversion would copy the whole blob,
    /// which is exactly what this overload exists to avoid.
    /// </summary>
    public RawDataReader(Il2CppStructArray<byte> data)
        : this(data.AsSpan())
    {
    }

    /// <summary>
    /// True when the byte data seems valid.
    /// </summary>
    public bool IsReady()
    {
        return _data.Length > 0;
    }

    private static string GetTypeName(object? obj)
    {
        if (obj == null)
        {
            return "<null>";
        }
        else
        {
            var typeObj = obj.GetType();
            if (typeObj.FullName == null)
            {
                return typeObj.ToString();
            }
            else
            {
                return typeObj.FullName;
            }
        }
    }

    /// <summary>
    /// Every SimStatus the save currently holds. Only called when
    /// <see cref="IsReady"/> is true.
    /// </summary>
    public List<SimStatusRow> GetSimStatuses()
    {
        // We only want the last table - skip the others
        for (int i = 0; i < TableNames.Length - 1; i++)
        {
            // Read just to advance the pointer
            ReadTable(consumeMarker: true);
        }

        // Could probably do this nicer by implementing a dedicated iterator,
        // and making ReadTable() use that, rather than vice-versa, but this
        // is good enough for now...
        LuaTable conversations = ReadTable(consumeMarker: true);

        List<SimStatusRow> rows = new();
        foreach (KeyValuePair<object, object?> convoPair in conversations.Entries)
        {
            if (convoPair.Key is int convID)
            {
                if (convoPair.Value is LuaTable convTable)
                {
                    if (!convTable.TryGetValue("Dialog", out object? dialogValue))
                    {
                        throw new InvalidDataException($"Conversation ID {convID} did not have a 'Dialog' entry");
                    }
                    if (dialogValue == null)
                    {
                        throw new InvalidDataException($"Conversation ID {convID} had a null 'Dialog' entry");
                    }
                    if (dialogValue is LuaTable dialogsTable)
                    {
                        foreach (KeyValuePair<object, object?> dialoguePair in dialogsTable.Entries)
                        {
                            if (dialoguePair.Key is int dialogueID)
                            {
                                if (dialoguePair.Value is LuaTable dialogData)
                                {
                                    if (!dialogData.TryGetValue("SimStatus", out object? statusObj))
                                    {
                                        throw new InvalidDataException($"Conversation {convID}, dialogue {dialogueID} did not have a 'SimStatus' entry");
                                    }
                                    if (statusObj == null)
                                    {
                                        throw new InvalidDataException($"Conversation {convoPair.Key}, dialogue {dialogueID} had a null 'SimStatus' entry");
                                    }
                                    if (statusObj is string status)
                                    {
                                        rows.Add(new SimStatusRow(convID, dialogueID, status));
                                    }
                                    else
                                    {
                                        throw new InvalidDataException($"Non-string value for conversation {convoPair.Key}, dialogue {dialogueID} 'SimStatus' entry: {GetTypeName(statusObj)}");
                                    }

                                }
                                else
                                {
                                    throw new InvalidDataException($"Non-table value for conversation {convID}, dialogue {dialogueID}: '{GetTypeName(dialoguePair.Value)}'");
                                }
                            }
                            else
                            {
                                throw new InvalidDataException($"Non-int dialog ID for converstion {convID}: '{dialoguePair.Key}' - {GetTypeName(dialoguePair.Key)}");
                            }
                        }

                    }
                    else
                    {
                        throw new InvalidDataException($"Non-table value for conversation ID {convID} 'Dialog' entry: '{GetTypeName(dialogValue)}'");
                    }
                }
                else
                {
                    throw new InvalidDataException($"Non-table value for conversation ID {convID}: '{GetTypeName(convoPair.Value)}'");
                }
            }
            else
            {
                throw new InvalidDataException($"Non-int conversation ID: '{convoPair.Key}' - {GetTypeName(convoPair.Key)}");
            }
        }
        return rows;
    }

    /// <summary>Number of bytes not yet consumed.</summary>
    public int Remaining => _data.Length - _pos;

    /// <summary>Reads the five top-level tables, keyed by name in file order.</summary>
    public static LuaTable ReadAllTables(byte[] data, out int trailingBytes)
    {
        var reader = new RawDataReader(data);
        var result = new LuaTable();
        foreach (string name in TableNames)
        {
            result.Add(name, reader.ReadTable(consumeMarker: true));
        }
        trailingBytes = reader.Remaining;
        return result;
    }

    /// <summary>Consume a lua table entry from the byte stream.</summary>
    public LuaTable ReadTable(bool consumeMarker = false)
    {
        if (consumeMarker)
        {
            char code = (char)ReadByte();
            if (code != TableMarker)
            {
                throw new InvalidDataException(
                    $"ReadTable expected table marker 'T' (0x{(int)TableMarker:X2}), got '{code}' (0x{(int)code:X2}) at offset {_pos - 1}"
                );
            }
        }
        var table = new LuaTable();

        int listCount = ReadInt32();
        if (listCount < 0)
        {
            throw new InvalidDataException($"Negative list count {listCount} at offset {_pos - 4}");
        }
        for (int i = 1; i <= listCount; i++)
        {
            // Lua lists are 1-indexed by convention.
            table.Add(i, ReadValue());
        }

        int dictCount = ReadInt32();
        if (dictCount < 0)
        {
            throw new InvalidDataException($"Negative dict count {dictCount} at offset {_pos - 4}");
        }
        for (int i = 0; i < dictCount; i++)
        {
            object? key = ReadValue();
            object? value = ReadValue();
            if (key is null)
            {
                throw new InvalidDataException($"nil table key at offset {_pos}");
            }
            table.Add(key, value);
        }
        return table;
    }

    /// <summary>Consume a lua value from the byte stream.</summary>
    public object? ReadValue()
    {
        char code = (char)ReadByte();
        switch (code)
        {
            case StringCode:
                return ReadString();
            case NumberCode:
                return NormalizeNumber(ReadDouble());
            case BooleanCode:
                return ReadBoolean();
            case TableMarker:
                return ReadTable(consumeMarker: false);
            case NilCode:
                return null;
            default:
                throw new InvalidDataException(
                    $"ReadValue unhandled type code '{code}' (0x{(int)code:X2}) at offset {_pos - 1}"
                );
        }
    }

    /// <summary>
    /// Collapses integer-valued doubles to integers, so IDs come out as 1
    /// rather than 1.0. IsInteger is false for inf/nan, so those stay doubles.
    /// </summary>
    private static object NormalizeNumber(double value)
    {
        // double.IsInteger() not available until .NET 7
        if (value % 1 != 0)
        {
            return value;
        }
        if (value >= int.MinValue && value <= int.MaxValue)
        {
            return Convert.ToInt32(value);
        }
        if (value >= long.MinValue && value <= long.MaxValue)
        {
            return Convert.ToInt64(value);
        }
        // We shouldn't get huge integers in this format - keep as double
        return value;
    }

    private byte ReadByte()
    {
        if (_pos >= _data.Length)
        {
            throw new EndOfStreamException($"Unexpected end of data at offset {_pos}");
        }
        return _data[_pos++];
    }

    /// <summary>Mirrors PeekChar: returns -1 at end of stream, does not advance.</summary>
    private int PeekByte() => _pos >= _data.Length ? -1 : _data[_pos];

    private int ReadInt32()
    {
        EnsureAvailable(sizeof(int));
        int value = BinaryPrimitives.ReadInt32LittleEndian(_data.Slice(_pos, sizeof(int)));
        _pos += sizeof(int);
        return value;
    }

    private double ReadDouble()
    {
        EnsureAvailable(sizeof(double));
        double value = BinaryPrimitives.ReadDoubleLittleEndian(_data.Slice(_pos, sizeof(double)));
        _pos += sizeof(double);
        return value;
    }

    private bool ReadBoolean() => ReadByte() != 0;

    /// <summary>.NET string-length prefix: base-128, low 7 bits per byte, high bit = continue.</summary>
    private int Read7BitEncodedInt()
    {
        int count = 0;
        int shift = 0;
        while (true)
        {
            if (shift > 4 * 7)
            {
                throw new InvalidDataException($"Malformed 7-bit encoded int at offset {_pos}");
            }
            byte b = ReadByte();
            count |= (b & 0x7F) << shift;
            if ((b & 0x80) == 0)
            {
                break;
            }
            shift += 7;
        }
        return count;
    }

    private string ReadString()
    {
        int length = Read7BitEncodedInt();
        if (length < 0)
        {
            throw new InvalidDataException($"Negative string length {length} at offset {_pos}");
        }
        EnsureAvailable(length);
        string value = Utf8.GetString(_data.Slice(_pos, length));
        _pos += length;
        return value;
    }

    private void EnsureAvailable(int count)
    {
        if (Remaining < count)
        {
            throw new EndOfStreamException(
                $"Unexpected end of data at offset {_pos}: need {count} bytes, have {Remaining}"
            );
        }
    }
}
