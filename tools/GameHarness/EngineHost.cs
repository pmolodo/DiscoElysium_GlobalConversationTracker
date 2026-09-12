// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using GlobalConversationTracker.Automation;

namespace GlobalConversationTracker.Harness
{
    /// <summary>
    /// The engine host as a command, for the verbs that read and write this repo's formats.
    /// </summary>
    /// <remarks>
    /// <para>THE FORMATS ARE DEFINED IN THE ENGINE, and the harness reaches them by running
    /// it rather than by holding a second copy. A fact stated twice drifts, and the half
    /// that drifts is whichever one no test happens to exercise - which is the arrangement
    /// this repository has already paid for twice.</para>
    ///
    /// <para>THE BINARY THIS TREE BUILT, not the one deployed beside the game: a fixture or
    /// a save is read by the code in this working tree, and reading it with whatever was
    /// last deployed would be reading it with another build.</para>
    /// </remarks>
    internal static class EngineHost
    {
        /// <summary>What the engine host is called where cargo builds it.</summary>
        private const string HostName = "gct-engine-host";

        /// <summary>What to run to build it, which is what a message says to do.</summary>
        private const string BuildCommand = "cargo build --release";

        /// <summary>Where cargo leaves it.</summary>
        /// <exception cref="FileNotFoundException">It has not been built.</exception>
        internal static string Path
        {
            get
            {
                string name = RuntimeInformation.IsOSPlatform(OSPlatform.Windows)
                    ? HostName + ".exe"
                    : HostName;
                string path = System.IO.Path.Combine(
                    GameInstall.RepoRoot(), "target", "release", name);

                if (!File.Exists(path))
                {
                    throw new FileNotFoundException(
                        $"The engine host is not at {path}, and it is what reads and writes "
                        + $"this repository's formats. Build it first:\n  {BuildCommand}",
                        path);
                }

                return path;
            }
        }

        /// <summary>Runs a verb and hands back what it wrote to stdout.</summary>
        /// <param name="about">What is being worked on, for a message that fails.</param>
        /// <param name="patience">How long to wait before giving up on it.</param>
        /// <param name="arguments">The verb and whatever it takes.</param>
        /// <returns>Its standard output, exactly as it wrote it.</returns>
        /// <exception cref="InvalidDataException">
        /// It would not start, did not answer in time, or refused what it was given.
        /// </exception>
        internal static string Run(string about, TimeSpan patience, params string[] arguments)
        {
            string host = Path;
            var run = new ProcessStartInfo(host)
            {
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
            };
            foreach (string argument in arguments)
            {
                run.ArgumentList.Add(argument);
            }

            using Process? engine = Process.Start(run);
            if (engine == null)
            {
                throw new InvalidDataException($"{host} would not start.");
            }

            // BOTH STREAMS ARE READ BEFORE THE WAIT. A process whose pipe fills up while
            // nobody is draining it blocks on the write, and waiting on a process that is
            // blocked writing to us is a deadlock rather than a slow run.
            string answer = engine.StandardOutput.ReadToEnd();
            string complaint = engine.StandardError.ReadToEnd();
            if (!engine.WaitForExit((int)patience.TotalMilliseconds))
            {
                engine.Kill(entireProcessTree: true);
                throw new InvalidDataException(
                    $"{HostName} did not answer within {patience.TotalSeconds:N0}s for {about}");
            }

            if (engine.ExitCode != 0)
            {
                throw new InvalidDataException($"{about}: {complaint.Trim()}");
            }

            return answer;
        }
    }
}
