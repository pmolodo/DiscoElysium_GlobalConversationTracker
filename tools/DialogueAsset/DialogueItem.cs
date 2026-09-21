// SPDX-License-Identifier: MIT
using System.Collections.Generic;

namespace GlobalConversationTracker.DialogueAsset
{
    /// <summary>One item, as answering <c>CheckItem</c> needs it.</summary>
    /// <remarks>
    /// <para>WHAT THE GAME ASKS OF AN ITEM, and nothing else about it.
    /// <c>CharacterItems.IsItemGained</c> - which is all <c>CheckItem</c> does - looks the
    /// item up and branches on its STACK NAME: one stacked as <c>key_ring</c> is held when the
    /// key pocket holds it, one stacked as <c>bullets</c> when the bullet count is above zero,
    /// and anything else when it is in the character's gained items.</para>
    ///
    /// <para>THE DISPLAY NAME IS HERE BECAUSE A SAVE WRITES THE KEY POCKET IN IT. The pocket
    /// is a list of English display names rather than ids, and the game turns them back into
    /// items on load by matching that name - so a reader with only a save cannot say which key
    /// is held without this.</para>
    /// </remarks>
    /// <para>THE GROUP IS A SECOND QUESTION ABOUT THE SAME ITEM. <c>CheckItemGroup</c> asks
    /// whether anything held belongs to a group, and the game answers it by walking the gained
    /// items and comparing <c>ItemUtil.GetItemGroup(item.group)</c> - a lookup of the item's
    /// <c>ItemGroup</c> against a table of names. So an offline reader needs each item's group
    /// by name, and the database records it as a number.</para>
    /// <param name="Name">The item's id, which is what a guard names.</param>
    /// <param name="StackName">
    /// What it stacks as, or empty: <c>key_ring</c>, <c>bullets</c>, or nothing.
    /// </param>
    /// <param name="DisplayName">What the game shows it as, in English.</param>
    /// <param name="Group">
    /// The group it belongs to, by the game's own name for it - <c>alcohol</c>, <c>smokes</c>,
    /// <c>ghb</c>, <c>speed</c>, <c>pyrholidon</c>, <c>tare</c> - or <c>none</c>, which is what
    /// nearly every item is.
    /// </param>
    /// <param name="Bonuses">
    /// What wearing or using it moves, as the database states it - see
    /// <see cref="ItemBonus"/>. Empty for the great majority of items.
    /// </param>
    public sealed record DialogueItem(
        string Name,
        string StackName,
        string DisplayName,
        string Group,
        IReadOnlyList<ItemBonus> Bonuses)
    {
        /// <summary>The same item with no bonuses, for a caller that does not read them.</summary>
        public DialogueItem(string name, string stackName, string displayName, string group)
            : this(name, stackName, displayName, group, System.Array.Empty<ItemBonus>())
        {
        }
    }

    /// <summary>One thing an item moves, and by how much.</summary>
    /// <remarks>
    /// <para>AS THE DATABASE STATES IT, not as the engine names it. The database writes a
    /// bonus in the prose a player reads - <c>+1 Rhetoric: The heroic deeds (of others)</c> -
    /// and what it calls a skill there is neither the engine's <c>SkillType</c> name nor
    /// consistent with itself: <c>Electrochemisty</c> appears beside
    /// <c>Electrochemistry</c>, <c>Reaction</c> beside <c>Reaction Speed</c>.</para>
    ///
    /// <para>NORMALISING IT IS NOT DONE HERE. The engine already holds the one list of
    /// skills - <c>core::thought_effects::SKILLS</c> - and a copy of it over here would be a
    /// second thing to keep in step with the game. So this carries the name verbatim and the
    /// reader maps it. See de-sr1u.2.</para>
    /// </remarks>
    /// <param name="Amount">How far it moves, signed: the database states +1 to +3 and -1 to -4.</param>
    /// <param name="Moves">What it moves, spelled as the database spells it.</param>
    public sealed record ItemBonus(int Amount, string Moves);
}
