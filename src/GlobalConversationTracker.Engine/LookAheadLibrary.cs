// SPDX-License-Identifier: MIT
using System;

using Wire = GlobalConversationTracker.Engine.Wire;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// The look-ahead engine, opened over a conversation index.
    /// </summary>
    /// <remarks>
    /// <para>Owns one engine for its lifetime. The index behind it is tens of megabytes and
    /// takes a moment to parse, so this is opened once when the plugin loads and kept -
    /// opening one per response menu would put that parse inside the frame that draws the
    /// menu.</para>
    ///
    /// <para>THE ENGINE IS A CHILD PROCESS, not a library this process loads - de-bnjy.1.
    /// It used to be a <c>DllImport</c> against a Rust <c>cdylib</c>, which meant that
    /// every way the engine could fail was a way the GAME could fail: an abort, an
    /// allocation the machine would not make, a stack overflow inside a recursive diagram
    /// operation. Out of process those are the child's death and not the game's, and this
    /// side hears about them as an exception rather than as a crash dump.</para>
    ///
    /// <para>The surface here did not change when the transport did, which is the point:
    /// the plugin, the harness and the tests all speak to this class and none of them had
    /// to learn what a frame is. <see cref="EngineHost"/> is where that lives.</para>
    /// </remarks>
    public sealed class LookAheadLibrary : IDisposable
    {
        private readonly EngineHost _host;

        private LookAheadLibrary(EngineHost host)
        {
            _host = host;
        }

        /// <summary>
        /// Where to find the engine executable, or null to look beside this assembly.
        /// </summary>
        /// <remarks>
        /// Deployed, the engine sits beside the plugin and the default finds it. In a test
        /// run it is a Cargo build artefact under <c>target/</c>, which nothing would look
        /// in - so a test points this at it, exactly as it used to install a
        /// <c>DllImport</c> resolver.
        /// </remarks>
        public static string? EnginePath
        {
            get => EngineHost.EnginePath;
            set => EngineHost.EnginePath = value;
        }

        /// <summary>
        /// How long any one read may wait for the engine, in milliseconds.
        /// </summary>
        /// <remarks>
        /// <para>NOT A PERFORMANCE LIMIT. A search is bounded by the time budget inside the
        /// request, which the engine enforces itself and reports as having stopped it. This
        /// is the answer to a child that will never answer at all - the case a blocking
        /// read cannot tell from a slow one - and it is deliberately far longer than
        /// anything legitimate.</para>
        ///
        /// <para>A read that passes it ENDS THE ENGINE: the child is killed and this object
        /// throws from then on. Recovering from that - saying so in the game, and starting
        /// another - is de-bnjy.1.2 and de-bnjy.1.3.</para>
        /// </remarks>
        public static int DeadlineMs
        {
            get => EngineHost.Deadline;
            set => EngineHost.Deadline = value;
        }

        /// <summary>
        /// The engine's version, for checking it is the one this was built against.
        /// </summary>
        /// <remarks>
        /// <para>Worth checking at load: a version mismatch discovered here is a log line,
        /// and discovered through a wrong marker is a bug report about the game.</para>
        ///
        /// <para>IT COSTS A PROCESS, which the DllImport version did not - there is no
        /// engine yet to ask, so one is started, asked, and closed again. That is the whole
        /// of the smoke test the plugin runs at load, and it is a better one than before:
        /// it proves the executable is there, that it runs, and that it speaks the
        /// protocol, where reading a static string only ever proved the library loaded.
        /// </para>
        /// </remarks>
        /// <exception cref="InvalidOperationException">The engine would not start or answer.</exception>
        public static string Version
        {
            get
            {
                using EngineHost host = EngineHost.Start();
                EngineHost.Answer answer = host.Ask(
                    new Wire.Request { Version = new Wire.VersionRequest() });
                return answer.Status == Status.Ok ? answer.Text ?? string.Empty : string.Empty;
            }
        }

        /// <summary>
        /// Opens the engine over <paramref name="indexPath"/>.
        /// </summary>
        /// <param name="indexPath">The conversation index shipped with the mod.</param>
        /// <param name="variablesPath">
        /// The database's variable table, shipped beside the index. REQUIRED: without it a
        /// dialogue variable the game would not answer reads Unknown, where with it the value
        /// the database declares is used instead - which is what an unwritten variable
        /// actually is, and the only answer that gets a counter's kind right. Unknown is also
        /// what a symbolic search cannot prune on, so a crawl without a table carries both
        /// branches of every guard reading such a variable.
        /// </param>
        /// <exception cref="ArgumentNullException">Either path is null.</exception>
        /// <exception cref="InvalidOperationException">The engine refused to open it.</exception>
        public static LookAheadLibrary Open(string indexPath, string variablesPath)
        {
            if (indexPath == null)
            {
                throw new ArgumentNullException(nameof(indexPath));
            }

            if (variablesPath == null)
            {
                throw new ArgumentNullException(nameof(variablesPath));
            }

            EngineHost host = EngineHost.Start();
            try
            {
                var open = new Wire.OpenRequest
                {
                    Index = indexPath,
                    Variables = variablesPath,
                };

                Status status = host.Ask(new Wire.Request { Open = open }).Status;
                if (status != Status.Ok)
                {
                    throw new InvalidOperationException(
                        $"the look-ahead engine would not open '{indexPath}': {status}");
                }
            }
            catch
            {
                // A child started and then not handed to anyone would be an orphan for the
                // life of the game, which is exactly the failure de-bnjy.1 is about.
                host.Dispose();
                throw;
            }

            return new LookAheadLibrary(host);
        }

        /// <summary>
        /// The engine process's id, or 0 once it has gone.
        /// </summary>
        /// <remarks>
        /// The mod now starts a process the player did not, so the log should say which
        /// one. It also gives a test something exact to watch: whether THIS child ended,
        /// rather than whether any process of that name did.
        /// </remarks>
        public int ProcessId => _host.ProcessId;

        /// <summary>How many conversations the index holds.</summary>
        public int ConversationCount =>
            Count(new Wire.Request
            {
                ConversationCount = new Wire.ConversationCountRequest(),
            });

        /// <summary>
        /// How many variables the deployed table declares, or 0 if none was read.
        /// </summary>
        /// <remarks>
        /// Worth logging at load for the same reason the conversation count is: a table
        /// that was not deployed, or that would not read, is a mod that still works and
        /// answers one variable in seventy-five less precisely - exactly the kind of thing
        /// that is never noticed unless a line says it.
        /// </remarks>
        public int VariableCount =>
            Count(new Wire.Request { VariableCount = new Wire.VariableCountRequest() });

        /// <summary>
        /// What version the opened index says it is, or 0 where it has no header.
        /// </summary>
        /// <remarks>
        /// Zero means the index cannot be validated at all - it is the full index, a build
        /// intermediate with no header and no hashes - rather than that it is wrong. A
        /// version this build does not read is refused at <see cref="Open"/>, so anything
        /// non-zero here is a version it understands.
        /// </remarks>
        public int IndexFormat =>
            Count(new Wire.Request { IndexFormat = new Wire.IndexFormatRequest() });

        /// <summary>
        /// How many entries one conversation holds, or -1 if the index has no such
        /// conversation.
        /// </summary>
        /// <remarks>
        /// The plugin's guard against a stale index. It builds its graph from the LIVE
        /// dialogue database while the engine reads a file shipped with the mod, and if a
        /// game update or another mod moves the two apart, the look-ahead would be
        /// answering about a conversation the player is not in. Comparing entry counts is
        /// the cheap half of noticing.
        /// </remarks>
        public int EntryCount(int conversation)
        {
            EngineHost.Answer answer = _host.Ask(new Wire.Request
            {
                EntryCount = new Wire.EntryCountRequest { Conversation = conversation },
            });
            return answer.Status == Status.Ok ? answer.Value : -1;
        }

        /// <summary>
        /// What the index says one conversation's content reduced to, or empty where it
        /// carries no hash.
        /// </summary>
        /// <remarks>
        /// The shipped index is a CACHE of the dialogue database, not ground truth, and
        /// this is the stored half of the comparison that says whether it still describes
        /// the database the player's game actually loaded. The live half is
        /// <see cref="ConversationHasher"/>, run over the running game.
        /// </remarks>
        /// <param name="conversation">The conversation id.</param>
        /// <exception cref="InvalidOperationException">The index has no such conversation.</exception>
        public string HashOf(int conversation)
        {
            EngineHost.Answer answer = _host.Ask(new Wire.Request
            {
                ConversationHash =
                    new Wire.ConversationHashRequest { Conversation = conversation },
            });
            if (answer.Status != Status.Ok)
            {
                throw new InvalidOperationException(
                    $"the look-ahead engine has no conversation {conversation}: "
                    + $"{answer.Status}");
            }

            return answer.Text ?? string.Empty;
        }

        /// <summary>
        /// Every question a crawl over <paramref name="conversation"/>'s group can ask.
        /// </summary>
        /// <remarks>
        /// CACHE THIS PER CONVERSATION. The questions cannot change while the game is
        /// running, the walk over a group's guards is not free, and a request answers the
        /// lists BY POSITION - so the cached list is also the agreement about what each
        /// answer means. See <see cref="WorldRawData.VariableValues"/>.
        /// </remarks>
        /// <param name="conversation">Any conversation in the group.</param>
        /// <exception cref="InvalidOperationException">The group could not be built.</exception>
        public LookAheadQuestions QuestionsFor(int conversation)
        {
            EngineHost.Answer answer = _host.Ask(new Wire.Request
            {
                Questions = new Wire.QuestionsRequest { Conversation = conversation },
            });
            if (answer.Status != Status.Ok)
            {
                throw new InvalidOperationException(
                    "the look-ahead engine would not describe conversation "
                    + $"{conversation}: {answer.Status}");
            }

            return WireConvert.Read(answer.Questions);
        }

        /// <summary>Answers a look-ahead request.</summary>
        /// <remarks>
        /// A request the engine could not serve at all comes back as a response carrying
        /// <see cref="LookAheadResponse.Error"/> rather than as an exception, so a caller
        /// has one thing to read. What throws here is what happens BEFORE there is a
        /// response: a request the engine would not accept, or an engine that has stopped
        /// answering.
        /// </remarks>
        /// <param name="request">The question, built against a cached questions list.</param>
        /// <exception cref="ArgumentNullException">The request is null.</exception>
        /// <exception cref="InvalidOperationException">The call itself failed.</exception>
        public LookAheadResponse Ask(LookAheadRequest request)
        {
            if (request == null)
            {
                throw new ArgumentNullException(nameof(request));
            }

            EngineHost.Answer answer = _host.Ask(new Wire.Request
            {
                LookAhead = WireConvert.Write(request),
            });
            if (answer.Status != Status.Ok)
            {
                throw new InvalidOperationException(
                    $"the look-ahead engine refused the request: {answer.Status}");
            }

            return WireConvert.Read(answer.LookAhead);
        }

        /// <summary>
        /// One of the calls whose whole answer is a number, with zero for a refusal.
        /// </summary>
        /// <remarks>
        /// Zero rather than an exception, for all three, because each of them is read into
        /// a log line at load and none is worth failing over: a count that cannot be got is
        /// reported as none, which is also what none looks like.
        /// </remarks>
        private int Count(Wire.Request request)
        {
            EngineHost.Answer answer = _host.Ask(request);
            return answer.Status == Status.Ok ? answer.Value : 0;
        }

        /// <inheritdoc/>
        public void Dispose()
        {
            _host.Dispose();
        }
    }
}
