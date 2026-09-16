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
    }
}
