using System;
using Il2CppInterop.Runtime.InteropTypes.Arrays;

namespace GlobalConversationTracker.Persistence.Interop;

/// <summary>Extension to allow Il2CppStructArray.AsSpan()</summary>
public static class Il2CppArrayExtensions
{
    // An IL2CPP array object is laid out as:
    //
    //     Il2CppObject { void* klass; void* monitor; }   // 2 pointers
    //     Il2CppArrayBounds* bounds;                     // 1 pointer
    //     il2cpp_array_size_t max_length;                // 1 pointer-sized int
    //     <element vector starts here>
    //
    // so the elements start 4 pointers in - 0x20 on x64, NOT the 0x10 that
    // covers only the Il2CppObject header. Il2CppInterop 1.4.6 hardcodes the
    // same "4 * IntPtr.Size" in its indexer but does not expose it (the
    // ArrayStartPointer property and the built-in AsSpan() only arrive in
    // 1.5.x), so rather than hardcode it a second time we measure it: write a
    // known pattern through the indexer - which by definition lands on the
    // real element vector - and find where it shows up relative to Pointer.

    // Search far enough to cover any plausible header, in pointer-sized steps.
    private const int MaxDataOffset = 0x40;

    // Length of the pattern compared at each candidate offset. 16 pseudo-random
    // bytes make a false match against header contents effectively impossible.
    private const int ProbePatternLength = 16;

    // The probe array is oversized so that reading MaxDataOffset +
    // ProbePatternLength bytes past Pointer always stays inside the allocation.
    private const int ProbeArrayLength = MaxDataOffset + ProbePatternLength * 2;

    // Negative until measured; see DataOffset.
    private static int _dataOffset = -1;

    /// <summary>Byte at index <paramref name="i"/> of the calibration pattern.</summary>
    private static byte ProbeByte(int i) => (byte)(0xA5 ^ (i * 31));

    /// <summary>
    /// Offset, in bytes, from an Il2CppStructArray's <c>Pointer</c> to its first element.
    /// Measured once on first use against a throwaway array.
    /// </summary>
    private static unsafe int DataOffset()
    {
        if (_dataOffset >= 0) return _dataOffset;

        var probe = new Il2CppStructArray<byte>(ProbeArrayLength);
        for (int i = 0; i < ProbeArrayLength; i++) probe[i] = ProbeByte(i);

        byte* basePtr = (byte*)probe.Pointer;
        for (int offset = 0; offset <= MaxDataOffset; offset += IntPtr.Size)
        {
            bool match = true;
            for (int i = 0; i < ProbePatternLength; i++)
            {
                if (basePtr[offset + i] != ProbeByte(i))
                {
                    match = false;
                    break;
                }
            }
            if (match) return _dataOffset = offset;
        }

        throw new InvalidOperationException(
            "Could not locate the element vector of an Il2CppStructArray<byte> within "
            + $"{MaxDataOffset} bytes of its object pointer; the IL2CPP array layout is not what "
            + "this code assumes.");
    }

    /// <summary>AsSpan extension method for Il2CppStructArray</summary>
    public static unsafe Span<byte> AsSpan(this Il2CppStructArray<byte> il2cppArray)
    {
        if (il2cppArray == null) return Span<byte>.Empty;

        byte* dataAddress = (byte*)il2cppArray.Pointer + DataOffset();
        return new Span<byte>(dataAddress, il2cppArray.Length);
    }
}
