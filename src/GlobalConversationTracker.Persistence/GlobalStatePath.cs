// SPDX-License-Identifier: MIT
using System;
using System.IO;

namespace GlobalConversationTracker.Persistence
{
    /// <summary>
    /// Works out where the global state file goes, honouring a testing override.
    /// </summary>
    /// <remarks>
    /// <para>The default is <see cref="GlobalStateStore.FileName"/> in the game's
    /// SaveGames directory, and that is what a player gets. The override exists so a
    /// build under test can be pointed somewhere harmless instead of accumulating into
    /// the real global state, which is the one file in this mod that cannot be
    /// regenerated - it is the record of every playthrough.</para>
    ///
    /// <para>An environment variable rather than a config entry, deliberately: it is set
    /// per launch, cannot be left switched on by accident in a shipped config, and does
    /// not invite a player to relocate the file permanently. ProjectGoal.md asked for a
    /// constant file name "for initial implementation", which this respects in every
    /// case the mod is actually played in.</para>
    /// </remarks>
    public static class GlobalStatePath
    {
        /// <summary>
        /// The environment variable that redirects the global state file.
        /// </summary>
        /// <remarks>
        /// Its value is a path. Absolute, it is used as given. Relative - including a
        /// bare file name - it is resolved against the SaveGames directory, so
        /// <c>test.json</c> lands beside the real file rather than in whatever directory
        /// the game happened to be launched from. A value ending in a directory
        /// separator names a directory, and the default file name is used inside it.
        ///
        /// <para>SPELLED IN FULL, where everything else of ours asks a helper for the prefix.
        /// The helper lives beside the automation tools and this assembly ships inside the
        /// game, which is a good enough reason not to pull it in for one constant - the
        /// prefix is here, once, in the only place this assembly names a variable.</para>
        /// </remarks>
        public const string OverrideVariable = "DEGCT_GLOBAL_STATE_PATH";

        /// <summary>
        /// The full path of the global state file.
        /// </summary>
        /// <param name="saveGameDirectory">The game's SaveGames directory.</param>
        /// <param name="overrideValue">
        /// The override's value, or null/empty for none. Normally read from
        /// <see cref="OverrideVariable"/>.
        /// </param>
        /// <exception cref="ArgumentException">
        /// <paramref name="saveGameDirectory"/> is null, empty or whitespace.
        /// </exception>
        public static string Resolve(string saveGameDirectory, string? overrideValue)
        {
            if (string.IsNullOrWhiteSpace(saveGameDirectory))
            {
                throw new ArgumentException(
                    "The SaveGames directory must not be empty.", nameof(saveGameDirectory));
            }

            string directory = saveGameDirectory.TrimEnd(
                Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);

            if (string.IsNullOrWhiteSpace(overrideValue))
            {
                return Path.Combine(directory, GlobalStateStore.FileName);
            }

            string value = overrideValue!.Trim();

            // A trailing separator means "this directory", not "a file with an empty
            // name", so the default file name goes inside it.
            if (EndsWithSeparator(value))
            {
                string named = value.TrimEnd(
                    Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
                return Path.IsPathRooted(named)
                    ? Path.Combine(named, GlobalStateStore.FileName)
                    : Path.Combine(directory, named, GlobalStateStore.FileName);
            }

            return Path.IsPathRooted(value) ? value : Path.Combine(directory, value);
        }

        /// <summary>
        /// The override's value from the environment, or null when it is not set.
        /// </summary>
        public static string? FromEnvironment()
        {
            string? value = Environment.GetEnvironmentVariable(OverrideVariable);
            return string.IsNullOrWhiteSpace(value) ? null : value;
        }

        private static bool EndsWithSeparator(string value)
        {
            char last = value[value.Length - 1];
            return last == Path.DirectorySeparatorChar || last == Path.AltDirectorySeparatorChar;
        }
    }
}
