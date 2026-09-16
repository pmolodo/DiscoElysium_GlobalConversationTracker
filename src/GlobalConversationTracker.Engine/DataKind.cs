// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.Engine
{
    /// <summary>The vocabulary of things the engine can ask to have read.</summary>
    /// <remarks>
    /// <para>AN ENUM RATHER THAN A STRING, so a kind spelled wrong on one side is a compile
    /// error rather than a request that comes back unserviced - which would read Unknown,
    /// permissive and silent, and be indistinguishable from a plugin that genuinely could
    /// not read the game.</para>
    ///
    /// <para>Mirrors <c>DataKind</c> in <c>proto/engine.proto</c>, which both sides generate
    /// from. Kept as a domain type of its own rather than the generated one for the same
    /// reason <see cref="WireValue"/> and <see cref="NodeRef"/> are: what crosses the wire
    /// and what the engine's callers speak are allowed to move separately.</para>
    /// </remarks>
    public enum DataKind
    {
        /// <summary>A kind this build does not know, which cannot be serviced.</summary>
        Unspecified = 0,

        /// <summary>
        /// The thoughts the cabinet is working on - <c>CharacterThoughts.cookingEffects</c>.
        /// </summary>
        ThoughtsCooking = 1,

        /// <summary>
        /// The thoughts already internalised - <c>CharacterThoughts.fixedEffects</c>.
        /// </summary>
        ThoughtsFixed = 2,

        /// <summary>
        /// The item in one equipment slot - <c>InventoryViewData.GetEquipped</c> - with the
        /// slot named by its <c>EquipmentSlotType</c> name in <see cref="DataRequest.Subject"/>.
        /// Answered as text: the item's name, or empty for an empty slot.
        /// </summary>
        EquippedInSlot = 3,

        /// <summary>
        /// Whether one inventory tab holds anything - the negation of
        /// <c>InventoryViewData.IsTabEmpty</c> - with the tab named by its <c>ItemTabGroup</c>
        /// name in <see cref="DataRequest.Subject"/>. Answered as a boolean.
        /// </summary>
        TabHoldsItems = 4,

        /// <summary>
        /// Every item the dialogue database files under one <c>ItemGroup</c>, named in
        /// <see cref="DataRequest.Subject"/>. Answered as a set of item names.
        /// </summary>
        ItemsInGroup = 5,

        /// <summary>
        /// The items of one <c>ItemGroup</c> the player holds, named in
        /// <see cref="DataRequest.Subject"/>. Answered as a set of item names.
        /// </summary>
        HeldItemsInGroup = 6,

        /// <summary>
        /// Whether the current scene is outdoors -
        /// <c>ApplicationManager.CurrentSceneProperties.IsOutside</c>. Names no subject; answered
        /// as a boolean.
        /// </summary>
        SceneIsOutside = 7,

        /// <summary>
        /// A skill's damage value - <c>World.you.volition.damageValue</c> and its sibling -
        /// with the skill named by its <c>SkillType</c> name in
        /// <see cref="DataRequest.Subject"/>. Answered as a number.
        /// </summary>
        SkillDamage = 8,

        /// <summary>
        /// The game mode - <c>GameModeController.currentMode</c> - by its enum name. Names no
        /// subject; answered as text.
        /// </summary>
        GameMode = 9,

        /// <summary>
        /// Whether a game has been finished in hardcore mode -
        /// <c>GameStatsManager.HardcorePlaythroughCompleted</c>. Names no subject; answered as a
        /// boolean.
        /// </summary>
        HardcorePlaythroughCompleted = 10,
    }
}
