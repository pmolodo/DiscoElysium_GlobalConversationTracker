// SPDX-License-Identifier: MIT
using System;
using GlobalConversationTracker.Session;
using HarmonyLib;
using PixelCrushers.DialogueSystem;
using TMPro;
using UnityEngine;
using UnityEngine.UI;

namespace GlobalConversationTracker
{
    /// <summary>
    /// Shortens a check's coloured band so the Pass / Fail line under it is drawn on the
    /// menu's own background instead of on the check's colour.
    /// </summary>
    /// <remarks>
    /// <para>de-8hh2.15, and the first thing that has worked. MEASURED IN THE GAME
    /// 2026-09-05, both halves out of the same screenshot: on a red check the words are on
    /// #0D0D0D at 8.35:1 where they were on #D7431B at 1.92:1, and on a white check on
    /// #0C0C0B at 8.36:1 where they were on #857F70 at 1.71:1. Dark red is the exception
    /// and gains nothing - 2.09:1 either way - because it is deliberately dim; what this
    /// buys is that a check's line now reads exactly as the same line reads on an ordinary
    /// option, which is all the palette was ever designed for.</para>
    ///
    /// <para>The defect: the line's three word colours are chosen against the black an
    /// option is drawn on, where they score 9.08 / 4.26 / 18.76, and a check draws them on
    /// its own band instead - #D7431B for a red check, #857F70 for a white one - where the
    /// best of them manages 1.93. Every way of fixing that from inside the text has been
    /// tried and measured: a mark tag draws its quad IN FRONT of the glyphs, voffset is
    /// followed by the box rather than escaping it, TMP has no outline tag, and no palette
    /// clears both bands without collapsing red and dark red into the same colour.</para>
    ///
    /// <para>So the line does not move; the BAND does. The prefab says this is possible -
    /// <c>Response Line (Button)</c>, whose <c>Image</c> child carries
    /// <c>LayoutElement.m_IgnoreLayout: 1</c> and anchors of (0,0)-(1,1) with a zero size
    /// delta. It stretches to the button's rect and the layout system cannot see it, so
    /// raising its bottom edge shortens what is painted and moves nothing else. The row's
    /// height is the layout group's business, computed from the TEXT's preferred height,
    /// which already includes the line. That is the whole idea: the reserved area is
    /// untouched, and the last line falls out of the band onto the menu's black.</para>
    ///
    /// <para>DRIVEN FROM <c>Update</c> rather than <c>InitialState</c>. It costs a float
    /// comparison per button per frame and buys three things. The offset is re-applied
    /// after anything the game does to the rect, so nothing has to be known about what
    /// <c>RebuildLayout</c> or a pooled button's <c>ResetState</c> writes. The text has been
    /// laid out by the time it runs, so the inset is the LAST LINE'S OWN measured height
    /// rather than a guess off the font size - and the font size is the player's to change.
    /// And a button reused for an ordinary option clears its own inset on the next frame,
    /// which is the whole of pooling handled by the same property.</para>
    ///
    /// <para>WHAT THAT LEAVES UNPROVEN, deliberately: whether the offset would SURVIVE if it
    /// were set once. Every run this has been through re-applied it every frame, so a
    /// cheaper <c>InitialState</c> version is not a simplification anyone has earned - it
    /// would need its own run against a menu that scrolls and a button that is reused.</para>
    ///
    /// <para>Two button classes with the same shape - <c>SunshineResponseButton</c> for the
    /// mouse UI, <c>SunshineResponseButtonPageSystem</c> for the page-system one - sharing a
    /// base that has none of the members this needs. Each carries its own
    /// <c>imageComponent</c>, <c>optionText</c>, <c>redCheck</c>, <c>whiteCheck</c> and
    /// <c>Update</c>, so each gets its own postfix handing the same shared decision what it
    /// found. The same arrangement <see cref="NovelResponseColorPatch"/> is in.</para>
    /// </remarks>
    internal static class CheckBandInsetPatch
    {
        /// <summary>The child of the button prefab carrying the coloured band.</summary>
        private const string BandChildName = "Image";

        /// <summary>
        /// How much of a line to leave below the band, as a fraction of that line's height.
        /// </summary>
        /// <remarks>
        /// Zero would put the band's edge exactly on the line's top. A little more clears
        /// the ascenders of the line above it as well, which is what stops the band looking
        /// like it is resting on the words rather than ending above them.
        /// </remarks>
        private const float LineClearance = 0.1f;

        /// <summary>Below this, a rect is treated as already carrying the offset.</summary>
        private const float SamePixel = 0.5f;

        private static HookFailureLimiter? _failures;
        private static IGlobalStateLog? _log;

        /// <summary>Applies the patch. Call once, from plugin load.</summary>
        /// <param name="harmony">The plugin's Harmony instance.</param>
        /// <param name="log">Where hook failures are reported.</param>
        /// <exception cref="ArgumentNullException">Any argument is null.</exception>
        /// <exception cref="Exception">
        /// Harmony could not patch the method. Nothing is hooked either way.
        /// </exception>
        internal static void Install(Harmony harmony, IGlobalStateLog log)
        {
            if (harmony == null)
            {
                throw new ArgumentNullException(nameof(harmony));
            }

            _log = log ?? throw new ArgumentNullException(nameof(log));
            _failures = new HookFailureLimiter(
                "shortening a check's band so its Pass / Fail line is readable", log);

            harmony.PatchAll(typeof(MouseButtonUpdatePatch));
            harmony.PatchAll(typeof(PageSystemButtonUpdatePatch));
        }

        /// <summary>
        /// Puts one button's band where it belongs - shortened under a check that carries a
        /// line, full height under anything else.
        /// </summary>
        /// <param name="owner">The button, for the band child if the field is empty.</param>
        /// <param name="image">The button's own band image, which may be null.</param>
        /// <param name="optionText">The button's text pair, which may be null.</param>
        /// <param name="isCheck">Whether the game drew this option as a rolled check.</param>
        /// <param name="entry">The option's destination, which says whether it has a line.</param>
        private static void Apply(
            Transform owner,
            Image image,
            Sunshine.ResponseText optionText,
            bool isCheck,
            DialogueEntry entry)
        {
            HookFailureLimiter? failures = _failures;
            if (failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                RectTransform? band = BandOf(owner, image);
                if (band is null)
                {
                    return;
                }

                Inset(band, WantedInset(optionText, isCheck, entry));
            }
            catch (Exception ex)
            {
                failures.Report(ex);
            }
        }

        /// <summary>How far this button's band should be lifted off its bottom edge.</summary>
        /// <remarks>
        /// Zero for everything that is not a check drawing a line, which is most of a menu.
        /// The two questions are asked in the cheap order: what kind of node it is, which is
        /// a field the game has already filled in, before what the look-ahead said about it.
        /// </remarks>
        private static float WantedInset(
            Sunshine.ResponseText optionText, bool isCheck, DialogueEntry entry)
        {
            if (!isCheck)
            {
                return 0f;
            }

            if (!ResponseLookAheadPatch.HasBranchLine(entry))
            {
                return 0f;
            }

            TextMeshProUGUI? text = TextOf(optionText);
            if (text is null)
            {
                return 0f;
            }

            TMP_TextInfo info = text.textInfo;
            if (info == null || info.lineCount < 2)
            {
                // One line means the line has not been laid out yet - the text is set
                // and the mesh has not caught up - and shortening the band to nothing
                // is worse than leaving it whole for a frame.
                return 0f;
            }

            float lineHeight = info.lineInfo[info.lineCount - 1].lineHeight;
            return lineHeight > 0f ? lineHeight * (1f + LineClearance) : 0f;
        }

        /// <summary>Lifts a band's bottom edge, or puts it back, saying so the first time.</summary>
        private static void Inset(RectTransform band, float inset)
        {
            Vector2 offset = band.offsetMin;
            if (Mathf.Abs(offset.y - inset) < SamePixel)
            {
                return;
            }

            band.offsetMin = new Vector2(offset.x, inset);
            _log?.Info(
                $"Check band inset: {offset.y:0.##} -> {inset:0.##} on '{band.parent?.name}', "
                + $"whose rect is {band.rect.width:0.##} x {band.rect.height:0.##}.");
        }

        /// <summary>The rect of the child that paints the band, or null if it has moved.</summary>
        private static RectTransform? BandOf(Transform owner, Image image)
        {
            if (image is null && owner is not null)
            {
                Transform child = owner.Find(BandChildName);
                image = child is null ? null! : child.GetComponent<Image>();
            }

            return image is null ? null : image.rectTransform;
        }

        /// <summary>The text the option and its line are drawn in, or null.</summary>
        private static TextMeshProUGUI? TextOf(Sunshine.ResponseText optionText) =>
            optionText is null ? null : optionText.TextField;

        /// <summary>The mouse UI's response button, on every frame it draws.</summary>
        [HarmonyPatch(typeof(SunshineResponseButton), "Update")]
        private static class MouseButtonUpdatePatch
        {
            [HarmonyPostfix]
            private static void Postfix(SunshineResponseButton __instance)
            {
                if (__instance != null)
                {
                    Apply(
                        __instance.transform,
                        __instance.imageComponent,
                        __instance.optionText,
                        __instance.redCheck || __instance.whiteCheck,
                        __instance.entry);
                }
            }
        }

        /// <summary>The page-system UI's response button, on every frame it draws.</summary>
        [HarmonyPatch(typeof(SunshineResponseButtonPageSystem), "Update")]
        private static class PageSystemButtonUpdatePatch
        {
            [HarmonyPostfix]
            private static void Postfix(SunshineResponseButtonPageSystem __instance)
            {
                if (__instance != null)
                {
                    Apply(
                        __instance.transform,
                        __instance.imageComponent,
                        __instance.optionText,
                        __instance.redCheck || __instance.whiteCheck,
                        __instance.entry);
                }
            }
        }
    }
}
