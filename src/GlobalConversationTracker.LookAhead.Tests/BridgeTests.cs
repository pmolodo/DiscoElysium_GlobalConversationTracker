// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
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

        /// <summary>
        /// Installed once, so <c>DllImport</c> finds the library in Cargo's output rather
        /// than beside this test assembly.
        /// </summary>
        /// <remarks>
        /// Deployed, the library sits next to the plugin and the ordinary search finds it.
        /// Here it sits in <c>target/</c>, which nothing would look in, so the resolver
        /// points at it directly. A static constructor rather than a fixture because it
        /// must run before the first <c>DllImport</c> in the process, whichever test that
        /// turns out to be.
        /// </remarks>
        static BridgeTests()
        {
            NativeLibrary.SetDllImportResolver(
                typeof(LookAheadLibrary).Assembly,
                (name, assembly, path) =>
                {
                    string? library = FindLibrary();
                    return library == null
                        ? IntPtr.Zero
                        : NativeLibrary.Load(library);
                });
        }

        public BridgeTests(ITestOutputHelper output)
        {
            _output = output;
        }

        /// <summary>The repository root, walked up from the test assembly.</summary>
        private static string? RepositoryRoot()
        {
            DirectoryInfo? directory = new DirectoryInfo(
                Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location)!);

            while (directory != null)
            {
                if (Directory.Exists(Path.Combine(directory.FullName, ".git")))
                {
                    return directory.FullName;
                }

                directory = directory.Parent;
            }

            return null;
        }

        /// <summary>
        /// Cargo's copy of the library, the more recently built of release and debug, or
        /// null if neither has been built.
        /// </summary>
        /// <remarks>
        /// NEWER rather than release-first, which is not a preference but a bug fix. A
        /// stale release build silently shadows a fresh debug one, and the symptom is an
        /// EntryPointNotFoundException naming a function that was added minutes ago -
        /// which reads as a marshalling problem and is not one. Whichever was built last
        /// is the one the developer meant.
        /// </remarks>
        private static string? FindLibrary()
        {
            string? root = RepositoryRoot();
            if (root == null)
            {
                return null;
            }

            string name = RuntimeInformation.IsOSPlatform(OSPlatform.Windows)
                ? "lookahead_engine.dll"
                : RuntimeInformation.IsOSPlatform(OSPlatform.OSX)
                    ? "liblookahead_engine.dylib"
                    : "liblookahead_engine.so";

            string? newest = null;
            DateTime newestAt = DateTime.MinValue;
            foreach (string profile in new[] { "release", "debug" })
            {
                string candidate = Path.Combine(root, "target", profile, name);
                if (!File.Exists(candidate))
                {
                    continue;
                }

                DateTime written = File.GetLastWriteTimeUtc(candidate);
                if (newest == null || written > newestAt)
                {
                    newest = candidate;
                    newestAt = written;
                }
            }

            return newest;
        }

        /// <summary>The conversation index, or null where it has not been extracted.</summary>
        private static string? FindIndex()
        {
            string? root = RepositoryRoot();
            if (root == null)
            {
                return null;
            }

            string candidate = Path.Combine(
                root, ".game_reference_copies", "derived", "conversation_index.jsonl");
            return File.Exists(candidate) ? candidate : null;
        }

        [Fact]
        public void TheLibraryReportsAVersion()
        {
            if (FindLibrary() == null)
            {
                _output.WriteLine("the native library is not built; skipping. Run: cargo build");
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
            if (FindLibrary() == null)
            {
                _output.WriteLine("the native library is not built; skipping. Run: cargo build");
                return;
            }

            InvalidOperationException refused = Assert.Throws<InvalidOperationException>(
                () => LookAheadLibrary.Open("no-such-index.jsonl"));

            Assert.Contains(nameof(Status.IndexUnreadable), refused.Message);
        }

        [Fact]
        public void TheIndexOpensAndReportsWhatIsInIt()
        {
            string? index = FindIndex();
            if (FindLibrary() == null || index == null)
            {
                _output.WriteLine("the library or the index is missing; skipping.");
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
            string? index = FindIndex();
            if (FindLibrary() == null || index == null)
            {
                _output.WriteLine("the library or the index is missing; skipping.");
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
            string? index = FindIndex();
            if (FindLibrary() == null || index == null)
            {
                _output.WriteLine("the library or the index is missing; skipping.");
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
            string? index = FindIndex();
            if (FindLibrary() == null || index == null)
            {
                _output.WriteLine("the library or the index is missing; skipping.");
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
            string? index = FindIndex();
            if (FindLibrary() == null || index == null)
            {
                _output.WriteLine("the library or the index is missing; skipping.");
                return;
            }

            LookAheadLibrary engine = LookAheadLibrary.Open(index);
            engine.Dispose();
            engine.Dispose();
        }
    }
}
