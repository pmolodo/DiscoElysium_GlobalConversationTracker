// SPDX-License-Identifier: MIT
using GlobalConversationTracker.Engine;
using Xunit;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// The walk the plugin records and sends with every look-ahead request.
    /// </summary>
    public class ConversationWalkTests
    {
        private static readonly NodeRef Greeting = new NodeRef(28, 0);
        private static readonly NodeRef Choice = new NodeRef(28, 528);
        private static readonly NodeRef Reply = new NodeRef(28, 1378);

        [Fact]
        public void EntriesAreKeptOldestFirst()
        {
            var walk = new ConversationWalk();
            walk.Record(Greeting);
            walk.Record(Choice);
            walk.Record(Reply);

            Assert.Equal(new[] { Greeting, Choice, Reply }, walk.Shown);
        }

        /// <summary>
        /// Both dialogue interfaces report a line, so the same entry twice in a row is one line.
        /// </summary>
        [Fact]
        public void AnEntryReportedTwiceInARowIsKeptOnce()
        {
            var walk = new ConversationWalk();
            walk.Record(Greeting);
            walk.Record(Greeting);

            Assert.Equal(new[] { Greeting }, walk.Shown);
        }

        /// <summary>A line shown again later is a second visit, and the walk keeps it.</summary>
        [Fact]
        public void AnEntryShownAgainLaterIsKept()
        {
            var walk = new ConversationWalk();
            walk.Record(Greeting);
            walk.Record(Choice);
            walk.Record(Greeting);

            Assert.Equal(new[] { Greeting, Choice, Greeting }, walk.Shown);
        }

        [Fact]
        public void ClearingForgetsTheWalk()
        {
            var walk = new ConversationWalk();
            walk.Record(Greeting);
            walk.Clear();

            Assert.Empty(walk.Shown);
        }

        [Fact]
        public void AnOverfullWalkDropsItsOldestEntries()
        {
            var walk = new ConversationWalk();
            for (int entry = 0; entry <= ConversationWalk.Capacity; entry++)
            {
                walk.Record(new NodeRef(28, entry));
            }

            Assert.Equal(ConversationWalk.Capacity, walk.Shown.Count);
            Assert.Equal(new NodeRef(28, 1), walk.Shown[0]);
        }
    }
}
