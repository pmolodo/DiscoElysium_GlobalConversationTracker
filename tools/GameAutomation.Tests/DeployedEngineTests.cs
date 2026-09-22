// SPDX-License-Identifier: MIT
using System;
using System.IO;
using GlobalConversationTracker.Automation;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// What <see cref="DeployedEngine"/> says about a deployed engine and a built one.
    /// </summary>
    /// <remarks>
    /// AGAINST FOLDERS THIS TEST MAKES, not against the machine's real game install or its
    /// real target directory: the answer has to be decided by the two times, and a test that
    /// read whatever happened to be built would pass or fail on what somebody did last.
    /// </remarks>
    public class DeployedEngineTests : IDisposable
    {
        private readonly string _temp;
        private readonly string? _was;

        /// <summary>Builds a repository and a plugin folder to compare.</summary>
        public DeployedEngineTests()
        {
            _temp = Path.Combine(Path.GetTempPath(), "degct-deployed-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(PluginFolder);
            Directory.CreateDirectory(Path.Combine(RepoRoot, "target", "release"));
            Directory.CreateDirectory(Path.Combine(RepoRoot, "target", "debug"));

            _was = DegctEnv.Get(DeployedEngine.CheckVariable);
            Environment.SetEnvironmentVariable(
                DegctEnv.Qualified(DeployedEngine.CheckVariable), "1");
        }

        private string RepoRoot => Path.Combine(_temp, "repo");

        private string PluginFolder => Path.Combine(_temp, "plugins", "GlobalConversationTracker");

        /// <inheritdoc/>
        public void Dispose()
        {
            Environment.SetEnvironmentVariable(
                DegctEnv.Qualified(DeployedEngine.CheckVariable), _was);
            try
            {
                Directory.Delete(_temp, recursive: true);
            }
            catch (IOException)
            {
                // A temp folder that will not delete is not this test's business.
            }

            GC.SuppressFinalize(this);
        }

        [Fact]
        public void A_deploy_newer_than_every_build_is_current()
        {
            Build("release", DateTime.UtcNow.AddMinutes(-10));
            Deploy(DateTime.UtcNow.AddMinutes(-10));

            Assert.Equal(DeployedEngine.Freshness.Current, Check().Freshness);
        }

        [Fact]
        public void A_build_newer_than_the_deploy_is_stale()
        {
            Deploy(DateTime.UtcNow.AddMinutes(-10));
            Build("release", DateTime.UtcNow);

            DeployedEngine.Result result = Check();
            Assert.Equal(DeployedEngine.Freshness.Stale, result.Freshness);
            Assert.Contains("NOT WHAT THIS TREE BUILT", result.What, StringComparison.Ordinal);
        }

        /// <summary>
        /// The 2026-09-04 failure exactly: release and the deploy agree, and the DEBUG build
        /// nobody deployed is the one that holds the change under test.
        /// </summary>
        [Fact]
        public void A_debug_build_nobody_deployed_is_stale_although_release_agrees()
        {
            DateTime yesterday = DateTime.UtcNow.AddDays(-1);
            Build("release", yesterday);
            Deploy(yesterday);
            Build("debug", DateTime.UtcNow);

            Assert.Equal(DeployedEngine.Freshness.Stale, Check().Freshness);
        }

        [Fact]
        public void Nothing_deployed_is_not_a_staleness()
        {
            Build("release", DateTime.UtcNow);

            DeployedEngine.Result result = Check();
            Assert.Equal(DeployedEngine.Freshness.Unknown, result.Freshness);
            Assert.Contains(DeployedEngine.DeployedFileName, result.What, StringComparison.Ordinal);
        }

        [Fact]
        public void Nothing_built_is_not_a_staleness()
        {
            Deploy(DateTime.UtcNow);

            Assert.Equal(DeployedEngine.Freshness.Unknown, Check().Freshness);
        }

        [Fact]
        public void Without_the_variable_nothing_is_checked()
        {
            Environment.SetEnvironmentVariable(
                DegctEnv.Qualified(DeployedEngine.CheckVariable), null);
            Deploy(DateTime.UtcNow.AddDays(-1));
            Build("release", DateTime.UtcNow);

            DeployedEngine.Result result = Check();
            Assert.Equal(DeployedEngine.Freshness.NotChecked, result.Freshness);
            Assert.Contains(
                DegctEnv.Qualified(DeployedEngine.CheckVariable),
                result.What,
                StringComparison.Ordinal);
        }

        private DeployedEngine.Result Check() => DeployedEngine.Check(PluginFolder, RepoRoot);

        private void Build(string profile, DateTime written) =>
            Write(
                Path.Combine(RepoRoot, "target", profile, DeployedEngine.BuiltFileName),
                written);

        private void Deploy(DateTime written) =>
            Write(Path.Combine(PluginFolder, DeployedEngine.DeployedFileName), written);

        private static void Write(string path, DateTime written)
        {
            File.WriteAllText(path, "not an engine");
            File.SetLastWriteTimeUtc(path, written);
        }
    }
}
