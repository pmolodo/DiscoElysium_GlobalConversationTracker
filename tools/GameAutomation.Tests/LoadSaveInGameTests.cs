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
    /// What every opted-in in-game test runs inside: Steam Cloud off for the whole
    /// collection.
    /// </summary>
    /// <remarks>
    /// <para>Off FIRST and back on LAST, around everything. A run stages a profile, and
    /// the profile is what Steam Cloud syncs: with sync on, a staged profile can be
    /// replaced underneath a test, and a staged profile can be uploaded over the
    /// player's own.</para>
    ///
    /// <para>The test probe is NOT installed here. It is a dependency of whatever drives
    /// the game - the look-ahead verb installs its own - so a test that never launches
    /// the game does not touch the player's plugins folder, and the verb stays runnable
    /// from a command line. It still ends up inside this scope, because this wraps the
    /// whole collection.</para>
    /// </remarks>
    public sealed class InGameTestContext : IDisposable
    {
        private readonly SteamCloudOverride? _cloud;

        /// <summary>Turns cloud sync off before the collection starts.</summary>
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
        }

        /// <summary>Restores the original cloud setting.</summary>
        public void Dispose()
        {
            _cloud?.Dispose();
        }
    }
}
