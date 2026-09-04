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
        /// <param name="variablesPath">
        /// The database's variable table, shipped beside it, or null. Optional and
        /// non-fatal: without it a dialogue variable the game would not answer reads
        /// Unknown, where with it the value the database declares is used instead - which
        /// is what an unwritten variable actually is, and the only answer that gets a
        /// counter's kind right. <see cref="VariableCount"/> says whether one was read.
        /// </param>
        /// <exception cref="ArgumentNullException">The index path is null.</exception>
        /// <exception cref="InvalidOperationException">The library refused to open it.</exception>
        public static LookAheadLibrary Open(string indexPath, string? variablesPath = null)
        {
            if (indexPath == null)
            {
                throw new ArgumentNullException(nameof(indexPath));
            }

            Status status = (Status)NativeMethods.gct_engine_open(
                NulTerminated(indexPath)!,
                NulTerminated(variablesPath),
                out IntPtr handle);
            if (status != Status.Ok)
            {
                throw new InvalidOperationException(
                    $"the look-ahead library would not open '{indexPath}': {status}");
            }

            return new LookAheadLibrary(new EngineHandle(handle));
        }

        /// <summary>
        /// A string as the NUL-terminated UTF-8 the library reads, or null for null.
        /// </summary>
        /// <remarks>
        /// Encoded here rather than left to the marshaller because the library reads UTF-8
        /// and nothing else, and a guess through the platform's default code page would
        /// turn a path with an accent in it into a file that is not there.
        /// </remarks>
        private static byte[]? NulTerminated(string? text)
        {
            return text == null ? null : Encoding.UTF8.GetBytes(text + "\0");
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
        /// How many variables the deployed table declares, or 0 if none was read.
        /// </summary>
        /// <remarks>
        /// Worth logging at load for the same reason the conversation count is: a table
        /// that was not deployed, or that would not read, is a mod that still works and
        /// answers one variable in seventy-five less precisely - exactly the kind of thing
        /// that is never noticed unless a line says it.
        /// </remarks>
        public int VariableCount
        {
            get
            {
                Status status = (Status)NativeMethods.gct_variable_count(
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
        /// Every question a crawl over <paramref name="conversation"/>'s group can ask.
        /// </summary>
        /// <remarks>
        /// CACHE THIS PER CONVERSATION. The questions cannot change while the game is
        /// running, the walk over a group's guards is not free, and a request answers the
        /// lists BY POSITION - so the cached list is also the agreement about what each
        /// answer means. See <see cref="WorldSnapshot.VariableValues"/>.
        /// </remarks>
        /// <param name="conversation">Any conversation in the group.</param>
        /// <exception cref="InvalidOperationException">The group could not be built.</exception>
        /// <exception cref="FormatException">The answer was not a questions document.</exception>
        public LookAheadQuestions QuestionsFor(int conversation)
        {
            return LookAheadQuestions.Parse(Questions(conversation));
        }

        /// <summary>Answers a look-ahead request.</summary>
        /// <param name="request">The question, built against a cached questions list.</param>
        /// <exception cref="ArgumentNullException">The request is null.</exception>
        /// <exception cref="InvalidOperationException">The call itself failed.</exception>
        /// <exception cref="FormatException">The answer was not a response document.</exception>
        public LookAheadResponse Ask(LookAheadRequest request)
        {
            if (request == null)
            {
                throw new ArgumentNullException(nameof(request));
            }

            return LookAheadResponse.Parse(LookAhead(request.ToJson()));
        }

        /// <summary>
        /// The same questions, as the JSON the engine produced.
        /// </summary>
        /// <remarks>
        /// The layer under <see cref="QuestionsFor"/>, kept public so a test can look at
        /// what actually crossed rather than at what this assembly made of it.
        /// </remarks>
        /// <param name="conversation">Any conversation in the group.</param>
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
                // UTF-8 and not PtrToStringAnsi, which would read this through the
                // platform's default code page and mangle any dialogue text outside ASCII.
                // The JSON crossing here carries conversation content, so that is not
                // hypothetical.
                return Marshal.PtrToStringUTF8(json) ?? string.Empty;
            }
            finally
            {
                NativeMethods.gct_string_free(json);
            }
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
