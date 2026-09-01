// SPDX-License-Identifier: MIT
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
    {
        /// <summary>The xUnit collection name shared by all in-game tests.</summary>
        public const string Name = "In-game tests";
    }
}
