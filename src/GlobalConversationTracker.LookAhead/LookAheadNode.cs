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
    /// <para><see cref="IsGroup"/> and <see cref="IsCheck"/> both change traversal, and
    /// for different reasons. A group is expanded in place by
    /// <c>EvaluateLinksAtPriority</c> and never becomes a state, so nothing ever marks
    /// its SimStatus - which leaves it permanently Untouched. Since 36.6% of the shipped
    /// database is groups, scoring them would put a novelty marker on very nearly every
    /// option, so they are traversed and skipped. A check is a node whose firing is
    /// decided by <c>isDialogueEntryValid</c>, not by <c>conditionsString</c>, which is
    /// usually empty on one.</para>
    /// </remarks>
    public sealed class LookAheadNode
    {
        /// <summary>Creates a node.</summary>
        /// <param name="id">Which entry this is.</param>
        /// <param name="isGroup">Whether it is a group.</param>
        /// <param name="isCheck">Whether its firing is decided by a skill check.</param>
        /// <param name="guard">Its parsed condition; use <see cref="GuardExpression.AlwaysTrue"/> for none.</param>
        /// <param name="actions">The state changes it makes.</param>
        /// <param name="links">The entries it links to, in order.</param>
        /// <param name="cost">What selecting it costs, in centimes; 0 for a free option.</param>
        /// <param name="costOnce">Whether the cost is charged only once.</param>
        /// <param name="hiddenWhenUnaffordable">
        /// Whether being unable to afford it hides the option rather than disabling it.
        /// </param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="ArgumentOutOfRangeException"><paramref name="cost"/> is negative.</exception>
        public LookAheadNode(
            DialogueNodeId id,
            bool isGroup,
            bool isCheck,
            GuardExpression guard,
            IReadOnlyList<DialogueAction> actions,
            IReadOnlyList<DialogueNodeId> links,
            int cost = 0,
            bool costOnce = false,
            bool hiddenWhenUnaffordable = false)
        {
            if (cost < 0)
            {
                throw new ArgumentOutOfRangeException(nameof(cost));
            }

            Id = id;
            IsGroup = isGroup;
            IsCheck = isCheck;
            Guard = guard ?? throw new ArgumentNullException(nameof(guard));
            Actions = actions ?? throw new ArgumentNullException(nameof(actions));
            Links = links ?? throw new ArgumentNullException(nameof(links));
            Cost = cost;
            CostOnce = costOnce;
            HiddenWhenUnaffordable = hiddenWhenUnaffordable;
        }

        /// <summary>Which entry this is.</summary>
        public DialogueNodeId Id { get; }

        /// <summary>
        /// Whether this is a group: traversed through, never scored, never marked by the
        /// game.
        /// </summary>
        public bool IsGroup { get; }

        /// <summary>
        /// Whether this entry's firing is decided by a skill check rather than by its
        /// condition.
        /// </summary>
        public bool IsCheck { get; }

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
        /// visible but disabled.
        /// </summary>
        public bool HiddenWhenUnaffordable { get; }

        /// <summary>Whether this entry charges anything.</summary>
        public bool IsCostOption => Cost > 0;

        /// <inheritdoc/>
        public override string ToString()
        {
            string kind = IsGroup ? "group" : IsCheck ? "check" : "node";
            string price = IsCostOption ? $", cost {Cost}" : string.Empty;
            return $"{kind} {Id.ConversationId}:{Id.EntryId}{price}";
        }
    }
}
