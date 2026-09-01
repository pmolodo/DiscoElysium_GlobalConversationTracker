// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Text.RegularExpressions;

namespace GlobalConversationTracker.Automation
{
    /// <summary>Reads and changes Steam's per-app cloud toggle in sharedconfig.vdf.</summary>
    public static class SteamCloud
    {
        /// <summary>The key Steam writes when cloud sync is disabled for an app.</summary>
        public const string CloudEnabledKey = "cloudenabled";

        // The per-app toggle lives in the roaming config, not localconfig.vdf: Steam's
        // own cloud storage is app 7, and the per-game "Keep games saves in the Steam
        // Cloud" checkbox writes "cloudenabled" "0" there.
        private const string SharedConfigRelativePath = @"7\remote\sharedconfig.vdf";

        /// <summary>Finds each account's sharedconfig.vdf under a Steam installation.</summary>
        public static string[] FindSharedConfigs(string steamRoot)
        {
            string userdata = Path.Combine(steamRoot, "userdata");
            if (!Directory.Exists(userdata))
            {
                return Array.Empty<string>();
            }

            var found = new List<string>();
            foreach (string account in Directory.GetDirectories(userdata))
            {
                string path = Path.Combine(account, SharedConfigRelativePath);
                if (File.Exists(path))
                {
                    found.Add(path);
                }
            }

            return found.ToArray();
        }

        /// <summary>
        /// Finds the configs that can hold the app's cloud setting, preferring accounts
        /// that already carry a settings block for it.
        /// </summary>
        public static string[] FindAppConfigs(string steamRoot, string appId)
        {
            var withApps = new List<string>();
            var withApp = new List<string>();
            foreach (string path in FindSharedConfigs(steamRoot))
            {
                string vdf;
                try
                {
                    vdf = File.ReadAllText(path);
                    AppsBody(vdf, out _, out _);
                }
                catch (InvalidDataException)
                {
                    // This account's roaming config has no apps block to edit.
                    continue;
                }

                withApps.Add(path);
                if (FindAppBlock(vdf, appId) != null)
                {
                    withApp.Add(path);
                }
            }

            return withApp.Count > 0 ? withApp.ToArray() : withApps.ToArray();
        }

        /// <summary>Whether cloud sync is enabled for an app.</summary>
        public static bool IsEnabled(string vdf, string appId)
        {
            AppBlock? app = FindAppBlock(vdf, appId);
            if (app == null)
            {
                // Steam only writes the key to turn the setting off.
                return true;
            }

            Match key = CloudKey().Match(app.Body);
            return !key.Success || key.Groups["value"].Value != "0";
        }

        /// <summary>Returns a copy with cloud sync enabled or disabled.</summary>
        public static string SetEnabled(string vdf, string appId, bool enabled)
        {
            AppBlock? app = FindAppBlock(vdf, appId);
            if (app == null)
            {
                return enabled ? vdf : InsertAppBlock(vdf, appId);
            }

            Match key = CloudKey().Match(app.Body);
            if (enabled)
            {
                if (!key.Success)
                {
                    return vdf;
                }

                string removed = vdf.Remove(app.BodyStart + key.Index, key.Length);
                // An app block we created ourselves held nothing else; drop it so the
                // file goes back to exactly what Steam wrote.
                string body = removed.Substring(app.BodyStart, app.Body.Length - key.Length);
                return string.IsNullOrWhiteSpace(body)
                    ? removed.Remove(app.BlockStart, app.BlockEnd - app.BlockStart - key.Length)
                    : removed;
            }

            if (key.Success)
            {
                if (key.Groups["value"].Value == "0")
                {
                    return vdf;
                }

                int valueStart = app.BodyStart + key.Groups["value"].Index;
                return vdf.Remove(valueStart, key.Groups["value"].Length).Insert(valueStart, "0");
            }

            return vdf.Insert(
                app.BodyStart, DisabledKeyLine(app.Indent + "\t", NewlineIn(vdf)));
        }

        private static string DisabledKeyLine(string indent, string newline)
        {
            // Steam separates key and value with tabs; spaces are not accepted.
            return indent + "\"" + CloudEnabledKey + "\"\t\t\"0\"" + newline;
        }

        private static string InsertAppBlock(string vdf, string appId)
        {
            string indent = AppsBody(vdf, out int appsStart, out _) + "\t";
            string newline = NewlineIn(vdf);
            string block =
                indent + "\"" + appId + "\"" + newline
                + indent + "{" + newline
                + DisabledKeyLine(indent + "\t", newline)
                + indent + "}" + newline;
            return vdf.Insert(appsStart, block);
        }

        /// <summary>The line ending the file already uses; Steam writes plain LF.</summary>
        private static string NewlineIn(string vdf)
        {
            int newline = vdf.IndexOf('\n');
            return newline > 0 && vdf[newline - 1] == '\r' ? "\r\n" : "\n";
        }

        private static Regex CloudKey()
        {
            return new Regex(
                "[^\\S\\r\\n]*\"" + CloudEnabledKey
                + "\"[^\\S\\r\\n]*\"(?<value>[^\"]*)\"[^\\r\\n]*(?:\\r?\\n|$)",
                RegexOptions.IgnoreCase);
        }

        /// <summary>One app's settings block, and where it sits in the file.</summary>
        private sealed class AppBlock
        {
            public AppBlock(
                string body, string indent, int bodyStart, int blockStart, int blockEnd)
            {
                Body = body;
                Indent = indent;
                BodyStart = bodyStart;
                BlockStart = blockStart;
                BlockEnd = blockEnd;
            }

            /// <summary>The text between the block's braces.</summary>
            public string Body { get; }

            /// <summary>The block's own indentation.</summary>
            public string Indent { get; }

            /// <summary>Where <see cref="Body"/> starts in the whole file.</summary>
            public int BodyStart { get; }

            /// <summary>The start of the line naming the app.</summary>
            public int BlockStart { get; }

            /// <summary>Just past the newline after the closing brace.</summary>
            public int BlockEnd { get; }
        }

        private static AppBlock? FindAppBlock(string vdf, string appId)
        {
            AppsBody(vdf, out int appsStart, out int appsEnd);
            Match? app = TryFindBlock(vdf, appId, appsStart, appsEnd);
            if (app == null)
            {
                return null;
            }

            int bodyStart = app.Index + app.Length;
            int bodyEnd = FindClosingBrace(vdf, bodyStart);
            return new AppBlock(
                vdf.Substring(bodyStart, bodyEnd - bodyStart),
                app.Groups["indent"].Value,
                bodyStart,
                app.Index,
                EndOfLine(vdf, bodyEnd));
        }

        /// <summary>
        /// Locates the apps block that holds every per-app setting, and returns its own
        /// indentation so anything inserted below it lines up.
        /// </summary>
        private static string AppsBody(string vdf, out int bodyStart, out int bodyEnd)
        {
            int start = 0;
            int end = vdf.Length;
            string indent = string.Empty;
            foreach (string key in new[] { "Software", "Valve", "Steam", "apps" })
            {
                Match block = FindBlock(vdf, key, start, end);
                indent = block.Groups["indent"].Value;
                start = block.Index + block.Length;
                end = FindClosingBrace(vdf, start);
            }

            bodyStart = start;
            bodyEnd = end;
            return indent;
        }

        private static Match FindBlock(string vdf, string key, int start, int end)
        {
            return TryFindBlock(vdf, key, start, end)
                ?? throw new InvalidDataException($"No '{key}' block was found.");
        }

        private static Match? TryFindBlock(string vdf, string key, int start, int end)
        {
            var pattern = new Regex(
                "(?<indent>[^\\S\\r\\n]*)\"" + Regex.Escape(key)
                + "\"[^\\S\\r\\n]*\\r?\\n[^\\S\\r\\n]*\\{[^\\S\\r\\n]*\\r?\\n",
                RegexOptions.IgnoreCase);
            MatchCollection matches = pattern.Matches(vdf, start);
            Match? found = null;
            foreach (Match match in matches)
            {
                if (match.Index >= end)
                {
                    break;
                }

                if (found != null)
                {
                    throw new InvalidDataException($"Found more than one '{key}' block.");
                }

                found = match;
            }

            return found;
        }

        private static int FindClosingBrace(string vdf, int bodyStart)
        {
            int depth = 1;
            bool quoted = false;
            bool escaped = false;
            for (int i = bodyStart; i < vdf.Length; i++)
            {
                char character = vdf[i];
                if (quoted)
                {
                    if (escaped)
                    {
                        escaped = false;
                    }
                    else if (character == '\\')
                    {
                        escaped = true;
                    }
                    else if (character == '"')
                    {
                        quoted = false;
                    }
                    continue;
                }

                if (character == '"')
                {
                    quoted = true;
                }
                else if (character == '{')
                {
                    depth++;
                }
                else if (character == '}' && --depth == 0)
                {
                    return i;
                }
            }

            throw new InvalidDataException("Unbalanced braces in sharedconfig.vdf.");
        }

        private static int EndOfLine(string text, int index)
        {
            int newline = text.IndexOf('\n', index);
            return newline < 0 ? text.Length : newline + 1;
        }
    }
}
