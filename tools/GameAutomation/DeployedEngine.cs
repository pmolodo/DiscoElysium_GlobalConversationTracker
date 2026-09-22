// SPDX-License-Identifier: MIT
using System;
using System.IO;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Whether the look-ahead engine deployed beside a game is the one this repository has
    /// built.
    /// </summary>
    /// <remarks>
    /// <para>WHAT THIS PREVENTS. An in-game run can test an engine from days ago and report
    /// every check green. It did, 2026-09-04: the plugin's build prefers
    /// <c>target/release</c> where one exists, a session had built only <c>target/debug</c>,
    /// and the run passed 66 of 66 against an engine that did not contain the change under
    /// test. Nothing said so - not the deploy, which lists the files it copied; not the
    /// plugin's own commit stamp, which is the C# assembly's and the C# had been rebuilt;
    /// not the run log's revision, which is the working tree's rather than the binary's.
    /// It took an offline test disagreeing with a run that had just passed.</para>
    ///
    /// <para>THE COMPARISON IS MODIFICATION TIMES, and it works because every copy on the way
    /// in preserves them: MSBuild's <c>Copy</c> and PowerShell's <c>Copy-Item</c> both give
    /// the destination the source's last-write time, so a deployed engine carries the time of
    /// the cargo build that produced it. Deployed older than built means the build has not
    /// been deployed.</para>
    ///
    /// <para>AGAINST BOTH PROFILES, which is what catches the failure above rather than a
    /// near miss of it. On that day <c>target/release</c> and the deployed copy agreed
    /// perfectly - both were the previous day's - and what disagreed with them was
    /// <c>target/debug</c>, which had just been built. Whichever engine this tree most
    /// recently produced is the one a run should be testing.</para>
    ///
    /// <para>WHAT IT DOES NOT CATCH is a tree edited and never built at all: no engine
    /// anywhere is newer than the deployed one, so nothing is out of order. Cargo is what
    /// notices that, and does, on the next build.</para>
    ///
    /// <para>OFF UNLESS ASKED FOR, by <c>DEGCT_CHECK_DEPLOY</c>. The check compares a game
    /// install against a repository, which is a pairing only a development machine has;
    /// anywhere else there is nothing to compare and the honest answer is that it was not
    /// checked. See <see cref="CheckVariable"/>.</para>
    /// </remarks>
    public static class DeployedEngine
    {
        /// <summary>What the engine is called once it is deployed beside the plugin.</summary>
        public const string DeployedFileName = "GlobalConversationTracker.Native.exe";

        /// <summary>What cargo calls it, before the plugin's build renames it.</summary>
        public const string BuiltFileName = "gct-engine-host.exe";

        /// <summary>
        /// The bare name of the variable that turns this on: <c>DEGCT_CHECK_DEPLOY</c>.
        /// </summary>
        /// <remarks>
        /// NOT A FLAG, because the thing that has to carry it is not a command line anybody
        /// types. An in-game run is started by the harness, by <c>dotnet test</c>, and by
        /// whatever a session reaches for that afternoon; the check has to be on for all of
        /// them or it protects only the paths somebody remembered. A variable set once in the
        /// environment covers every caller, including ones added later.
        /// </remarks>
        public const string CheckVariable = "CHECK_DEPLOY";

        /// <summary>How a deployed engine compares with what this tree has built.</summary>
        public enum Freshness
        {
            /// <summary>The deployed engine is at least as new as anything built here.</summary>
            Current,

            /// <summary>Something built here is newer. The run would test the older one.</summary>
            Stale,

            /// <summary>Nothing can be said - no deployed engine, or none built.</summary>
            Unknown,

            /// <summary>Nobody asked. <see cref="CheckVariable"/> is not set.</summary>
            NotChecked,
        }

        /// <summary>What a check found, and how to say it.</summary>
        public sealed class Result
        {
            /// <summary>Creates a result.</summary>
            /// <param name="freshness">What was found.</param>
            /// <param name="what">One line saying it, for the report.</param>
            public Result(Freshness freshness, string what)
            {
                Freshness = freshness;
                What = what ?? throw new ArgumentNullException(nameof(what));
            }

            /// <summary>What was found.</summary>
            public Freshness Freshness { get; }

            /// <summary>One line saying it.</summary>
            public string What { get; }

            /// <inheritdoc/>
            public override string ToString() => What;
        }

        /// <summary>
        /// Whether the engine deployed beside a game is what this tree has built.
        /// </summary>
        /// <param name="pluginFolder">The mod's folder inside the game's BepInEx plugins.</param>
        /// <param name="repoRoot">The repository root, or null to find it.</param>
        /// <returns>What was found, and how to say it.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="pluginFolder"/> is null.</exception>
        public static Result Check(string pluginFolder, string? repoRoot = null)
        {
            if (pluginFolder == null)
            {
                throw new ArgumentNullException(nameof(pluginFolder));
            }

            if (!DegctEnv.IsSet(CheckVariable))
            {
                return new Result(
                    Freshness.NotChecked,
                    "not checked against what this tree has built - set "
                    + $"{DegctEnv.Qualified(CheckVariable)} to check it");
            }

            string deployedPath = Path.Combine(pluginFolder, DeployedFileName);
            if (!File.Exists(deployedPath))
            {
                return new Result(
                    Freshness.Unknown,
                    $"no look-ahead engine is deployed ({DeployedFileName} is not in "
                    + $"{pluginFolder}), so there is nothing to check - deploy again");
            }

            string root = repoRoot ?? GameInstall.RepoRoot();
            string? builtPath = NewestBuilt(root);
            if (builtPath == null)
            {
                return new Result(
                    Freshness.Unknown,
                    "this tree has built no look-ahead engine, so the deployed one cannot "
                    + "be checked against it - run: cargo build --release");
            }

            DateTime deployed = File.GetLastWriteTimeUtc(deployedPath);
            DateTime built = File.GetLastWriteTimeUtc(builtPath);
            if (deployed >= built)
            {
                return new Result(
                    Freshness.Current,
                    $"the deployed look-ahead engine is this tree's ({When(deployed)})");
            }

            return new Result(
                Freshness.Stale,
                "THE DEPLOYED LOOK-AHEAD ENGINE IS NOT WHAT THIS TREE BUILT. It was deployed "
                + $"{When(deployed)} and {builtPath} was built {When(built)}, so this run "
                + "would test the older one - deploy again");
        }

        /// <summary>
        /// The most recently built engine under <c>target/</c>, or null where there is none.
        /// </summary>
        /// <remarks>
        /// <para>EVERY PROFILE, newest wins. Which one the plugin's build would have copied is
        /// not the question - the question is whether anything this tree produced is newer than
        /// what is deployed, and a debug build nobody deployed answers it as well as a release
        /// one.</para>
        ///
        /// <para>SO THE FOLDERS ARE ASKED FOR RATHER THAN LISTED. A named list is a list that
        /// goes short: a profile this file has never heard of holds an engine this file cannot
        /// see, the newest it does see agrees with the deployed copy, and the check says the
        /// deployment is current while a run tests something else. Cargo puts each profile's
        /// output in a folder of its own under <c>target/</c>, so the folders that are there
        /// are the profiles that were built.</para>
        /// </remarks>
        /// <param name="repoRoot">The repository root.</param>
        /// <returns>The path of the newest built engine, or null.</returns>
        private static string? NewestBuilt(string repoRoot)
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
                string path = Path.Combine(profile, BuiltFileName);
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

        /// <summary>A time, as a line of a report says one.</summary>
        /// <param name="utc">The time, in UTC.</param>
        /// <returns>It, local and to the second.</returns>
        private static string When(DateTime utc) =>
            utc.ToLocalTime().ToString("yyyy-MM-dd HH:mm:ss", System.Globalization.CultureInfo.InvariantCulture);
    }
}
