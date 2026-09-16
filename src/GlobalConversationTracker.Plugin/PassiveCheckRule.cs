// SPDX-License-Identifier: MIT
using GlobalConversationTracker.Engine;
using PixelCrushers.DialogueSystem;
using Sunshine.Metric;

namespace GlobalConversationTracker
{
    /// <summary>
    /// Decides whether a passive skill check fires, the way the game decides it.
    /// </summary>
    /// <remarks>
    /// <para>Mirrors <c>PassiveNode.CheckSuccess</c>, which
    /// <c>ReturnDialogueOptionValidator.IsEntryValid</c> calls for every entry carrying a
    /// <c>DifficultyPass</c> field - 10,500 of them in the shipped database. There is no
    /// dice roll: the outcome is a function of the character's skills, the thoughts they
    /// are running, and the entry's difficulty, which is what makes a look-ahead from the
    /// current state able to give a definite answer instead of exploring both branches.
    /// </para>
    ///
    /// <para>The parts that could drift are the game's own and are called rather than
    /// copied - <c>ArticyBridge.DifficultyIdToThreshold</c> and the two
    /// <c>ThoughtAlterant</c> hooks, all of which are pure reads. This class only gathers
    /// their inputs; the comparison and the antipassive inversion live in
    /// <see cref="PassiveCheck"/>, where they can be tested without the game.
    /// <c>CheckSuccess</c> itself cannot simply be called: it writes
    /// <c>falseConditionAction</c> on the entry and logs an error for a non-skill actor,
    /// and a look-ahead touches thousands of nodes per menu.</para>
    ///
    /// <para>Anything it cannot determine comes back <see cref="Ternary.Unknown"/>, which
    /// makes the engine carry both outcomes - more markers than earned, never fewer.</para>
    /// </remarks>
    internal static class PassiveCheckRule
    {
        /// <summary>The field whose presence makes an entry a passive check.</summary>
        internal const string DifficultyPassField = "DifficultyPass";

        /// <summary>
        /// The field marking a check that fires when it FAILS rather than when it passes.
        /// </summary>
        internal const string AntipassiveField = "Antipassive";

        /// <summary>Whether this entry's passive check fires.</summary>
        /// <param name="entry">The entry carrying the check.</param>
        /// <returns>
        /// True or False when the character is known, <see cref="Ternary.Unknown"/> when
        /// the skill or the character sheet cannot be resolved.
        /// </returns>
        internal static Ternary Evaluate(DialogueEntry? entry)
        {
            if (!TryRead(entry, out _, out int skillValue, out int threshold) || entry == null)
            {
                return Ternary.Unknown;
            }

            bool antipassive = Field.FieldExists(entry.fields, AntipassiveField);

            // A thought can force a passive through regardless of the numbers, so it is
            // applied to the comparison's result rather than folded into the threshold.
            if (ThoughtAlterant.PassiveSuccess(false, entry))
            {
                return antipassive ? Ternary.False : Ternary.True;
            }

            return PassiveCheck.Outcome(skillValue, threshold, antipassive);
        }

        /// <summary>
        /// This entry's margin, where it is a check on a skill damage moves and no thought
        /// forces it through; otherwise null. See <see cref="CheckMargin"/>.
        /// </summary>
        /// <param name="entry">The entry carrying the check.</param>
        /// <param name="node">The entry's id, as the margin names it.</param>
        internal static CheckMargin? MarginOf(DialogueEntry? entry, NodeRef node)
        {
            if (!TryRead(entry, out SkillType skill, out int skillValue, out int threshold)
                || entry == null
                || ThoughtAlterant.PassiveSuccess(false, entry))
            {
                return null;
            }

            string? damaged = DamagedSkillName(skill);
            return damaged == null
                ? null
                : new CheckMargin(node, damaged, PassiveCheck.Margin(skillValue, threshold));
        }

        /// <summary>
        /// The skill a check tests, the character's value in it, and its threshold after
        /// thoughts - or false where any of them cannot be resolved.
        /// </summary>
        private static bool TryRead(
            DialogueEntry? entry,
            out SkillType skill,
            out int skillValue,
            out int threshold)
        {
            skill = SkillType.NONE;
            skillValue = 0;
            threshold = 0;
            if (entry == null)
            {
                return false;
            }

            skill = ArticyBridge.ActorIdToSkillType(entry.ActorID);
            if (skill == SkillType.NONE)
            {
                // The game logs an error here. We are speculating about a node the player
                // may never reach, so an Unknown is the honest answer and a quiet one.
                return false;
            }

            CharacterSheet? character = PlayerSheet();
            if (character == null)
            {
                return false;
            }

            threshold = ThoughtAlterant.ModifyPassiveTargetValue(
                ArticyBridge.DifficultyIdToThreshold(
                    Field.LookupInt(entry.fields, DifficultyPassField)),
                entry,
                skill);
            skillValue = character.GetSkillValue(skill);
            return true;
        }

        /// <summary>
        /// The name the engine's damage slots use for a skill damage moves, or null for any
        /// other skill. <c>CharacterSheet.GetSkill</c> answers Convalescence with Endurance.
        /// </summary>
        private static string? DamagedSkillName(SkillType skill)
        {
            switch (skill)
            {
                case SkillType.VOLITION:
                    return "VOLITION";
                case SkillType.ENDURANCE:
                case SkillType.CONVALESCENCE:
                    return "ENDURANCE";
                default:
                    return null;
            }
        }

        /// <summary>
        /// The player's character sheet, or null before the world exists.
        /// </summary>
        /// <remarks>
        /// Not cached: a sheet is only as good as the moment it is read, skills move with
        /// thoughts and clothing, and this is one property read per crawl.
        /// </remarks>
        private static CharacterSheet? PlayerSheet()
        {
            World? world = World.Singleton;
            return world == null ? null : world.you;
        }
    }
}
