// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.IO;
using System.Text.RegularExpressions;
using GlobalConversationTracker.Automation;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>The name a run's log takes, and the two implementations of it.</summary>
    public class RunLogTests
    {
        private const string Tool = "GameHarness";
        private const string Verb = "look-ahead";

        [Fact]
        public void ANameCarriesTheDateTheRevisionTheToolAndTheVerb()
        {
            string name = RunLog.FileName(
                new DateTime(2026, 9, 4), "1e08319064b7bd9d115f26c3abf35145d3fb7d8e",
                Tool, Verb);

            Assert.Equal(
                "2026-09-04_1e08319064b7bd9d115f26c3abf35145d3fb7d8e_GameHarness_look-ahead.txt",
                name);
        }

        [Theory]
        [InlineData("dotnet test", "dotnet-test")]
        [InlineData("cargo/corpus", "cargo-corpus")]
        [InlineData("look ahead", "look-ahead")]
        public void AwkwardCharactersInAComponentBecomeDashes(string verb, string expected)
        {
            string name = RunLog.FileName(new DateTime(2026, 9, 4), "abc", Tool, verb);

            Assert.Equal("2026-09-04_abc_GameHarness_" + expected + ".txt", name);
        }

        [Fact]
        public void AnEmptyComponentIsRefused()
        {
            Assert.Throws<ArgumentException>(
                () => RunLog.FileName(new DateTime(2026, 9, 4), "abc", Tool, string.Empty));
        }

        [Fact]
        public void ASecondRunAtTheSameCommitOnTheSameDayGetsItsOwnFile()
        {
            string folder = Path.Combine(Path.GetTempPath(), Path.GetRandomFileName());
            Directory.CreateDirectory(folder);
            try
            {
                string first = Path.Combine(folder, "run.txt");
                Assert.Equal(first, RunLog.Unique(first));

                File.WriteAllText(first, string.Empty);
                string second = RunLog.Unique(first);
                Assert.Equal(Path.Combine(folder, "run_2.txt"), second);

                File.WriteAllText(second, string.Empty);
                Assert.Equal(Path.Combine(folder, "run_3.txt"), RunLog.Unique(first));
            }
            finally
            {
                Directory.Delete(folder, recursive: true);
            }
        }

        [Fact]
        public void TheRevisionIsThisRepositorysCommit()
        {
            string revision = RunLog.Revision(GameInstall.RepoRoot());

            Assert.Matches(
                "^[0-9a-f]{40}(" + Regex.Escape(RunLog.DirtySuffix) + ")?$", revision);
        }

        [Fact]
        public void SomewhereThatIsNotARepositoryHasNoRevision()
        {
            string folder = Path.Combine(Path.GetTempPath(), Path.GetRandomFileName());
            Directory.CreateDirectory(folder);
            try
            {
                Assert.Equal(RunLog.NoRevision, RunLog.Revision(folder));
            }
            finally
            {
                Directory.Delete(folder, recursive: true);
            }
        }

        /// <summary>
        /// The shell wrapper and this assembly name a run identically.
        /// </summary>
        /// <remarks>
        /// The format is written out twice - here in C# for the runs that call
        /// <c>Program.Main</c>, and in <c>tools/run-logged.sh</c> for the ones that cannot,
        /// cargo and dotnet test among them. Neither can be expressed in terms of the
        /// other: the script has to work before anything is built, and the harness has to
        /// work without a shell. So they are held to each other here instead, and a change
        /// to one that is not made to the other fails this.
        /// </remarks>
        [Fact]
        public void TheShellWrapperNamesARunTheSameWay()
        {
            string root = GameInstall.RepoRoot();
            string fromShell = Run(
                Bash(),
                "\"" + Path.Combine(root, "tools", "run-logged.sh").Replace('\\', '/')
                    + "\" --name-only " + Tool + " " + Verb,
                root);

            Assert.Equal(
                Path.GetFileName(RunLog.PathFor(Tool, Verb, root)),
                Path.GetFileName(fromShell.Trim()));
        }

        /// <summary>
        /// Git's own bash, which is the one that can read a Windows path.
        /// </summary>
        /// <remarks>
        /// NOT whatever <c>bash</c> is first on PATH. On Windows that is often
        /// <c>System32\bash.exe</c>, the WSL launcher, which is a different machine with a
        /// different filesystem and cannot see <c>D:\...</c> at all. Deriving it from git
        /// works on both platforms: <c>C:/Program Files/Git/mingw64/libexec/git-core</c>
        /// and <c>/usr/lib/git-core</c> both lead to a bash three levels up.
        /// </remarks>
        private static string Bash()
        {
            string root = Path.GetFullPath(
                Path.Combine(Run("git", "--exec-path", null).Trim(), "..", "..", ".."));

            foreach (string relative in new[]
            {
                Path.Combine("bin", "bash.exe"),
                Path.Combine("usr", "bin", "bash.exe"),
                Path.Combine("bin", "bash"),
            })
            {
                string candidate = Path.Combine(root, relative);
                if (File.Exists(candidate))
                {
                    return candidate;
                }
            }

            throw new FileNotFoundException($"No bash beside git, under {root}.");
        }

        /// <summary>Runs a command and answers what it wrote.</summary>
        private static string Run(string executable, string arguments, string? workingDirectory)
        {
            var start = new ProcessStartInfo(executable, arguments)
            {
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true,
            };

            if (workingDirectory != null)
            {
                start.WorkingDirectory = workingDirectory;
            }

            using Process process = Process.Start(start)
                ?? throw new InvalidOperationException($"Could not start {executable}.");
            string output = process.StandardOutput.ReadToEnd();
            string errors = process.StandardError.ReadToEnd();
            process.WaitForExit();

            Assert.True(
                process.ExitCode == 0,
                $"{executable} {arguments} exited {process.ExitCode}: {errors}");
            return output;
        }
    }
}
