// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Text.RegularExpressions;

namespace GlobalConversationTracker.Automation
{
    /// <summary>Reading and writing Steam's per-game cloud toggle.</summary>
    /// <remarks>
    /// <para>Steam Auto-Cloud syncs a game's configured folders when it launches and again
    /// when it exits, downloading the cloud copy BEFORE the game starts. For a test that
    /// stages a settings file, that means the staged file is replaced moments after being
    /// written, and the game reads the player's real settings instead.</para>
    ///
    /// <para>The per-game toggle lives in localconfig.vdf under the account's userdata, as
    /// a "cloudenabled" key inside the app's block. The key is ABSENT when cloud is
    /// enabled - Steam only writes it when the box is unchecked - so absent and "1" both
    /// mean enabled, and restoring means removing the key again rather than writing "1".
    /// </para>
    ///
    /// <para>Editing requires Steam to be stopped. Steam keeps this file in memory and
    /// rewrites it on exit, so an edit made while it is running is simply lost. The file
    /// also carries client preferences, per-game launch options and a license cache, which
    /// is why every write here is surgical - one key in one block - rather than restoring a
    /// whole-file backup that would revert whatever else Steam legitimately changed while
    /// the test ran.</para>
    /// </remarks>
    public static class SteamCloud
    {
        /// <summary>The key Steam writes when the per-game cloud box is unchecked.</summary>
        public const string CloudEnabledKey = "cloudenabled";

        /// <summary>Finds the localconfig.vdf files under a Steam installation.</summary>
        /// <param name="steamRoot">The Steam directory containing userdata.</param>
        /// <returns>One path per account found.</returns>
        public static string[] FindLocalConfigs(string steamRoot)
        {
            string userdata = Path.Combine(steamRoot, "userdata");
            if (!Directory.Exists(userdata))
            {
                return Array.Empty<string>();
            }

            var found = new List<string>();
            foreach (string account in Directory.GetDirectories(userdata))
            {
                string config = Path.Combine(account, "config", "localconfig.vdf");
                if (File.Exists(config))
                {
                    found.Add(config);
                }
            }

            return found.ToArray();
        }

        /// <summary>Whether cloud sync is enabled for an app.</summary>
        /// <param name="vdf">The localconfig.vdf contents.</param>
        /// <param name="appId">The Steam app id.</param>
        /// <returns>True when enabled, which includes the key being absent.</returns>
        public static bool IsEnabled(string vdf, string appId)
        {
            string? value = ReadKey(vdf, appId, CloudEnabledKey);
            return value == null || value != "0";
        }

        /// <summary>Sets, or clears, the cloud toggle for an app.</summary>
        /// <param name="vdf">The localconfig.vdf contents.</param>
        /// <param name="appId">The Steam app id.</param>
        /// <param name="enabled">
        /// False writes the key as "0". True REMOVES the key, which is how Steam itself
        /// represents enabled; writing "1" would leave behind a key Steam never writes.
        /// </param>
        /// <exception cref="InvalidDataException">The app block was not found exactly once.</exception>
        public static string SetEnabled(string vdf, string appId, bool enabled)
        {
            Match block = FindAppBlock(vdf, appId);
            string? existing = ReadKey(vdf, appId, CloudEnabledKey);
            int bodyStart = block.Index + block.Length;

            if (enabled)
            {
                if (existing == null)
                {
                    return vdf;
                }

                var line = new Regex(
                    "[^\\S\\r\\n]*\"" + CloudEnabledKey + "\"[^\\r\\n]*\\r?\\n",
                    RegexOptions.IgnoreCase);
                Match found = line.Match(vdf, bodyStart);
                if (!found.Success)
                {
                    throw new InvalidDataException(
                        $"Found a {CloudEnabledKey} value for app {appId} but could not locate its "
                        + "line to remove.");
                }

                return vdf.Remove(found.Index, found.Length);
            }

            if (existing == "0")
            {
                return vdf;
            }

            // Match the file's own indentation: one level deeper than the block's brace.
            string indent = block.Groups["indent"].Value + "\t";
            return vdf.Insert(bodyStart, indent + "\"" + CloudEnabledKey + "\"\t\t\"0\"\r\n");
        }

        /// <summary>Reads a key from inside an app's block.</summary>
        /// <param name="vdf">The localconfig.vdf contents.</param>
        /// <param name="appId">The Steam app id.</param>
        /// <param name="key">The key to read.</param>
        /// <returns>Its value, or null when absent.</returns>
        public static string? ReadKey(string vdf, string appId, string key)
        {
            Match block = FindAppBlock(vdf, appId);
            string body = BodyOf(vdf, block);
            var pattern = new Regex(
                "\"" + Regex.Escape(key) + "\"\\s*\"([^\"]*)\"", RegexOptions.IgnoreCase);
            Match match = pattern.Match(body);
            return match.Success ? match.Groups[1].Value : null;
        }

        /// <summary>
        /// Locates an app's settings block, as opposed to the other places the same id
        /// appears in the file.
        /// </summary>
        /// <remarks>
        /// The id also appears as a plain key/value pair in the licenses and language
        /// sections, so matching the id alone finds the wrong thing. What identifies the
        /// settings block is the id being followed by a BRACE rather than a value.
        /// </remarks>
        /// <param name="vdf">The localconfig.vdf contents.</param>
        /// <param name="appId">The Steam app id.</param>
        /// <exception cref="InvalidDataException">Not found, or found more than once.</exception>
        private static Match FindAppBlock(string vdf, string appId)
        {
            var pattern = new Regex(
                "(?<indent>[^\\S\\r\\n]*)\"" + Regex.Escape(appId) + "\"[^\\S\\r\\n]*\\r?\\n"
                + "[^\\S\\r\\n]*\\{[^\\S\\r\\n]*\\r?\\n");

            MatchCollection matches = pattern.Matches(vdf);
            if (matches.Count != 1)
            {
                throw new InvalidDataException(
                    $"Expected exactly one settings block for app {appId} in localconfig.vdf, "
                    + $"found {matches.Count}. Refusing to guess which one to edit.");
            }

            return matches[0];
        }

        /// <summary>The text inside an app's block, to its closing brace.</summary>
        private static string BodyOf(string vdf, Match block)
        {
            int start = block.Index + block.Length;
            int depth = 1;
            for (int i = start; i < vdf.Length; i++)
            {
                if (vdf[i] == '{')
                {
                    depth++;
                }
                else if (vdf[i] == '}')
                {
                    depth--;
                    if (depth == 0)
                    {
                        return vdf.Substring(start, i - start);
                    }
                }
            }

            throw new InvalidDataException("Unbalanced braces in localconfig.vdf.");
        }
    }
}
