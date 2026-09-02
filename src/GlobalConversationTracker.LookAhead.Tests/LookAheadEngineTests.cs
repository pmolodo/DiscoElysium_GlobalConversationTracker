// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using GlobalConversationTracker.LookAhead;
using Xunit;

namespace GlobalConversationTracker.LookAhead.Tests
{
    public class LookAheadEngineTests
    {
        private static NoveltyLookup Novel(params int[] unseenAnywhere)
        {
            var set = new HashSet<int>(unseenAnywhere);
            return node => set.Contains(node.EntryId)
                ? Novelty.UnseenAnyGame
                : Novelty.SeenThisGame;
        }

        private static NoveltyLookup Scores(IReadOnlyDictionary<int, Novelty> scores)
        {
            return node => scores.TryGetValue(node.EntryId, out Novelty value)
                ? value
                : Novelty.SeenThisGame;
        }

        private static LookAheadResult Run(
            LookAheadGraph graph,
            FakeWorld world,
            NoveltyLookup novelty,
            int start = 0,
            LookAheadOptions? options = null)
        {
            return new LookAheadEngine(options)
                .Evaluate(graph, GraphBuilder.Node(start), world, novelty);
        }

        [Fact]
        public void FindsNoveltyDownstream()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, links: new[] { 2 })
                .Add(2)
                .Build();

            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, new FakeWorld(), Novel(2)).Best);
        }

        [Fact]
        public void ReportsNothingWhenEverythingDownstreamIsSeen()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1)
                .Build();

            Assert.Equal(Novelty.SeenThisGame, Run(graph, new FakeWorld(), Novel()).Best);
        }

        /// <summary>
        /// The marker describes what lies BEYOND the option; the option's own novelty is
        /// already its text colour.
        /// </summary>
        [Fact]
        public void StartNodeIsNotScored()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1)
                .Build();

            Assert.Equal(Novelty.SeenThisGame, Run(graph, new FakeWorld(), Novel(0)).Best);
        }

        /// <summary>
        /// Groups are never marked by the game, so they are permanently Untouched. With
        /// 36.6% of the database being groups, scoring them would mark nearly everything.
        /// </summary>
        [Fact]
        public void GroupsAreTraversedButNeverScored()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, isGroup: true, links: new[] { 2 })
                .Add(2)
                .Build();

            Assert.Equal(Novelty.SeenThisGame, Run(graph, new FakeWorld(), Novel(1)).Best);
            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, new FakeWorld(), Novel(2)).Best);
        }

        [Fact]
        public void FalseGuardBlocks()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, guard: "Variable[\"gate\"]", links: new[] { 2 })
                .Add(2)
                .Build();

            var closed = new FakeWorld().WithVariable("gate", false);
            Assert.Equal(Novelty.SeenThisGame, Run(graph, closed, Novel(2)).Best);

            var open = new FakeWorld().WithVariable("gate", true);
            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, open, Novel(2)).Best);
        }

        /// <summary>
        /// The soundness rule: only a definite false blocks. Under-reporting tells the
        /// player a branch is exhausted when it is not.
        /// </summary>
        [Fact]
        public void UnknownGuardDoesNotBlock()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, guard: "IsKimHere()", links: new[] { 2 })
                .Add(2)
                .Build();

            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, new FakeWorld(), Novel(2)).Best);
        }

        [Fact]
        public void ActionsUnlockTheirOwnDownstreamGuards()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, script: "SetVariableValue(\"gate\", true)", links: new[] { 2 })
                .Add(2, guard: "Variable[\"gate\"]", links: new[] { 3 })
                .Add(3)
                .Build();

            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, new FakeWorld(), Novel(3)).Best);
        }

        [Fact]
        public void CyclesTerminate()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, links: new[] { 2 })
                .Add(2, links: new[] { 1, 3 })
                .Add(3)
                .Build();

            LookAheadResult result = Run(graph, new FakeWorld(), Novel(3));
            Assert.Equal(Novelty.UnseenAnyGame, result.Best);
            Assert.False(result.BudgetExhausted);
        }

        // ---- cost options -------------------------------------------------------

        [Fact]
        public void UnaffordableOptionCannotBeTraversed()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, cost: 5000, links: new[] { 2 })
                .Add(2)
                .Build();

            Assert.Equal(
                Novelty.SeenThisGame,
                Run(graph, new FakeWorld().WithMoney(4999), Novel(2)).Best);
            Assert.Equal(
                Novelty.UnseenAnyGame,
                Run(graph, new FakeWorld().WithMoney(5000), Novel(2)).Best);
        }

        /// <summary>
        /// Conversation 451, JAM / faln sneakers on a pedestal of speakers. The speakers
        /// cost 50 centimes but are gated behind the 5,000-centime sneakers, so reaching
        /// them needs 5,050 - not the 5,000 the larger cost alone suggests, and not the
        /// 50 the speakers' own price suggests. This is the case no state-free crawl and
        /// no per-cost check can get right.
        /// </summary>
        [Theory]
        [InlineData(5049, Novelty.SeenThisGame)]
        [InlineData(5050, Novelty.UnseenAnyGame)]
        [InlineData(5300, Novelty.UnseenAnyGame)]
        public void SiilengSpeakers_NeedTheSumOfBothPurchases(int money, Novelty expected)
        {
            LookAheadGraph graph = BuildSiileng();
            var world = new FakeWorld()
                .WithMoney(money)
                .WithVariable("jam.siileng_learned_when_you_can_buy_speakers", true);

            Assert.Equal(expected, Run(graph, world, Novel(11)).Best);
        }

        /// <summary>
        /// With the sneakers already bought, the speakers need only their own 50 - the
        /// same graph, a different starting state, a different answer.
        /// </summary>
        [Fact]
        public void SiilengSpeakers_AreCheapOnceTheSneakersAreOwned()
        {
            LookAheadGraph graph = BuildSiileng();
            var world = new FakeWorld()
                .WithMoney(50)
                .WithVariable("jam.siileng_learned_when_you_can_buy_speakers", true)
                .WithVariable("jam.siileng_bought_faln_sneakers", true)
                .WithItem("shoes_faln");

            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, world, Novel(11)).Best);
        }

        /// <summary>
        /// The guard also requires not already owning the speakers, so a replay finds
        /// nothing new - the CheckItem half of the condition, read from crawl state.
        /// </summary>
        [Fact]
        public void SiilengSpeakers_AreNotOfferedTwice()
        {
            LookAheadGraph graph = BuildSiileng();
            var world = new FakeWorld()
                .WithMoney(10000)
                .WithVariable("jam.siileng_learned_when_you_can_buy_speakers", true)
                .WithVariable("jam.siileng_bought_faln_sneakers", true)
                .WithItem("shoes_faln")
                .WithItem("samaran_speakers");

            Assert.Equal(Novelty.SeenThisGame, Run(graph, world, Novel(11)).Best);
        }

        /// <summary>
        /// Node 86 is the player's line and carries the cost; the GainItem lands on the
        /// node after it, which is how 46 of the 84 cost options in the database are
        /// built.
        /// </summary>
        private static LookAheadGraph BuildSiileng()
        {
            return new GraphBuilder()
                .Add(0, links: new[] { 100 })
                .Add(100, isGroup: true, links: new[] { 86, 11 })
                .Add(86, cost: 5000, links: new[] { 87 })
                .Add(
                    87,
                    script: "GainItem(\"shoes_faln\");\n"
                        + "SetVariableValue(\"jam.siileng_bought_faln_sneakers\", true)",
                    links: new[] { 100 })
                .Add(
                    11,
                    guard: "Variable[\"jam.siileng_bought_faln_sneakers\"] == true"
                        + "  and  Variable[\"jam.siileng_learned_when_you_can_buy_speakers\"] == true"
                        + "  and  CheckItem(\"samaran_speakers\") == false",
                    cost: 50,
                    links: new[] { 12 })
                .Add(12, script: "GainItem(\"samaran_speakers\")")
                .Build();
        }

        [Fact]
        public void CostOnce_IsChargedOnlyOnce()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, cost: 2000, costOnce: true, links: new[] { 2 })
                .Add(2, links: new[] { 1, 3 })
                .Add(3, guard: "MoneyAmount() >= 1000")
                .Build();

            // 2,000 pays for the room once and leaves 0; a second charge would be
            // refused, but CostOnce means there is no second charge - and the balance
            // still cannot reach 1,000, so nothing downstream of the guard is found.
            LookAheadResult result = Run(graph, new FakeWorld().WithMoney(2000), Novel(3));
            Assert.Equal(Novelty.SeenThisGame, result.Best);

            // With 3,000 the balance after the single charge is 1,000, which clears it.
            Assert.Equal(
                Novelty.UnseenAnyGame,
                Run(graph, new FakeWorld().WithMoney(3000), Novel(3)).Best);
        }

        /// <summary>
        /// Garte's nightly room: repeatable, so a cycle can pay it more than once, and
        /// the balance is what stops the loop rather than a visit count.
        /// </summary>
        /// <remarks>
        /// The loop charges 2,000 until it cannot, so the balance it ends on is the
        /// starting one modulo the cost, and that residue is what the guard sees. From
        /// 5,000 the residue is 1,000, which clears a guard of "under 1,500"; from 3,800
        /// it is 1,800, which does not. Both runs must stop on their own - it is the
        /// balance, not a visit count, that ends the cycle.
        /// </remarks>
        [Fact]
        public void RepeatablePurchaseInACycle_TerminatesOnMoney()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, cost: 2000, links: new[] { 2 })
                .Add(2, links: new[] { 1, 3 })
                .Add(3, guard: "MoneyAmount() < 1500")
                .Build();

            LookAheadResult drained = Run(graph, new FakeWorld().WithMoney(5000), Novel(3));
            Assert.Equal(Novelty.UnseenAnyGame, drained.Best);
            Assert.False(drained.BudgetExhausted);

            LookAheadResult stranded = Run(graph, new FakeWorld().WithMoney(3800), Novel(3));
            Assert.Equal(Novelty.SeenThisGame, stranded.Best);
            Assert.False(stranded.BudgetExhausted);
        }

        // ---- the clock ----------------------------------------------------------

        /// <summary>
        /// The case that makes time worth modelling: a path advances the clock past
        /// noon, and a guard downstream asks whether it is the afternoon. Evaluated
        /// against the pre-crawl clock the answer is no, and content is hidden.
        /// </summary>
        [Fact]
        public void PassTime_MovesGuardsPastTheHour()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, script: "PassTime()", links: new[] { 2 })
                .Add(2, script: "PassTime()", links: new[] { 3 })
                .Add(3, guard: "IsAfternoon()")
                .Build();

            // 11:40 plus two quarter-hours is 12:10 - noon, which IsAfternoon includes.
            var justBefore = new FakeWorld().AtTime(11, 40);
            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, justBefore, Novel(3)).Best);

            // 11:00 plus two is 11:30, still the morning.
            var tooEarly = new FakeWorld().AtTime(11, 0);
            Assert.Equal(Novelty.SeenThisGame, Run(graph, tooEarly, Novel(3)).Best);
        }

        /// <summary>
        /// The mirror, and the reason this is not merely cosmetic: without the clock the
        /// engine would report the morning guard as still open.
        /// </summary>
        [Fact]
        public void PassTime_ClosesGuardsItMovesPast()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, script: "PassTime()", links: new[] { 2 })
                .Add(2, script: "PassTime()", links: new[] { 3 })
                .Add(3, guard: "IsMorning()")
                .Build();

            var justBefore = new FakeWorld().AtTime(11, 40);
            Assert.Equal(Novelty.SeenThisGame, Run(graph, justBefore, Novel(3)).Best);
        }

        /// <summary>NormalTimeForward refuses to move a locked clock.</summary>
        [Fact]
        public void PassTime_DoesNothingWhileTheClockIsLocked()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, script: "PassTime()", links: new[] { 2 })
                .Add(2, script: "PassTime()", links: new[] { 3 })
                .Add(3, guard: "IsAfternoon()")
                .Build();

            var locked = new FakeWorld().AtTime(11, 40).WithLockedClock();
            Assert.Equal(Novelty.SeenThisGame, Run(graph, locked, Novel(3)).Best);
        }

        /// <summary>
        /// DayCount is the story's counter, which PassTime does not touch, so it stays
        /// the host's answer however much time a path burns.
        /// </summary>
        [Fact]
        public void PassTime_DoesNotAdvanceTheDay()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, script: "PassTime()", links: new[] { 2 })
                .Add(2, guard: "DayCount() == 2")
                .Build();

            var dayOne = new FakeWorld().AtTime(23, 55).WithDay(1).WithQuery("DayCount", 1d);
            Assert.Equal(Novelty.SeenThisGame, Run(graph, dayOne, Novel(2)).Best);

            var dayTwo = new FakeWorld().AtTime(23, 55).WithDay(2).WithQuery("DayCount", 2d);
            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, dayTwo, Novel(2)).Best);
        }

        // ---- skill checks -------------------------------------------------------

        /// <summary>
        /// The conversation 825 shape: a passive check sets a flag, and the options past
        /// it are guarded on that flag being false. Whether they are reachable depends
        /// entirely on whether the check fires, which is a property of the character.
        /// </summary>
        private static LookAheadGraph BuildParaplegic()
        {
            return new GraphBuilder()
                .Add(0, links: new[] { 387 })
                .Add(387, links: new[] { 415 })
                .Add(
                    415,
                    kind: DialogueCheckKind.Passive,
                    script: "SetVariableValue(\"whirling.lena_intro_encyc_paraplegic\", true)",
                    links: new[] { 286 })
                .Add(286, isGroup: true, links: new[] { 189 })
                .Add(
                    189,
                    guard: "Variable[\"whirling.lena_intro_encyc_paraplegic\"] == false")
                .Build();
        }

        [Fact]
        public void PassingCheck_ClosesTheOptionsItsFlagGuards()
        {
            LookAheadGraph graph = BuildParaplegic();
            var world = new FakeWorld()
                .WithCheck(GraphBuilder.Node(415), Ternary.True);

            Assert.Equal(Novelty.SeenThisGame, Run(graph, world, Novel(189)).Best);
        }

        [Fact]
        public void FailingCheck_LeavesThemOpen()
        {
            LookAheadGraph graph = BuildParaplegic();
            var world = new FakeWorld()
                .WithCheck(GraphBuilder.Node(415), Ternary.False);

            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, world, Novel(189)).Best);
        }

        /// <summary>
        /// An unknown outcome has to explore both, and the reachable branch wins -
        /// the soundness rule again, applied to checks rather than guards.
        /// </summary>
        [Fact]
        public void UnknownCheck_ExploresBothOutcomes()
        {
            LookAheadGraph graph = BuildParaplegic();
            var world = new FakeWorld { DefaultCheck = Ternary.Unknown };

            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, world, Novel(189)).Best);
        }

        // ---- diagnostics --------------------------------------------------------

        [Fact]
        public void NoTraceIsKeptUnlessAsked()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1)
                .Build();

            Assert.Null(Run(graph, new FakeWorld(), Novel()).Trace);
        }

        /// <summary>
        /// A trace has to name the entries reached in the most distinct states, because
        /// that is what identifies a blow-up; a count alone is not diagnosable.
        /// </summary>
        [Fact]
        public void Trace_NamesTheHottestEntriesWorstFirst()
        {
            // 1 is reached in several states because 2 keeps changing a counter it reads.
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, guard: "Variable[\"q.count\"] < 4", links: new[] { 2 })
                .Add(
                    2,
                    script: "SetVariableValue(\"q.count\", Variable[\"q.count\"] + 1)",
                    links: new[] { 1 })
                .Build();

            var options = new LookAheadOptions { CollectTrace = true };
            LookAheadResult result = Run(graph, new FakeWorld(), Novel(), options: options);

            LookAheadTrace trace = Assert.IsType<LookAheadTrace>(result.Trace);
            Assert.Equal(GraphBuilder.Node(0), trace.Start);
            Assert.Equal(graph.Count, trace.GraphNodeCount);
            Assert.NotEmpty(trace.HottestNodes);

            for (int i = 1; i < trace.HottestNodes.Count; i++)
            {
                Assert.True(
                    trace.HottestNodes[i - 1].States >= trace.HottestNodes[i].States,
                    "hottest entries must be ordered worst first");
            }
        }

        [Fact]
        public void Trace_RecordsTheStateTheCrawlStartedFrom()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1)
                .Build();

            var world = new FakeWorld().WithMoney(4200).AtTime(17, 30).WithDay(3);
            var options = new LookAheadOptions { CollectTrace = true };

            LookAheadTrace trace = Assert.IsType<LookAheadTrace>(
                Run(graph, world, Novel(), options: options).Trace);

            Assert.Equal(4200, trace.Money);
            Assert.Equal((17 * 60) + 30, trace.DayMinutes);
            Assert.Equal(3, trace.DayCounter);
            Assert.False(trace.ClockLocked);
        }

        [Fact]
        public void Trace_IsCappedSoAReportStaysReadable()
        {
            var builder = new GraphBuilder();
            var links = new int[20];
            for (int i = 0; i < 20; i++)
            {
                links[i] = i + 1;
                builder.Add(i + 1);
            }

            builder.Add(0, links: links);
            LookAheadGraph graph = builder.Build();

            var options = new LookAheadOptions { CollectTrace = true, TraceNodeLimit = 5 };
            LookAheadTrace trace = Assert.IsType<LookAheadTrace>(
                Run(graph, new FakeWorld(), Novel(), options: options).Trace);

            Assert.Equal(5, trace.HottestNodes.Count);
        }

        // ---- cross-conversation, budget, ordering -------------------------------

        [Fact]
        public void CrawlFollowsLinksIntoAnotherConversation()
        {
            var builder = new GraphBuilder();
            builder.Add(0, links: new[] { 1 });
            builder.AddCrossing(1, 1, new[] { new DialogueNodeId(2, 5) });
            builder.Add(5, conversation: 2);
            LookAheadGraph graph = builder.Build();

            NoveltyLookup novelty = node =>
                node.ConversationId == 2 && node.EntryId == 5
                    ? Novelty.UnseenAnyGame
                    : Novelty.SeenThisGame;

            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, new FakeWorld(), novelty).Best);
        }

        [Fact]
        public void LinkOutsideTheLoadedGroup_EndsThatBranchWithoutFailing()
        {
            var builder = new GraphBuilder();
            builder.Add(0, links: new[] { 1 });
            builder.AddCrossing(1, 1, new[] { new DialogueNodeId(99, 1) });
            LookAheadGraph graph = builder.Build();

            LookAheadResult result = Run(graph, new FakeWorld(), Novel());
            Assert.Equal(Novelty.SeenThisGame, result.Best);
        }

        /// <summary>The stronger of two novelties wins, whichever is found first.</summary>
        [Fact]
        public void ReportsTheStrongestNoveltyReachable()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1, 2 })
                .Add(1)
                .Add(2)
                .Build();

            var scores = new Dictionary<int, Novelty>
            {
                [1] = Novelty.UnseenThisGame,
                [2] = Novelty.UnseenAnyGame,
            };
            Assert.Equal(Novelty.UnseenAnyGame, Run(graph, new FakeWorld(), Scores(scores)).Best);

            scores[2] = Novelty.SeenThisGame;
            Assert.Equal(Novelty.UnseenThisGame, Run(graph, new FakeWorld(), Scores(scores)).Best);
        }

        [Fact]
        public void StopsAsSoonAsTheStrongestNoveltyIsFound()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, links: new[] { 2 })
                .Add(2, links: new[] { 3 })
                .Add(3)
                .Build();

            LookAheadResult result = Run(graph, new FakeWorld(), Novel(1));
            Assert.Equal(Novelty.UnseenAnyGame, result.Best);

            // It found the answer at the first child and did not walk the tail.
            Assert.Equal(2, result.NodesReached);
        }

        [Fact]
        public void ExhaustingTheBudgetIsReported()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, links: new[] { 2 })
                .Add(2, links: new[] { 1 })
                .Build();

            var options = new LookAheadOptions { StateBudget = 2 };
            LookAheadResult result = Run(graph, new FakeWorld(), Novel(), options: options);
            Assert.True(result.BudgetExhausted);
            Assert.Equal(LookAheadLimit.States, result.StoppedBy);
        }

        /// <summary>A counter loop, whose state space is finite but not tiny.</summary>
        /// <remarks>
        /// Each visit increments a counter, so the same entry is reached in a new state
        /// each time until the counter cap saturates it. Enough states to run into a
        /// small state budget, and not so many that a test waits for it.
        /// </remarks>
        private static LookAheadGraph CountingGraph() =>
            new GraphBuilder()
                .Add(0, links: new[] { 1 })
                .Add(1, script: "Variable[\"n\"] = Variable[\"n\"] + 1", links: new[] { 2 })
                .Add(2, links: new[] { 1 })
                .Build();

        /// <remarks>
        /// A budget already spent when the crawl starts, rather than a real duration.
        /// Timing a crawl against a wall clock and asserting it stopped would measure the
        /// build machine as much as the code; what wants proving is that the clock is
        /// consulted at all, that it ends the crawl, and that it is reported as the clock
        /// and not as the state budget.
        /// </remarks>
        [Fact]
        public void RunningOutOfTimeIsReportedAsTimeRatherThanStates()
        {
            var options = new LookAheadOptions
            {
                // High enough that the clock is the only thing that can stop it.
                StateBudget = int.MaxValue,
                TimeBudget = TimeSpan.FromTicks(1),
                TimeCheckInterval = 1,
            };

            LookAheadResult result = Run(
                CountingGraph(), new FakeWorld().WithVariable("n", 0), Novel(), options: options);

            Assert.True(result.BudgetExhausted);
            Assert.Equal(LookAheadLimit.Time, result.StoppedBy);
        }

        [Fact]
        public void TheStateBudgetStillAppliesWhenATimeBudgetIsSet()
        {
            // The two sit beside each other, and the state budget stays the one that
            // makes a result reproducible. A generous clock must not disable it.
            var options = new LookAheadOptions
            {
                StateBudget = 2,
                TimeBudget = TimeSpan.FromMinutes(5),
            };

            LookAheadResult result = Run(
                CountingGraph(), new FakeWorld().WithVariable("n", 0), Novel(), options: options);

            Assert.Equal(LookAheadLimit.States, result.StoppedBy);
        }

        /// <remarks>
        /// An interval already elapsed, for the same reason the time-budget test uses a
        /// spent budget: what wants proving is that a running crawl reports itself and
        /// says where it has got to, not how fast the build machine is.
        /// </remarks>
        [Fact]
        public void ALongCrawlReportsThatItIsStillGoing()
        {
            var reports = new List<(DialogueNodeId Start, int States, int Nodes)>();
            var options = new LookAheadOptions
            {
                TimeBudget = TimeSpan.Zero,
                ProgressInterval = TimeSpan.FromTicks(1),
                TimeCheckInterval = 1,
                OnProgress = (start, states, nodes, _) =>
                    reports.Add((start, states, nodes)),
            };

            Run(CountingGraph(), new FakeWorld().WithVariable("n", 0), Novel(), options: options);

            Assert.NotEmpty(reports);
            Assert.All(reports, r => Assert.True(r.States > 0));
            Assert.All(reports, r => Assert.Equal(0, r.Start.EntryId));
        }

        [Fact]
        public void NothingIsReportedWhenNobodyIsListening()
        {
            // The clock is not read at all without a time budget or a listener, so this
            // is really a check that the crawl still completes with both switched off.
            var options = new LookAheadOptions
            {
                TimeBudget = TimeSpan.Zero,
                OnProgress = null,
            };

            LookAheadResult result = Run(
                CountingGraph(), new FakeWorld().WithVariable("n", 0), Novel(), options: options);

            Assert.Equal(LookAheadLimit.None, result.StoppedBy);
        }

        [Fact]
        public void NoTimeBudgetMeansNoClock()
        {
            // Zero is not "no time at all"; it is the default, and means unlimited.
            var options = new LookAheadOptions { TimeBudget = TimeSpan.Zero };

            LookAheadResult result = Run(
                new GraphBuilder().Add(0, links: new[] { 1 }).Add(1).Build(),
                new FakeWorld(),
                Novel(),
                options: options);

            Assert.Equal(LookAheadLimit.None, result.StoppedBy);
            Assert.False(result.BudgetExhausted);
        }

        [Fact]
        public void UnreachableStart_ReportsNothing()
        {
            LookAheadGraph graph = new GraphBuilder()
                .Add(0, guard: "Variable[\"never\"]", links: new[] { 1 })
                .Add(1)
                .Build();

            LookAheadResult result = Run(
                graph, new FakeWorld().WithVariable("never", false), Novel(1));
            Assert.Equal(Novelty.SeenThisGame, result.Best);
            Assert.Equal(0, result.NodesReached);
        }
    }
}
