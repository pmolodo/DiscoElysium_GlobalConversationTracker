// SPDX-License-Identifier: MIT
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
    /// <param name="Name">The item's id, which is what a guard names.</param>
    /// <param name="StackName">
    /// What it stacks as, or empty: <c>key_ring</c>, <c>bullets</c>, or nothing.
    /// </param>
    /// <param name="DisplayName">What the game shows it as, in English.</param>
    public sealed record DialogueItem(string Name, string StackName, string DisplayName);
}
