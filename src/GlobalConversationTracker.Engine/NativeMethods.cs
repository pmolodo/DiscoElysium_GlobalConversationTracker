// SPDX-License-Identifier: MIT
using System;
using System.Runtime.InteropServices;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// The C entry points of the Rust look-ahead library, declared as they are written.
    /// </summary>
    /// <remarks>
    /// <para>A transcription and nothing else: no convenience, no defaults, no
    /// interpretation. Anything friendlier belongs in <see cref="LookAheadLibrary"/>, so
    /// that a mismatch between the two languages is a compile error here rather than a
    /// wrong answer somewhere further up.</para>
    ///
    /// <para>Every entry point returns an <c>int</c> status and writes its result through
    /// an out parameter, because the library must never let a panic cross into managed
    /// frames - it catches its own and reports <see cref="Status.Panic"/> instead. So
    /// there is no such thing as an exception from the native side, only a code.</para>
    /// </remarks>
    internal static class NativeMethods
    {
        /// <summary>
        /// The library's file name, without extension or path.
        /// </summary>
        /// <remarks>
        /// Resolved by the platform's ordinary search: beside the plugin assembly when the
        /// mod is deployed, and through a resolver the tests install when it is not.
        /// </remarks>
        internal const string Library = "GlobalConversationTracker.Native";

        /// <summary>The library's version, as a static string not to be freed.</summary>
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr gct_version();

        /// <summary>Opens the engine over a conversation index.</summary>
        /// <param name="indexPathUtf8">
        /// The path as UTF-8 bytes, NUL-terminated. Encoded by the caller rather than by
        /// the marshaller: <c>UnmanagedType.LPUTF8Str</c> does not exist in
        /// netstandard2.0, and the library reads UTF-8 and nothing else, so guessing
        /// through the platform's default code page would turn a path with an accent in
        /// it into a file that is not there.
        /// </param>
        /// <param name="handle">Receives the engine handle, on success only.</param>
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern int gct_engine_open(byte[] indexPathUtf8, out IntPtr handle);

        /// <summary>Closes an engine. Closing <see cref="IntPtr.Zero"/> is allowed.</summary>
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern int gct_engine_close(IntPtr handle);

        /// <summary>How many conversations the index holds.</summary>
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern int gct_conversation_count(IntPtr handle, out int count);

        /// <summary>How many entries one conversation holds.</summary>
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern int gct_entry_count(IntPtr handle, int conversation, out int count);

        /// <summary>Frees a string the library handed out.</summary>
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
        internal static extern void gct_string_free(IntPtr text);
    }

    /// <summary>
    /// What a native call reports. Zero is success; everything else is a reason.
    /// </summary>
    /// <remarks>
    /// Kept in step with the constants in <c>src/ffi.rs</c> by hand. There are six of
    /// them and they are append-only, so a generator would cost more than it saved - but
    /// the names and numbers must match, and <c>BridgeTests</c> checks the ones a test
    /// can provoke.
    /// </remarks>
    public enum Status
    {
        /// <summary>The call succeeded.</summary>
        Ok = 0,

        /// <summary>A handle was null, or not one the library handed out.</summary>
        BadHandle = -1,

        /// <summary>An argument was null, or a string was not valid UTF-8.</summary>
        BadArgument = -2,

        /// <summary>The conversation index could not be read.</summary>
        IndexUnreadable = -3,

        /// <summary>Something panicked inside the library. The call did nothing.</summary>
        Panic = -4,

        /// <summary>The index holds no such conversation.</summary>
        NoSuchConversation = -5,
    }
}
