// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Text.RegularExpressions;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Asking Sysinternals' Handle which processes have a path open.
    /// </summary>
    /// <remarks>
    /// <para>The only thing that answers the question this harness actually has, which is
    /// about a DIRECTORY: Explorer showing a folder holds a handle on the directory itself
    /// for change notifications, and none on any file inside it, so it blocks a rename
    /// while appearing in no Restart Manager list.</para>
    ///
    /// <para>Shelling out rather than reimplementing. Doing this in-process means
    /// NtQuerySystemInformation with an information class Microsoft does not document,
    /// then duplicating every file handle out of every process to name it - and the two
    /// hard parts of that are exactly what Handle already solves: name queries that block
    /// forever on synchronous pipe handles, and needing elevation to see other processes.
    /// An attempt at the in-process version hung for over two minutes on a single lookup
    /// before this replaced it.</para>
    ///
    /// <para>Optional by design. When Handle is not installed the caller says so and falls
    /// back to what can be managed without it; nothing here is required for a test to run,
    /// only for it to explain itself when a move fails.</para>
    /// </remarks>
    public static class SysinternalsHandle
    {
        /// <summary>How long to let it run before giving up.</summary>
        /// <remarks>
        /// It walks every handle in the system, which takes a few seconds, and it is being
        /// run inside an error path. A diagnostic that hangs is worse than no diagnostic.
        /// </remarks>
        public static readonly TimeSpan Timeout = TimeSpan.FromSeconds(30);

        private static readonly string[] Names = { "handle64.exe", "handle.exe" };

        private static readonly string[] CommonDirectories =
        {
            @"C:\Sysinternals",
            @"C:\tools\sysinternals",
            @"C:\Program Files\Sysinternals",
            @"C:\Program Files (x86)\Sysinternals",
        };

        /// <summary>
        /// Output lines look like: name pid: 1234 type: File 4B8: C:\some\path
        /// </summary>
        private static readonly Regex Line = new Regex(
            @"^(?<process>.+?)\s+pid:\s*(?<pid>\d+)\s+type:\s*\w+\s+(?:[0-9A-Fa-f]+:\s*)?(?<name>.+?)\s*$",
            RegexOptions.Compiled);

        /// <summary>Where Handle is, or null if it is not installed.</summary>
        public static string? Find()
        {
            foreach (string name in Names)
            {
                string? onPath = FindOnPath(name);
                if (onPath != null)
                {
                    return onPath;
                }
            }

            foreach (string directory in CommonDirectories)
            {
                foreach (string name in Names)
                {
                    string candidate = Path.Combine(directory, name);
                    if (File.Exists(candidate))
                    {
                        return candidate;
                    }
                }
            }

            return null;
        }

        /// <summary>What to tell someone who does not have it.</summary>
        public const string InstallHint =
            "Install Sysinternals Handle for a direct answer "
            + "(https://learn.microsoft.com/sysinternals/downloads/handle), put it on PATH, "
            + "and run this again as administrator.";

        /// <summary>Which processes hold a path, or anything beneath it.</summary>
        /// <param name="path">The file or folder to ask about.</param>
        /// <param name="executable">Where Handle is; found automatically when omitted.</param>
        /// <returns>What it reported, or null if Handle is not installed.</returns>
        public static LockHolder[]? WhoIsHolding(string path, string? executable = null)
        {
            string? handle = executable ?? Find();
            if (handle == null || !File.Exists(handle))
            {
                // Absent, or configured to somewhere it is not. Both are "no answer
                // available", which is a different thing from "nothing is holding it" -
                // hence null rather than an empty array.
                return null;
            }

            string full = Path.GetFullPath(path).TrimEnd(
                Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);

            // -nobanner keeps the copyright header out of the output; -accepteula stops it
            // blocking on the licence dialog the first time it is ever run.
            string output = Run(handle, $"-nobanner -accepteula \"{full}\"");
            return Parse(output, full);
        }

        /// <summary>Reads Handle's search output into holders.</summary>
        /// <param name="output">What Handle printed.</param>
        /// <param name="target">The path asked about, for filtering.</param>
        public static LockHolder[] Parse(string output, string target)
        {
            var found = new Dictionary<int, LockHolder>();
            if (string.IsNullOrEmpty(output))
            {
                return Array.Empty<LockHolder>();
            }

            foreach (string raw in output.Split('\n'))
            {
                Match match = Line.Match(raw.TrimEnd('\r'));
                if (!match.Success)
                {
                    continue;
                }

                if (!int.TryParse(match.Groups["pid"].Value, out int pid))
                {
                    continue;
                }

                string name = match.Groups["name"].Value.Trim();

                // Handle matches a FRAGMENT anywhere in the path, so it will report
                // siblings whose names merely begin the same way. Keep only what is really
                // the target or inside it.
                bool isTarget = name.Equals(target, StringComparison.OrdinalIgnoreCase)
                    || name.StartsWith(target + Path.DirectorySeparatorChar,
                        StringComparison.OrdinalIgnoreCase);
                if (!isTarget)
                {
                    continue;
                }

                if (!found.ContainsKey(pid))
                {
                    found[pid] = new LockHolder(
                        pid, match.Groups["process"].Value.Trim(), name);
                }
            }

            var holders = new List<LockHolder>(found.Values);
            return holders.ToArray();
        }

        private static string Run(string executable, string arguments)
        {
            var start = new ProcessStartInfo(executable, arguments)
            {
                UseShellExecute = false,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                CreateNoWindow = true,
            };

            using Process? process = Process.Start(start);
            if (process == null)
            {
                return string.Empty;
            }

            string output = process.StandardOutput.ReadToEnd();
            if (!process.WaitForExit((int)Timeout.TotalMilliseconds))
            {
                try
                {
                    process.Kill();
                }
                catch (Exception)
                {
                    // Already gone.
                }

                return output;
            }

            return output;
        }

        private static string? FindOnPath(string name)
        {
            string? path = Environment.GetEnvironmentVariable("PATH");
            if (string.IsNullOrEmpty(path))
            {
                return null;
            }

            foreach (string directory in path!.Split(Path.PathSeparator))
            {
                if (directory.Length == 0)
                {
                    continue;
                }

                try
                {
                    string candidate = Path.Combine(directory, name);
                    if (File.Exists(candidate))
                    {
                        return candidate;
                    }
                }
                catch (Exception)
                {
                    // A malformed PATH entry; skip it.
                }
            }

            return null;
        }
    }
}
