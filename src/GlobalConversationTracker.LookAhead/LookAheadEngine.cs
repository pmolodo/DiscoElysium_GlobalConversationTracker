// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>How novel a dialogue entry is, as the engine's caller sees it.</summary>
    /// <param name="node">The entry to score.</param>
    /// <returns>Its novelty.</returns>
    public delegate Novelty NoveltyLookup(DialogueNodeId node);

    /// <summary>What one look-ahead crawl found.</summary>
    public sealed class LookAheadResult
    {
        /// <summary>Creates a result.</summary>
        /// <param name="best">The most novel entry reachable beyond the start.</param>
        /// <param name="statesExplored">How many (entry, state) pairs were visited.</param>
        /// <param name="nodesReached">How many distinct entries were reached.</param>
        /// <param name="budgetExhausted">Whether the search stopped early.</param>
        public LookAheadResult(
            Novelty best, int statesExplored, int nodesReached, bool budgetExhausted)
        {
            Best = best;
            StatesExplored = statesExplored;
            NodesReached = nodesReached;
            BudgetExhausted = budgetExhausted;
        }

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
        /// Whether the search hit its budget and stopped early. When true,
        /// <see cref="Best"/> is a lower bound rather than an answer.
        /// </summary>
        public bool BudgetExhausted { get; }

        /// <inheritdoc/>
        public override string ToString()
        {
            return $"{Best} after {StatesExplored} states over {NodesReached} nodes"
                + (BudgetExhausted ? " (budget exhausted)" : string.Empty);
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
        /// Where counter increments saturate. Guards in the shipped database compare
        /// counters against constants no larger than 5, and any value above the largest
        /// such constant is indistinguishable from it, so capping keeps the domain finite
        /// without changing an answer.
        /// </summary>
        public int CounterCap { get; set; } = 16;

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
                return new LookAheadResult(Novelty.SeenThisGame, 0, 0, false);
            }

            var seen = new HashSet<StateKey>();
            var queue = new Queue<StateKey>();
            var reached = new HashSet<DialogueNodeId>();
            var first = new StateKey(start, entered);
            seen.Add(first);
            queue.Enqueue(first);
            reached.Add(start);

            Novelty best = Novelty.SeenThisGame;
            bool exhausted = false;

            while (queue.Count > 0)
            {
                if (seen.Count >= _options.StateBudget)
                {
                    exhausted = true;
                    break;
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
                                        best, seen.Count, reached.Count, false);
                                }
                            }
                        }

                        var key = new StateKey(childId, next);
                        if (seen.Add(key))
                        {
                            reached.Add(childId);
                            queue.Enqueue(key);
                        }
                    }
                }
            }

            return new LookAheadResult(best, seen.Count, reached.Count, exhausted);
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
                    if (!context.World.IsSeen(node.Id))
                    {
                        yield return Charge(node, state, symbols);
                    }

                    yield break;

                case DialogueCheckKind.KimSwitch:
                    if (node.BooleanOnly || !context.World.IsSeen(node.Id))
                    {
                        yield return Charge(node, state, symbols);
                    }

                    yield break;

                case DialogueCheckKind.Red:
                case DialogueCheckKind.White:
                    foreach (LookAheadState next in EnterRolled(node, state, symbols))
                    {
                        yield return next;
                    }

                    yield break;

                case DialogueCheckKind.Passive:
                {
                    Ternary passes = context.World.CheckPasses(node.Id);
                    if (passes != Ternary.False)
                    {
                        yield return Charge(node, state, symbols);
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
                    yield return Charge(node, state, symbols);
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
            LookAheadNode node, LookAheadState state, StateSymbols symbols)
        {
            bool passed = node.FlagSlot >= 0 && state.IsSet(node.FlagSlot);
            bool failed = node.FailedFlagSlot >= 0 && state.IsSet(node.FailedFlagSlot);
            if (passed || (node.Kind == DialogueCheckKind.Red && failed))
            {
                yield break;
            }

            LookAheadState entered = Charge(node, state, symbols);

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

        /// <summary>Pays for the node, then applies its actions.</summary>
        private LookAheadState Charge(
            LookAheadNode node, LookAheadState state, StateSymbols symbols)
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

            return DialogueAction.Apply(
                node.Actions, paid, symbols.Once(node.Id), _options.CounterCap);
        }

        /// <summary>
        /// Builds the starting state by asking the world for every slot the graph
        /// mentions.
        /// </summary>
        private static LookAheadState Seed(LookAheadGraph graph, ILookAheadWorld world)
        {
            StateSymbols symbols = graph.Symbols;
            LookAheadState state = LookAheadState.Empty(symbols.Count, world.Money);
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
