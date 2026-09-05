// SPDX-License-Identifier: MIT
using System;
using System.IO;
using GlobalConversationTracker.Engine;
using Xunit;
using Xunit.Abstractions;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// Can managed code load the Rust look-ahead library and get a right answer back?
    /// </summary>
    /// <remarks>
    /// <para>The walking skeleton for de-i5xj. It proves the part that cannot be proved
    /// by reading: that the crate's <c>cdylib</c> builds into something this platform's
    /// loader accepts, that the calling convention agrees, and that a handle survives a
    /// round trip. Everything the bridge will carry later rides on those three facts.</para>
    ///
    /// <para>Opt-in, the same way the corpus suites are. The library is a build artefact
    /// and the index is extracted game content, and neither is committed - so where they
    /// have not been produced these pass silently rather than failing on a machine that
    /// was never going to have them. Produce them with
    /// <c>cargo build</c> and
    /// <c>dotnet run --project tools/DialogueExtract -- conversation-index</c>.</para>
    /// </remarks>
    public class BridgeTests
    {
        private readonly ITestOutputHelper _output;

        public BridgeTests(ITestOutputHelper output)
        {
            _output = output;
            NativeLookAhead.Install();
        }

        [Fact]
        public void TheLibraryReportsAVersion()
        {
            if (NativeLookAhead.Engine == null)
            {
                _output.WriteLine("the engine is not built; skipping. Run: cargo build");
                return;
            }

            string version = LookAheadLibrary.Version;
            _output.WriteLine($"native look-ahead library version {version}");
            Assert.False(string.IsNullOrEmpty(version));
        }

        /// <summary>
        /// A path that is not an index is refused, rather than crashing the process.
        /// </summary>
        /// <remarks>
        /// The failure mode this whole bridge is written against. A modded game that
        /// disappears has no useful report in it; one that logs a refusal does.
        /// </remarks>
        [Fact]
        public void AnIndexThatIsNotThereIsRefused()
        {
            if (NativeLookAhead.Engine == null)
            {
                _output.WriteLine("the engine is not built; skipping. Run: cargo build");
                return;
            }

            InvalidOperationException refused = Assert.Throws<InvalidOperationException>(
                () => LookAheadLibrary.Open("no-such-index.jsonl"));

            Assert.Contains(nameof(Status.IndexUnreadable), refused.Message);
        }

        [Fact]
        public void TheIndexOpensAndReportsWhatIsInIt()
        {
            string? index = NativeLookAhead.Index;
            if (NativeLookAhead.Engine == null || index == null)
            {
                _output.WriteLine("the engine or the index is missing; skipping.");
                return;
            }

            using LookAheadLibrary engine = LookAheadLibrary.Open(index);

            int conversations = engine.ConversationCount;
            _output.WriteLine($"{conversations} conversations in the index");
            Assert.True(conversations > 1000, $"expected the whole database, got {conversations}");

            // Conversation 631's group is the one the symbolic work is measured on, so its
            // shape is known independently of this bridge.
            int entries = engine.EntryCount(631);
            _output.WriteLine($"conversation 631 has {entries} entries");
            Assert.True(entries > 0);

            Assert.Equal(-1, engine.EntryCount(-12345));
        }

        /// <summary>
        /// The engine names the questions its own answers will be looked up under.
        /// </summary>
        [Fact]
        public void TheEngineDescribesWhatItNeedsToKnow()
        {
            string? index = NativeLookAhead.Index;
            if (NativeLookAhead.Engine == null || index == null)
            {
                _output.WriteLine("the engine or the index is missing; skipping.");
                return;
            }

            using LookAheadLibrary engine = LookAheadLibrary.Open(index);

            // Conversation 631's group is the one every measurement uses, so its shape is
            // known independently of this bridge: six conversations and 4,514 entries.
            string questions = engine.Questions(631);
            _output.WriteLine(
                questions.Length > 400 ? questions.Substring(0, 400) + "..." : questions);

            Assert.Contains("\"conversations\"", questions);
            Assert.Contains("\"queries\"", questions);
            Assert.Contains("\"entries\"", questions);
            // The group, not just the conversation asked about.
            Assert.Contains("636", questions);
        }

        /// <summary>
        /// A whole look-ahead question crosses and comes back answered.
        /// </summary>
        /// <remarks>
        /// The end of the round trip this project exists to make: a request built here, a
        /// crawl run over there, an answer parsed back. Deliberately asks about a small
        /// conversation - what is being checked is the crossing, not the search.
        /// </remarks>
        [Fact]
        public void AQuestionCrossesAndComesBackAnswered()
        {
            string? index = NativeLookAhead.Index;
            if (NativeLookAhead.Engine == null || index == null)
            {
                _output.WriteLine("the engine or the index is missing; skipping.");
                return;
            }

            using LookAheadLibrary engine = LookAheadLibrary.Open(index);

            const string Request = @"{
                ""conversation"": 1123,
                ""starts"": [ { ""conversation"": 1123, ""entry"": 0 } ],
                ""unseen_any_game"": [ { ""conversation"": 1123, ""entry"": 3 } ],
                ""world"": {
                    ""money"": 0, ""day_minutes"": 720,
                    ""day_counter"": 1, ""clock_locked"": false
                }
            }";

            string response = engine.LookAhead(Request);
            _output.WriteLine(response);

            Assert.Contains("\"answers\"", response);
            Assert.Contains("\"start\"", response);
            Assert.DoesNotContain("\"error\":\"", response);
        }

        /// <summary>
        /// A request that is not JSON is refused as a call, not as a response.
        /// </summary>
        [Fact]
        public void ARequestThatIsNotJsonIsRefused()
        {
            string? index = NativeLookAhead.Index;
            if (NativeLookAhead.Engine == null || index == null)
            {
                _output.WriteLine("the engine or the index is missing; skipping.");
                return;
            }

            using LookAheadLibrary engine = LookAheadLibrary.Open(index);

            InvalidOperationException refused = Assert.Throws<InvalidOperationException>(
                () => engine.LookAhead("not json"));

            Assert.Contains(nameof(Status.BadArgument), refused.Message);
        }

        /// <summary>
        /// Disposing twice is harmless, which is what the SafeHandle is for.
        /// </summary>
        [Fact]
        public void ClosingTwiceIsHarmless()
        {
            string? index = NativeLookAhead.Index;
            if (NativeLookAhead.Engine == null || index == null)
            {
                _output.WriteLine("the engine or the index is missing; skipping.");
                return;
            }

            LookAheadLibrary engine = LookAheadLibrary.Open(index);
            engine.Dispose();
            engine.Dispose();
        }
    }
}
