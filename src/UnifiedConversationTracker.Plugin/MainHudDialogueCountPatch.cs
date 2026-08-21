using System;
using System.Globalization;
using System.IO;
using System.Reflection;
using HarmonyLib;
using Il2CppInterop.Runtime.InteropTypes.Arrays;
using TMPro;
using UnifiedConversationTracker.Session;
using UnityEngine;
using UnityEngine.UI;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// The display hook: the main HUD carries the number of dialogue entries reached
    /// across every save, in the gap between the thought cabinet button and the
    /// money/time panel at the bottom of the screen.
    /// </summary>
    /// <remarks>
    /// <para><b>Why the HUD and not the character sheet.</b> The character sheet is
    /// gated behind two pieces of early-game progression - picking up the ledger, and
    /// reading it under halogen lights - so a count shown there is invisible for the
    /// first stretch of a playthrough. The main HUD is up from the first frame of
    /// gameplay, which is what this display needs.</para>
    ///
    /// <para><b>What the HUD actually is.</b> Not the <c>HUD Page</c> prefab, whose
    /// money subtree has no controller on it at all and is inert; the live HUD is the
    /// one built into the <c>Init</c> scene, under
    /// <c>Global UI Canvas/Global UI Fitter</c>. The relevant object there is
    /// <c>HUD Money Time</c>: a 316.53 x 123.7 panel pinned to the bottom-right corner
    /// of the fitter, holding the money display, the clock, the day counter and the
    /// hand-equip slots. Everything this patch measures is measured off that panel at
    /// runtime rather than assumed, because the fitter's size depends on the screen's
    /// aspect ratio and none of these numbers are fixed in screen pixels.</para>
    ///
    /// <para><b>Why <c>HudMoneyController.Start</c> is the trigger.</b> It is a Unity
    /// message, so IL2CPP cannot inline it away - the engine invokes it by name - and
    /// it hands over the money display itself as <c>__instance</c>, whose parent is
    /// the panel and whose own children carry the exact font, size and colour the HUD
    /// uses for a number. One call, one attach, everything needed in hand. The
    /// alternatives were all worse: <c>ClockToggler</c> keeps its instance in a
    /// private static, and <c>HudController</c> belongs to a HUD hierarchy that is
    /// not the one on screen.</para>
    ///
    /// <para><b>Where the text goes.</b> The panel's own left edge is the right-hand
    /// wall of the gap this display sits in, so the text is placed against that edge
    /// with a right-hand pivot and grows leftwards, towards the thought cabinet
    /// button, on the money's own centre line.</para>
    ///
    /// <para><b>Why it is a child of the money display and not of the panel.</b>
    /// Because that is what makes it disappear at the right times. The HUD does not
    /// hide itself as a panel: each element - the money, the clock, the held items -
    /// carries its own <c>CanvasGroup</c> and its own alpha tween, and the panel they
    /// share is left alone. A child of the panel would therefore stay on screen
    /// through every dialogue, cutscene and menu that fades the rest of the HUD out.
    /// A child of the money display inherits its group, so the count fades exactly
    /// when the number beside it does.</para>
    ///
    /// <para><b>Why the speech bubble is a texture and not a character.</b> Because
    /// there is no character to use. Every font asset the game ships is a static SDF
    /// atlas with no source font behind it, so nothing can be added at runtime, and
    /// the highest codepoint in any of them is U+FF70 - emoji start at U+1F300. The
    /// project's TMP sprite asset, which is what would otherwise stand in, is
    /// TextMesh Pro's own EmojiOne sample: fourteen smileys and no bubble. With
    /// <c>m_missingGlyphCharacter: 0</c> and warnings off in TMP Settings, an emoji
    /// character would therefore draw as nothing at all and say nothing about it. So
    /// the icon is a flat white PNG embedded in the plugin, drawn through an
    /// <see cref="Image"/> and tinted to the count's own colour.</para>
    ///
    /// <para><b>Read-only, and cheap.</b> The number is
    /// <see cref="UnifiedConversationState.EntryCount"/>, already the count of entries
    /// above Untouched across all saves - Untouched is never stored - so there is
    /// nothing to count and nothing to enumerate. Nothing polls: the count can only
    /// change when a mark is recorded or a savegame is loaded, and both of those hooks
    /// call <see cref="RefreshDisplayedCount"/> on their way out. As with every other
    /// hook, a failure here costs the display and never the playthrough.</para>
    /// </remarks>
    internal static class MainHudDialogueCountPatch
    {
        /// <summary>
        /// How far left of the money/time panel's left edge the count's right-hand
        /// edge sits, in canvas units. Negative is left. The measured gap between that
        /// edge and the thought cabinet button is about 117 units, so this leaves room
        /// for six or seven digits before anything collides.
        /// </summary>
        internal const float DefaultOffsetX = -10f;

        /// <summary>
        /// How far the count sits above (positive) or below (negative) the money
        /// display's own centre line, in canvas units. Zero puts the two numbers on
        /// one line, which is the point.
        /// </summary>
        internal const float DefaultOffsetY = 0f;

        /// <summary>
        /// The name given to the object the count is drawn on. Long and explicit
        /// because it appears in the game's own hierarchy, where anything ambiguous
        /// would look like something the game shipped.
        /// </summary>
        private const string DisplayObjectName = "UnifiedConversationTracker Dialogue Count";

        /// <summary>
        /// Group separators, no decimals: the count runs into five figures, and the
        /// money display beside it is grouped the same way.
        /// </summary>
        private const string CountFormat = "N0";

        /// <summary>
        /// How wide the count's own rect is, in canvas units. The text is right
        /// aligned and does not wrap, so this is headroom to grow leftwards into and
        /// not a box anything is fitted to.
        /// </summary>
        private const float RectWidth = 240f;

        /// <summary>
        /// The height used when the money display's rect cannot supply one. Only
        /// affects where the text sits vertically inside its own rect, which is
        /// centred either way.
        /// </summary>
        private const float FallbackRectHeight = 48f;

        /// <summary>
        /// The file the speech-bubble icon is embedded in the plugin under. Matched by
        /// suffix at runtime, so the manifest prefix MSBuild chooses does not matter.
        /// </summary>
        private const string IconResourceFileName = "dialogue-count-icon.png";

        /// <summary>The name given to the object the icon is drawn on.</summary>
        private const string IconObjectName = "UnifiedConversationTracker Dialogue Icon";

        /// <summary>
        /// How tall the icon's box is as a multiple of the count's font size. The
        /// bubble fills about five-sixths of that box, which puts it a little taller
        /// than the digits beside it - the same proportion an emoji would have.
        /// </summary>
        private const float IconHeightInFontSizes = 1f;

        /// <summary>The gap between the icon and the first digit, in canvas units.</summary>
        private const float IconGap = 6f;

        private static UnifiedStateSession? _session;
        private static HookFailureLimiter? _failures;
        private static IUnifiedStateLog? _log;
        private static float _offsetX;
        private static float _offsetY;

        private static TextMeshProUGUI? _display;
        private static RectTransform? _icon;
        private static Sprite? _iconSprite;
        private static int _displayedCount = -1;

        /// <summary>
        /// Applies the patch. Call once, from plugin load, after the session exists.
        /// </summary>
        /// <param name="harmony">The plugin's Harmony instance.</param>
        /// <param name="session">The session the displayed count is read from.</param>
        /// <param name="log">Where hook failures are reported.</param>
        /// <param name="offsetX">
        /// Horizontal placement, as <see cref="DefaultOffsetX"/> describes it.
        /// </param>
        /// <param name="offsetY">
        /// Vertical placement, as <see cref="DefaultOffsetY"/> describes it.
        /// </param>
        /// <exception cref="ArgumentNullException">Any argument is null.</exception>
        /// <exception cref="Exception">
        /// Harmony could not patch the method - it was not found, or the detour
        /// failed. The caller decides what that means; the HUD is unchanged either
        /// way.
        /// </exception>
        internal static void Install(
            Harmony harmony,
            UnifiedStateSession session,
            IUnifiedStateLog log,
            float offsetX,
            float offsetY)
        {
            if (harmony == null)
            {
                throw new ArgumentNullException(nameof(harmony));
            }

            _session = session ?? throw new ArgumentNullException(nameof(session));
            _log = log ?? throw new ArgumentNullException(nameof(log));
            _failures = new HookFailureLimiter(
                "showing the across-all-saves dialogue count on the main HUD", log);
            _offsetX = offsetX;
            _offsetY = offsetY;

            harmony.PatchAll(typeof(MoneyStartPatch));
        }

        /// <summary>
        /// Brings the displayed number up to date with the session, if it is out of
        /// date and if there is anything on screen to update.
        /// </summary>
        /// <remarks>
        /// <para><b>This never throws.</b> It is called from inside the two tracking
        /// hooks, which have their own failure budgets to spend on tracking; a display
        /// that cannot draw itself must not be able to spend theirs, so this reports
        /// through its own limiter and returns.</para>
        ///
        /// <para><b>And it is called rather than polled.</b> The count changes exactly
        /// when a mark is recorded or a savegame is resynced, so those two callers see
        /// every change there is. Between them the display costs nothing per frame.</para>
        /// </remarks>
        internal static void RefreshDisplayedCount()
        {
            TextMeshProUGUI? display = _display;
            UnifiedStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if (display is null || session == null || failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                Write(display, session.EnsureInitialized().EntryCount);
            }
            catch (Exception ex)
            {
                failures.Report(ex);
            }
        }

        /// <summary>
        /// Postfixes the money display's own startup, which is where the live HUD
        /// hands over both the panel to hang the count off and the text to copy.
        /// </summary>
        /// <remarks>
        /// The method is named by string rather than by <c>nameof</c> because
        /// <c>Start</c> is private in the game's own source; the interop assembly
        /// happens to publish it as public, but Harmony finds it either way and this
        /// does not depend on which.
        /// </remarks>
        [HarmonyPatch(typeof(HudMoneyController), "Start")]
        private static class MoneyStartPatch
        {
            [HarmonyPostfix]
            private static void Postfix(HudMoneyController __instance)
            {
                Attach(__instance);
            }
        }

        /// <summary>
        /// Builds the count's text object beside the given money display, replacing
        /// any earlier one.
        /// </summary>
        private static void Attach(HudMoneyController money)
        {
            UnifiedStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            IUnifiedStateLog? log = _log;
            if (session == null || failures == null || log == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                if (money is null)
                {
                    return;
                }

                RectTransform moneyRect = money.GetComponent<RectTransform>();
                if (moneyRect is null || moneyRect.parent is null)
                {
                    log.Warning(
                        "The HUD money display is not a child rect the way the Init scene builds it, so "
                        + "there is nowhere to put the dialogue count. The HUD is unchanged.");
                    return;
                }

                RectTransform panel = moneyRect.parent.GetComponent<RectTransform>();
                if (panel is null)
                {
                    log.Warning(
                        "The HUD money display's parent is not a rect, so the dialogue count has no panel "
                        + "edge to sit beside. The HUD is unchanged.");
                    return;
                }

                // Taken from the money display's own flip clock, which is the number
                // this one is meant to look like a sibling of. Copying beats guessing:
                // the font asset is whichever one the current language loaded.
                TextMeshProUGUI donor = moneyRect.GetComponentInChildren<TextMeshProUGUI>(true);
                if (donor is null)
                {
                    log.Warning(
                        "The HUD money display has no text component to copy a font from, so the dialogue "
                        + "count would be invisible. The HUD is unchanged.");
                    return;
                }

                // Start() can only run once per HUD, but a rebuilt HUD would run it
                // again; leaving the old object behind would stack up copies.
                Transform stale = moneyRect.Find(DisplayObjectName);
                if (stale is not null)
                {
                    UnityEngine.Object.Destroy(stale.gameObject);
                }

                TextMeshProUGUI display = Build(panel, moneyRect, donor);
                _display = display;
                _icon = BuildIcon(display, log);
                _displayedCount = -1;
                Write(display, session.EnsureInitialized().EntryCount);

                RectTransform placed = display.rectTransform;
                log.Info(
                    $"Dialogue count added to the main HUD, under '{panel.name}/{moneyRect.name}' at "
                    + $"{placed.anchoredPosition.ToString()}, {(-_offsetX).ToString(CultureInfo.InvariantCulture)} "
                    + $"units left of a {panel.rect.width.ToString(CultureInfo.InvariantCulture)} x "
                    + $"{panel.rect.height.ToString(CultureInfo.InvariantCulture)} panel, at font size "
                    + $"{display.fontSize.ToString(CultureInfo.InvariantCulture)}. Nudge it with "
                    + "HudCountOffsetX / HudCountOffsetY in the plugin's config file.");
            }
            catch (Exception ex)
            {
                failures.Report(ex);
            }
        }

        /// <summary>
        /// Creates the text object under the money display, styles it after that
        /// display's own number and places it against the panel's left edge.
        /// </summary>
        /// <remarks>
        /// <para><b>Why the placement is computed and not written down.</b> The whole
        /// HUD lives inside <c>Global UI Fitter</c>, whose size follows the screen's
        /// aspect ratio, and the panel is pinned to its bottom-right corner. Reading
        /// the panel's rect and the money display's rect at attach time is therefore
        /// the only way to land in the same place on an ultrawide monitor as on a
        /// 16:9 one.</para>
        ///
        /// <para><b>Two coordinate systems, because the parent is not the reference.</b>
        /// The count hangs off the money display so that it inherits its fading, but
        /// what it is positioned against is the panel's left edge, and the money's own
        /// rect is far wider than the number drawn in it - the digits are right
        /// aligned inside it, and its left edge is off past the other side of the
        /// screen. So the panel's left edge is converted into the money's coordinates
        /// and the count is placed there, with a right-hand pivot so a negative x
        /// offset moves it into the gap and the number grows leftwards, away from the
        /// panel. The vertical is the money rect's own centre line, which is where its
        /// flip clock draws.</para>
        /// </remarks>
        private static TextMeshProUGUI Build(RectTransform panel, RectTransform moneyRect, TextMeshProUGUI donor)
        {
            var carrier = new GameObject(DisplayObjectName);

            // Layer 5 is UI, but taking the money display's own layer is the version
            // of that which stays right if the game ever moves its HUD somewhere else.
            carrier.layer = moneyRect.gameObject.layer;
            carrier.transform.SetParent(moneyRect, false);

            TextMeshProUGUI display = carrier.AddComponent<TextMeshProUGUI>();
            display.font = donor.font;
            display.fontSharedMaterial = donor.fontSharedMaterial;
            display.fontSize = donor.fontSize;
            display.color = donor.color;
            display.alignment = TextAlignmentOptions.Right;
            display.enableWordWrapping = false;
            display.overflowMode = TextOverflowModes.Overflow;
            display.richText = false;

            // Nothing here is clickable, and the HUD underneath is.
            display.raycastTarget = false;

            Rect panelRect = panel.rect;
            Rect moneyLocal = moneyRect.rect;

            // The panel's bottom-left corner, said in the money display's own
            // coordinates. Only the x of it is used; the y comes from the money rect,
            // which is the line the number has to share.
            float panelLeft = moneyRect.InverseTransformPoint(
                panel.TransformPoint(new Vector3(panelRect.x, panelRect.y, 0f))).x;
            float moneyCentreY = moneyLocal.y + (moneyLocal.height / 2f);

            RectTransform rect = display.rectTransform;

            // Anchored to the money rect's own bottom-left corner, so anchoredPosition
            // below is measured from (moneyLocal.x, moneyLocal.y).
            rect.anchorMin = new Vector2(0f, 0f);
            rect.anchorMax = new Vector2(0f, 0f);
            rect.pivot = new Vector2(1f, 0.5f);
            rect.sizeDelta = new Vector2(
                RectWidth,
                moneyLocal.height > 0f ? moneyLocal.height : FallbackRectHeight);
            rect.anchoredPosition = new Vector2(
                (panelLeft + _offsetX) - moneyLocal.x,
                (moneyCentreY + _offsetY) - moneyLocal.y);

            // Drawn after the money display's own flip clock. The two do not overlap,
            // but a number that could end up behind another one would be a silent
            // failure rather than a visible one.
            rect.SetAsLastSibling();

            return display;
        }

        /// <summary>
        /// Creates the speech-bubble icon just left of the count, or returns null and
        /// says why if the icon cannot be had. The count works without it.
        /// </summary>
        /// <remarks>
        /// It is a child of the count's own text object so that the two share a right
        /// edge: the digits are right aligned against it, so pushing the icon left by
        /// the text's own width always lands it against the leftmost digit, whatever
        /// the number is. <see cref="PositionIcon"/> is what redoes that when the
        /// count grows a digit.
        /// </remarks>
        private static RectTransform? BuildIcon(TextMeshProUGUI display, IUnifiedStateLog log)
        {
            Sprite? sprite = LoadIconSprite(log);
            if (sprite is null)
            {
                return null;
            }

            var carrier = new GameObject(IconObjectName);
            carrier.layer = display.gameObject.layer;
            carrier.transform.SetParent(display.rectTransform, false);

            Image image = carrier.AddComponent<Image>();
            image.sprite = sprite;

            // The icon ships as flat white, so the tint is what makes it the HUD's
            // colour, and keeps it the same colour as the digits if that ever changes.
            image.color = display.color;
            image.raycastTarget = false;
            image.preserveAspect = true;

            float height = display.fontSize * IconHeightInFontSizes;
            RectTransform rect = image.rectTransform;
            rect.anchorMin = new Vector2(1f, 0.5f);
            rect.anchorMax = new Vector2(1f, 0.5f);
            rect.pivot = new Vector2(1f, 0.5f);
            rect.sizeDelta = new Vector2(height * (sprite.rect.width / sprite.rect.height), height);
            return rect;
        }

        /// <summary>
        /// Decodes the embedded icon once per session. Returns null, having said so,
        /// if it cannot be read or decoded.
        /// </summary>
        private static Sprite? LoadIconSprite(IUnifiedStateLog log)
        {
            if (_iconSprite is not null)
            {
                return _iconSprite;
            }

            Assembly assembly = typeof(MainHudDialogueCountPatch).Assembly;
            string? resource = null;
            foreach (string candidate in assembly.GetManifestResourceNames())
            {
                if (candidate.EndsWith(IconResourceFileName, StringComparison.Ordinal))
                {
                    resource = candidate;
                    break;
                }
            }

            if (resource is null)
            {
                log.Warning(
                    $"The plugin has no embedded {IconResourceFileName}, so the HUD dialogue count is "
                    + "shown without its icon.");
                return null;
            }

            byte[] png;
            using (Stream? stream = assembly.GetManifestResourceStream(resource))
            {
                if (stream is null)
                {
                    log.Warning(
                        $"The embedded {resource} could not be opened, so the HUD dialogue count is "
                        + "shown without its icon.");
                    return null;
                }

                using var buffer = new MemoryStream();
                stream.CopyTo(buffer);
                png = buffer.ToArray();
            }

            // Hidden and not saved: this texture belongs to the mod, and nothing in the
            // game should be able to collect it out from under the sprite or write it
            // into a scene.
            var texture = new Texture2D(2, 2, TextureFormat.RGBA32, false)
            {
                name = IconObjectName,
                hideFlags = HideFlags.HideAndDontSave,
                wrapMode = TextureWrapMode.Clamp,
                filterMode = FilterMode.Bilinear,
            };

            if (!ImageConversion.LoadImage(texture, new Il2CppStructArray<byte>(png)))
            {
                log.Warning(
                    $"The embedded {resource} is not a texture Unity could decode, so the HUD dialogue "
                    + "count is shown without its icon.");
                return null;
            }

            Sprite sprite = Sprite.Create(
                texture,
                new Rect(0f, 0f, texture.width, texture.height),
                new Vector2(0.5f, 0.5f));
            sprite.hideFlags = HideFlags.HideAndDontSave;
            _iconSprite = sprite;
            return sprite;
        }

        /// <summary>Writes the count, if it is not already what is on screen.</summary>
        private static void Write(TextMeshProUGUI display, int count)
        {
            if (count == _displayedCount)
            {
                return;
            }

            display.text = count.ToString(CountFormat, CultureInfo.InvariantCulture);
            _displayedCount = count;
            PositionIcon(display);
        }

        /// <summary>
        /// Puts the icon against the leftmost digit. Called after every write, because
        /// the number it is measured from is what just changed.
        /// </summary>
        private static void PositionIcon(TextMeshProUGUI display)
        {
            RectTransform? icon = _icon;
            if (icon is null)
            {
                return;
            }

            // preferredWidth is the width of the digits themselves, not of the rect
            // they are right aligned in, which is deliberately much wider.
            icon.anchoredPosition = new Vector2(-(display.preferredWidth + IconGap), 0f);
        }
    }
}
