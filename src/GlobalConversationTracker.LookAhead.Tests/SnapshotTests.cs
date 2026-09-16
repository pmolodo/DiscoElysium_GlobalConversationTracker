// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.Linq;

using GlobalConversationTracker.Engine;

using Google.Protobuf;

using Wire = GlobalConversationTracker.Engine.Wire;

using Xunit;
using Xunit.Abstractions;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// Does a world written here mean the same thing over there?
    /// </summary>
    /// <remarks>
    /// <para>The managed half of de-i5xj.7. What can be checked without the game is the
    /// SHAPE: that an entry set writes the runs the engine reads, that a positional answer
    /// list lands on the names the engine asked under, and that a request built here comes
    /// back answered. What needs the game - that the answers are the ones
    /// <c>GameLookAheadWorld</c> would give - is checked in the in-game suite.</para>
    ///
    /// <para>The shape tests need nothing at all. The round trip needs the library and the
    /// index, and skips loudly where they have not been produced, like every other suite
    /// here.</para>
    /// </remarks>
    public class SnapshotTests
    {

        /// <summary>A set's runs, as one line, so an encoding can be asserted as text.</summary>
        /// <remarks>
        /// The runs are what actually crosses, and they are the thing worth pinning: a set
        /// that collapsed differently from one call to the next would make two requests
        /// impossible to compare, and comparing them is how the in-game check against an
        /// offline run works at all.
        /// </remarks>
        private static string Runs(Wire.NodeSet set)
        {
            return string.Join(
                "; ",
                set.Conversations.Select(conversation =>
                    $"{conversation.Conversation}: "
                    + string.Join(
                        ", ",
                        conversation.Runs.Select(run => $"{run.First}-{run.Last}"))));
        }
        private readonly ITestOutputHelper _output;

        public SnapshotTests(ITestOutputHelper output)
        {
            _output = output;
            NativeLookAhead.Install();
        }

        /// <summary>
        /// The runs an entry set writes, pinned - the Rust side pins the identical string.
        /// </summary>
        [Fact]
        public void AnEntrySetIsWrittenAsRunsByConversation()
        {
            var set = new NodeSet();
            foreach (int entry in new[] { 0, 1, 2, 3, 5, 9, 10 })
            {
                set.Add(new NodeRef(631, entry));
            }

            set.Add(new NodeRef(636, 7));

            Assert.Equal(
                "631: 0-3, 5-5, 9-10; 636: 7-7", Runs(WireConvert.Write(set)));
        }

        /// <summary>An empty set is an empty object, not an absent one.</summary>
        [Fact]
        public void AnEmptyEntrySetIsAnEmptyObject()
        {
            Assert.Equal("", Runs(WireConvert.Write(new NodeSet())));
        }

        /// <summary>Adding an entry twice does not write it twice.</summary>
        [Fact]
        public void AnEntryAddedTwiceIsOneEntry()
        {
            var set = new NodeSet();
            set.Add(new NodeRef(631, 4));
            set.Add(new NodeRef(631, 4));

            Assert.Equal(1, set.Count);
            Assert.Equal("631: 4-4", Runs(WireConvert.Write(set)));
        }

        /// <summary>
        /// A request carries the world under the names the engine reads it by.
        /// </summary>
        /// <remarks>
        /// Checked against the property names rather than the whole document, because the
        /// point is the agreement about names - a field this writes and the engine does not
        /// read is silently ignored over there, and the world is then answered by defaults.
        /// </remarks>
        [Fact]
        public void ARequestIsWrittenUnderTheNamesTheEngineReads()
        {
            var world = new WorldSnapshot { Money = 250, DayMinutes = 720, DayCounter = 1 };
            world.VariableValues.Add(WireValue.FromNumber(3));
            world.QueryValues.Add(WireValue.FromBoolean(true));
            world.Items.Add("badge");
            world.Seen.Add(new NodeRef(631, 2));

            var request = new LookAheadRequest(631, world);
            request.Starts.Add(new NodeRef(631, 4));
            request.UnseenAnyGame.Add(new NodeRef(631, 9));

            Wire.LookAheadRequest sent = WireConvert.Write(request);
            _output.WriteLine(sent.ToString());

            Assert.Equal(631, sent.Conversation);
            Assert.Equal(4, Assert.Single(sent.Starts).Entry);
            Assert.Equal("631: 9-9", Runs(sent.UnseenAnyGame));
            Assert.Empty(sent.UnseenThisGame.Conversations);

            Wire.WorldSnapshot snapshot = sent.World;
            Assert.Equal(250, snapshot.Money);
            Assert.Equal(720, snapshot.DayMinutes);
            Assert.False(snapshot.ClockLocked);
            Assert.Equal(
                Wire.WireValue.ValueOneofCase.Number,
                snapshot.VariableValues[0].ValueCase);
            Assert.Equal(
                Wire.WireValue.ValueOneofCase.Boolean,
                snapshot.QueryValues[0].ValueCase);
            Assert.Equal("badge", snapshot.Items[0]);
            Assert.Equal("631: 2-2", Runs(snapshot.Seen));
        }

        /// <summary>A value crosses as the one member of the oneof that names its kind.</summary>
        /// <remarks>
        /// AND UNKNOWN CROSSES AS NONE OF THEM. That is what silence means on the far
        /// side, it is what a default-constructed value already is, and it costs no bytes -
        /// so there is no way to send a value that means neither this nor an answer.
        /// </remarks>
        [Fact]
        public void EachKindOfAnswerCrossesAsItsOwnMember()
        {
            Assert.Equal(
                Wire.WireValue.ValueOneofCase.Boolean, Sent(WireValue.FromBoolean(true)).ValueCase);
            Assert.Equal(
                Wire.WireValue.ValueOneofCase.Number, Sent(WireValue.FromNumber(3)).ValueCase);
            Assert.Equal(
                Wire.WireValue.ValueOneofCase.Text, Sent(WireValue.FromText("blue")).ValueCase);
            Assert.Equal(
                Wire.WireValue.ValueOneofCase.None, Sent(WireValue.Unknown).ValueCase);

            Assert.True(Sent(WireValue.FromBoolean(true)).Boolean);
            Assert.Equal(3, Sent(WireValue.FromNumber(3)).Number);
            Assert.Equal("blue", Sent(WireValue.FromText("blue")).Text);
            Assert.Empty(Sent(WireValue.Unknown).ToByteArray());
        }

        /// <summary>One value, as it leaves a snapshot.</summary>
        private static Wire.WireValue Sent(WireValue value)
        {
            var world = new WorldSnapshot();
            world.VariableValues.Add(value);
            return Assert.Single(WireConvert.Write(world).VariableValues);
        }

        /// <summary>Reading the questions gives back the lists the engine sent.</summary>
        [Fact]
        public void TheQuestionsAreReadBackAsTheyWereSent()
        {
            var sent = new Wire.Questions();
            sent.Conversations.AddRange(new[] { 631, 636 });
            sent.Variables.Add("jam.asked");
            sent.Queries.Add("IsKimHere()");
            sent.Items.Add("badge");
            sent.Thoughts.Add("jamais_vu");
            sent.Checks.Add(WireConvert.Write(new NodeRef(631, 12)));
            sent.Entries.Add(WireConvert.Write(new NodeRef(631, 0)));

            LookAheadQuestions questions = WireConvert.Read(sent);

            Assert.Equal(new[] { 631, 636 }, questions.Conversations);
            Assert.Equal("jam.asked", Assert.Single(questions.Variables));
            Assert.Equal("IsKimHere()", Assert.Single(questions.Queries));
            Assert.Equal("badge", Assert.Single(questions.Items));
            Assert.Equal("jamais_vu", Assert.Single(questions.Thoughts));
            Assert.Equal(new NodeRef(631, 12), Assert.Single(questions.Checks));
            Assert.Equal(new NodeRef(631, 0), Assert.Single(questions.Entries));
        }

        /// <summary>An answer document is read back into answers.</summary>
        [Fact]
        public void AResponseIsReadBackIntoAnswers()
        {
            var sent = new Wire.LookAheadResponse();
            sent.Answers.Add(new Wire.LookAheadAnswer
            {
                Start = WireConvert.Write(new NodeRef(631, 4)),
                Best = Wire.Novelty.UnseenAnyGame,
                Complete = true,
                ElapsedMs = 7,
            });

            LookAheadResponse response = WireConvert.Read(sent);

            Assert.Null(response.Error);
            LookAheadAnswer answer = Assert.Single(response.Answers);
            Assert.Equal(new NodeRef(631, 4), answer.Start);
            Assert.Equal(2, answer.Best);
            Assert.True(answer.Complete);
            Assert.Equal(7, answer.ElapsedMs);
        }

        /// <summary>A request the engine could not serve at all reports why.</summary>
        [Fact]
        public void AFailedRequestComesBackAsAReasonRatherThanAnException()
        {
            var sent = new Wire.LookAheadResponse
            {
                Error = "no such conversation",
            };

            LookAheadResponse response = WireConvert.Read(sent);

            Assert.Empty(response.Answers);
            Assert.Equal("no such conversation", response.Error);
        }

        /// <summary>
        /// A whole menu crosses: the engine's questions, answered positionally, and
        /// answers back for every option.
        /// </summary>
        /// <remarks>
        /// The end of the round trip the managed side exists for. Everything is answered
        /// the same way, so what this checks is the CROSSING - that the lists line up, that
        /// the runs decode, and that nothing came back as an error.
        /// </remarks>
        [Fact]
        public void AWholeMenuCrossesAndComesBackAnswered()
        {
            string? index = NativeLookAhead.Index;
            if (NativeLookAhead.Engine == null || index == null)
            {
                _output.WriteLine("the engine or the index is missing; skipping.");
                return;
            }

            using LookAheadLibrary engine = LookAheadLibrary.Open(
                index, NativeLookAhead.Variables);

            // Conversation 631's group is the one every measurement uses, so its shape is
            // known independently of this bridge.
            LookAheadQuestions questions = engine.QuestionsFor(631);
            _output.WriteLine(
                $"{questions.Conversations.Count} conversations, "
                + $"{questions.Variables.Count} variables, {questions.Queries.Count} queries, "
                + $"{questions.Checks.Count} checks, {questions.Entries.Count} entries");
            Assert.Contains(636, questions.Conversations);
            Assert.NotEmpty(questions.Entries);

            var world = new WorldSnapshot { Money = 250, DayMinutes = 720, DayCounter = 1 };
            foreach (string _ in questions.Variables)
            {
                world.VariableValues.Add(WireValue.Unknown);
            }

            foreach (string _ in questions.Queries)
            {
                world.QueryValues.Add(WireValue.FromBoolean(true));
            }

            var request = new LookAheadRequest(631, world);
            // Every entry unseen anywhere, which is the state a crawl has most to say about.
            foreach (NodeRef entry in questions.Entries)
            {
                request.UnseenAnyGame.Add(entry);
            }

            // A handful of options rather than all 4,514 entries: the crossing is what is
            // being checked, and a crawl from every entry of the biggest group in the game
            // is a measurement, not a test.
            foreach (NodeRef entry in Take(questions.Entries, 4))
            {
                request.Starts.Add(entry);
            }

            _output.WriteLine(
                $"the request is {WireConvert.Write(request).CalculateSize()} bytes");

            LookAheadResponse response = engine.Ask(request);
            Assert.Null(response.Error);

            // AT LEAST ONE PER START, not exactly one. A rolled check is two options
            // wearing one line of text and comes back as two answers (de-8hh2.6), so a
            // menu of n options with k checks is answered by n + k. This used to demand
            // equality and passed only because the four entries it happens to take are
            // not checks - which is a property of the ordering, not of the claim.
            Assert.True(
                response.Answers.Count >= request.Starts.Count,
                $"{request.Starts.Count} starts came back with {response.Answers.Count} answers");

            foreach (LookAheadAnswer answer in response.Answers)
            {
                _output.WriteLine(
                    $"{answer.Start}{(answer.Branch is null ? "" : " " + answer.Branch)}: "
                    + $"best {answer.Best}, complete {answer.Complete}, "
                    + $"{answer.ElapsedMs} ms");
                Assert.Contains(answer.Start, request.Starts);
            }

            // And every start is accounted for, once as an option or twice as a check.
            foreach (NodeRef start in request.Starts)
            {
                bool answered = response.Find(start, null) is not null
                    || response.OutcomesOf(start) is not null;
                Assert.True(answered, $"{start} was asked about and not answered");
            }
        }

        /// <summary>
        /// A rolled check crosses as TWO answers, one per outcome.
        /// </summary>
        /// <remarks>
        /// <para>THE SHAPE OF THE WIRE, asked of the real library rather than of a parser
        /// over a hand-written document. Nothing here did: the menu test above takes
        /// whatever entries come first, and none of them rolls, so the two sides could have
        /// disagreed about what a check looks like and every managed test would still have
        /// passed - the parser reads an absent outcome name as "ordinary option", which is
        /// exactly what an old library's answer looks like.</para>
        ///
        /// <para>9:50 is the ceiling fan's "Grab the tie", the one rolled check the harness
        /// relies on being on screen, and the same entry the branch-shape scenarios are
        /// built around.</para>
        /// </remarks>
        [Fact]
        public void ARolledCheckCrossesAsTwoAnswers()
        {
            string? index = NativeLookAhead.Index;
            if (NativeLookAhead.Engine == null || index == null)
            {
                _output.WriteLine("the engine or the index is missing; skipping.");
                return;
            }

            using LookAheadLibrary engine = LookAheadLibrary.Open(
                index, NativeLookAhead.Variables);
            LookAheadQuestions questions = engine.QuestionsFor(FanConversation);

            var world = new WorldSnapshot { DayMinutes = 720, DayCounter = 1 };
            foreach (string _ in questions.Variables)
            {
                world.VariableValues.Add(WireValue.Unknown);
            }

            foreach (string _ in questions.Queries)
            {
                world.QueryValues.Add(WireValue.FromBoolean(true));
            }

            var request = new LookAheadRequest(FanConversation, world);
            foreach (NodeRef entry in questions.Entries)
            {
                request.UnseenAnyGame.Add(entry);
            }

            var check = new NodeRef(FanConversation, GrabTheTieEntry);
            request.Starts.Add(check);

            LookAheadResponse response = engine.Ask(request);
            Assert.Null(response.Error);

            Outcomes both = Assert.NotNull(response.OutcomesOf(check));
            Assert.Equal(LookAheadAnswer.Pass, both.Pass.Branch);
            Assert.Equal(LookAheadAnswer.Fail, both.Fail.Branch);

            // The option itself is not an answer any more, which is what stops a caller
            // that forgot the outcome from silently getting one half.
            Assert.Null(response.Find(check, null));

            _output.WriteLine(
                $"pass: destination {both.Pass.Destination}, best {both.Pass.Best}; "
                + $"fail: destination {both.Fail.Destination}, best {both.Fail.Best}");
        }

        /// <summary>The conversation the fan's check lives in.</summary>
        private const int FanConversation = 9;

        /// <summary>The white check in it: "Grab the tie".</summary>
        private const int GrabTheTieEntry = 50;

        /// <summary>
        /// Answering a DIFFERENT number of questions is refused, not zipped as far as it
        /// goes.
        /// </summary>
        /// <remarks>
        /// The failure a positional list makes possible, and the reason it is safe to use
        /// one: a caller answering a stale questions list would otherwise have every answer
        /// after the first difference land on the wrong variable, and the marker would be
        /// wrong with nothing at all to report it.
        /// </remarks>
        [Fact]
        public void AnswersToADifferentSetOfQuestionsAreRefused()
        {
            string? index = NativeLookAhead.Index;
            if (NativeLookAhead.Engine == null || index == null)
            {
                _output.WriteLine("the engine or the index is missing; skipping.");
                return;
            }

            using LookAheadLibrary engine = LookAheadLibrary.Open(index);

            var world = new WorldSnapshot();
            world.VariableValues.Add(WireValue.FromBoolean(true));

            var request = new LookAheadRequest(631, world);
            request.Starts.Add(new NodeRef(631, 0));

            LookAheadResponse response = engine.Ask(request);
            _output.WriteLine(response.Error ?? "(no error, which is the bug)");

            Assert.NotNull(response.Error);
            Assert.Contains("different list", response.Error!);
            Assert.Empty(response.Answers);
        }

        /// <summary>The deployed variable table is read, and says how many it declares.</summary>
        [Fact]
        public void TheVariableTableIsReadWhenItIsThere()
        {
            string? index = NativeLookAhead.Index;
            string? variables = NativeLookAhead.Variables;
            if (NativeLookAhead.Engine == null || index == null || variables == null)
            {
                _output.WriteLine("the engine, the index or the variable table is missing; skipping.");
                return;
            }

            using LookAheadLibrary with = LookAheadLibrary.Open(index, variables);
            _output.WriteLine($"{with.VariableCount} declared variables");
            Assert.True(
                with.VariableCount > 10_000,
                $"expected the whole table, got {with.VariableCount}");

            // And without it the mod still opens, answering unset variables less precisely.
            using LookAheadLibrary without = LookAheadLibrary.Open(index);
            Assert.Equal(0, without.VariableCount);
        }

        private static IEnumerable<T> Take<T>(IReadOnlyList<T> items, int count)
        {
            for (int index = 0; index < count && index < items.Count; index++)
            {
                yield return items[index];
            }
        }
    }
}
