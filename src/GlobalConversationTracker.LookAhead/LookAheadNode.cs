// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>One dialogue entry, as the look-ahead needs it.</summary>
    /// <remarks>
    /// <para>A flattened view of <c>DialogueEntry</c> holding only what reachability and
    /// scoring depend on, so the engine never references a game type and can be tested
    /// without the game running.</para>
    ///
    /// <para><see cref="IsGroup"/> changes traversal on its own account: a group is
    /// expanded in place by <c>EvaluateLinksAtPriority</c> and never becomes a state, so
    /// nothing ever marks its SimStatus, which leaves it permanently Untouched. Since
    /// 36.6% of the shipped database is groups, scoring them would put a novelty marker
    /// on very nearly every option, so they are traversed and skipped.</para>
    /// </remarks>
    public sealed class LookAheadNode
    {
        /// <summary>Creates an ordinary entry.</summary>
        /// <param name="id">Which entry this is.</param>
        /// <param name="isGroup">Whether it is a group.</param>
        /// <param name="guard">Its parsed condition; use <see cref="GuardExpression.AlwaysTrue"/> for none.</param>
        /// <param name="actions">The state changes it makes.</param>
        /// <param name="links">The entries it links to, in order.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        public LookAheadNode(
            DialogueNodeId id,
            bool isGroup,
            GuardExpression guard,
            IReadOnlyList<DialogueAction> actions,
            IReadOnlyList<DialogueNodeId> links)
            : this(id, isGroup, DialogueCheckKind.None, guard, actions, links)
        {
        }

        /// <summary>Creates an entry.</summary>
        /// <param name="id">Which entry this is.</param>
        /// <param name="isGroup">Whether it is a group.</param>
        /// <param name="kind">Which special node type it is, if any.</param>
        /// <param name="guard">Its parsed condition.</param>
        /// <param name="actions">The state changes it makes.</param>
        /// <param name="links">The entries it links to, in order.</param>
        /// <param name="cost">What selecting it costs, in centimes; 0 for a free option.</param>
        /// <param name="costOnce">Whether the cost is charged only once.</param>
        /// <param name="hiddenWhenUnaffordable">
        /// Whether being unable to afford it hides the option rather than disabling it.
        /// </param>
        /// <param name="flagSlot">
        /// The slot for a red or white check's success flag, or -1. Named
        /// <c>FlagName</c> on the entry.
        /// </param>
        /// <param name="failedFlagSlot">
        /// The slot for a red check's failure flag (<c>FlagName_failed</c>), or -1.
        /// </param>
        /// <param name="booleanOnly">
        /// For a Kim switch, whether it stays available after being seen.
        /// </param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="ArgumentOutOfRangeException"><paramref name="cost"/> is negative.</exception>
        public LookAheadNode(
            DialogueNodeId id,
            bool isGroup,
            DialogueCheckKind kind,
            GuardExpression guard,
            IReadOnlyList<DialogueAction> actions,
            IReadOnlyList<DialogueNodeId> links,
            int cost = 0,
            bool costOnce = false,
            bool hiddenWhenUnaffordable = false,
            int flagSlot = -1,
            int failedFlagSlot = -1,
            bool booleanOnly = false)
        {
            if (cost < 0)
            {
                throw new ArgumentOutOfRangeException(nameof(cost));
            }

            Id = id;
            IsGroup = isGroup;
            Kind = kind;
            Guard = guard ?? throw new ArgumentNullException(nameof(guard));
            Actions = actions ?? throw new ArgumentNullException(nameof(actions));
            Links = links ?? throw new ArgumentNullException(nameof(links));
            Cost = cost;
            CostOnce = costOnce;
            HiddenWhenUnaffordable = hiddenWhenUnaffordable;
            FlagSlot = flagSlot;
            FailedFlagSlot = failedFlagSlot;
            BooleanOnly = booleanOnly;
        }

        /// <summary>Which entry this is.</summary>
        public DialogueNodeId Id { get; }

        /// <summary>
        /// Whether this is a group: traversed through, never scored, never marked by the
        /// game.
        /// </summary>
        public bool IsGroup { get; }

        /// <summary>Which special node type this is, if any.</summary>
        public DialogueCheckKind Kind { get; }

        /// <summary>Its parsed condition.</summary>
        public GuardExpression Guard { get; }

        /// <summary>The state changes it makes when reached.</summary>
        public IReadOnlyList<DialogueAction> Actions { get; }

        /// <summary>The entries it links to.</summary>
        public IReadOnlyList<DialogueNodeId> Links { get; }

        /// <summary>What selecting it costs, in centimes.</summary>
        public int Cost { get; }

        /// <summary>Whether the cost is charged only the first time.</summary>
        public bool CostOnce { get; }

        /// <summary>
        /// Whether poverty hides this option rather than greying it out. True for
        /// exactly one node in the shipped database; the other 83 cost options stay
        /// visible but disabled. Either way the option cannot be selected, so either way
        /// the crawl stops - the difference is only what the player sees.
        /// </summary>
        public bool HiddenWhenUnaffordable { get; }

        /// <summary>A red or white check's success flag slot, or -1.</summary>
        public int FlagSlot { get; }

        /// <summary>A red check's failure flag slot, or -1.</summary>
        public int FailedFlagSlot { get; }

        /// <summary>For a Kim switch, whether being seen leaves it available.</summary>
        public bool BooleanOnly { get; }

        /// <summary>Whether this entry charges anything.</summary>
        public bool IsCostOption => Cost > 0;

        /// <summary>
        /// Whether this entry's outcome is rolled rather than determined. Red and white
        /// checks go through the dice roller, so a look-ahead has to allow for both
        /// results; a passive check does not.
        /// </summary>
        public bool IsRolled => Kind == DialogueCheckKind.Red || Kind == DialogueCheckKind.White;

        /// <inheritdoc/>
        public override string ToString()
        {
            string kind = IsGroup ? "group" : Kind == DialogueCheckKind.None
                ? "node"
                : Kind.ToString().ToLowerInvariant();
            string price = IsCostOption ? $", cost {Cost}" : string.Empty;
            return $"{kind} {Id.ConversationId}:{Id.EntryId}{price}";
        }
    }
}
