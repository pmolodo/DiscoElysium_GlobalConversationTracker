// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;

using Google.Protobuf;

using Wire = GlobalConversationTracker.Engine.Wire;

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

        /// <summary>
        /// How long any one read may wait for the engine, in milliseconds.
        /// </summary>
        /// <remarks>
        /// <para>NOT A PERFORMANCE LIMIT. What bounds a search is the time budget inside
        /// the request, which the engine enforces itself and reports as having stopped it;
        /// this is the answer to a child that will never answer at all, and it should be
        /// comfortably longer than anything legitimate so that it never fires on a slow
        /// one.</para>
        ///
        /// <para>THE NUMBER IT HAS TO STAY ABOVE IS <c>LookAheadMenuTimeBudgetMs</c>, which
        /// is what bounds a whole answer where the per-option budget bounds a term of it.
        /// The wall ships at three seconds and the slowest menu measured is two, so this
        /// sits an order of magnitude out - which is the gap it exists to hold, because the
        /// two limits are different in kind. Crossing the wall costs the options at the
        /// bottom of a menu their markers; crossing this KILLS THE ENGINE. See de-dt75.3 and
        /// <c>performance/menu_wall.rs</c>.</para>
        ///
        /// <para>DELIBERATELY NOT A CONFIGURATION SETTING, unlike the budgets it stands
        /// behind. It is a liveness check rather than a preference: there is no value of it
        /// a player would want that the menu wall does not express better, and one set below
        /// the wall would turn a slow menu into a dead engine. A player who wants menus to
        /// appear sooner lowers the wall.</para>
        ///
        /// <para>Settable so a test can prove the deadline fires without waiting thirty
        /// seconds for it.</para>
        /// </remarks>
        internal static int Deadline { get; set; } = 30_000;

        /// <summary>
        /// How many lines of the engine's stderr to keep.
        /// </summary>
        /// <remarks>
        /// The TAIL, because a process that dies says what matters last. Bounded because
        /// the alternative is holding whatever a long-running engine chose to print for as
        /// long as it ran, in the address space of the game.
        /// </remarks>
        private const int LastWordsLines = 20;

        /// <summary>
        /// What Rust's allocator prints before it aborts, in the form it prints it.
        /// </summary>
        /// <remarks>
        /// <c>std::alloc::handle_alloc_error</c> writes "memory allocation of N bytes
        /// failed" and aborts - not a panic, so nothing inside the engine can catch it, and
        /// the message on stderr is the only account of it that exists. Matching on the
        /// engine's OWN WORDS rather than on an exit code, because Windows reports an abort
        /// as the same status whatever caused it.
        /// </remarks>
        private const string AllocationFailed = "memory allocation of";

        /// <summary>
        /// How long to wait for a dying engine's last words, in milliseconds.
        /// </summary>
        /// <remarks>
        /// Short. This runs on the thread that was waiting for an answer, so it is time the
        /// game spends on a failure it has already suffered - and a process that has broken
        /// its pipe is either already gone or is not going to explain itself.
        /// </remarks>
        private const int LastWordsMs = 250;

        private readonly Process _child;
        private readonly Stream _toChild;
        private readonly Stream _fromChild;
        private readonly Queue<string> _lastWords = new Queue<string>();
        private bool _closed;

        /// <summary>
        /// How it died, once it has. Null while it is alive or merely closed.
        /// </summary>
        /// <remarks>
        /// KEPT so that every later call reports the SAME death rather than a fresh and
        /// less informative failure. A caller that asks again after the engine has gone
        /// should be told what happened the first time, not that a pipe is closed - the
        /// first account is the one with the engine's own words in it.
        /// </remarks>
        private EngineDiedException? _died;

        private EngineHost(Process child)
        {
            _child = child;
            _toChild = child.StandardInput.BaseStream;
            _fromChild = child.StandardOutput.BaseStream;

            // DRAINED, ALWAYS. A redirected stream nobody reads is a child that blocks once
            // it has filled the pipe - a hang whose cause is a diagnostic message, which
            // would be a poor joke - so the reader is attached before anything is asked of
            // it and runs until the stream ends.
            child.ErrorDataReceived += Remember;
            child.BeginErrorReadLine();
        }

        /// <summary>Keeps the last few lines the engine wrote to its stderr.</summary>
        private void Remember(object sender, DataReceivedEventArgs line)
        {
            if (line.Data == null)
            {
                // The stream ended, which is not a line.
                return;
            }

            lock (_lastWords)
            {
                _lastWords.Enqueue(line.Data);
                while (_lastWords.Count > LastWordsLines)
                {
                    _lastWords.Dequeue();
                }
            }
        }

        /// <summary>
        /// Waits, briefly, for the last of the engine's stderr to arrive.
        /// </summary>
        /// <remarks>
        /// <para>THE READER IS ASYNCHRONOUS, so the line that explains a death can still be
        /// in flight when the pipe breaks and this side notices. Without this the message
        /// naming an allocation failure would be there or not depending on how the threads
        /// happened to interleave, which is the worst kind of diagnostic.</para>
        ///
        /// <para>The documented way to be sure is the pair: the timed overload for the
        /// process, then the parameterless one, which is the only one that waits for the
        /// event handlers - "call the WaitForExit() overload that takes no parameter after
        /// receiving a true from this overload", per
        /// https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.waitforexit.
        /// The parameterless one is reached ONLY after the timed one says the process has
        /// gone, so it cannot wait indefinitely for a child that is still running.</para>
        /// </remarks>
        private void SettleLastWords()
        {
            try
            {
                if (_child.WaitForExit(LastWordsMs))
                {
                    _child.WaitForExit();
                }
            }
            catch (Exception)
            {
                // Never started, or already reaped. Either way there is nothing to wait for
                // and whatever was captured is all there will be.
            }
        }

        /// <summary>The tail of what the engine said, or empty if it said nothing.</summary>
        private string LastWords()
        {
            lock (_lastWords)
            {
                return string.Join(Environment.NewLine, _lastWords);
            }
        }

        /// <summary>
        /// The child's process id, or 0 once it has gone.
        /// </summary>
        /// <remarks>
        /// Worth having in a log line. A player reporting a process they did not start can
        /// be answered from the log rather than from guesswork, and a test can watch the
        /// exact child rather than counting processes by name and hoping no other test is
        /// running one.
        /// </remarks>
        internal int ProcessId
        {
            get
            {
                try
                {
                    return _child.Id;
                }
                catch (Exception)
                {
                    return 0;
                }
            }
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
                // CAPTURED, because it is the only account of an abort that exists: Rust's
                // allocator prints what it could not get and then ends the process, with no
                // panic for anything to catch. Drained on a reader thread from the moment
                // the child starts - see the constructor - so the pipe cannot fill.
                RedirectStandardError = true,
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

            // Adopted so it cannot outlive a game that was KILLED rather than closed - see
            // ProcessJob. Best effort by design: an engine that is running but not adopted
            // is a mod that works with one failure mode back, and there is nothing useful
            // to do here about a machine that would not make a job object.
            ProcessJob.Adopt(child);

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
        /// <param name="request">One request, as the schema describes it.</param>
        /// <exception cref="InvalidOperationException">The engine stopped answering.</exception>
        internal Answer Ask(Wire.Request request)
        {
            // The death first, and repeated verbatim: an engine that has gone should keep
            // saying how, rather than degrading into "the pipe is closed" on the second ask.
            if (_died != null)
            {
                throw new EngineDiedException(
                    _died.Death, _died.Message, _died.LastWords);
            }

            if (_closed)
            {
                throw new InvalidOperationException(
                    "the look-ahead engine has been closed.");
            }

            WriteFrame(request.ToByteArray());
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
        /// <para>A pipe read returns what is available rather than what was asked for, so
        /// the loop is not optional: a large response arrives in pieces, and taking the
        /// first piece for the whole would misread the next frame's length out of the
        /// middle of this one.</para>
        ///
        /// <para>EVERY READ HAS A DEADLINE - de-bnjy.1.1.3 - because "the child is
        /// thinking" and "the child will never answer" look identical to a blocking read,
        /// and telling them apart is the whole reason the engine was moved out of process.
        /// A read that has not finished by <see cref="Deadline"/> ends the engine: the
        /// child is killed and this host is closed, so nothing later reads the tail of an
        /// answer that arrived after everyone stopped waiting.</para>
        ///
        /// <para>The deadline restarts per read rather than bounding the whole answer,
        /// which is the right shape: a large response arrives in many pieces and none of
        /// them should be waited on for longer than the last.</para>
        /// </remarks>
        private byte[] ReadExactly(int count, string what)
        {
            byte[] buffer = new byte[count];
            int filled = 0;
            while (filled < count)
            {
                // ReadAsync rather than Read, only so that there is something to stop
                // waiting on. The task itself cannot be cancelled - a pending pipe read is
                // the operating system's, not ours - so what happens on a timeout is that
                // this stops waiting and kills the child, which makes the abandoned read
                // fail and the buffer it was filling unreachable.
                Task<int> reading = _fromChild.ReadAsync(buffer, filled, count - filled);

                int read;
                try
                {
                    if (!WaitForRead(reading, what))
                    {
                        Kill();
                        _died = new EngineDiedException(
                            EngineDeath.Unresponsive,
                            $"the look-ahead engine did not answer within {Deadline} ms "
                            + $"while {what} was being read. It has been stopped.",
                            LastWords());
                        throw _died;
                    }

                    read = reading.Result;
                }
                catch (AggregateException failed)
                    when (failed.InnerException is IOException gone)
                {
                    throw Died($"reading {what}", gone);
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

        /// <summary>Ends the child now, and marks this host unusable.</summary>
        /// <remarks>
        /// Both halves matter. Killing without closing would leave a host whose next call
        /// blocks on a pipe with nothing behind it; closing without killing would leave the
        /// orphan the job object is a second line of defence against.
        /// </remarks>
        private void Kill()
        {
            _closed = true;
            try
            {
                _child.Kill();
            }
            catch (Exception)
            {
                // Already gone, which is one of the ways a read times out.
            }
        }

        /// <summary>
        /// The engine is not there any more, said with whatever can be found out about why.
        /// </summary>
        /// <remarks>
        /// <para>The exit code is the useful half and is only available once the process has
        /// actually gone, so this asks rather than assumes. A child still running with a
        /// broken pipe is a different fault from one that exited, and the message says
        /// which - because the whole point of moving out of process is that this failure is
        /// legible instead of fatal.</para>
        ///
        /// <para>THE HOST IS DEAD AFTER THIS, whatever the caller does with the exception.
        /// A pipe that broke once does not mend, and a host that kept answering calls with
        /// a fresh failure each time would turn one report into one per response menu.</para>
        ///
        /// <para>The KIND of death comes from the engine's own words rather than from its
        /// exit code, because Windows reports an abort as the same status whatever caused
        /// it - see <see cref="AllocationFailed"/>. Anything the engine did not explain is
        /// <see cref="EngineDeath.Crashed"/>, which is the honest answer rather than a
        /// guess between two messages only one of which a player can act on.</para>
        /// </remarks>
        /// <summary>
        /// How often to ask whether the child is gone, while waiting for a read.
        /// </summary>
        /// <remarks>
        /// Short enough that a dead engine is noticed in a frame or two rather than at the
        /// deadline, long enough that a healthy read is not asking about a process
        /// hundreds of times a second. See <see cref="WaitForRead"/>.
        /// </remarks>
        private const int ExitCheckMs = 100;

        /// <summary>
        /// How long a read may still deliver after the child has exited.
        /// </summary>
        /// <remarks>
        /// A child that wrote a complete answer and then exited leaves those bytes in the
        /// pipe, and they are worth having: without this grace the answer would be thrown
        /// away and reported as a death, which is the opposite mistake to the one
        /// <see cref="WaitForRead"/> exists to fix.
        /// </remarks>
        private const int ExitDrainMs = 250;

        /// <summary>
        /// Waits for one read, giving up early when the child is already gone.
        /// </summary>
        /// <remarks>
        /// <para>de-wncd.4. <see cref="Deadline"/> is thirty seconds because a child that
        /// is still thinking and a child that will never answer look identical to a
        /// blocking read - but A CHILD THAT HAS EXITED IS NOT THINKING, and asking the
        /// operating system settles it immediately.</para>
        ///
        /// <para>WHY IT MATTERS: a killed child usually breaks the pipe and the read fails
        /// at once, which is the fast path. But depending on where the kill lands, a write
        /// can succeed into a pipe whose reader is gone and the read then blocks for the
        /// whole deadline - and the thing waiting on it is a RESPONSE MENU BEING DRAWN.
        /// Measured in game 2026-09-07: the same suite drew its menu instantly on one run
        /// and reported `advance-to-menu after 68.5s` on the next, which is two of these
        /// waits. A thirty-second freeze mid-conversation is much worse than the missing
        /// asterisk this feature is allowed to cost.</para>
        ///
        /// <para>Returns true when the read finished, false when the deadline passed with
        /// the child still running - which is the one case that means "unresponsive".</para>
        /// </remarks>
        private bool WaitForRead(Task<int> reading, string what)
        {
            int waited = 0;
            while (waited < Deadline)
            {
                int slice = Math.Min(ExitCheckMs, Deadline - waited);
                if (reading.Wait(slice))
                {
                    return true;
                }

                waited += slice;

                bool gone;
                try
                {
                    gone = _child.HasExited;
                }
                catch (InvalidOperationException)
                {
                    // Nothing can be learned about the child, so fall back to the deadline
                    // rather than guessing that it is dead.
                    continue;
                }

                if (gone)
                {
                    // It may have written a whole answer on its way out; those bytes are
                    // already in the pipe and are worth the moment it takes to collect
                    // them. Only after that is silence a death.
                    if (reading.Wait(ExitDrainMs))
                    {
                        return true;
                    }

                    throw Died($"reading {what}", null);
                }
            }

            return false;
        }

        private EngineDiedException Died(string doing, Exception? cause)
        {
            _closed = true;
            SettleLastWords();

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

            string lastWords = LastWords();
            EngineDeath death = lastWords.Contains(AllocationFailed)
                ? EngineDeath.OutOfMemory
                : EngineDeath.Crashed;

            string said = lastWords.Length == 0
                ? " It said nothing on its way out."
                : $" It last said: {lastWords}";

            _died = new EngineDiedException(
                death,
                $"the look-ahead engine stopped answering while {doing}: {ended}.{said}",
                lastWords,
                cause);
            return _died;
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
        /// One answer: a status, and whichever payload the call it answers has.
        /// </summary>
        /// <remarks>
        /// A thin reading of the generated <c>Wire.Response</c> rather than the message
        /// itself, so a caller says <c>answer.Value</c> without first asking whether the
        /// member is present. The payload members are populated by the calls that have one
        /// and absent otherwise, which is what lets a caller read one member per call
        /// rather than a discriminant it already knows from what it asked.
        /// </remarks>
        /// <param name="Status">What the engine made of the request.</param>
        /// <param name="Value">The answer to a call that returns a count or a format.</param>
        /// <param name="Text">The answer to a call that returns text.</param>
        /// <param name="Questions">The answer to a questions call, or null.</param>
        /// <param name="LookAhead">The answer to a look-ahead call, or null.</param>
        internal readonly record struct Answer(
            Status Status,
            int Value,
            string? Text,
            Wire.Questions? Questions,
            Wire.LookAheadResponse? LookAhead)
        {
            /// <summary>Reads one response frame.</summary>
            /// <remarks>
            /// A STATUS THIS BUILD DOES NOT NAME is refused rather than passed on. It can
            /// only come from an engine built from different sources, and a code read as
            /// whatever this build happens to map it to is a wrong answer wearing the
            /// clothes of a right one - which is the whole reason the numbers are pinned
            /// on both sides.
            /// </remarks>
            /// <exception cref="FormatException">It was not a response.</exception>
            internal static Answer Parse(byte[] frame)
            {
                try
                {
                    Wire.Response response = Wire.Response.Parser.ParseFrom(frame);
                    if (!Enum.IsDefined(typeof(Status), (int)response.Status))
                    {
                        throw new FormatException(
                            "the look-ahead engine answered with a status this build does "
                            + $"not know: {(int)response.Status}");
                    }

                    return new Answer(
                        (Status)(int)response.Status,
                        response.HasValue ? response.Value : 0,
                        response.HasText ? response.Text : null,
                        response.Questions,
                        response.LookAhead);
                }
                catch (InvalidProtocolBufferException error)
                {
                    throw new FormatException(
                        "the look-ahead engine sent something that is not a response: "
                        + $"{frame.Length} byte(s)", error);
                }
            }
        }
    }
}
