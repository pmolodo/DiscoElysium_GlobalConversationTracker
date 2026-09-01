// SPDX-License-Identifier: MIT
using System;
using System.IO;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>End-to-end checks that drive an installed copy of the game.</summary>
    [Collection(InGameTestCollection.Name)]
    public class LoadSaveInGameTests
    {
        /// <summary>The baseline template save can be staged and loaded.</summary>
        [InGameFact]
        public void LoadSaveLoadsTheTemplateSave()
        {
            int exitCode = GlobalConversationTracker.Harness.Program.Main(
                new[] { "load-save", "--close-holders" });

            Assert.Equal(0, exitCode);
        }
    }

    /// <summary>Prevents tests that replace the player's profile from overlapping.</summary>
    [CollectionDefinition(Name, DisableParallelization = true)]
    public sealed class InGameTestCollection
        : ICollectionFixture<InGameTestContext>
    {
        /// <summary>The xUnit collection name shared by all in-game tests.</summary>
        public const string Name = "In-game tests";
    }

    /// <summary>
    /// What every opted-in in-game test runs inside: Steam Cloud off, and the test
    /// probe installed.
    /// </summary>
    /// <remarks>
    /// The order is the point, and it is why these two live together rather than one
    /// per fixture. Cloud sync goes off FIRST and comes back on LAST, so nothing
    /// written in between - the staged profile, the probe - can be replaced underneath
    /// a run by a sync, and nothing the tests staged can be uploaded.
    /// </remarks>
    public sealed class InGameTestContext : IDisposable
    {
        private readonly SteamCloudOverride? _cloud;
        private readonly ProbeDeployment? _probe;

        /// <summary>Opens both scopes before the collection starts.</summary>
        public InGameTestContext()
        {
            if (!InGameFactAttribute.IsOptedIn)
            {
                return;
            }

            string steam = SteamClient.FindExecutable();
            string stamp = DateTime.Now.ToString("yyyyMMdd-HHmmss");
            string backup = Path.Combine(
                Path.GetTempPath(), $"steam-sharedconfig-{stamp}.vdf.bak");
            _cloud = SteamCloudOverride.Disable(
                steam,
                backup,
                TimeSpan.FromSeconds(60),
                message => Console.WriteLine($"Steam Cloud: {message}"));

            try
            {
                _probe = ProbeDeployment.Deploy(
                    GameInstall.FindGame(),
                    GameInstall.FindProbeAssembly(),
                    message => Console.WriteLine($"Test probe: {message}"));
            }
            catch (Exception)
            {
                // Without this the cloud setting would stay off after a failure here,
                // and the player would find their game silently not syncing.
                _cloud.Dispose();
                throw;
            }
        }

        /// <summary>Removes the probe, then restores the cloud setting.</summary>
        public void Dispose()
        {
            try
            {
                _probe?.Dispose();
            }
            finally
            {
                _cloud?.Dispose();
            }
        }
    }
}
