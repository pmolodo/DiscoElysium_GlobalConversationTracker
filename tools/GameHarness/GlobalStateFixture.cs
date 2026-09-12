// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Text.Json;
using GlobalConversationTracker.Core;

namespace GlobalConversationTracker.Harness
{
    /// <summary>
    /// A suite's global state fixture, as the whole state the game has to be handed.
    /// </summary>
    /// <remarks>
    /// <para>A FIXTURE MAY BE A DIFF OF ANOTHER ONE. The saves have been written that way
    /// from the start - a change over a named base, down a chain - and the state files were
    /// not, so a fixture differing from the one beside it by one orb and two entries cost
    /// another copy of the 29 KB it differed from.</para>
    ///
    /// <para>THE READING IS NOT HERE. It is in the engine's own <c>formats</c> module, and
    /// this runs the engine host to reach it. That is the whole point: the offline runs read
    /// these fixtures with the same code, so a state written as a diff cannot mean one thing
    /// to the harness and another to the tests. A second implementation in C# is exactly the
    /// arrangement that has already cost this repository twice.</para>
    ///
    /// <para>THE MOD STILL SEES A WHOLE STATE, since what is staged into the profile is the
    /// resolved document. Nothing about diffs reaches the game.</para>
    /// </remarks>
    public static class GlobalStateFixture
    {
        /// <summary>What a fixture written as a change to another one says it is.</summary>
        /// <remarks>
        /// The engine's own json-diff, named here because the harness only ever asks
        /// whether a file is one before handing it to the engine that reads it.
        /// </remarks>
        private const string DiffFormat = "json-diff";

        /// <summary>The verb on the engine host that resolves one.</summary>
        private const string ResolveVerb = "resolve";

        /// <summary>How long to wait for it, which is a formality on a 30 KB document.</summary>
        private static readonly TimeSpan Patience = TimeSpan.FromSeconds(30);

        /// <summary>One fixture as a whole state, whether it was written as one or not.</summary>
        /// <param name="scenarioRoot">The folder the fixtures live in.</param>
        /// <param name="fileName">The fixture, by the name a suite gives it.</param>
        /// <returns>The state's JSON.</returns>
        /// <exception cref="FileNotFoundException">It, or the engine host, is not there.</exception>
        /// <exception cref="InvalidDataException">It will not resolve.</exception>
        public static string Resolve(string scenarioRoot, string fileName)
        {
            string path = Path.Combine(scenarioRoot, fileName);
            if (!File.Exists(path))
            {
                throw new FileNotFoundException($"There is no global state at {path}.", path);
            }

            // A WHOLE STATE IS STAGED AS THE BYTES IT IS, rather than round-tripped through
            // a parser that would rewrite it. Every fixture but one is written whole today,
            // and a resolver that reformatted them all on the way to the game would put a
            // diff nobody asked for in the one file a run depends on.
            return IsDiff(path) ? Resolved(path) : File.ReadAllText(path);
        }

        /// <summary>Whether a fixture is a diff of another one.</summary>
        /// <remarks>
        /// BY THE FORMAT IT NAMES, not by whether it names one. Every document carries a
        /// header now, including a whole state, so what tells the two apart is what the
        /// header says.
        /// </remarks>
        /// <param name="path">The fixture.</param>
        /// <returns>True where it says it is a diff.</returns>
        /// <exception cref="InvalidDataException">It is not JSON.</exception>
        private static bool IsDiff(string path)
        {
            try
            {
                using JsonDocument document = JsonDocument.Parse(File.ReadAllText(path));
                return document.RootElement.ValueKind == JsonValueKind.Object
                    && document.RootElement.TryGetProperty(
                        FormatStamp.FormatPropertyName, out JsonElement named)
                    && named.GetString() == DiffFormat;
            }
            catch (JsonException malformed)
            {
                throw new InvalidDataException($"{path} is not JSON: {malformed.Message}");
            }
        }

        /// <summary>The document the engine makes of it.</summary>
        /// <param name="path">The fixture.</param>
        /// <returns>Its JSON, with every diff beneath it applied.</returns>
        /// <exception cref="InvalidDataException">The engine refused it.</exception>
        private static string Resolved(string path)
        {
            return EngineHost.Run($"{path} will not resolve", Patience, ResolveVerb, path);
        }
    }
}
