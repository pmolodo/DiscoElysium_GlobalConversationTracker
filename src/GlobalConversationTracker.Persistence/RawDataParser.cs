// SPDX-License-Identifier: MIT
using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.IO;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;
using GlobalConversationTracker.Core;

namespace GlobalConversationTracker.Persistence;

/// <summary>
/// Walks the binary "raw data" blob stored in a {save}.ntwtf.lua file, telling an
/// <see cref="IRawDataVisitor"/> what it finds as it goes.
///
/// This is a port of PixelCrushers.DialogueSystem.PersistentDataManager's
/// ApplyRawData / ReadValue / ReadTable, which use a .NET BinaryReader over a
/// MemoryStream. The blob is five consecutive Lua tables - Actor, Item,
/// Location, Variable, Conversation - optionally followed by "extra data"
/// (length-prefixed Lua source strings) that this tool does not interpret.
///
/// The parser decides where every value ends; the visitor decides what any of it
/// means. That is what makes a caller wanting one field out of one table cheap - see
/// <see cref="SimStatusVisitor"/>, which reads the SimStatus strings without building
/// an in-memory representation of everything around them.
///
/// Binary format notes (.NET BinaryReader semantics):
///   - Int32 / Double are little-endian.
///   - Strings are prefixed with their byte length, encoded 7 bits at a time
///     (LEB128-style), followed by UTF-8 bytes.
///   - The type-code markers ('T', 'S', 'N', 'B', 'X') are read with
///     ReadChar / PeekChar; they are ASCII, so one byte each.
/// </summary>
// Implemented as ref struct so it can hold a ReadOnlySpan<byte>.
public ref struct RawDataParser
{
    /// <summary>
    /// The type-code markers, as produced by reader.ReadChar / PeekChar. They are
    /// ASCII, so the underlying value is the single byte in the file.
    /// </summary>
    public enum DataType : byte
    {
        /// <summary>A nested table: a list part followed by a hash part.</summary>
        Table = (byte)'T',

        /// <summary>A length-prefixed UTF-8 string.</summary>
        String = (byte)'S',

        /// <summary>A double.</summary>
        Number = (byte)'N',

        /// <summary>A single byte, zero for false.</summary>
        Boolean = (byte)'B',

        /// <summary>Lua nil. No payload.</summary>
        Nil = (byte)'X',
    }

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
    private readonly IRawDataVisitor _visitor;
    private int _pos;
    private int _depth;

    /// <summary>Constructor - takes bytes from a .ntwtf.lua file.</summary>
    public RawDataParser(ReadOnlySpan<byte> data, IRawDataVisitor visitor)
    {
        _data = data;
        _visitor = visitor ?? throw new ArgumentNullException(nameof(visitor));
        _pos = 0;
        _depth = 0;
    }

    /// <summary>Offset of the next byte to be consumed. Useful for error messages.</summary>
    public readonly int Position => _pos;

    /// <summary>Number of bytes not yet consumed.</summary>
    public readonly int Remaining => _data.Length - _pos;

    /// <summary>
    /// How many tables enclose the value currently being visited. Zero for the five
    /// top-level values themselves.
    /// </summary>
    public readonly int Depth => _depth;

    /// <summary>True when the byte data seems valid.</summary>
    public readonly bool IsReady() => _data.Length > 0;

    /// <summary>
    /// Walk the whole blob, calling the visitor on everything found. Afterwards
    /// <see cref="Remaining"/> is the blob's uninterpreted "extra data".
    /// </summary>
    public void Parse()
    {
        _visitor.BeginParse(ref this);
        // Read as plain values rather than assumed to be tables; the blob marks them
        // DataType.Table like any nested one.
        for (int i = 0; i < TableNames.Length; i++)
        {
            _visitor.BeginTopLevelValue(ref this, i, TableNames[i]);
            ParseValue();
        }
        _visitor.EndParse(ref this);
    }

    /// <summary>
    /// Every SimStatus the save holds, read without building the tables around them.
    /// </summary>
    public static List<SimStatusRow> GetSimStatuses(ReadOnlySpan<byte> data) =>
        GetSimStatuses(data, out _);

    /// <summary>
    /// Every SimStatus the save holds, together with how much of the blob had to be
    /// walked to find them.
    /// </summary>
    /// <param name="data">The blob, as stored in a {save}.ntwtf.lua file.</param>
    /// <param name="counts">
    /// What the parse stepped over, for a caller timing this call. The counting happens
    /// either way, so the overload without them costs nothing extra.
    /// </param>
    public static List<SimStatusRow> GetSimStatuses(
        ReadOnlySpan<byte> data,
        out SimStatusParseCounts counts
    )
    {
        SimStatusVisitor visitor = new();
        RawDataParser parser = new(data, visitor);
        parser.Parse();
        counts = new SimStatusParseCounts(
            visitor.ConversationCount,
            visitor.TableCount,
            visitor.ValueCount,
            // Whatever Parse did not consume is the blob's uninterpreted extra data.
            parser.Remaining
        );
        return visitor.Rows;
    }

    private void ParseValue()
    {
        int markerPos = _pos;
        DataType dataType = ConsumeDataType();
        _visitor.VisitValue(ref this, dataType);
        switch (dataType)
        {
            case DataType.String:
                ParseString();
                break;
            case DataType.Number:
                ParseNumber();
                break;
            case DataType.Boolean:
                ParseBoolean();
                break;
            case DataType.Table:
                ParseTable();
                break;
            case DataType.Nil:
                _visitor.VisitNil(ref this);
                break;
            default:
                throw new InvalidDataException(
                    $"ParseValue unhandled type code '{(char)dataType}' "
                        + $"(0x{(byte)dataType:X2}) at offset {markerPos}"
                );
        }
    }

    private void ParseString()
    {
        int length = Consume7BitEncodedInt();
        if (length < 0)
        {
            throw new InvalidDataException($"Negative string length {length} at offset {_pos}");
        }
        EnsureAvailable(length);
        _visitor.VisitString(ref this, length);
        _pos += length;
    }

    private void ParseNumber()
    {
        EnsureAvailable(sizeof(double));
        _visitor.VisitNumber(ref this);
        _pos += sizeof(double);
    }

    private void ParseBoolean()
    {
        EnsureAvailable(sizeof(byte));
        _visitor.VisitBoolean(ref this);
        _pos += sizeof(byte);
    }

    private void ParseTable()
    {
        _visitor.OpenTable(ref this);
        _depth++;

        int listCount = ConsumeInt32();
        if (listCount < 0)
        {
            throw new InvalidDataException(
                $"Negative list count {listCount} at offset {_pos - sizeof(int)}"
            );
        }
        _visitor.OpenList(ref this, listCount);
        for (int i = 1; i <= listCount; i++)
        {
            // Lua lists are 1-indexed by convention.
            _visitor.VisitListIndex(ref this, i);
            ParseValue();
        }
        _visitor.CloseList(ref this);

        int dictCount = ConsumeInt32();
        if (dictCount < 0)
        {
            throw new InvalidDataException(
                $"Negative dict count {dictCount} at offset {_pos - sizeof(int)}"
            );
        }
        _visitor.OpenDict(ref this, dictCount);
        for (int i = 0; i < dictCount; i++)
        {
            _visitor.OpenKeyValuePair(ref this);
            _visitor.VisitDictKey(ref this);
            // A nil key is a format error rather than a visitor's business, and the
            // marker is the only place it shows, so it is checked here once for every
            // visitor rather than left to whichever one happens to build a table.
            if (PeekDataType() == DataType.Nil)
            {
                throw new InvalidDataException($"nil table key at offset {_pos}");
            }
            ParseValue();
            _visitor.VisitDictValue(ref this);
            ParseValue();
            _visitor.CloseKeyValuePair(ref this);
        }
        _visitor.CloseDict(ref this);

        _depth--;
        _visitor.CloseTable(ref this);
    }

    /// <summary>
    /// Collapses integer-valued doubles to integers, so IDs come out as 1
    /// rather than 1.0. Out of int/long range, or not integral at all - which
    /// includes inf and nan - it stays a double.
    /// </summary>
    public static object NormalizeNumber(double value)
    {
        if (TryNumberToInt32(value, out int asInt))
        {
            return asInt;
        }
        // double.IsInteger() is not available until .NET 7.
        if (value % 1 != 0)
        {
            return value;
        }
        if (value >= long.MinValue && value <= long.MaxValue)
        {
            return Convert.ToInt64(value);
        }
        // We shouldn't get huge integers in this format - keep as double.
        return value;
    }

    /// <summary>
    /// The Int32 an integral double stands for, without boxing it first. False when
    /// the value has a fractional part or does not fit, which is also false for inf
    /// and nan.
    /// </summary>
    public static bool TryNumberToInt32(double value, out int result)
    {
        if (value % 1 == 0 && value >= int.MinValue && value <= int.MaxValue)
        {
            result = (int)value;
            return true;
        }
        result = 0;
        return false;
    }

    // ---------------------------------------------------------------------------
    // Peek / Consume
    //
    // Every fixed-size read is one of these two shapes over the same decoding: Peek
    // leaves the cursor alone, Consume advances it. Both bounds-check first, so the
    // Interpreter that does the decoding does not have to.
    // ---------------------------------------------------------------------------

    /// <summary>
    /// One fixed-size decoding of the bytes at an offset, plus how many bytes that
    /// takes. Implementations are structs used as generic type arguments, so the JIT
    /// specializes <see cref="Peek{TInterpreter, T}"/> and
    /// <see cref="Consume{TInterpreter, T}"/> per implementation and inlines both
    /// <see cref="Size"/> and <see cref="Interpret"/> away.
    /// </summary>
    /// <remarks>
    /// Size is an interface member rather than an attribute so it is a constant to the
    /// JIT; an attribute would only be readable by reflection.
    /// </remarks>
    private interface IInterpreter<T>
    {
        /// <summary>Bytes consumed by one <see cref="Interpret"/>.</summary>
        int Size { get; }

        /// <summary>
        /// Decodes at <paramref name="pos"/> without bounds-checking - the caller has
        /// already guaranteed <see cref="Size"/> bytes are there.
        /// </summary>
        T Interpret(ReadOnlySpan<byte> data, int pos);
    }

    private readonly struct ByteInterpreter : IInterpreter<byte>
    {
        public int Size => sizeof(byte);

        public byte Interpret(ReadOnlySpan<byte> data, int pos) =>
            Unsafe.Add(ref MemoryMarshal.GetReference(data), pos);
    }

    private readonly struct BooleanInterpreter : IInterpreter<bool>
    {
        public int Size => sizeof(byte);

        public bool Interpret(ReadOnlySpan<byte> data, int pos) =>
            Unsafe.Add(ref MemoryMarshal.GetReference(data), pos) != 0;
    }

    private readonly struct Int32Interpreter : IInterpreter<int>
    {
        public int Size => sizeof(int);

        public int Interpret(ReadOnlySpan<byte> data, int pos)
        {
            int value = Unsafe.ReadUnaligned<int>(
                ref Unsafe.Add(ref MemoryMarshal.GetReference(data), pos)
            );
            return BitConverter.IsLittleEndian ? value : BinaryPrimitives.ReverseEndianness(value);
        }
    }

    private readonly struct DoubleInterpreter : IInterpreter<double>
    {
        public int Size => sizeof(double);

        public double Interpret(ReadOnlySpan<byte> data, int pos)
        {
            long bits = Unsafe.ReadUnaligned<long>(
                ref Unsafe.Add(ref MemoryMarshal.GetReference(data), pos)
            );
            if (!BitConverter.IsLittleEndian)
            {
                bits = BinaryPrimitives.ReverseEndianness(bits);
            }
            return BitConverter.Int64BitsToDouble(bits);
        }
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private readonly T Peek<TInterpreter, T>()
        where TInterpreter : struct, IInterpreter<T>
    {
        TInterpreter interpreter = default;
        EnsureAvailable(interpreter.Size);
        return interpreter.Interpret(_data, _pos);
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private T Consume<TInterpreter, T>()
        where TInterpreter : struct, IInterpreter<T>
    {
        TInterpreter interpreter = default;
        int size = interpreter.Size;
        EnsureAvailable(size);
        T value = interpreter.Interpret(_data, _pos);
        _pos += size;
        return value;
    }

    /// <summary>The byte at the cursor, without advancing it.</summary>
    public readonly byte PeekByte() => Peek<ByteInterpreter, byte>();

    /// <summary>The boolean at the cursor, without advancing it.</summary>
    public readonly bool PeekBoolean() => Peek<BooleanInterpreter, bool>();

    /// <summary>The little-endian Int32 at the cursor, without advancing it.</summary>
    public readonly int PeekInt32() => Peek<Int32Interpreter, int>();

    /// <summary>The little-endian double at the cursor, without advancing it.</summary>
    public readonly double PeekDouble() => Peek<DoubleInterpreter, double>();

    /// <summary>
    /// The number at the cursor as int, long or double, whichever it fits - see
    /// <see cref="NormalizeNumber"/>. Boxes; prefer <see cref="PeekDouble"/> plus
    /// <see cref="TryNumberToInt32"/> where the box is not wanted.
    /// </summary>
    public readonly object PeekNumber() => NormalizeNumber(PeekDouble());

    /// <summary>
    /// The next <paramref name="count"/> bytes, without advancing. Lets a visitor
    /// compare a string against a known name without decoding it.
    /// </summary>
    public readonly ReadOnlySpan<byte> PeekBytes(int count)
    {
        EnsureAvailable(count);
        return _data.Slice(_pos, count);
    }

    /// <summary>
    /// The next <paramref name="length"/> bytes decoded as UTF-8, without advancing.
    /// The length is the one handed to <see cref="IRawDataVisitor.VisitString"/>.
    /// </summary>
    public readonly string PeekString(int length) => Utf8.GetString(PeekBytes(length));

    /// <summary>The type marker at the cursor, without advancing.</summary>
    public readonly DataType PeekDataType() => ToDataType(PeekByte(), _pos);

    private byte ConsumeByte() => Consume<ByteInterpreter, byte>();

    private int ConsumeInt32() => Consume<Int32Interpreter, int>();

    private DataType ConsumeDataType()
    {
        int markerPos = _pos;
        return ToDataType(ConsumeByte(), markerPos);
    }

    private static DataType ToDataType(byte marker, int offset)
    {
        switch ((DataType)marker)
        {
            case DataType.Table:
            case DataType.String:
            case DataType.Number:
            case DataType.Boolean:
            case DataType.Nil:
                return (DataType)marker;
            default:
                throw new InvalidDataException(
                    $"Unhandled type code '{(char)marker}' (0x{marker:X2}) at offset {offset}"
                );
        }
    }

    /// <summary>.NET string-length prefix: base-128, low 7 bits per byte, high bit = continue.</summary>
    private int Consume7BitEncodedInt()
    {
        int count = 0;
        int shift = 0;
        while (true)
        {
            if (shift > 4 * 7)
            {
                throw new InvalidDataException($"Malformed 7-bit encoded int at offset {_pos}");
            }
            byte b = ConsumeByte();
            count |= (b & 0x7F) << shift;
            if ((b & 0x80) == 0)
            {
                break;
            }
            shift += 7;
        }
        return count;
    }

    private readonly void EnsureAvailable(int count)
    {
        if (Remaining < count)
        {
            throw new EndOfStreamException(
                $"Unexpected end of data at offset {_pos}: need {count} bytes, have {Remaining}"
            );
        }
    }
}
