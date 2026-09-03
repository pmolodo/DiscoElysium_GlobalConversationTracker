// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Diagnostics;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>How novel a dialogue entry is, as the engine's caller sees it.</summary>
    /// <param name="node">The entry to score.</param>
    /// <returns>Its novelty.</returns>
    public delegate Novelty NoveltyLookup(DialogueNodeId node);

    /// <summary>Says that a crawl is still going, and how far it has got.</summary>
    /// <param name="start">The option the crawl began from.</param>
    /// <param name="statesExplored">How many (entry, state) pairs it has visited.</param>
    /// <param name="nodesReached">How many distinct entries it has reached.</param>
    /// <param name="elapsed">How long it has been running.</param>
    public delegate void LookAheadProgressReport(
        DialogueNodeId start, int statesExplored, int nodesReached, TimeSpan elapsed);

    /// <summary>What ended a crawl before it had explored everything reachable.</summary>
    public enum LookAheadLimit
    {
        /// <summary>Nothing did: the crawl finished, or found the best there is.</summary>
        None = 0,

        /// <summary>It ran out of state budget.</summary>
        States = 1,

        /// <summary>It ran out of time.</summary>
        Time = 2,
    }

    /// <summary>What one look-ahead crawl found.</summary>
    public sealed class LookAheadResult
    {
        /// <summary>Creates a result.</summary>
        /// <param name="best">The most novel entry reachable beyond the start.</param>
        /// <param name="statesExplored">How many (entry, state) pairs were visited.</param>
        /// <param name="nodesReached">How many distinct entries were reached.</param>
        /// <param name="budgetExhausted">Whether the search stopped early.</param>
        /// <param name="trace">
        /// What the crawl did, when <see cref="LookAheadOptions.CollectTrace"/> asked for
        /// it; null otherwise.
        /// </param>
        public LookAheadResult(
            Novelty best,
            int statesExplored,
            int nodesReached,
            bool budgetExhausted,
            LookAheadTrace? trace = null)
            : this(
                best,
                statesExplored,
                nodesReached,
                budgetExhausted ? LookAheadLimit.States : LookAheadLimit.None,
                trace)
        {
        }

        /// <summary>Creates a result, saying which limit stopped it.</summary>
        /// <param name="best">The most novel entry reachable beyond the start.</param>
        /// <param name="statesExplored">How many (entry, state) pairs were visited.</param>
        /// <param name="nodesReached">How many distinct entries were reached.</param>
        /// <param name="stoppedBy">Which limit ended it, if either did.</param>
        /// <param name="trace">
        /// What the crawl did, when <see cref="LookAheadOptions.CollectTrace"/> asked for
        /// it; null otherwise.
        /// </param>
        public LookAheadResult(
            Novelty best,
            int statesExplored,
            int nodesReached,
            LookAheadLimit stoppedBy,
            LookAheadTrace? trace = null)
        {
            Best = best;
            StatesExplored = statesExplored;
            NodesReached = nodesReached;
            StoppedBy = stoppedBy;
            Trace = trace;
        }

        /// <summary>Which limit ended the crawl, if either did.</summary>
        public LookAheadLimit StoppedBy { get; }

        /// <summary>
        /// The most novel entry reachable strictly beyond the starting entry.
        /// </summary>
        /// <remarks>
        /// Strictly beyond, because the starting entry's own novelty is already shown as
        /// the option's text colour. The marker exists to say what lies past it.
        /// </remarks>
        public Novelty Best { get; }

        /// <summary>How many (entry, state) pairs the search visited.</summary>
        public int StatesExplored { get; }

        /// <summary>How many distinct entries the search reached.</summary>
        public int NodesReached { get; }

        /// <summary>
        /// Whether the search hit a limit and stopped early. When true,
        /// <see cref="Best"/> is a lower bound rather than an answer.
        /// </summary>
        /// <remarks>
        /// Says that the answer is incomplete, which is what every caller acting on a
        /// result needs to know; <see cref="StoppedBy"/> says which limit did it, which
        /// only the diagnostics care about.
        /// </remarks>
        public bool BudgetExhausted => StoppedBy != LookAheadLimit.None;

        /// <summary>
        /// What the crawl did, when it was asked to keep track; null otherwise.
        /// </summary>
        public LookAheadTrace? Trace { get; }

        /// <inheritdoc/>
        public override string ToString()
        {
            return $"{Best} after {StatesExplored} states over {NodesReached} nodes"
                + StoppedBy switch
                {
                    LookAheadLimit.States => " (state budget exhausted)",
                    LookAheadLimit.Time => " (out of time)",
                    _ => string.Empty,
                };
        }
    }

    /// <summary>Limits and knobs for a crawl.</summary>
    public sealed class LookAheadOptions
    {
        /// <summary>
        /// The most (entry, state) pairs to visit before giving up. Reached only by
        /// pathological regions; a whole conversation group is a couple of thousand
        /// entries.
        /// </summary>
        public int StateBudget { get; set; } = 200_000;

        /// <summary>
        /// The longest one crawl may run for, or <see cref="TimeSpan.Zero"/> for no
        /// limit.
        /// </summary>
        /// <remarks>
        /// <para>What a player actually notices is how long the menu takes to appear, and
        /// the state budget is only a proxy for that. It is a decent one - measured over
        /// the largest conversations in the game, cost per state stayed within about a
        /// third of itself - but it is a proxy for the wrong quantity, and it cannot
        /// account for the machine the game is running on.</para>
        ///
        /// <para>It does not replace <see cref="StateBudget"/>, it sits beside it, and
        /// the state budget stays the one that makes a result reproducible: the same menu
        /// on the same save marks the same way twice, which a clock cannot promise. Read
        /// the pair as a deterministic ceiling with a wall-clock backstop for the machine
        /// that is slower than the one this was measured on.</para>
        ///
        /// <para>One second by default. That is far above anything measured in game -
        /// the worst single crawl over the largest conversations came to about three
        /// quarters of that, and almost everything is under a hundredth of it - so it is
        /// a backstop rather than a working limit, and it is the machine slower than the
        /// one those numbers came from that it exists for. Set it to
        /// <see cref="TimeSpan.Zero"/> to measure without it.</para>
        /// </remarks>
        public TimeSpan TimeBudget { get; set; } = TimeSpan.FromSeconds(1);

        /// <summary>
        /// How many states pass between readings of the clock.
        /// </summary>
        /// <remarks>
        /// Reading it every state would put a timer call in the loop that decides what
        /// the feature costs. Reading it every few hundred bounds the overshoot at the
        /// time those states take - single-digit milliseconds at the rates measured in
        /// game - for an overhead too small to find.
        /// </remarks>
        public int TimeCheckInterval { get; set; } = 512;

        /// <summary>
        /// Called while a crawl is still running, no more often than
        /// <see cref="ProgressInterval"/>; null to say nothing.
        /// </summary>
        /// <remarks>
        /// For the crawls that take long enough to wonder about. At the default interval
        /// this never fires in play - a menu's crawls are done in milliseconds, and one
        /// that is not is stopped by the time budget shortly after the first report - so
        /// it costs nothing and says nothing until something is genuinely slow, which is
        /// exactly when a run that has printed nothing for a minute is impossible to tell
        /// from a run that has hung.
        /// </remarks>
        public LookAheadProgressReport? OnProgress { get; set; }

        /// <summary>How often <see cref="OnProgress"/> may be called.</summary>
        public TimeSpan ProgressInterval { get; set; } = TimeSpan.FromSeconds(1);

        /// <summary>
        /// Handed every <see cref="StateSampleInterval"/>th state as it is first
        /// reached; null to sample nothing.
        /// </summary>
        /// <remarks>
        /// For finding out what a crawl's states actually differ in. The counts say a
        /// conversation reached two hundred thousand states and which entries they piled
        /// up at; only the states themselves say WHICH of the tracked slots are doing the
        /// multiplying, and that is the difference between knowing a conversation is
        /// expensive and knowing why.
        ///
        /// Off by default, and skipped entirely when null, so it costs a crawl nothing to
        /// have the option.
        /// </remarks>
        public Action<DialogueNodeId, LookAheadState, int>? OnStateReached { get; set; }

        /// <summary>
        /// How many states pass between calls to <see cref="OnStateReached"/>; zero or
        /// less samples none.
        /// </summary>
        public int StateSampleInterval { get; set; }

        /// <summary>
        /// Where counter increments saturate. Guards in the shipped database compare
        /// counters against constants no larger than 5, and any value above the largest
        /// such constant is indistinguishable from it, so capping keeps the domain finite
        /// without changing an answer.
        /// </summary>
        public int CounterCap { get; set; } = 16;

        /// <summary>
        /// An optional cap for one state slot. When null, every counter uses
        /// <see cref="CounterCap"/>.
        /// </summary>
        /// <remarks>
        /// A graph can often prove a tighter cap for one counter from the constants in
        /// its guards. Keeping that decision outside the engine makes the default
        /// conservative while allowing an offline analysis to test the proof.
        /// </remarks>
        public Func<int, int>? CounterCapForSlot { get; set; }

        /// <summary>
        /// Whether a failed skill check still lets the crawl walk past the entry.
        /// </summary>
        /// <remarks>
        /// True by default, and deliberately so. A check that does not fire means its
        /// line is not shown, but the conversation plainly does not dead-end there - and
        /// the alternative reading, that the whole subtree becomes unreachable, would
        /// under-report, which is the one error direction that defeats the feature.
        /// Passing through without applying the entry's actions is the conservative
        /// reading. Confirm against the game before turning this off.
        /// </remarks>
        public bool FailedChecksPassThrough { get; set; } = true;

        /// <summary>
        /// Whether to keep a per-entry tally so a crawl can explain itself afterwards.
        /// </summary>
        /// <remarks>
        /// Off by default. It adds a dictionary write per state, in the loop that decides
        /// what the feature costs, so it is for diagnosing a budget overflow rather than
        /// for running with.
        /// </remarks>
        public bool CollectTrace { get; set; }

        /// <summary>
        /// How many entries a trace names. Only the worst matter - a blow-up is one or
        /// two entries reached in hundreds of states, not a long flat list.
        /// </summary>
        public int TraceNodeLimit { get; set; } = 15;
    }

    /// <summary>
    /// Walks forward from a dialogue entry and reports the most novel thing still
    /// reachable from it.
    /// </summary>
    /// <remarks>
    /// <para>A breadth-first search over (entry, state) pairs. State is carried because
    /// reachability depends on it: conversation 451 gates a 0.50 real purchase behind a
    /// 50.00 real one, so whether the cheap option is reachable depends on money the
    /// path itself already spent, and no state-free crawl can see that.</para>
    ///
    /// <para>Termination rests on two facts rather than on a depth limit. Money only
    /// falls, because a purchase is gated on affording it and cannot overdraw; and the
    /// discrete slots are finite, with counters saturating at
    /// <see cref="LookAheadOptions.CounterCap"/>. The budget is a backstop against a
    /// database that breaks those assumptions, not the primary bound.</para>
    ///
    /// <para>The search stops the instant it finds <see cref="Novelty.UnseenAnyGame"/>,
    /// since nothing outranks it. That is what makes the common case cheap: an option
    /// leading somewhere genuinely new usually proves it within a few nodes.</para>
    /// </remarks>
    public sealed class LookAheadEngine
    {
        private readonly LookAheadOptions _options;

        /// <summary>Creates an engine.</summary>
        /// <param name="options">Limits and knobs, or null for the defaults.</param>
        public LookAheadEngine(LookAheadOptions? options = null)
        {
            _options = options ?? new LookAheadOptions();
        }

        /// <summary>
        /// Whether this conversation group contains any scoreable entry that could
        /// improve an option with <paramref name="ownNovelty"/>.
        /// </summary>
        /// <remarks>
        /// This is deliberately a structural upper bound: it does not decide whether a
        /// candidate is reachable in the current world. Its useful negative answer is
        /// exact, though. If no entry in the complete group outranks the option, no walk
        /// can produce a marker, so the caller can avoid building crawl state entirely.
        /// </remarks>
        /// <param name="graph">The complete conversation group.</param>
        /// <param name="ownNovelty">The novelty already shown by the option.</param>
        /// <param name="novelty">How novel each entry is.</param>
        /// <returns>True when a crawl might improve the option; otherwise false.</returns>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public static bool HasPotentialImprovement(
            LookAheadGraph graph, Novelty ownNovelty, NoveltyLookup novelty)
        {
            if (graph == null)
            {
                throw new ArgumentNullException(nameof(graph));
            }

            if (novelty == null)
            {
                throw new ArgumentNullException(nameof(novelty));
            }

            foreach (LookAheadNode node in graph.Nodes)
            {
                // Groups are expanded in place. The game never writes their SimStatus,
                // so treating one as an unseen candidate would make this check useless.
                if (!node.IsGroup && novelty(node.Id) > ownNovelty)
                {
                    return true;
                }
            }

            return false;
        }

        /// <summary>
        /// Finds the most novel entry reachable beyond <paramref name="start"/>.
        /// </summary>
        /// <param name="graph">The conversation group to walk.</param>
        /// <param name="start">The option being evaluated.</param>
        /// <param name="world">The player's situation as the crawl begins.</param>
        /// <param name="novelty">How novel each entry is.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="KeyNotFoundException"><paramref name="start"/> is not in the graph.</exception>
        public LookAheadResult Evaluate(
            LookAheadGraph graph,
            DialogueNodeId start,
            ILookAheadWorld world,
            NoveltyLookup novelty)
        {
            if (graph == null)
            {
                throw new ArgumentNullException(nameof(graph));
            }

            if (world == null)
            {
                throw new ArgumentNullException(nameof(world));
            }

            if (novelty == null)
            {
                throw new ArgumentNullException(nameof(novelty));
            }

            LookAheadNode startNode = graph.Get(start);
            var context = new CrawlContext(graph.Symbols, world);
            LookAheadState initial = Seed(graph, world);

            // Entering the start node is the premise of the question: the marker says
            // what follows from picking this option, so its cost is paid and its actions
            // have run before anything downstream is considered.
            if (!TryEnter(startNode, initial, context, graph.Symbols, out LookAheadState entered))
            {
                return new LookAheadResult(
                    Novelty.SeenThisGame, 0, 0, false,
                    BuildTrace(graph, start, world,
                        _options.CollectTrace ? new Dictionary<DialogueNodeId, int>() : null));
            }

            var seen = new HashSet<StateKey>();
            var queue = new Queue<StateKey>();
            var reached = new HashSet<DialogueNodeId>();
            Dictionary<DialogueNodeId, int>? tally =
                _options.CollectTrace ? new Dictionary<DialogueNodeId, int>() : null;
            var first = new StateKey(start, entered);
            seen.Add(first);
            queue.Enqueue(first);
            reached.Add(start);
            Count(tally, start);

            Action<DialogueNodeId, LookAheadState, int>? sample = _options.OnStateReached;
            int sampleEvery = _options.StateSampleInterval;
            bool sampling = sample != null && sampleEvery > 0;
            if (sampling)
            {
                sample!(start, entered, seen.Count);
            }

            Novelty best = Novelty.SeenThisGame;
            LookAheadLimit stoppedBy = LookAheadLimit.None;

            // Read only when something is going to read it. A crawl that is neither timed
            // nor reporting should not pay for a timer.
            bool timed = _options.TimeBudget > TimeSpan.Zero;
            LookAheadProgressReport? report = _options.OnProgress;
            bool reporting = report != null && _options.ProgressInterval > TimeSpan.Zero;
            bool clocked = timed || reporting;

            long started = clocked ? Stopwatch.GetTimestamp() : 0;
            long deadline = timed ? started + Ticks(_options.TimeBudget) : 0;
            long nextReport = reporting ? started + Ticks(_options.ProgressInterval) : 0;
            int untilClockCheck = _options.TimeCheckInterval;

            while (queue.Count > 0)
            {
                if (seen.Count >= _options.StateBudget)
                {
                    stoppedBy = LookAheadLimit.States;
                    break;
                }

                if (clocked && --untilClockCheck <= 0)
                {
                    untilClockCheck = _options.TimeCheckInterval;
                    long now = Stopwatch.GetTimestamp();
                    if (timed && now >= deadline)
                    {
                        stoppedBy = LookAheadLimit.Time;
                        break;
                    }

                    if (reporting && now >= nextReport)
                    {
                        nextReport = now + Ticks(_options.ProgressInterval);
                        report!(start, seen.Count, reached.Count, Elapsed(started, now));
                    }
                }

                StateKey current = queue.Dequeue();
                LookAheadNode node = graph.Get(current.Node);

                for (int i = 0; i < node.Links.Count; i++)
                {
                    DialogueNodeId childId = node.Links[i];
                    if (!graph.TryGet(childId, out LookAheadNode child))
                    {
                        // A link out of the loaded group. Not an error: the caller
                        // decides how wide the group is, and a dangling edge simply ends
                        // this branch.
                        continue;
                    }

                    foreach (LookAheadState next in Enter(child, current.State, context, graph.Symbols))
                    {
                        // Groups are never scored: the game expands them in place and
                        // never marks their SimStatus, so every one of them looks
                        // permanently unseen.
                        if (!child.IsGroup)
                        {
                            Novelty score = novelty(childId);
                            if (score > best)
                            {
                                best = score;
                                if (best == Novelty.UnseenAnyGame)
                                {
                                    reached.Add(childId);
                                    return new LookAheadResult(
                                        best,
                                        seen.Count,
                                        reached.Count,
                                        false,
                                        BuildTrace(graph, start, world, tally));
                                }
                            }
                        }

                        var key = new StateKey(childId, next);
                        if (seen.Add(key))
                        {
                            reached.Add(childId);
                            Count(tally, childId);
                            queue.Enqueue(key);
                            if (sampling && seen.Count % sampleEvery == 0)
                            {
                                sample!(childId, next, seen.Count);
                            }
                        }
                    }
                }
            }

            return new LookAheadResult(
                best, seen.Count, reached.Count, stoppedBy,
                BuildTrace(graph, start, world, tally));
        }

        /// <summary>A duration as Stopwatch ticks.</summary>
        private static long Ticks(TimeSpan span) =>
            (long)(span.TotalSeconds * Stopwatch.Frequency);

        /// <summary>The span between two Stopwatch timestamps.</summary>
        private static TimeSpan Elapsed(long from, long to) =>
            TimeSpan.FromSeconds((double)(to - from) / Stopwatch.Frequency);

        /// <summary>Notes that an entry was reached in one more distinct state.</summary>
        private static void Count(
            Dictionary<DialogueNodeId, int>? tally, DialogueNodeId node)
        {
            if (tally == null)
            {
                return;
            }

            tally.TryGetValue(node, out int count);
            tally[node] = count + 1;
        }

        /// <summary>Turns the tally into a trace, worst entries first.</summary>
        private LookAheadTrace? BuildTrace(
            LookAheadGraph graph,
            DialogueNodeId start,
            ILookAheadWorld world,
            Dictionary<DialogueNodeId, int>? tally)
        {
            if (tally == null)
            {
                return null;
            }

            var hottest = new List<NodeStateCount>(tally.Count);
            foreach (KeyValuePair<DialogueNodeId, int> pair in tally)
            {
                hottest.Add(new NodeStateCount(pair.Key, pair.Value));
            }

            hottest.Sort((left, right) => right.States.CompareTo(left.States));
            if (hottest.Count > _options.TraceNodeLimit)
            {
                hottest.RemoveRange(
                    _options.TraceNodeLimit, hottest.Count - _options.TraceNodeLimit);
            }

            return new LookAheadTrace(
                start,
                graph.Count,
                graph.Symbols.Count,
                world.Money,
                world.DayMinutes,
                world.DayCounter,
                world.IsClockLocked,
                hottest);
        }

        /// <summary>
        /// The states a node can be entered in: usually one, but two when an outcome is
        /// undetermined and both possibilities have to be carried forward.
        /// </summary>
        /// <remarks>
        /// This is where the game's two gates are applied in order - the entry's
        /// condition, then the <c>isDialogueEntryValid</c> dispatch that
        /// <see cref="DialogueCheckKind"/> mirrors.
        /// </remarks>
        private IEnumerable<LookAheadState> Enter(
            LookAheadNode node,
            LookAheadState state,
            CrawlContext context,
            StateSymbols symbols)
        {
            context.Bind(state);
            bool clockLocked = context.World.IsClockLocked;
            if (!TernaryLogic.CanPass(node.Guard.Test(context)))
            {
                yield break;
            }

            if (!CanAfford(node, state, symbols))
            {
                yield break;
            }

            switch (node.Kind)
            {
                case DialogueCheckKind.Test:
                    // Hidden outside developer mode, so never reachable in play.
                    yield break;

                case DialogueCheckKind.Fake:
                    // Offered until it has been seen; its result is forced rather than
                    // rolled, so there is only one way through it.
                    if (!HasBeenSeen(node, state))
                    {
                        yield return Charge(node, state, symbols, clockLocked);
                    }

                    yield break;

                case DialogueCheckKind.KimSwitch:
                    if (node.BooleanOnly || !HasBeenSeen(node, state))
                    {
                        yield return Charge(node, state, symbols, clockLocked);
                    }

                    yield break;

                case DialogueCheckKind.Red:
                case DialogueCheckKind.White:
                    foreach (LookAheadState next in EnterRolled(
                        node, state, symbols, clockLocked))
                    {
                        yield return next;
                    }

                    yield break;

                case DialogueCheckKind.Passive:
                {
                    Ternary passes = context.World.CheckPasses(node.Id);
                    if (passes != Ternary.False)
                    {
                        yield return Charge(node, state, symbols, clockLocked);
                    }

                    if (passes != Ternary.True && _options.FailedChecksPassThrough)
                    {
                        // PassiveNode.CheckSuccess sets falseConditionAction to
                        // "Passthrough" on the entry as it evaluates it, so a failure
                        // does not end the branch: the line is not shown and its actions
                        // do not run, but the conversation walks on to the children.
                        yield return state;
                    }

                    yield break;
                }

                default:
                    yield return Charge(node, state, symbols, clockLocked);
                    yield break;
            }
        }

        /// <summary>
        /// A red or white check. Its outcome comes off the dice, so both results stay
        /// possible - but whether it is OFFERED at all is settled by its flags, and those
        /// are ordinary variables the crawl tracks.
        /// </summary>
        /// <remarks>
        /// A red check is one shot: either flag closes it. A white check is retryable, so
        /// only success closes it, which is why a failed white check leaves the state
        /// untouched rather than setting a failure flag.
        /// </remarks>
        private IEnumerable<LookAheadState> EnterRolled(
            LookAheadNode node, LookAheadState state, StateSymbols symbols, bool clockLocked)
        {
            bool passed = node.FlagSlot >= 0 && state.IsSet(node.FlagSlot);
            bool failed = node.FailedFlagSlot >= 0 && state.IsSet(node.FailedFlagSlot);
            if (passed || (node.Kind == DialogueCheckKind.Red && failed))
            {
                yield break;
            }

            LookAheadState entered = Charge(node, state, symbols, clockLocked);

            LookAheadState success = node.FlagSlot >= 0
                ? entered.With(node.FlagSlot, 1)
                : entered;
            yield return success;

            if (node.Kind == DialogueCheckKind.Red && node.FailedFlagSlot >= 0)
            {
                yield return entered.With(node.FailedFlagSlot, 1);
            }
            else if (node.Kind == DialogueCheckKind.White)
            {
                // A failed white check changes nothing and can be tried again.
                yield return entered;
            }
        }

        private bool TryEnter(
            LookAheadNode node,
            LookAheadState state,
            CrawlContext context,
            StateSymbols symbols,
            out LookAheadState entered)
        {
            foreach (LookAheadState next in Enter(node, state, context, symbols))
            {
                entered = next;
                return true;
            }

            entered = state;
            return false;
        }

        /// <summary>
        /// Whether the option can be selected at this balance. A cost option the player
        /// cannot afford is drawn but disabled, so it cannot be walked through.
        /// </summary>
        private static bool CanAfford(
            LookAheadNode node, LookAheadState state, StateSymbols symbols)
        {
            if (!node.IsCostOption)
            {
                return true;
            }

            if (node.CostOnce && state.IsSet(symbols.Once(node.Id)))
            {
                return true;
            }

            return node.Cost <= state.Money;
        }

        /// <summary>
        /// Whether this entry counts as displayed at this point on the path.
        /// </summary>
        /// <remarks>
        /// Speculative, and deliberately NOT the same question as
        /// <see cref="Novelty.SeenThisGame"/>. Novelty is what the player has really
        /// read, and decides the marker; this is what a hypothetical path would have
        /// displayed by the time it stands here, and decides whether an option is still
        /// offered. They start equal - the slot is seeded from the save - and diverge as
        /// the crawl walks.
        /// </remarks>
        private static bool HasBeenSeen(LookAheadNode node, LookAheadState state)
        {
            return node.SeenSlot >= 0 && state.IsSet(node.SeenSlot);
        }

        /// <summary>Pays for the node, then applies its actions.</summary>
        private LookAheadState Charge(
            LookAheadNode node, LookAheadState state, StateSymbols symbols, bool clockLocked)
        {
            LookAheadState paid = state;
            if (node.IsCostOption)
            {
                int onceSlot = symbols.Once(node.Id);
                bool alreadyPaid = node.CostOnce && state.IsSet(onceSlot);
                if (!alreadyPaid)
                {
                    paid = state.WithMoney(state.Money - node.Cost);
                    if (node.CostOnce)
                    {
                        paid = paid.With(onceSlot, 1);
                    }
                }
            }

            if (node.SeenSlot >= 0)
            {
                // Walking through an entry displays it, which is what closes a fake
                // check or a Kim switch the second time a path comes round to it.
                paid = paid.With(node.SeenSlot, 1);
            }

            return DialogueAction.Apply(
                node.Actions, paid, symbols.Once(node.Id), _options.CounterCap,
                clockLocked, _options.CounterCapForSlot);
        }

        /// <summary>
        /// Builds the starting state by asking the world for every slot the graph
        /// mentions.
        /// </summary>
        private static LookAheadState Seed(LookAheadGraph graph, ILookAheadWorld world)
        {
            StateSymbols symbols = graph.Symbols;
            LookAheadState state = LookAheadState.Empty(
                symbols.Count, world.Money, world.DayMinutes);
            for (int slot = 0; slot < symbols.Count; slot++)
            {
                string name = symbols.NameOf(slot);
                if (name.StartsWith(StateSymbols.OncePrefix, StringComparison.Ordinal))
                {
                    // A per-path marker, meaningless before the path starts.
                    continue;
                }

                if (name.StartsWith(StateSymbols.ItemPrefix, StringComparison.Ordinal))
                {
                    if (world.HasItem(name.Substring(StateSymbols.ItemPrefix.Length)))
                    {
                        state = state.With(slot, 1);
                    }

                    continue;
                }

                if (name.StartsWith(StateSymbols.TaskPrefix, StringComparison.Ordinal))
                {
                    if (world.IsTaskActive(name.Substring(StateSymbols.TaskPrefix.Length)))
                    {
                        state = state.With(slot, 1);
                    }

                    continue;
                }

                GuardValue value = world.GetVariable(name);
                if (value.Kind == GuardValueKind.Boolean && value.Boolean)
                {
                    state = state.With(slot, 1);
                }
                else if (value.Kind == GuardValueKind.Number && value.Number != 0)
                {
                    state = state.With(slot, (int)value.Number);
                }
            }

            // Seeded from the save, so the crawl starts believing exactly what the
            // player has actually read, and only diverges where a path displays
            // something.
            foreach (LookAheadNode node in graph.Nodes)
            {
                if (node.SeenSlot >= 0 && world.IsSeen(node.Id))
                {
                    state = state.With(node.SeenSlot, 1);
                }
            }

            return state;
        }

        /// <summary>One visited (entry, state) pair.</summary>
        private readonly struct StateKey : IEquatable<StateKey>
        {
            public StateKey(DialogueNodeId node, LookAheadState state)
            {
                Node = node;
                State = state;
            }

            public DialogueNodeId Node { get; }

            public LookAheadState State { get; }

            public bool Equals(StateKey other)
            {
                return Node.Equals(other.Node) && State.Equals(other.State);
            }

            public override bool Equals(object? obj)
            {
                return obj is StateKey other && Equals(other);
            }

            public override int GetHashCode()
            {
                unchecked
                {
                    return (Node.GetHashCode() * 397) ^ State.GetHashCode();
                }
            }
        }

        /// <summary>
        /// Answers guards from the crawl's own state where it tracks the answer, and
        /// from the world where it does not.
        /// </summary>
        /// <remarks>
        /// The split is what makes the Siileng case work. <c>CheckItem</c> has to come
        /// from the crawl, because <c>GainItem("shoes_faln")</c> happens mid-path; but
        /// <c>IsKimHere</c> has to come from the world, because nothing in dialogue moves
        /// Kim. Answering either from the wrong side produces a wrong reachable set.
        /// </remarks>
        private sealed class CrawlContext : IGuardContext
        {
            private readonly StateSymbols _symbols;
            private LookAheadState? _state;

            public CrawlContext(StateSymbols symbols, ILookAheadWorld world)
            {
                _symbols = symbols;
                World = world;
            }

            public ILookAheadWorld World { get; }

            public void Bind(LookAheadState state)
            {
                _state = state;
            }

            public GuardValue GetVariable(string name)
            {
                int slot = _symbols.Find(name);
                if (slot < 0 || _state == null)
                {
                    return World.GetVariable(name);
                }

                int value = _state.Get(slot);

                // A variable the graph only ever assigns true or false reads back as a
                // boolean; one it counts reads back as a number. Guards written
                // `Variable["x"] == true` and `Variable["x"] >= 3` both then work, and
                // Lua's refusal to coerce between the two is preserved.
                GuardValue initial = World.GetVariable(name);
                if (initial.Kind == GuardValueKind.Number)
                {
                    return GuardValue.FromNumber(value);
                }

                return GuardValue.FromBoolean(value != 0);
            }

            public GuardValue Query(string name, IReadOnlyList<GuardValue> arguments)
            {
                if (_state != null && ClockTime.Owns(name))
                {
                    // Answered from the crawl's clock, not the world's: a path that ran
                    // PassTime is standing at a later hour than the player is.
                    return ClockTime.Answer(
                        name, arguments, _state.DayMinutes, World.DayCounter);
                }

                switch (name)
                {
                    case "MoneyAmount":
                        return _state == null
                            ? World.Query(name, arguments)
                            : GuardValue.FromNumber(_state.Money);

                    case "CheckItem":
                        return LookUp(StateSymbols.ItemPrefix, arguments, name);

                    case "IsTaskActive":
                        return LookUp(StateSymbols.TaskPrefix, arguments, name);

                    default:
                        return World.Query(name, arguments);
                }
            }

            /// <summary>
            /// Reads a slot the crawl tracks, falling back to the world when the graph
            /// never mentions that name - an item no action in this group grants cannot
            /// change during the crawl, so the world's answer is the right one.
            /// </summary>
            private GuardValue LookUp(
                string prefix, IReadOnlyList<GuardValue> arguments, string name)
            {
                if (_state == null || arguments.Count == 0
                    || arguments[0].Kind != GuardValueKind.Text)
                {
                    return World.Query(name, arguments);
                }

                int slot = _symbols.Find(prefix + arguments[0].Text);
                return slot < 0
                    ? World.Query(name, arguments)
                    : GuardValue.FromBoolean(_state.IsSet(slot));
            }
        }
    }
}
