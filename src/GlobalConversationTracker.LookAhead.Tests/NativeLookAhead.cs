// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using GlobalConversationTracker.Engine;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// Finding the look-ahead engine and the files it opens, from a test run.
    /// </summary>
    /// <remarks>
    /// <para>Deployed, all three sit beside the plugin and the default finds them. Here they
    /// are build artefacts and extracted game content in <c>target/</c> and
    /// <c>.game_reference_copies/</c>, which nothing would look in - so
    /// <see cref="LookAheadLibrary.EnginePath"/> is pointed at the executable directly and
    /// the rest are found by path.</para>
    ///
    /// <para>THE ENGINE IS A PROCESS, not a library this one loads - de-bnjy.1. What used
    /// to be installed here was a <c>DllImport</c> resolver, which could only be installed
    /// once per assembly and so had to be shared by every test class that needed it. The
    /// path is shared for the same reason, and setting it twice is harmless where
    /// installing a resolver twice was not.</para>
    /// </remarks>
    internal static class NativeLookAhead
    {
        /// <summary>
        /// Points the engine at Cargo's build, before the first call that starts one.
        /// </summary>
        static NativeLookAhead()
        {
            LookAheadLibrary.EnginePath = Engine;
        }

        /// <summary>Makes sure the path is set. Call before starting an engine.</summary>
        internal static void Install()
        {
            // The static constructor is the whole of it; this exists so a test can say so.
        }

        /// <summary>
        /// Cargo's copy of the engine executable, the more recently built of release and
        /// debug, or null if neither has been built.
        /// </summary>
        /// <remarks>
        /// NEWER rather than release-first, which is not a preference but a bug fix. A
        /// stale release build silently shadows a fresh debug one, and the symptom is an
        /// answer that does not contain a change made minutes ago - which reads as a bug in
        /// the change and is not one. Whichever was built last is the one the developer
        /// meant.
        /// </remarks>
        internal static string? Engine
        {
            get
            {
                string? root = RepositoryRoot;
                if (root == null)
                {
                    return null;
                }

                string name = RuntimeInformation.IsOSPlatform(OSPlatform.Windows)
                    ? "gct-engine-host.exe"
                    : "gct-engine-host";

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
        }

        /// <summary>The conversation index, or null where it has not been extracted.</summary>
        internal static string? Index => Derived("conversation_index.jsonl");

        /// <summary>The variable table, or null where it has not been extracted.</summary>
        internal static string? Variables => Derived("variables.jsonl");

        /// <summary>The same table, for a test that has already found the index.</summary>
        /// <remarks>
        /// The engine refuses to open without one, so every test that opens has to name it.
        /// Both files come out of the same extraction, so an index with no table beside it is
        /// a half-built working copy rather than a machine without the game data - which is
        /// what the null check on <see cref="Index"/> is there to skip.
        /// </remarks>
        internal static string Declared =>
            Variables ?? throw new InvalidOperationException(
                "variables.jsonl is not beside the extracted index; re-run the extractor.");

        /// <summary>One extracted file, or null if it is not there.</summary>
        private static string? Derived(string fileName)
        {
            string? root = RepositoryRoot;
            if (root == null)
            {
                return null;
            }

            string candidate = Path.Combine(
                root, ".game_reference_copies", "derived", fileName);
            return File.Exists(candidate) ? candidate : null;
        }

        /// <summary>The repository root, walked up from the test assembly.</summary>
        private static string? RepositoryRoot
        {
            get
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
        }
    }
}
