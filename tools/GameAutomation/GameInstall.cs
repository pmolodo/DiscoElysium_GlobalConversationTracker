// SPDX-License-Identifier: MIT
using System;
using System.IO;

namespace GlobalConversationTracker.Automation
{
    /// <summary>
    /// Finding the things a run needs on this machine: the game, the repository it is
    /// driven from, and the built test probe.
    /// </summary>
    /// <remarks>
    /// Here rather than in the harness because the xUnit in-game fixture needs the same
    /// three answers, and two copies of "where is disco.exe" drift the moment one of
    /// them learns about a new Steam library.
    /// </remarks>
    public static class GameInstall
    {
        /// <summary>The file that marks the repository root.</summary>
        public const string SolutionFileName = "GlobalConversationTracker.slnx";

        /// <summary>Where the build puts its output.</summary>
        public const string BuildFolderName = ".build";

        /// <summary>Where disco.exe usually is.</summary>
        public static readonly string[] DefaultGamePaths =
        {
            @"C:\apps (x86)\games\steam\steamapps\common\Disco Elysium\disco.exe",
            @"C:\Program Files (x86)\Steam\steamapps\common\Disco Elysium\disco.exe",
        };

        /// <summary>Resolves disco.exe.</summary>
        /// <param name="explicitPath">A path given on the command line, or null.</param>
        /// <exception cref="FileNotFoundException">It is not where it was looked for.</exception>
        public static string FindGame(string? explicitPath = null)
        {
            if (explicitPath != null)
            {
                if (!File.Exists(explicitPath))
                {
                    throw new FileNotFoundException($"No game at {explicitPath}.", explicitPath);
                }

                return explicitPath;
            }

            foreach (string candidate in DefaultGamePaths)
            {
                if (File.Exists(candidate))
                {
                    return candidate;
                }
            }

            throw new FileNotFoundException("Could not find disco.exe. Pass --game.");
        }

        /// <summary>The repository root, found by walking up from this assembly.</summary>
        /// <exception cref="InvalidOperationException">This is not inside the repository.</exception>
        public static string RepoRoot()
        {
            var directory = new DirectoryInfo(AppDomain.CurrentDomain.BaseDirectory);
            while (directory != null)
            {
                if (File.Exists(Path.Combine(directory.FullName, SolutionFileName)))
                {
                    return directory.FullName;
                }

                directory = directory.Parent;
            }

            throw new InvalidOperationException(
                "Could not find the repository root; pass --artifacts and --settings.");
        }

        /// <summary>
        /// The most recently built copy of the test probe.
        /// </summary>
        /// <remarks>
        /// Searched for rather than composed from a configuration and a target
        /// framework, because those are two more things to keep in step with the csproj
        /// and both fail silently: a path built from the wrong configuration simply does
        /// not exist, and the run reports a missing probe rather than a stale one.
        /// Newest wins, so building and running picks up what was just built.
        /// </remarks>
        /// <param name="repoRoot">The repository root, or null to find it.</param>
        /// <exception cref="FileNotFoundException">It has not been built.</exception>
        public static string FindProbeAssembly(string? repoRoot = null)
        {
            string root = repoRoot ?? RepoRoot();
            string searchRoot = Path.Combine(
                root, BuildFolderName, "bin", "GlobalConversationTracker.TestProbe");

            FileInfo? newest = null;
            if (Directory.Exists(searchRoot))
            {
                foreach (string path in Directory.GetFiles(
                    searchRoot, ProbeDeployment.ProbeFileName, SearchOption.AllDirectories))
                {
                    var candidate = new FileInfo(path);
                    if (newest == null || candidate.LastWriteTimeUtc > newest.LastWriteTimeUtc)
                    {
                        newest = candidate;
                    }
                }
            }

            return newest?.FullName ?? throw new FileNotFoundException(
                $"No {ProbeDeployment.ProbeFileName} under {searchRoot}. Build "
                + "src/GlobalConversationTracker.TestProbe first.");
        }
    }
}
