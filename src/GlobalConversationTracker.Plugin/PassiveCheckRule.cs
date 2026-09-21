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
        /// This entry's skill and margin, where it is a passive check no thought forces
        /// through; otherwise null. See <see cref="CheckMargin"/>.
        /// </summary>
        /// <remarks>
        /// <para>EVERY PASSIVE CHECK, not only the two that damage can move. The engine used to
        /// hear a skill for Volition and Endurance alone, because that was all
        /// <c>checks_damage_can_flip</c> needed - and it needs no more now, since it looks the
        /// skill up among the ones the group's own damage moves and a margin for any other
        /// simply does not match. What the rest are for is clothing: a garment moves a named
        /// skill, and nothing could say which checks that reaches while the engine heard no
        /// skill for them. See de-sr1u.4.</para>
        ///
        /// <para>A THOUGHT THAT FORCES THE CHECK THROUGH still yields null. Its outcome is then
        /// not a comparison at all, so there is no margin to state.</para>
        /// </remarks>
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

            string? named = SkillName(skill);
            return named == null
                ? null
                : new CheckMargin(node, named, PassiveCheck.Margin(skillValue, threshold));
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
        /// What the engine calls <paramref name="skill"/>, or null where it names no skill.
        /// </summary>
        /// <remarks>
        /// <para>THE ENUM'S OWN SPELLING, which is the engine's: every member of
        /// <c>SkillType</c> from <c>LOGIC</c> to <c>COMPOSURE</c> is a name
        /// <c>core::thought_effects::SKILLS</c> holds, and `tests/corpus.rs` reads the game's
        /// enum and holds the two lists to each other rather than trusting that.</para>
        ///
        /// <para>THREE ARE NOT SKILLS THE ENGINE HOLDS. <c>NONE</c> and <c>ALT</c> name none at
        /// all. <c>CONVALESCENCE</c> is the game's and not the engine's -
        /// <c>CharacterSheet.GetSkill</c> answers it with Endurance - so it is folded there and
        /// one name serves both.</para>
        /// </remarks>
        private static string? SkillName(SkillType skill)
        {
            switch (skill)
            {
                case SkillType.NONE:
                case SkillType.ALT:
                    return null;
                case SkillType.CONVALESCENCE:
                    return "ENDURANCE";
                default:
                    return skill.ToString();
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
