// SPDX-License-Identifier: MIT
using System.Collections.Generic;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>
    /// Everything outside the dialogue graph that the look-ahead needs to know, read
    /// once at the moment the crawl starts.
    /// </summary>
    /// <remarks>
    /// <para>The look-ahead answers "what can I still reach from HERE", not "what is
    /// reachable in principle", so the player's situation is an input. Reading it once
    /// per crawl rather than per node is deliberate: these values are fixed for the
    /// duration of a response menu, and a per-node call would put game code in the
    /// engine's inner loop.</para>
    ///
    /// <para>The one thing this does NOT cover is state the crawl itself changes -
    /// variables, items, tasks and money all start here and are then tracked by the
    /// search, because a path that spends 50 real must be seen to have spent it.</para>
    /// </remarks>
    public interface ILookAheadWorld
    {
        /// <summary>The player's balance, in centimes, before the crawl.</summary>
        int Money { get; }

        /// <summary>The current value of a dialogue variable.</summary>
        /// <param name="name">The variable's name.</param>
        /// <returns>Its value, or <see cref="GuardValue.Unknown"/> if unset.</returns>
        GuardValue GetVariable(string name);

        /// <summary>Whether the player currently holds an item.</summary>
        /// <param name="name">The item's name.</param>
        bool HasItem(string name);

        /// <summary>Whether a task is currently active.</summary>
        /// <param name="name">The task's name.</param>
        bool IsTaskActive(string name);

        /// <summary>
        /// The result of a world query the crawl cannot change, such as
        /// <c>IsKimHere()</c> or <c>DayCount()</c>.
        /// </summary>
        /// <param name="name">The function's name.</param>
        /// <param name="arguments">Its evaluated arguments.</param>
        /// <returns>
        /// Its result, or <see cref="GuardValue.Unknown"/> for one this host cannot
        /// answer. Unknown never blocks the crawl.
        /// </returns>
        GuardValue Query(string name, IReadOnlyList<GuardValue> arguments);

        /// <summary>
        /// Whether a skill check on this entry passes.
        /// </summary>
        /// <remarks>
        /// Passive checks are deterministic given the character's skills - value plus
        /// modifiers against the entry's <c>DifficultyPass</c>, no dice - so a host that
        /// knows the character can answer definitely. Returning
        /// <see cref="Ternary.Unknown"/> makes the engine explore both outcomes, which is
        /// correct but doubles the branching at that node.
        /// </remarks>
        /// <param name="node">The entry carrying the check.</param>
        Ternary CheckPasses(DialogueNodeId node);
    }
}
