// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// The look-ahead engine running as a child process, spoken to over its own pipes.
    /// </summary>
    /// <remarks>
    /// <para>What replaced the <c>DllImport</c>s - see de-bnjy.1. A library loaded into the
    /// game's address space cannot fail alone: an abort, an allocation the machine will not
    /// make, a stack overflow inside a recursive diagram operation, each of them ends the
    /// player's session and leaves nothing to read. A child process can die on its own, and
    /// this side finds out by the pipe going quiet rather than by ceasing to exist.</para>
    ///
    /// <para>THE PROTOCOL IS <c>src/host.rs</c>, and its unit tests spell out the exact
    /// bytes this class writes and reads. A frame is a four-byte little-endian length and
    /// then that many bytes of UTF-8 JSON, both directions. Requests are serde's external
    /// tagging - a bare string for a call with no arguments, an object of one property for
    /// a call with them - and a response is an object carrying a numeric status and at most
    /// one of <c>value</c> and <c>text</c>.</para>
    ///
    /// <para>ONE ENGINE PER PROCESS, which is why nothing here passes a handle: the process
    /// IS the handle, opened by the first request and closed by <see cref="Dispose"/>.</para>
    ///
    /// <para>NOT YET DEADLINED. A read blocks until the child answers, so a child that
    /// hangs hangs this thread with it - de-bnjy.1.1.3 is that, along with the job object
    /// that stops a child outliving a game that was killed rather than closed. Everything
    /// that has to change for a deadline is <see cref="ReadExactly"/>.</para>
    /// </remarks>
    internal sealed class EngineHost : IDisposable
    {
        /// <summary>
        /// The largest frame this will read, matching <c>host::MAX_FRAME</c>.
        /// </summary>
        /// <remarks>
        /// So that four bytes nobody meant to send are an error rather than an allocation
        /// of whatever they happened to say. Both ends refuse the same size, deliberately:
        /// a limit only one side enforces is a limit the other side can be talked past.
        /// </remarks>
        internal const int MaxFrame = 16 * 1024 * 1024;

        /// <summary>
        /// What the engine is called where the mod deploys it.
        /// </summary>
        /// <remarks>
        /// Named to match the deployment predicate, which takes
        /// <c>GlobalConversationTracker*</c> and nothing else - the same trick the native
        /// library, the index and the variable table already use, so deploy, release
        /// packaging and the uninstaller all handle it without being told it exists.
        /// </remarks>
        internal const string DeployedName = "GlobalConversationTracker.Native";

        /// <summary>How long to wait for a child to end before killing it.</summary>
        /// <remarks>
        /// It ends by itself when its stdin closes, and it has nothing to flush, so this is
        /// generous rather than tuned. What it protects against is a child that is wedged:
        /// disposal must not block the game, and a killed engine is no worse than one that
        /// was about to be closed anyway.
        /// </remarks>
        private const int ShutdownMs = 2000;

        private readonly Process _child;
        private readonly Stream _toChild;
        private readonly Stream _fromChild;
        private bool _closed;

        private EngineHost(Process child)
        {
            _child = child;
            _toChild = child.StandardInput.BaseStream;
            _fromChild = child.StandardOutput.BaseStream;
        }

        /// <summary>
        /// Where to find the engine executable, or null to look beside this assembly.
        /// </summary>
        /// <remarks>
        /// <para>What replaced <c>NativeLibrary.SetDllImportResolver</c>. Deployed, the
        /// engine sits beside the plugin and the default finds it; in a test run it is a
        /// Cargo build artefact under <c>target/</c>, which nothing would look in.</para>
        ///
        /// <para>A settable static, like the resolver it replaces, and for the same reason:
        /// <see cref="LookAheadLibrary.Version"/> is static and has to find the engine
        /// before anything has been opened.</para>
        /// </remarks>
        internal static string? EnginePath { get; set; }

        /// <summary>The engine executable this would run, whether or not it exists.</summary>
        internal static string Executable
        {
            get
            {
                if (EnginePath != null)
                {
                    return EnginePath;
                }

                string name = RuntimeInformation.IsOSPlatform(OSPlatform.Windows)
                    ? DeployedName + ".exe"
                    : DeployedName;
                string? beside = Path.GetDirectoryName(
                    typeof(EngineHost).Assembly.Location);
                return beside == null ? name : Path.Combine(beside, name);
            }
        }

        /// <summary>Starts the engine and returns it, ready for a request.</summary>
        /// <exception cref="InvalidOperationException">It would not start.</exception>
        internal static EngineHost Start()
        {
            string executable = Executable;
            var start = new ProcessStartInfo(executable)
            {
                RedirectStandardInput = true,
                RedirectStandardOutput = true,
                // INHERITED, not redirected. The engine says nothing there unless something
                // has gone wrong, and a redirected stream nobody drains is a child that
                // blocks once it has filled the pipe - a hang whose cause is a diagnostic
                // message, which would be a poor joke.
                RedirectStandardError = false,
                UseShellExecute = false,
                CreateNoWindow = true,
            };

            Process? child;
            try
            {
                child = Process.Start(start);
            }
            catch (Exception error)
            {
                throw new InvalidOperationException(
                    $"the look-ahead engine at '{executable}' would not start: "
                    + $"{error.GetType().Name}: {error.Message}",
                    error);
            }

            if (child == null)
            {
                throw new InvalidOperationException(
                    $"the look-ahead engine at '{executable}' would not start.");
            }

            return new EngineHost(child);
        }

        /// <summary>
        /// Sends one request and reads the answer.
        /// </summary>
        /// <remarks>
        /// One frame in, one frame out, in that order and with nothing in between: the
        /// protocol has no way to say which answer belongs to which question, so it relies
        /// on this. Not thread-safe for the same reason, and does not need to be - the
        /// plugin asks from the frame that draws the menu.
        /// </remarks>
        /// <param name="requestJson">One request, as the protocol spells it.</param>
        /// <exception cref="InvalidOperationException">The engine stopped answering.</exception>
        internal Answer Ask(string requestJson)
        {
            if (_closed)
            {
                throw new InvalidOperationException(
                    "the look-ahead engine has been closed.");
            }

            WriteFrame(Encoding.UTF8.GetBytes(requestJson));
            return Answer.Parse(ReadFrame());
        }

        /// <summary>Writes the length, then the body, then flushes.</summary>
        /// <remarks>
        /// FLUSHED BEFORE RETURNING, because this thread is about to block on the answer.
        /// A buffered write that held the last frame would be a deadlock that looks exactly
        /// like a slow search.
        /// </remarks>
        private void WriteFrame(byte[] body)
        {
            if (body.Length > MaxFrame)
            {
                throw new InvalidOperationException(
                    $"a request of {body.Length} bytes is past the {MaxFrame} limit.");
            }

            try
            {
                _toChild.Write(BitConverter.GetBytes(body.Length), 0, sizeof(int));
                _toChild.Write(body, 0, body.Length);
                _toChild.Flush();
            }
            catch (IOException gone)
            {
                throw Died("writing a request", gone);
            }
        }

        /// <summary>Reads one whole frame.</summary>
        private byte[] ReadFrame()
        {
            byte[] length = ReadExactly(sizeof(int), "a frame length");
            int size = BitConverter.ToInt32(length, 0);
            if (size < 0 || size > MaxFrame)
            {
                throw new InvalidOperationException(
                    $"the look-ahead engine announced a frame of {size} bytes, which is "
                    + $"not between 0 and {MaxFrame}. The stream is out of step.");
            }

            return ReadExactly(size, "a frame body");
        }

        /// <summary>
        /// Reads exactly <paramref name="count"/> bytes, or says the engine has gone.
        /// </summary>
        /// <remarks>
        /// A pipe read returns what is available rather than what was asked for, so the
        /// loop is not optional: a large response arrives in pieces, and taking the first
        /// piece for the whole would misread the next frame's length out of the middle of
        /// this one.
        ///
        /// THIS IS WHERE A DEADLINE GOES - de-bnjy.1.1.3. Zero from a pipe means the far
        /// end has closed it, which this reports; what it cannot yet tell apart is a child
        /// that is thinking from a child that will never answer.
        /// </remarks>
        private byte[] ReadExactly(int count, string what)
        {
            byte[] buffer = new byte[count];
            int filled = 0;
            while (filled < count)
            {
                int read;
                try
                {
                    read = _fromChild.Read(buffer, filled, count - filled);
                }
                catch (IOException gone)
                {
                    throw Died($"reading {what}", gone);
                }

                if (read <= 0)
                {
                    throw Died($"reading {what}", null);
                }

                filled += read;
            }

            return buffer;
        }

        /// <summary>
        /// The engine is not there any more, said with whatever can be found out about why.
        /// </summary>
        /// <remarks>
        /// The exit code is the useful half and is only available once the process has
        /// actually gone, so this asks rather than assumes. A child still running with a
        /// broken pipe is a different fault from one that exited, and the message says
        /// which - because the whole point of moving out of process is that this failure is
        /// legible instead of fatal.
        /// </remarks>
        private InvalidOperationException Died(string doing, Exception? cause)
        {
            string ended;
            try
            {
                ended = _child.HasExited
                    ? $"it exited with code {_child.ExitCode}"
                    : "it is still running but the pipe is broken";
            }
            catch (InvalidOperationException)
            {
                ended = "and nothing can be learned about how it ended";
            }

            return new InvalidOperationException(
                $"the look-ahead engine stopped answering while {doing}: {ended}.", cause);
        }

        /// <inheritdoc/>
        /// <remarks>
        /// Closing stdin is how the engine is told there is no more; it ends its loop and
        /// exits. Killed only if it does not, because a child that is asked to leave and
        /// does is worth waiting a moment for - it is the case where an exit code means
        /// something.
        /// </remarks>
        public void Dispose()
        {
            if (_closed)
            {
                return;
            }

            _closed = true;

            try
            {
                _toChild.Close();
            }
            catch (Exception)
            {
                // Already broken, which is one of the ways this is reached.
            }

            try
            {
                if (!_child.WaitForExit(ShutdownMs))
                {
                    _child.Kill();
                }
            }
            catch (Exception)
            {
                // Gone, or never started properly. Either way there is nothing to close.
            }

            _child.Dispose();
        }

        /// <summary>
        /// The requests, written the way <c>src/host.rs</c> reads them.
        /// </summary>
        /// <remarks>
        /// <para>Serde's EXTERNAL TAGGING: a call with no arguments is a bare JSON string
        /// naming it, and a call with arguments is an object of exactly one property, whose
        /// name is the call and whose value holds them. Both spellings are asserted on the
        /// Rust side in <c>host::tests::a_request_looks_on_the_wire_the_way_the_client_
        /// writes_it</c>, so a derive attribute changed there fails there rather than in
        /// the game.</para>
        ///
        /// <para>Built with <see cref="Utf8JsonWriter"/> rather than by concatenation
        /// because the arguments include WINDOWS PATHS, which are full of backslashes, and
        /// a look-ahead request, which is itself JSON carried as a string. Both need
        /// escaping and neither would forgive getting it wrong.</para>
        /// </remarks>
        internal static class Requests
        {
            /// <summary>This build's version.</summary>
            internal const string Version = "\"version\"";

            /// <summary>How many conversations the open index holds.</summary>
            internal const string ConversationCount = "\"conversation_count\"";

            /// <summary>How many variables the deployed table declares.</summary>
            internal const string VariableCount = "\"variable_count\"";

            /// <summary>What version the open index says it is.</summary>
            internal const string IndexFormat = "\"index_format\"";

            /// <summary>Open the engine over an index, and optionally a variable table.</summary>
            internal static string Open(string index, string? variables)
            {
                return Tagged("open", writer =>
                {
                    writer.WriteString("index", index);
                    if (variables == null)
                    {
                        writer.WriteNull("variables");
                    }
                    else
                    {
                        writer.WriteString("variables", variables);
                    }
                });
            }

            /// <summary>How many entries one conversation holds.</summary>
            internal static string EntryCount(int conversation) =>
                About("entry_count", conversation);

            /// <summary>What the index says one conversation's content reduced to.</summary>
            internal static string ConversationHash(int conversation) =>
                About("conversation_hash", conversation);

            /// <summary>Every question a crawl over one conversation's group can ask.</summary>
            internal static string Questions(int conversation) =>
                About("questions", conversation);

            /// <summary>Answer a look-ahead request, whose body is the wire's own JSON.</summary>
            internal static string LookAhead(string requestJson) =>
                Tagged("look_ahead", writer => writer.WriteString("request", requestJson));

            /// <summary>One of the calls whose only argument is a conversation.</summary>
            private static string About(string kind, int conversation) =>
                Tagged(kind, writer => writer.WriteNumber("conversation", conversation));

            /// <summary>An object of one property, named for the call.</summary>
            private static string Tagged(string kind, Action<Utf8JsonWriter> arguments)
            {
                var buffer = new MemoryStream();
                using (var writer = new Utf8JsonWriter(buffer))
                {
                    writer.WriteStartObject();
                    writer.WritePropertyName(kind);
                    writer.WriteStartObject();
                    arguments(writer);
                    writer.WriteEndObject();
                    writer.WriteEndObject();
                }

                return Encoding.UTF8.GetString(buffer.ToArray());
            }
        }

        /// <summary>
        /// One answer: a status, and at most one of a number and a string.
        /// </summary>
        /// <param name="Status">What the engine made of the request.</param>
        /// <param name="Value">The answer to a call that returns a count or a format.</param>
        /// <param name="Text">The answer to a call that returns text or a JSON document.</param>
        internal readonly record struct Answer(Status Status, int Value, string? Text)
        {
            /// <summary>Reads one response frame.</summary>
            /// <exception cref="FormatException">It was not a response.</exception>
            internal static Answer Parse(byte[] frame)
            {
                try
                {
                    using JsonDocument document = JsonDocument.Parse(frame);
                    JsonElement root = document.RootElement;

                    // Absent rather than null where a call has no payload, so the two are
                    // read with TryGetProperty rather than by looking at a null.
                    int value = root.TryGetProperty("value", out JsonElement number)
                        ? number.GetInt32()
                        : 0;
                    string? text = root.TryGetProperty("text", out JsonElement written)
                        ? written.GetString()
                        : null;

                    return new Answer(
                        (Status)root.GetProperty("status").GetInt32(), value, text);
                }
                catch (Exception error)
                    when (error is JsonException || error is KeyNotFoundException)
                {
                    throw new FormatException(
                        "the look-ahead engine sent something that is not a response: "
                        + Encoding.UTF8.GetString(frame), error);
                }
            }
        }
    }
}
