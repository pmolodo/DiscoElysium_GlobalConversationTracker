// SPDX-License-Identifier: MIT
using System;
using HarmonyLib;
using PixelCrushers.DialogueSystem;
using GlobalConversationTracker.Session;
using UnityEngine;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The second display hook: dialogue options nobody has ever picked, in any
    /// playthrough, are drawn in their own colour.
    /// </summary>
    /// <remarks>
    /// <para>The game distinguishes two states: an option picked in <i>this</i> save is
    /// drawn in <c>ColorExhausted</c>, everything else in <c>ColorOption</c>. The global
    /// state knows a third thing - whether the option was ever picked in some
    /// <i>other</i> save - so this hook splits the second group. Picked before in any
    /// save keeps the stock <c>ColorOption</c>; never picked anywhere gets the novel
    /// colour. The two states the game already draws are untouched.</para>
    ///
    /// <para>"Picked" means WasDisplayed, not "not Untouched". WasDisplayed is the
    /// game's own definition - <c>SunshineNode.IsSeen</c> is
    /// <c>simStatus.Equals("WasDisplayed")</c>, and that one predicate drives the
    /// greyed-out option colour, the check nodes, the log renderer and the Kim switch.
    /// And WasOffered is already set on everything on screen by the time this runs:
    /// <c>ConversationModel.EvaluateLinksAtPriority</c> marks every option WasOffered as
    /// it builds the response list, before a single button is styled, so a "not
    /// Untouched" test would never fire.</para>
    ///
    /// <para><c>GetData</c> is the hook: the one place that decides a response button's
    /// colours, handed the dialogue entry, and - unlike <c>StyleRegular</c> and
    /// <c>StyleSeen</c>, which IL2CPP inlined into it - a real callable method. A
    /// postfix lands after the game has chosen a colour and before the
    /// <c>ShowNormal</c>/<c>ShowHighlight</c> that applies it. All three callers
    /// (<c>InitialState</c>, <c>Update</c>'s selection-changed path and
    /// <c>OnPointerEnter</c>) go through it.</para>
    ///
    /// <para>Plain options only. Skill checks, cost options, fake checks and hidden test
    /// nodes get their colours from their own branches of <c>GetData</c>, where colour
    /// carries meaning - red for a red check, the money colour for a paid line. This
    /// hook re-tests the same predicates and leaves those branches alone, so it can only
    /// repaint an option the game drew in flat <c>ColorOption</c>.</para>
    ///
    /// <para>Two button classes with the same shape - <c>SunshineResponseButton</c> for
    /// the mouse UI, <c>SunshineResponseButtonPageSystem</c> for the page-system one -
    /// sharing a base that has none of the relevant members, so each gets its own
    /// postfix over the same shared decision.</para>
    ///
    /// <para>Read-only: one dictionary read and a colour assignment. A failure costs the
    /// display, never the playthrough.</para>
    ///
    /// <para>With <c>MarkNovelOptions</c> false the hook is not installed at all, so the
    /// game's own <c>GetData</c> runs undetoured. Tracking is unaffected either way. The
    /// switch is global rather than per save, because the mod has no notion of which
    /// save is loaded - the current-save tally is an anonymous in-memory counter.</para>
    /// </remarks>
    internal static class NovelResponseColorPatch
    {
        /// <summary>
        /// The colour given to an option no save has ever picked, as an HTML colour
        /// string. A warm orange: brighter than the stock option colour, which is a
        /// pale desaturated green-white, so newness reads at a glance without looking
        /// like one of the game's own check colours.
        /// </summary>
        internal const string DefaultNovelColorHtml = "#FF8C42";

        private static GlobalStateSession? _session;
        private static HookFailureLimiter? _failures;
        private static Color _novelColor;

        /// <summary>
        /// Applies the patch. Call once, from plugin load, after the session exists.
        /// </summary>
        /// <param name="harmony">The plugin's Harmony instance.</param>
        /// <param name="session">The session the global statuses are read from.</param>
        /// <param name="log">Where hook failures are reported.</param>
        /// <param name="novelColorHtml">
        /// The colour for never-picked options, as an HTML colour string
        /// (<c>#RRGGBB</c>, <c>#RRGGBBAA</c> or a name Unity knows).
        /// </param>
        /// <exception cref="ArgumentNullException">Any argument is null.</exception>
        /// <exception cref="ArgumentException">The colour string is not parseable.</exception>
        /// <exception cref="Exception">
        /// Harmony could not patch a method. Nothing is hooked either way.
        /// </exception>
        internal static void Install(
            Harmony harmony,
            GlobalStateSession session,
            IGlobalStateLog log,
            string novelColorHtml)
        {
            if (harmony == null)
            {
                throw new ArgumentNullException(nameof(harmony));
            }

            if (log == null)
            {
                throw new ArgumentNullException(nameof(log));
            }

            _session = session ?? throw new ArgumentNullException(nameof(session));
            _failures = new HookFailureLimiter(
                "colouring never-before-picked dialogue options", log);
            _novelColor = ParseColor(novelColorHtml);

            harmony.PatchAll(typeof(MouseButtonGetDataPatch));
            harmony.PatchAll(typeof(PageSystemButtonGetDataPatch));
        }

        /// <summary>
        /// Turns a configured colour string into a colour, refusing anything it cannot
        /// read rather than quietly falling back.
        /// </summary>
        /// <remarks>
        /// A silent fallback would be indistinguishable from the hook not working: the
        /// player would see the stock colour with no way to tell whether the typo was in
        /// the config or the mod was broken.
        /// </remarks>
        private static Color ParseColor(string html)
        {
            if (string.IsNullOrWhiteSpace(html))
            {
                throw new ArgumentException("The colour must not be empty.", nameof(html));
            }

            if (!ColorUtility.TryParseHtmlString(html, out Color color))
            {
                throw new ArgumentException(
                    $"'{html}' is not a colour Unity can read. Use #RRGGBB, #RRGGBBAA, or a "
                    + "colour name such as 'orange'.",
                    nameof(html));
            }

            return color;
        }

        /// <summary>
        /// Decides whether this entry's button should be repainted, and with what.
        /// </summary>
        /// <remarks>
        /// One try/catch around the whole decision: every step reaches into game code -
        /// the node predicates and <c>SunshineNode.IsSeen</c> all run Lua - and there is
        /// nothing useful to do with a failure except stop colouring.
        /// </remarks>
        /// <param name="entry">The response's destination entry, as GetData got it.</param>
        /// <param name="color">The colour to paint, when the result is true.</param>
        /// <returns>True if the button should be repainted.</returns>
        private static bool ShouldPaintAsNovel(DialogueEntry entry, out Color color)
        {
            color = _novelColor;

            GlobalStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if (session == null || failures == null || failures.HasGivenUp)
            {
                return false;
            }

            try
            {
                if (entry == null)
                {
                    return false;
                }

                // Anything the game styles from a branch of its own keeps that branch's
                // colours; only the flat ColorOption case is ours to repaint.
                if (RedCheckNode.IsRedCheckNode(entry)
                    || WhiteCheckNode.IsWhiteCheckNode(entry)
                    || FakeCheckNode.IsFakeCheckNode(entry)
                    || TestOptionNode.IsTestOptionNode(entry)
                    || CostOptionNode.IsCostOptionNode(entry))
                {
                    return false;
                }

                // Picked in this save: the game has already drawn it exhausted, and that
                // is the strongest of the three states. Leave it.
                if (SunshineNode.IsSeen(entry))
                {
                    return false;
                }

                // Picked in some other save: stock colour, which is what the player has
                // always seen for an unpicked option.
                SimStatus global = session.EnsureInitialized()
                    .GetStatus(entry.conversationID, entry.id);
                return global != SimStatus.WasDisplayed;
            }
            catch (Exception ex)
            {
                failures.Report(ex);
                return false;
            }
        }

        /// <summary>The mouse UI's response button.</summary>
        [HarmonyPatch(typeof(SunshineResponseButton), nameof(SunshineResponseButton.GetData))]
        private static class MouseButtonGetDataPatch
        {
            /// <summary>
            /// The parameter name is matched against the patched method by Harmony, so
            /// it has to stay <c>entry</c>.
            /// </summary>
            [HarmonyPostfix]
            private static void Postfix(SunshineResponseButton __instance, DialogueEntry entry)
            {
                if (ShouldPaintAsNovel(entry, out Color color))
                {
                    __instance.regularColor = color;
                }
            }
        }

        /// <summary>The page-system UI's response button.</summary>
        [HarmonyPatch(
            typeof(SunshineResponseButtonPageSystem),
            nameof(SunshineResponseButtonPageSystem.GetData))]
        private static class PageSystemButtonGetDataPatch
        {
            [HarmonyPostfix]
            private static void Postfix(SunshineResponseButtonPageSystem __instance, DialogueEntry entry)
            {
                if (ShouldPaintAsNovel(entry, out Color color))
                {
                    __instance.regularColor = color;
                }
            }
        }
    }
}
