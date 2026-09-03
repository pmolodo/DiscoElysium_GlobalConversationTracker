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
        /// Cargo's copy of the library, release preferred over debug, or null if neither
        /// has been built.
        /// </summary>
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

            foreach (string profile in new[] { "release", "debug" })
            {
                string candidate = Path.Combine(root, "target", profile, name);
                if (File.Exists(candidate))
                {
                    return candidate;
                }
            }

            return null;
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
