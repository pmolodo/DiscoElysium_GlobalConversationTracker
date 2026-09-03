// SPDX-License-Identifier: MIT
using System;
using System.Runtime.InteropServices;
using System.Text;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// The Rust look-ahead engine, opened over a conversation index.
    /// </summary>
    /// <remarks>
    /// <para>Owns one native handle for its lifetime. The index behind it is tens of
    /// megabytes and takes a moment to parse, so this is opened once when the plugin
    /// loads and kept - opening one per response menu would put that parse inside the
    /// frame that draws the menu.</para>
    ///
    /// <para>A <see cref="SafeHandle"/> rather than a raw pointer and a finaliser,
    /// because the failure being guarded against is a modded game: a handle leaked
    /// through an exception would hold the index for the session, and one freed twice
    /// would corrupt the heap of the process the player is playing in.</para>
    /// </remarks>
    public sealed class LookAheadLibrary : IDisposable
    {
        private readonly EngineHandle _handle;

        private LookAheadLibrary(EngineHandle handle)
        {
            _handle = handle;
        }

        /// <summary>
        /// The native library's version, for checking it is the one this was built
        /// against.
        /// </summary>
        /// <remarks>
        /// A static string on the other side, so it is read and not freed. Worth checking
        /// at load: a version mismatch discovered here is a log line, and discovered
        /// through a wrong marker is a bug report about the game.
        /// </remarks>
        public static string Version
        {
            get
            {
                IntPtr text = NativeMethods.gct_version();
                return text == IntPtr.Zero
                    ? string.Empty
                    : Marshal.PtrToStringAnsi(text) ?? string.Empty;
            }
        }

        /// <summary>
        /// Opens the engine over <paramref name="indexPath"/>.
        /// </summary>
        /// <param name="indexPath">The conversation index shipped with the mod.</param>
        /// <exception cref="ArgumentNullException">The path is null.</exception>
        /// <exception cref="InvalidOperationException">The library refused to open it.</exception>
        public static LookAheadLibrary Open(string indexPath)
        {
            if (indexPath == null)
            {
                throw new ArgumentNullException(nameof(indexPath));
            }

            // NUL-terminated UTF-8, encoded here because the library reads UTF-8 and the
            // platform's default code page is not it.
            byte[] path = Encoding.UTF8.GetBytes(indexPath + "\0");

            Status status = (Status)NativeMethods.gct_engine_open(path, out IntPtr handle);
            if (status != Status.Ok)
            {
                throw new InvalidOperationException(
                    $"the look-ahead library would not open '{indexPath}': {status}");
            }

            return new LookAheadLibrary(new EngineHandle(handle));
        }

        /// <summary>How many conversations the index holds.</summary>
        public int ConversationCount
        {
            get
            {
                Status status = (Status)NativeMethods.gct_conversation_count(
                    _handle.DangerousGetHandle(), out int count);
                return status == Status.Ok ? count : 0;
            }
        }

        /// <summary>
        /// How many entries one conversation holds, or -1 if the index has no such
        /// conversation.
        /// </summary>
        /// <remarks>
        /// The plugin's guard against a stale index. It builds its graph from the LIVE
        /// dialogue database while the library reads a file shipped with the mod, and if a
        /// game update or another mod moves the two apart, the look-ahead would be
        /// answering about a conversation the player is not in. Comparing entry counts is
        /// the cheap half of noticing.
        /// </remarks>
        public int EntryCount(int conversation)
        {
            Status status = (Status)NativeMethods.gct_entry_count(
                _handle.DangerousGetHandle(), conversation, out int count);
            return status == Status.Ok ? count : -1;
        }

        /// <summary>
        /// Every question a crawl over <paramref name="conversation"/>'s group can ask,
        /// as the JSON the engine produced.
        /// </summary>
        /// <remarks>
        /// The keys are the engine's, and the answers must come back under them exactly -
        /// see <see cref="LookAhead"/>. Cache this per conversation: the questions cannot
        /// change while the game is running, and the walk over a group's guards is not
        /// free.
        /// </remarks>
        /// <exception cref="InvalidOperationException">The group could not be built.</exception>
        public string Questions(int conversation)
        {
            Status status = (Status)NativeMethods.gct_questions(
                _handle.DangerousGetHandle(), conversation, out IntPtr json);
            if (status != Status.Ok)
            {
                throw new InvalidOperationException(
                    $"the look-ahead library would not describe conversation "
                    + $"{conversation}: {status}");
            }

            return Take(json);
        }

        /// <summary>
        /// Answers a look-ahead request, both sides as JSON.
        /// </summary>
        /// <remarks>
        /// A request the engine could not serve at all comes back as a response carrying
        /// an <c>error</c> field rather than as an exception, so a caller has one thing to
        /// parse. What throws here is what happens BEFORE there is a response: a request
        /// that is not JSON, or a library that is not there.
        /// </remarks>
        /// <exception cref="ArgumentNullException">The request is null.</exception>
        /// <exception cref="InvalidOperationException">The call itself failed.</exception>
        public string LookAhead(string requestJson)
        {
            if (requestJson == null)
            {
                throw new ArgumentNullException(nameof(requestJson));
            }

            byte[] request = Encoding.UTF8.GetBytes(requestJson + "\0");
            Status status = (Status)NativeMethods.gct_look_ahead(
                _handle.DangerousGetHandle(), request, out IntPtr json);
            if (status != Status.Ok)
            {
                throw new InvalidOperationException(
                    $"the look-ahead library refused the request: {status}");
            }

            return Take(json);
        }

        /// <summary>
        /// Reads a string the library handed out, and gives it back.
        /// </summary>
        /// <remarks>
        /// The two allocators are different, so a string from over there must be freed
        /// over there. Copied into a managed string first, then freed, in a finally so a
        /// failure to decode still returns the memory.
        /// </remarks>
        private static string Take(IntPtr json)
        {
            if (json == IntPtr.Zero)
            {
                return string.Empty;
            }

            try
            {
                return PtrToStringUtf8(json);
            }
            finally
            {
                NativeMethods.gct_string_free(json);
            }
        }

        /// <summary>
        /// Reads a NUL-terminated UTF-8 string, which netstandard2.0 cannot do for itself.
        /// </summary>
        /// <remarks>
        /// <c>Marshal.PtrToStringUTF8</c> arrived after netstandard2.0, and
        /// <c>PtrToStringAnsi</c> would read this through the platform's default code page
        /// - which is not UTF-8, and would mangle any dialogue text that left the ASCII
        /// range. The JSON crossing here carries conversation content, so that is not
        /// hypothetical.
        /// </remarks>
        private static string PtrToStringUtf8(IntPtr text)
        {
            int length = 0;
            while (Marshal.ReadByte(text, length) != 0)
            {
                length++;
            }

            if (length == 0)
            {
                return string.Empty;
            }

            byte[] bytes = new byte[length];
            Marshal.Copy(text, bytes, 0, length);
            return Encoding.UTF8.GetString(bytes);
        }

        /// <inheritdoc/>
        public void Dispose()
        {
            _handle.Dispose();
        }

        /// <summary>
        /// One native engine handle, closed exactly once.
        /// </summary>
        private sealed class EngineHandle : SafeHandle
        {
            internal EngineHandle(IntPtr handle)
                : base(IntPtr.Zero, ownsHandle: true)
            {
                SetHandle(handle);
            }

            /// <inheritdoc/>
            public override bool IsInvalid => handle == IntPtr.Zero;

            /// <inheritdoc/>
            protected override bool ReleaseHandle()
            {
                return NativeMethods.gct_engine_close(handle) == (int)Status.Ok;
            }
        }
    }
}
