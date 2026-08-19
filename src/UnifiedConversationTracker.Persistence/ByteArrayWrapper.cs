using System;
using Il2CppInterop.Runtime;
using Il2CppInterop.Runtime.InteropTypes.Arrays;

/// <summary>
/// A wrapper for a byte array that can be either a managed byte[] or an Il2CppStructArray&lt;byte&gt;.
/// </summary>
public readonly ref struct ByteArrayWrapper
{
    private readonly byte[]? _managed;
    private readonly Il2CppStructArray<byte>? _il2cpp;

    /// <summary>
    /// Offset into the underlying data source for slices.
    /// </summary>
    public int Offset { get; }

    /// <summary>
    /// Length of the data slice represented by this wrapper. This is the length of the slice, not the underlying array.
    /// </summary>
    public int Length { get; }

    /// <summary>
    /// Constructor from just a byte[]
    /// </summary>
    public ByteArrayWrapper(byte[] managed) : this(managed, 0, managed.Length) { }

    /// <summary>
    /// Constructor from just an Il2CppStructArray
    /// </summary>
    public ByteArrayWrapper(Il2CppStructArray<byte> il2cpp) : this(il2cpp, 0, il2cpp.Length) { }

    private ByteArrayWrapper(byte[] managed, int offset, int length)
    {
        _managed = managed;
        _il2cpp = null;
        Offset = offset;
        Length = length;
        if (Offset + Length > _managed.Length)
            throw new ArgumentOutOfRangeException(nameof(length), "Slice exceeds array bounds.");
    }

    private ByteArrayWrapper(Il2CppStructArray<byte> il2cpp, int offset, int length)
    {
        _il2cpp = il2cpp;
        _managed = null;
        Offset = offset;
        Length = length;
        if (Offset + Length > _il2cpp.Length)
            throw new ArgumentOutOfRangeException(nameof(length), "Slice exceeds array bounds.");
    }

    /// <summary>
    /// Indexer (respects the current slice offset)
    /// </summary>
    /// <param name="i"></param>
    /// <returns></returns>
    /// <exception cref="IndexOutOfRangeException"></exception>
    public byte this[int i]
    {
        get
        {
            if ((uint)i >= (uint)Length) throw new IndexOutOfRangeException();
            return _managed != null ? _managed[Offset + i] : _il2cpp![Offset + i];
        }
    }

    /// <summary>
    /// Slicing Support (Returns a new ByteArrayWrapper window)
    /// </summary>
    /// <param name="start"></param>
    /// <returns></returns>
    public ByteArrayWrapper Slice(int start) => Slice(start, Length - start);

    /// <summary>
    /// Slicing Support (Returns a new ByteArrayWrapper window)
    /// </summary>
    /// <param name="start"></param>
    /// <param name="length"></param>
    /// <returns></returns>
    public ByteArrayWrapper Slice(int start, int length)
    {
        if ((uint)start > (uint)Length || (uint)(start + length) > (uint)Length)
            throw new ArgumentOutOfRangeException();

        return _managed != null
            ? new ByteArrayWrapper(_managed, Offset + start, length)
            : new ByteArrayWrapper(_il2cpp!, Offset + start, length);
    }

    /// <summary>
    /// Copies the contents of this ByteArrayWrapper to a destination Span&lt;byte&gt;.
    /// </summary>
    /// <param name="destination">The destination span to copy to.</param>
    /// <exception cref="ArgumentException">Thrown if the destination span is too short</exception>
    public void CopyTo(Span<byte> destination)
    {
        if (destination.Length < Length)
            throw new ArgumentException("Destination span is too short.");

        if (_managed != null)
        {
            new ReadOnlySpan<byte>(_managed, Offset, Length).CopyTo(destination);
        }
        else if (_il2cpp != null)
        {
            // Fallback loop for IL2CPP array without allocating a new managed array
            for (int i = 0; i < Length; i++)
            {
                destination[i] = _il2cpp[Offset + i];
            }
        }
    }

    /// <summary>
    /// Implicit conversion from byte[] to ByteArrayWrapper
    /// </summary>
    /// <param name="array"></param>
    public static implicit operator ByteArrayWrapper(byte[] array) => new(array);

    /// <summary>
    /// Implicit conversion from Il2CppStructArray&lt;byte&gt; to ByteArrayWrapper
    /// </summary>
    /// <param name="array"></param>
    public static implicit operator ByteArrayWrapper(Il2CppStructArray<byte> array) => new(array);
}
