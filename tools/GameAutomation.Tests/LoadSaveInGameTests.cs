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

    /// <summary>Protects every opted-in in-game test from Steam Cloud.</summary>
    public sealed class InGameTestContext : IDisposable
    {
        private readonly SteamCloudOverride? _cloud;

        /// <summary>Disables Disco Elysium cloud sync before the collection starts.</summary>
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

        /// <summary>Restores the original Disco Elysium cloud setting.</summary>
        public void Dispose()
        {
            _cloud?.Dispose();
        }
    }
}
