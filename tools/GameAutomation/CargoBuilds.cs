// SPDX-License-Identifier: MIT
using System;
using System.IO;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// What cargo has built under <c>target/</c>, whichever profile built it.
    /// </summary>
    /// <remarks>
    /// <para>ONE RULE FOR EVERY CALLER that looks for a cargo build from C#: the engine check
    /// before an in-game run, the harness's own use of the engine, and the LookAhead tests,
    /// which compile this file in as a linked source because they cannot reference this
    /// project.</para>
    /// </remarks>
    public static class CargoBuilds
    {
        /// <summary>
        /// The most recently built copy of <paramref name="fileName"/> in any profile's folder
        /// under <c>target/</c>, or null where no profile has built it.
        /// </summary>
        /// <remarks>
        /// <para>NEWEST WINS, which is not a preference. A stale build in one profile silently
        /// shadows a fresh one in another, and the symptom is an answer that does not contain a
        /// change made minutes ago - which reads as a bug in the change and is not one.
        /// Whichever was built last is the one the developer meant.</para>
        ///
        /// <para>SO THE FOLDERS ARE ASKED FOR RATHER THAN LISTED. A named list is a list that
        /// goes short: a profile it has never heard of - <c>release-incremental</c>, which the
        /// build scripts use - holds a build it cannot see, and the newest it does see is older
        /// than the one the developer just made. Cargo puts each profile's output in a folder of
        /// its own under <c>target/</c>, so the folders that are there are the profiles that
        /// were built.</para>
        /// </remarks>
        /// <param name="repoRoot">The repository root.</param>
        /// <param name="fileName">What cargo calls the artefact.</param>
        /// <returns>Its path, or null.</returns>
        public static string? Newest(string repoRoot, string fileName)
        {
            string target = Path.Combine(repoRoot, "target");
            if (!Directory.Exists(target))
            {
                return null;
            }

            string? newest = null;
            DateTime when = DateTime.MinValue;
            foreach (string profile in Directory.EnumerateDirectories(target))
            {
                string path = Path.Combine(profile, fileName);
                if (!File.Exists(path))
                {
                    continue;
                }

                DateTime written = File.GetLastWriteTimeUtc(path);
                if (newest == null || written > when)
                {
                    newest = path;
                    when = written;
                }
            }

            return newest;
        }
    }
}
