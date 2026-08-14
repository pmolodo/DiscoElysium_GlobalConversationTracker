using System.Buffers.Binary;
using System.Numerics;
using System.Text;

namespace NtwtfDecode;

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
public sealed class RawDataReader
{
    // Type-code markers (as produced by reader.ReadChar / PeekChar).
    private const byte TableMarker = (byte)'T';
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

    private readonly byte[] _data;
    private int _pos;

    public RawDataReader(byte[] data)
    {
        _data = data;
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
            result.Add(name, reader.ReadTable());
        }
        trailingBytes = reader.Remaining;
        return result;
    }

    public LuaTable ReadTable()
    {
        ReadByte(); // consume the 'T' table marker (C#: reader.Read())
        var table = new LuaTable();

        int listCount = ReadInt32();
        if (listCount < 0)
        {
            throw new InvalidDataException($"Negative list count {listCount} at offset {_pos - 4}");
        }
        for (int i = 1; i <= listCount; i++)
        {
            // Lua lists are 1-indexed by convention.
            table.Add((long)i, ReadValue());
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

    public object? ReadValue()
    {
        if (PeekByte() == TableMarker)
        {
            return ReadTable();
        }

        char code = (char)ReadByte();
        switch (code)
        {
            case StringCode:
                return ReadString();
            case NumberCode:
                return NormalizeNumber(ReadDouble());
            case BooleanCode:
                return ReadBoolean();
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
        if (!double.IsInteger(value))
        {
            return value;
        }
        if (value >= long.MinValue && value <= long.MaxValue)
        {
            return (long)value;
        }
        // Huge integral doubles still have an exact integer value; keep it
        // exact rather than losing it to long overflow.
        return new BigInteger(value);
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
        int value = BinaryPrimitives.ReadInt32LittleEndian(_data.AsSpan(_pos));
        _pos += sizeof(int);
        return value;
    }

    private double ReadDouble()
    {
        EnsureAvailable(sizeof(double));
        double value = BinaryPrimitives.ReadDoubleLittleEndian(_data.AsSpan(_pos));
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
        string value = Utf8.GetString(_data, _pos, length);
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
