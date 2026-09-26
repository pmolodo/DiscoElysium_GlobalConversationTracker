// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// A white check the game holds as failed, as <c>FailedWhiteChecks.WhiteCheckCache</c> keeps
    /// it.
    /// </summary>
    /// <remarks>
    /// The game offers a failed white check again once <c>difficulty</c> plus the bonuses of the
    /// check's modifiers that hold falls below the target it was failed against. The engine
    /// closes the check and asks exactly that, from these numbers - the game's own, so the
    /// difficulty already carries the game mode's adjustment.
    /// </remarks>
    public sealed class FailedWhiteCheck
    {
        /// <summary>Creates a failed check.</summary>
        /// <param name="flag">The check's <c>FlagName</c>.</param>
        /// <param name="difficulty"><c>WhiteCheck.difficulty</c>: the target before any modifier.</param>
        /// <param name="lastTarget"><c>WhiteCheck.LastTargetValue</c>: the target it failed against.</param>
        public FailedWhiteCheck(string flag, int difficulty, int lastTarget)
        {
            Flag = flag;
            Difficulty = difficulty;
            LastTarget = lastTarget;
        }

        /// <summary>The check's <c>FlagName</c>.</summary>
        public string Flag { get; }

        /// <summary>The target before any modifier.</summary>
        public int Difficulty { get; }

        /// <summary>The target the check was failed against.</summary>
        public int LastTarget { get; }
    }
}
