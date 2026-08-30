// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>What kind of change an action makes.</summary>
    public enum DialogueActionKind
    {
        /// <summary>A call this model does not track. Recorded, never applied.</summary>
        Unmodelled = 0,

        /// <summary>Assigns a slot a fixed value.</summary>
        Assign = 1,

        /// <summary>Adds to a slot, saturating at a cap.</summary>
        Increment = 2,

        /// <summary>Adds to the balance.</summary>
        GainMoney = 3,

        /// <summary>Subtracts from the balance.</summary>
        LoseMoney = 4,
    }

    /// <summary>One state change a dialogue entry's <c>userScript</c> makes.</summary>
    /// <remarks>
    /// <para>Only the calls that touch state a guard can read are modelled. The rest -
    /// <c>ReputationGrows</c>, <c>XPPicoSetBool</c>, <c>ShowDialogueImage</c> and the
    /// other 60-odd - are kept as <see cref="DialogueActionKind.Unmodelled"/> rather than
    /// dropped, so that a later pass can find them without re-parsing, and so a reader
    /// can see they were considered rather than missed.</para>
    ///
    /// <para><c>PassTime</c> is the known gap: it moves the clock, and
    /// <c>DayCount()</c> / <c>IsHourBetween()</c> guards read the clock. Those queries
    /// are answered once per crawl by the host, so a path that passes time is evaluated
    /// against the pre-crawl clock. 207 nodes call it. It is recorded as unmodelled and
    /// documented rather than silently wrong.</para>
    /// </remarks>
    public sealed class DialogueAction
    {
        private DialogueAction(
            DialogueActionKind kind, int slot, int value, bool once, string name)
        {
            Kind = kind;
            Slot = slot;
            Value = value;
            Once = once;
            Name = name;
        }

        /// <summary>What kind of change this is.</summary>
        public DialogueActionKind Kind { get; }

        /// <summary>The slot it changes, or -1 for money and unmodelled calls.</summary>
        public int Slot { get; }

        /// <summary>The value assigned, added, or moved.</summary>
        public int Value { get; }

        /// <summary>
        /// Whether this fires only the first time its node is reached on a path.
        /// </summary>
        /// <remarks>
        /// Covers both the <c>+once(n)</c> idiom the counter increments use and the
        /// <c>GainMoneyOnce</c> / <c>LoseMoneyOnce</c> family. Without it, a
        /// money-gaining node inside a cycle - and all 28 <c>GainMoneyOnce</c> nodes are
        /// inside one - would let the search mint funds indefinitely.
        /// </remarks>
        public bool Once { get; }

        /// <summary>The source call's name, for diagnostics and unmodelled calls.</summary>
        public string Name { get; }

        /// <summary>Assigns a slot.</summary>
        /// <param name="slot">The slot to write.</param>
        /// <param name="value">The value to write.</param>
        /// <param name="name">The originating call's name.</param>
        public static DialogueAction Assign(int slot, int value, string name)
        {
            return new DialogueAction(DialogueActionKind.Assign, slot, value, false, name);
        }

        /// <summary>Adds to a slot.</summary>
        /// <param name="slot">The slot to add to.</param>
        /// <param name="amount">How much to add.</param>
        /// <param name="once">Whether it fires only once per path.</param>
        /// <param name="name">The originating call's name.</param>
        public static DialogueAction Increment(int slot, int amount, bool once, string name)
        {
            return new DialogueAction(DialogueActionKind.Increment, slot, amount, once, name);
        }

        /// <summary>Changes the balance.</summary>
        /// <param name="gain">True to add, false to subtract.</param>
        /// <param name="amount">How many centimes.</param>
        /// <param name="once">Whether it fires only once per path.</param>
        /// <param name="name">The originating call's name.</param>
        public static DialogueAction Money(bool gain, int amount, bool once, string name)
        {
            return new DialogueAction(
                gain ? DialogueActionKind.GainMoney : DialogueActionKind.LoseMoney,
                -1, amount, once, name);
        }

        /// <summary>Records a call this model does not track.</summary>
        /// <param name="name">The call's name.</param>
        public static DialogueAction Unmodelled(string name)
        {
            return new DialogueAction(DialogueActionKind.Unmodelled, -1, 0, false, name);
        }

        /// <inheritdoc/>
        public override string ToString()
        {
            switch (Kind)
            {
                case DialogueActionKind.Assign:
                    return $"{Name}: slot {Slot} = {Value}";
                case DialogueActionKind.Increment:
                    return $"{Name}: slot {Slot} += {Value}{(Once ? " (once)" : string.Empty)}";
                case DialogueActionKind.GainMoney:
                    return $"{Name}: money += {Value}{(Once ? " (once)" : string.Empty)}";
                case DialogueActionKind.LoseMoney:
                    return $"{Name}: money -= {Value}{(Once ? " (once)" : string.Empty)}";
                default:
                    return $"{Name}: not modelled";
            }
        }

        /// <summary>
        /// Applies a node's actions to a state.
        /// </summary>
        /// <param name="actions">The actions to apply, in order.</param>
        /// <param name="state">The state to apply them to.</param>
        /// <param name="onceSlot">
        /// The slot marking that this node already fired on this path, consulted and set
        /// by any action with <see cref="Once"/>.
        /// </param>
        /// <param name="counterCap">
        /// The value at which increments saturate. Any value above the largest constant
        /// a guard compares against is indistinguishable from it, so capping keeps a
        /// counter's domain finite without changing a single guard's answer.
        /// </param>
        /// <returns>The resulting state.</returns>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public static LookAheadState Apply(
            IReadOnlyList<DialogueAction> actions,
            LookAheadState state,
            int onceSlot,
            int counterCap)
        {
            if (actions == null)
            {
                throw new ArgumentNullException(nameof(actions));
            }

            if (state == null)
            {
                throw new ArgumentNullException(nameof(state));
            }

            if (actions.Count == 0)
            {
                return state;
            }

            bool alreadyFired = state.IsSet(onceSlot);
            bool firedSomethingOnce = false;
            var changes = new List<KeyValuePair<int, int>>(actions.Count + 1);
            int money = state.Money;

            for (int i = 0; i < actions.Count; i++)
            {
                DialogueAction action = actions[i];
                if (action.Once)
                {
                    if (alreadyFired)
                    {
                        continue;
                    }

                    firedSomethingOnce = true;
                }

                switch (action.Kind)
                {
                    case DialogueActionKind.Assign:
                        changes.Add(new KeyValuePair<int, int>(action.Slot, action.Value));
                        break;

                    case DialogueActionKind.Increment:
                    {
                        int current = Latest(changes, action.Slot, state);
                        int raised = current + action.Value;
                        changes.Add(new KeyValuePair<int, int>(
                            action.Slot, raised > counterCap ? counterCap : raised));
                        break;
                    }

                    case DialogueActionKind.GainMoney:
                        money += action.Value;
                        break;

                    case DialogueActionKind.LoseMoney:
                        money -= action.Value;
                        break;

                    default:
                        break;
                }
            }

            if (firedSomethingOnce)
            {
                changes.Add(new KeyValuePair<int, int>(onceSlot, 1));
            }

            return changes.Count == 0 && money == state.Money
                ? state
                : state.With(changes, money);
        }

        /// <summary>
        /// The value a slot will hold, accounting for earlier changes in this same batch.
        /// Two increments of the same counter in one script must compound, not race.
        /// </summary>
        private static int Latest(
            List<KeyValuePair<int, int>> changes, int slot, LookAheadState state)
        {
            for (int i = changes.Count - 1; i >= 0; i--)
            {
                if (changes[i].Key == slot)
                {
                    return changes[i].Value;
                }
            }

            return state.Get(slot);
        }
    }
}
