// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using GlobalConversationTracker.Engine;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// Finding the native look-ahead library and the files it opens, from a test run.
    /// </summary>
    /// <remarks>
    /// <para>Deployed, all three sit beside the plugin and the ordinary search finds them.
    /// Here they are build artefacts and extracted game content in <c>target/</c> and
    /// <c>.game_reference_copies/</c>, which nothing would look in - so the resolver is
    /// pointed at the library directly and the rest are found by path.</para>
    ///
    /// <para>Shared by every test class that needs them. The <c>DllImport</c> resolver may
    /// only be installed once per assembly, and installing it from one test class's static
    /// constructor would leave a second class depending on which ran first.</para>
    /// </remarks>
    internal static class NativeLookAhead
    {
        /// <summary>
        /// Installed once, before the first <c>DllImport</c> in the process, whichever test
        /// turns out to make it.
        /// </summary>
        static NativeLookAhead()
        {
            NativeLibrary.SetDllImportResolver(
                typeof(LookAheadLibrary).Assembly,
                (name, assembly, path) =>
                {
                    string? library = Library;
                    return library == null ? IntPtr.Zero : NativeLibrary.Load(library);
                });
        }

        /// <summary>Makes sure the resolver is in place. Call before any native call.</summary>
        internal static void Install()
        {
            // The static constructor is the whole of it; this exists so a test can say so.
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
        internal static string? Library
        {
            get
            {
                string? root = RepositoryRoot;
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
        }

        /// <summary>The conversation index, or null where it has not been extracted.</summary>
        internal static string? Index => Derived("conversation_index.jsonl");

        /// <summary>The variable table, or null where it has not been extracted.</summary>
        internal static string? Variables => Derived("variables.jsonl");

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
