// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// How far a passive check on a skill damage moves is from flipping.
    /// </summary>
    /// <remarks>
    /// The skill value plus the check's bonus, minus its threshold after thoughts - see
    /// <see cref="PassiveCheck.Margin"/>. Zero or more clears the check. Damage lowers the skill
    /// value and healing raises it, so the engine carries both outcomes of a check only where the
    /// group's damage or healing can cross this.
    /// </remarks>
    public sealed class CheckMargin
    {
        /// <summary>Creates a margin.</summary>
        /// <param name="node">The entry carrying the check.</param>
        /// <param name="skill">The skill it tests, by <c>SkillType</c> name.</param>
        /// <param name="margin">The margin.</param>
        public CheckMargin(NodeRef node, string skill, int margin)
        {
            Node = node;
            Skill = skill;
            Margin = margin;
        }

        /// <summary>The entry carrying the check.</summary>
        public NodeRef Node { get; }

        /// <summary>The skill it tests, by <c>SkillType</c> name.</summary>
        public string Skill { get; }

        /// <summary>The skill value plus the bonus, minus the threshold.</summary>
        public int Margin { get; }
    }
}
