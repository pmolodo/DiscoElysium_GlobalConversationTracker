// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Reflection;
using BepInEx.Logging;
using GlobalConversationTracker.Engine;

namespace GlobalConversationTracker
{
    /// <summary>
    /// Says whether the native look-ahead library loaded, in one log line at startup.
    /// </summary>
    /// <remarks>
    /// <para>The look-ahead is moving from C# into Rust (de-i5xj), and the part that
    /// cannot be checked anywhere but here is whether the plugin can reach it INSIDE THE
    /// GAME. The unit tests prove the library loads under a test runner, which is the
    /// same runtime family - BepInEx 6 runs IL2CPP plugins on CoreCLR - but not the same
    /// process, the same folder, or the same loader state.</para>
    ///
    /// <para>So this is deliberately small: call the library, write what it said, and
    /// never throw. It is a smoke test that ships, not a feature. What it buys is that a
    /// bridge which is broken says so in the log on every launch, rather than being
    /// discovered later through a marker that is quietly wrong.</para>
    ///
    /// <para>NOTHING HERE MAY FAIL THE LOAD. A missing or unloadable library is a mod
    /// with one fewer capability, not a game that will not start - so every failure is
    /// caught and logged, including the ones that arrive as a
    /// <see cref="DllNotFoundException"/> from the first call rather than from anything
    /// this file does.</para>
    /// </remarks>
    internal static class NativeEngineCheck
    {
        /// <summary>
        /// The prefix every line here starts with, so a harness can find them.
        /// </summary>
        /// <remarks>
        /// One string, referenced by the plugin and by the test that greps for it, rather
        /// than a message written twice and kept in step by hope.
        /// </remarks>
        internal const string LogPrefix = "Native look-ahead:";

        /// <summary>The conversation index, if it was deployed beside the plugin.</summary>
        /// <remarks>
        /// Named to match the deployment predicate rather than after the file it is built
        /// from: only <c>GlobalConversationTracker*</c> is installed, so a
        /// <c>conversation_index.jsonl</c> would be built, ignored, and never noticed
        /// missing.
        /// </remarks>
        internal const string IndexFileName = "GlobalConversationTracker.Index.jsonl";

        /// <summary>
        /// Reports what the native library says about itself, and about the index if one
        /// is deployed.
        /// </summary>
        internal static void Report(ManualLogSource log)
        {
            string version;
            try
            {
                version = LookAheadLibrary.Version;
            }
            catch (Exception error)
            {
                // The first call is where a missing DLL, a wrong architecture or an
                // unresolvable dependency shows up, and all three arrive here rather than
                // at load. Naming the type matters: DllNotFoundException and
                // BadImageFormatException mean quite different things to whoever reads it.
                log.LogWarning(
                    $"{LogPrefix} unavailable ({error.GetType().Name}: {error.Message}). "
                    + "The look-ahead will use the managed engine.");
                return;
            }

            log.LogMessage($"{LogPrefix} library v{version} loaded.");

            string? index = FindIndex();
            if (index == null)
            {
                log.LogMessage(
                    $"{LogPrefix} no {IndexFileName} beside the plugin; "
                    + "nothing to open yet.");
                return;
            }

            try
            {
                using LookAheadLibrary engine = LookAheadLibrary.Open(index);
                log.LogMessage(
                    $"{LogPrefix} index opened, {engine.ConversationCount} conversations.");
            }
            catch (Exception error)
            {
                log.LogWarning(
                    $"{LogPrefix} index at {index} would not open "
                    + $"({error.GetType().Name}: {error.Message}).");
            }
        }

        /// <summary>The index beside this assembly, or null if it was not deployed.</summary>
        private static string? FindIndex()
        {
            string? directory = Path.GetDirectoryName(
                Assembly.GetExecutingAssembly().Location);
            if (string.IsNullOrEmpty(directory))
            {
                return null;
            }

            string candidate = Path.Combine(directory, IndexFileName);
            return File.Exists(candidate) ? candidate : null;
        }
    }
}
