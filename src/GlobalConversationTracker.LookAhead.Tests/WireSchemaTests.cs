// SPDX-License-Identifier: MIT
using System.Linq;

using GlobalConversationTracker.Engine.Wire;

using Google.Protobuf;

using Xunit;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// What <c>proto/engine.proto</c> describes, as this side generates it.
    /// </summary>
    /// <remarks>
    /// <para>The mirror of <c>tests/wire_schema.rs</c>, and mirrored on purpose. The
    /// schema exists so that ONE description serves both sides, and the way that fails is
    /// not a build error - it is one side generating from a stale copy, or a member
    /// arriving as a default because the two ends disagree about which number it is
    /// under.</para>
    ///
    /// <para>So these fill every member, put it through the encoder, and compare. A field
    /// dropped from the schema stops compiling here, and a field whose number moved comes
    /// back wrong in the round trip.</para>
    /// </remarks>
    public class WireSchemaTests
    {
        private static NodeRef Node(int conversation, int entry) =>
            new NodeRef { Conversation = conversation, Entry = entry };

        private static NodeSet Runs(int conversation, params (int First, int Last)[] spans)
        {
            var conversations = new ConversationRuns { Conversation = conversation };
            foreach ((int first, int last) in spans)
            {
                conversations.Runs.Add(new NodeRun { First = first, Last = last });
            }

            var set = new NodeSet();
            set.Conversations.Add(conversations);
            return set;
        }

        private static WireValue Text(string text) => new WireValue { Text = text };

        /// <summary>Encodes and decodes, which is what both ends do to everything.</summary>
        private static T RoundTrip<T>(T message, MessageParser<T> parser)
            where T : IMessage<T>
        {
            T back = parser.ParseFrom(message.ToByteArray());
            Assert.Equal(message, back);
            return back;
        }

        /// <summary>A snapshot with nothing left at its default.</summary>
        private static WorldSnapshot FullSnapshot()
        {
            var snapshot = new WorldSnapshot
            {
                Money = 5100,
                DayMinutes = (7 * 60) + 42,
                DayCounter = 3,
                ClockLocked = true,
                RedChecksFail = true,
                ChecksPass = Runs(9, (50, 50)),
                ChecksFail = Runs(9, (42, 42), (19, 19)),
                Seen = Runs(9, (0, 40), (42, 42), (50, 99)),
            };

            snapshot.Variables.Add("kim_trust", Text("high"));
            snapshot.VariableValues.Add(new WireValue { Boolean = true });
            snapshot.VariableValues.Add(new WireValue { Number = 2.5 });
            // Not knowable, which is the permissive answer and the default.
            snapshot.VariableValues.Add(new WireValue());
            snapshot.Queries.Add("is_indoors", Text("no"));
            snapshot.QueryValues.Add(Text("raining"));
            snapshot.Items.Add("FALN_sneakers");
            snapshot.Tasks.Add("find_the_body");
            snapshot.Thoughts.Add("the_precarious_world");
            return snapshot;
        }

        [Fact]
        public void AWorldSnapshotSurvivesTheWire()
        {
            RoundTrip(FullSnapshot(), WorldSnapshot.Parser);
        }

        [Fact]
        public void ALookAheadRequestSurvivesTheWire()
        {
            var request = new LookAheadRequest
            {
                Conversation = 631,
                UnseenAnyGame = Runs(631, (1, 5)),
                UnseenThisGame = Runs(631, (6, 9)),
                StateBudget = 1,
                TimeBudgetMs = 1000,
                MenuTimeBudgetMs = 4000,
                MemoryBudgetMb = 256,
                World = FullSnapshot(),
            };
            request.Starts.Add(Node(631, 3));
            request.Starts.Add(Node(631, 7));
            request.Encountered.Add(Node(631, 0));
            request.Encountered.Add(Node(631, 2));

            RoundTrip(request, LookAheadRequest.Parser);
        }

        [Fact]
        public void ALookAheadResponseSurvivesTheWire()
        {
            var response = new LookAheadResponse();
            response.Answers.Add(new LookAheadAnswer
            {
                Start = Node(9, 50),
                Branch = Branch.Pass,
                Destination = Novelty.UnseenThisGame,
                Best = Novelty.UnseenAnyGame,
                Witness = Node(9, 42),
                Complete = false,
                ElapsedMs = 17,
                StatesExplored = 200_000,
                NodesReached = 1_284,
                StoppedBy = StoppedBy.Time,
            });

            RoundTrip(response, LookAheadResponse.Parser);
        }

        /// <summary>A refusal is an ordinary response whose body says so, not a status.</summary>
        [Fact]
        public void ARefusedLookAheadCarriesItsReasonAndNoAnswers()
        {
            LookAheadResponse back = RoundTrip(
                new LookAheadResponse { Error = "the index has no group for 4242" },
                LookAheadResponse.Parser);

            Assert.Empty(back.Answers);
            Assert.Equal("the index has no group for 4242", back.Error);
        }

        [Fact]
        public void TheQuestionsSurviveTheWire()
        {
            var questions = new Questions();
            questions.Conversations.AddRange(new[] { 9, 13 });
            questions.Variables.Add("kim_trust");
            questions.Queries.Add("is_indoors");
            questions.Items.Add("FALN_sneakers");
            questions.Tasks.Add("find_the_body");
            questions.Thoughts.Add("the_precarious_world");
            questions.Checks.Add(Node(9, 50));
            questions.Entries.AddRange(new[] { Node(9, 0), Node(9, 1) });

            RoundTrip(questions, Questions.Parser);
        }

        [Fact]
        public void EveryRequestKindSurvivesTheWire()
        {
            var kinds = new[]
            {
                new Request { Version = new VersionRequest() },
                new Request
                {
                    Open = new OpenRequest { Index = "index.jsonl", Variables = "variables.jsonl" },
                },
                // The optional half left out, which is what a caller with no table sends.
                new Request { Open = new OpenRequest { Index = "index.jsonl" } },
                new Request { ConversationCount = new ConversationCountRequest() },
                new Request { VariableCount = new VariableCountRequest() },
                new Request { EntryCount = new EntryCountRequest { Conversation = 9 } },
                new Request { ConversationHash = new ConversationHashRequest { Conversation = 9 } },
                new Request { IndexFormat = new IndexFormatRequest() },
                new Request { Questions = new QuestionsRequest { Conversation = 9 } },
                new Request { LookAhead = new LookAheadRequest { Conversation = 9 } },
            };

            foreach (Request kind in kinds)
            {
                RoundTrip(kind, Request.Parser);
            }
        }

        [Fact]
        public void AResponseSurvivesTheWireWithEachPayloadItCanCarry()
        {
            var payloads = new[]
            {
                // A refusal, which carries nothing but its reason for being one.
                new Response { Status = Status.NoSuchConversation },
                new Response { Status = Status.Ok, Value = 1345 },
                new Response { Status = Status.Ok, Text = "0.1.0" },
                new Response { Status = Status.Ok, Questions = new Questions() },
                new Response { Status = Status.Ok, LookAhead = new LookAheadResponse() },
            };

            foreach (Response payload in payloads)
            {
                RoundTrip(payload, Response.Parser);
            }
        }

        /// <summary>
        /// The numbers are a contract, and this side has an enum of its own that has to
        /// agree with them.
        /// </summary>
        /// <remarks>
        /// The schema restates the numbers rather than deriving them - which is what makes
        /// them a contract rather than an implementation detail, and is also how the two
        /// could come apart. The Rust suite holds the same pairing on its side.
        /// </remarks>
        [Fact]
        public void EveryStatusNumberIsTheOneThisSideReports()
        {
            var pairs = new (Status OnTheWire, Engine.Status InTheEngine)[]
            {
                (Status.Ok, Engine.Status.Ok),
                (Status.BadHandle, Engine.Status.BadHandle),
                (Status.BadArgument, Engine.Status.BadArgument),
                (Status.IndexUnreadable, Engine.Status.IndexUnreadable),
                (Status.Panic, Engine.Status.Panic),
                (Status.NoSuchConversation, Engine.Status.NoSuchConversation),
                (Status.SerialiseFailed, Engine.Status.SerialiseFailed),
            };

            foreach ((Status onTheWire, Engine.Status inTheEngine) in pairs)
            {
                Assert.Equal((int)inTheEngine, (int)onTheWire);
            }

            // AND THAT THE LIST IS WHOLE, so a status added to one of them alone is
            // caught rather than simply never compared.
            Assert.Equal(
                System.Enum.GetValues<Engine.Status>().Length,
                pairs.Select(pair => pair.InTheEngine).Distinct().Count());
        }

        /// <summary>An unanswered question is "not knowable", and silence has to mean it.</summary>
        [Fact]
        public void AnUnsetValueIsTheUnknowableOne()
        {
            var unknown = new WireValue();
            Assert.Equal(WireValue.ValueOneofCase.None, unknown.ValueCase);

            WireValue back = RoundTrip(unknown, WireValue.Parser);
            Assert.Equal(WireValue.ValueOneofCase.None, back.ValueCase);

            // And it costs nothing to send, which is what makes it safe as the default for
            // every question a caller did not answer.
            Assert.Empty(unknown.ToByteArray());
        }

        /// <summary>The sets are most of a request, and they cross as runs.</summary>
        [Fact]
        public void AnEntrySetCrossesAsRunsRatherThanAsEveryId()
        {
            NodeSet wholeGroup = Runs(631, (0, 999));

            var individually = new ConversationRuns { Conversation = 631 };
            for (int entry = 0; entry <= 999; entry++)
            {
                individually.Runs.Add(new NodeRun { First = entry, Last = entry });
            }

            var listed = new NodeSet();
            listed.Conversations.Add(individually);

            RoundTrip(wholeGroup, NodeSet.Parser);
            Assert.True(wholeGroup.CalculateSize() * 100 < listed.CalculateSize());
        }
    }
}
