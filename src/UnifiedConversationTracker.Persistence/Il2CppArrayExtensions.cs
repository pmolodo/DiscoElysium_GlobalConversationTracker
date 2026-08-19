using System;
using Il2CppInterop.Runtime;
using Il2CppInterop.Runtime.InteropTypes.Arrays;

/// <summary>Extension to allow Il2CppStructArray.AsSpan()</summary>
public static class Il2CppArrayExtensions
{
    /// <summary>AsSpan extension method for Il2CppStructArray</summary>
    public static unsafe Span<byte> AsSpan(this Il2CppStructArray<byte> il2cppArray)
    {
        if (il2cppArray == null) return Span<byte>.Empty;

        // 1. Get the raw IntPtr of the IL2CPP object
        IntPtr rawObjectPointer = il2cppArray.Pointer;

        // 2. Add the standard IL2CPP array data offset (0x10 bytes)
        // to skip the object headers and point directly to the raw byte elements.
        byte* dataAddress = (byte*)rawObjectPointer.ToInt64() + 0x10;

        // 3. Construct a standard .NET Span pointing directly to that native memory location
        return new Span<byte>(dataAddress, il2cppArray.Length);
    }
}
