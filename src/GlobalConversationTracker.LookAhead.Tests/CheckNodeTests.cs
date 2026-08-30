// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using GlobalConversationTracker.LookAhead;
using Xunit;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>
    /// The <c>isDialogueEntryValid</c> gate, one node type at a time.
    /// </summary>
    /// <remarks>
    /// Mirrors <c>ReturnDialogueOptionValidator.IsEntryValid</c>. Every case here is a
    /// way an option disappears WITHOUT its <c>conditionsString</c> being false, which is
    /// the class of behaviour a guard-only model cannot see.
    /// </remarks>
    public class CheckNodeTests
    {
        private static NoveltyLookup Novel(params int[] unseen)
        {
            var set = new HashSet<int>(unseen);
            return node => set.Contains(node.EntryId)
                ? Novelty.UnseenAnyGame
                : Novelty.SeenThisGame;
        }

        private static LookAheadResult Run(LookAheadGraph graph, FakeWorld world, NoveltyLookup novelty)
        {
            return new LookAheadEngine().Evaluate(graph, GraphBuilder.Node(0), world, novelty);
        }

        /// <summary>A graph whose only route to node 2 runs through the check at node 1.</summary>
        private static LookAheadGraph Gated(
            DialogueCheckKind kind, string? flag = null, bool booleanOnly = false)
        {
            return new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, kind: kind, flag: flag, booleanOnly: booleanOnly, links: new[] { 2 })
                .Add(2)
                .Build();
        }

        // ---- test options -------------------------------------------------------

        /// <summary>
        /// HiddenTest entries are hidden unless the game is in developer mode, so in a
        /// real playthrough they and everything behind them are unreachable.
        /// </summary>
        [Fact]
        public void TestOption_IsNeverReachable()
        {
            LookAheadGraph graph = Gated(DialogueCheckKind.Test);
            Assert.Equal(Novelty.SeenThisGame, Run(graph, new FakeWorld(), Novel(2)).Best);
        }

        // ---- fake checks --------------------------------------------------------

        /// <summary>DifficultyAtmo entries are offered until they have been seen.</summary>
        [Fact]
        public void FakeCheck_ClosesOnceSeen()
        {
            LookAheadGraph graph = Gated(DialogueCheckKind.Fake);

            Assert.Equal(
                Novelty.UnseenAnyGame,
                Run(graph, new FakeWorld(), Novel(2)).Best);

            var seen = new FakeWorld().WithSeen(GraphBuilder.Node(1));
            Assert.Equal(Novelty.SeenThisGame, Run(graph, seen, Novel(2)).Best);
        }

        // ---- Kim switches -------------------------------------------------------

        [Fact]
        public void KimSwitch_ClosesOnceSeen()
        {
            LookAheadGraph graph = Gated(DialogueCheckKind.KimSwitch);
            var seen = new FakeWorld().WithSeen(GraphBuilder.Node(1));
            Assert.Equal(Novelty.SeenThisGame, Run(graph, seen, Novel(2)).Best);
        }

        /// <summary>A boolean_only switch stays available however often it is seen.</summary>
        [Fact]
        public void BooleanOnlyKimSwitch_StaysOpenWhenSeen()
        {
            LookAheadGraph graph = Gated(DialogueCheckKind.KimSwitch, booleanOnly: true);
            var seen = new FakeWorld().WithSeen(GraphBuilder.Node(1));
            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, seen, Novel(2)).Best);
        }

        /// <summary>
        /// Walking a fake check displays it, so a path that loops back finds it closed -
        /// the game would not offer it twice, and neither should the crawl.
        /// </summary>
        [Fact]
        public void FakeCheck_ClosesAfterThePathWalksThroughIt()
        {
            // 1 is the fake check; 2 loops back to it and also leads to 3.
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, kind: DialogueCheckKind.Fake, links: new[] { 2 })
                .Add(2, links: new[] { 1, 3 })
                .Add(3)
                .Build();

            // Node 3 is still found: the loop is not needed to reach it.
            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, new FakeWorld(), Novel(3)).Best);

            // And the crawl terminates rather than cycling through the check forever.
            LookAheadResult result = Run(graph, new FakeWorld(), Novel());
            Assert.False(result.BudgetExhausted);
        }

        /// <summary>
        /// The speculative marker is seeded from the save, so an entry the player has
        /// really seen is closed from the first step - the two notions start equal.
        /// </summary>
        [Fact]
        public void SpeculativeSeen_StartsFromTheSave()
        {
            LookAheadGraph graph = Gated(DialogueCheckKind.Fake);

            Assert.Equal(
                Novelty.UnseenAnyGame, Run(graph, new FakeWorld(), Novel(2)).Best);
            Assert.Equal(
                Novelty.SeenThisGame,
                Run(graph, new FakeWorld().WithSeen(GraphBuilder.Node(1)), Novel(2)).Best);
        }

        /// <summary>
        /// Walking an entry does NOT make it count as read. Novelty is what the player
        /// has actually seen and decides the marker; the speculative flag only decides
        /// whether an option is still offered. Conflating them would silently erase the
        /// novelty of everything a path touches.
        /// </summary>
        [Fact]
        public void WalkingAnEntry_DoesNotChangeItsNovelty()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, kind: DialogueCheckKind.Fake, links: new[] { 2 })
                .Add(2)
                .Build();

            // The fake check itself is the unseen content, and the crawl walks it.
            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, new FakeWorld(), Novel(1)).Best);
        }

        /// <summary>A boolean_only Kim switch is exempt, so a loop does not close it.</summary>
        [Fact]
        public void BooleanOnlyKimSwitch_StaysOpenAfterBeingWalked()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, kind: DialogueCheckKind.KimSwitch, booleanOnly: true, links: new[] { 2 })
                .Add(2, links: new[] { 1, 3 })
                .Add(3)
                .Build();

            LookAheadResult result = Run(graph, new FakeWorld(), Novel(3));
            Assert.Equal(Novelty.UnseenAnyGame, result.Best);
            Assert.False(result.BudgetExhausted);
        }

        // ---- red checks ---------------------------------------------------------

        /// <summary>
        /// A red check is rolled, so both results are possible and the look-ahead must
        /// carry both. Content behind either outcome is reachable.
        /// </summary>
        [Fact]
        public void RedCheck_ExploresBothOutcomes()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, kind: DialogueCheckKind.Red, flag: "check.red", links: new[] { 2, 3 })
                .Add(2, guard: "Variable[\"check.red\"]")
                .Add(3, guard: "Variable[\"check.red_failed\"]")
                .Build();

            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, new FakeWorld(), Novel(2)).Best);
            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, new FakeWorld(), Novel(3)).Best);
        }

        /// <summary>One shot: a red check already decided either way is gone.</summary>
        [Theory]
        [InlineData("check.red")]
        [InlineData("check.red_failed")]
        public void RedCheck_ClosesOnceDecided(string flag)
        {
            LookAheadGraph graph = Gated(DialogueCheckKind.Red, "check.red");
            var world = new FakeWorld().WithVariable(flag, true);
            Assert.Equal(Novelty.SeenThisGame, Run(graph, world, Novel(2)).Best);
        }

        // ---- white checks -------------------------------------------------------

        /// <summary>
        /// Retryable: only success closes a white check, so a previous failure leaves it
        /// open. This is the difference from a red check, and it is why a failed white
        /// check must not write a failure flag.
        /// </summary>
        [Fact]
        public void WhiteCheck_ClosesOnlyOnSuccess()
        {
            LookAheadGraph graph = Gated(DialogueCheckKind.White, "check.white");

            var failedBefore = new FakeWorld().WithVariable("check.white_failed", true);
            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, failedBefore, Novel(2)).Best);

            var passedBefore = new FakeWorld().WithVariable("check.white", true);
            Assert.Equal(Novelty.SeenThisGame, Run(graph, passedBefore, Novel(2)).Best);
        }

        // ---- passive checks -----------------------------------------------------

        /// <summary>
        /// PassiveNode.CheckSuccess sets falseConditionAction to "Passthrough" as it
        /// evaluates the entry, so a failed passive check does not end the branch. Its
        /// actions do not run, but the conversation walks on.
        /// </summary>
        [Fact]
        public void FailedPassiveCheck_PassesThroughToChildren()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(
                    1,
                    kind: DialogueCheckKind.Passive,
                    script: "SetVariableValue(\"fired\", true)",
                    links: new[] { 2 })
                .Add(2, guard: "(Variable[\"fired\"]) == false")
                .Build();

            // The check fails: node 2's guard still holds, because the action never ran.
            var failed = new FakeWorld().WithCheck(GraphBuilder.Node(1), Ternary.False);
            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, failed, Novel(2)).Best);

            // The check passes: the flag is set, and node 2 is closed behind it.
            var passed = new FakeWorld().WithCheck(GraphBuilder.Node(1), Ternary.True);
            Assert.Equal(Novelty.SeenThisGame, Run(graph, passed, Novel(2)).Best);
        }

        /// <summary>
        /// With the outcome undetermined both branches are carried, so anything either
        /// outcome reaches is reported - the soundness rule.
        /// </summary>
        [Fact]
        public void UndeterminedPassiveCheck_ReachesEitherSide()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(
                    1,
                    kind: DialogueCheckKind.Passive,
                    script: "SetVariableValue(\"fired\", true)",
                    links: new[] { 2, 3 })
                .Add(2, guard: "Variable[\"fired\"]")
                .Add(3, guard: "(Variable[\"fired\"]) == false")
                .Build();

            var world = new FakeWorld { DefaultCheck = Ternary.Unknown };
            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, world, Novel(2)).Best);
            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, world, Novel(3)).Best);
        }
    }
}
