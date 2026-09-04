// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Linq;
using System.Text.RegularExpressions;
using GlobalConversationTracker.Automation;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// The check that stops a run testing a library it was not built from.
    /// </summary>
    /// <remarks>
    /// What can be checked without a game: that the hash answers for this repository, that
    /// it moves only when the library's own sources move, and that every way of not knowing
    /// reads as not knowing rather than as agreement. Whether the DEPLOYED library matches
    /// is the run's job, and it refuses when it does not.
    /// </remarks>
    public class NativeEngineStampTests
    {
        [Fact]
        public void TheHashAnswersForThisRepository()
        {
            string? hash = NativeEngineStamp.TreeHash(GameInstall.RepoRoot());

            Assert.NotNull(hash);
            Assert.Matches("^[0-9a-f]{16}$", hash);
        }

        /// <summary>
        /// Asking twice gives the same answer, and leaves the tree as it was.
        /// </summary>
        /// <remarks>
        /// The hash stages the working tree into a COPY of the index, and the whole
        /// technique is only safe because the real one is untouched. A regression here
        /// would silently stage a developer's work in progress.
        /// </remarks>
        [Fact]
        public void AskingDoesNotDisturbTheIndex()
        {
            string root = GameInstall.RepoRoot();
            string index = Path.Combine(root, ".git", "index");
            DateTime before = File.GetLastWriteTimeUtc(index);

            string? first = NativeEngineStamp.TreeHash(root);
            string? second = NativeEngineStamp.TreeHash(root);

            Assert.Equal(first, second);
            Assert.Equal(before, File.GetLastWriteTimeUtc(index));
        }

        /// <summary>
        /// Somewhere that is not a repository cannot be hashed, and says so.
        /// </summary>
        [Fact]
        public void SomewhereThatIsNotARepositoryHasNoHash()
        {
            string folder = Path.Combine(Path.GetTempPath(), Path.GetRandomFileName());
            Directory.CreateDirectory(folder);
            try
            {
                Assert.Null(NativeEngineStamp.TreeHash(folder));
            }
            finally
            {
                Directory.Delete(folder, recursive: true);
            }
        }

        /// <summary>
        /// A missing stamp is unknown, not stale and not current.
        /// </summary>
        /// <remarks>
        /// The distinction the whole type turns on. A plugin folder with no stamp is an
        /// older deploy, not a mismatch, and refusing the run would make the check
        /// impossible to introduce.
        /// </remarks>
        [Fact]
        public void NoStampIsUnknown()
        {
            string folder = Path.Combine(Path.GetTempPath(), Path.GetRandomFileName());
            Directory.CreateDirectory(folder);
            try
            {
                NativeEngineStamp.Result result = NativeEngineStamp.Check(folder);

                Assert.Equal(NativeEngineStamp.Freshness.Unknown, result.Freshness);
                Assert.Contains(NativeEngineStamp.FileName, result.What);
            }
            finally
            {
                Directory.Delete(folder, recursive: true);
            }
        }

        [Theory]
        [InlineData("{\"tree\":\"nogit\"}", "git could not say")]
        [InlineData("{\"commit\":\"abc\"}", "git could not say")]
        [InlineData("not json at all", "will not read")]
        public void AStampThatCannotSayReadsAsUnknown(string contents, string expected)
        {
            string folder = Path.Combine(Path.GetTempPath(), Path.GetRandomFileName());
            Directory.CreateDirectory(folder);
            try
            {
                File.WriteAllText(
                    Path.Combine(folder, NativeEngineStamp.FileName), contents);

                NativeEngineStamp.Result result = NativeEngineStamp.Check(folder);

                Assert.Equal(NativeEngineStamp.Freshness.Unknown, result.Freshness);
                Assert.Contains(expected, result.What);
            }
            finally
            {
                Directory.Delete(folder, recursive: true);
            }
        }

        /// <summary>
        /// A stamp naming other sources is stale, and the message says what to do.
        /// </summary>
        [Fact]
        public void AStampFromOtherSourcesIsStale()
        {
            string folder = Path.Combine(Path.GetTempPath(), Path.GetRandomFileName());
            Directory.CreateDirectory(folder);
            try
            {
                File.WriteAllText(
                    Path.Combine(folder, NativeEngineStamp.FileName),
                    "{\"tree\":\"0000000000000000\"}");

                NativeEngineStamp.Result result = NativeEngineStamp.Check(folder);

                Assert.Equal(NativeEngineStamp.Freshness.Stale, result.Freshness);
                Assert.Contains("cargo build --release", result.What);
            }
            finally
            {
                Directory.Delete(folder, recursive: true);
            }
        }

        /// <summary>
        /// A stamp naming these sources is current.
        /// </summary>
        [Fact]
        public void AStampFromTheseSourcesIsCurrent()
        {
            string root = GameInstall.RepoRoot();
            string folder = Path.Combine(Path.GetTempPath(), Path.GetRandomFileName());
            Directory.CreateDirectory(folder);
            try
            {
                File.WriteAllText(
                    Path.Combine(folder, NativeEngineStamp.FileName),
                    $"{{\"tree\":\"{NativeEngineStamp.TreeHash(root)}\"}}");

                NativeEngineStamp.Result result = NativeEngineStamp.Check(folder, root);

                Assert.Equal(NativeEngineStamp.Freshness.Current, result.Freshness);
            }
            finally
            {
                Directory.Delete(folder, recursive: true);
            }
        }

        /// <summary>
        /// The build side and this side name the same sources.
        /// </summary>
        /// <remarks>
        /// The list is written out twice - here and in <c>build.rs</c> - because one runs
        /// during a cargo build with no .NET in sight and the other in a harness that must
        /// work without cargo. A path added to one and not the other makes the check
        /// silently narrower, and nothing would fail. This is what fails.
        /// </remarks>
        [Fact]
        public void TheBuildSideNamesTheSameSources()
        {
            string script = Path.Combine(GameInstall.RepoRoot(), "build.rs");
            Assert.True(File.Exists(script), $"{script} is where the stamp is written");

            Match declared = Regex.Match(
                File.ReadAllText(script), @"const SOURCES: &\[&str\] = &\[(?<paths>[^\]]*)\]");
            Assert.True(declared.Success, "build.rs no longer declares SOURCES");

            string[] fromBuild = Regex.Matches(declared.Groups["paths"].Value, "\"([^\"]+)\"")
                .Select(match => match.Groups[1].Value)
                .ToArray();

            Assert.Equal(NativeEngineStamp.Sources, fromBuild);
        }
    }
}
