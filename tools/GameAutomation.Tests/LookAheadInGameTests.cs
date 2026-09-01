// SPDX-License-Identifier: MIT
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// The look-ahead asterisk, checked against the running game.
    /// </summary>
    /// <remarks>
    /// <para>What this covers that the unit suite cannot: the crawl runs over the real
    /// dialogue database, from the real world state, and the marker is read off the
    /// text the game was about to draw. The unit tests prove the search is correct over
    /// a graph handed to it; this proves the search is wired to the game's graph, its
    /// money, and its option rendering.</para>
    ///
    /// <para>Deliberately one test. It drives three money scenarios in a single launch,
    /// and splitting them would pay for a cold start three times to report the same
    /// three lines.</para>
    /// </remarks>
    [Collection(InGameTestCollection.Name)]
    public class LookAheadInGameTests
    {
        /// <summary>
        /// An option is marked exactly when the crawl can still reach unread text,
        /// which for these scenarios means it could afford both purchases on the way.
        /// </summary>
        [InGameFact]
        public void TheMarkerAppearsOnlyWhenTheCrawlCanAffordToReachTheUnseenLine()
        {
            int exitCode = GlobalConversationTracker.Harness.Program.Main(
                new[] { "look-ahead" });

            Assert.Equal(0, exitCode);
        }
    }
}
