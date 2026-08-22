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
    /// The display hook: the main HUD carries how many dialogue entries have been
    /// reached, in the gap between the thought cabinet button and the money/time
    /// panel at the bottom of the screen. Two rows - this save above, every save
    /// below - each prefixed by its own icon.
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
    /// wall of the gap this display sits in, so the rows are placed against that edge
    /// with a right-hand pivot and grow leftwards, towards the thought cabinet button,
    /// straddling the money's centre line. The two numbers share a right edge, and the
    /// two icons share a left one: the icon column is measured off whichever number is
    /// wider, so the icons stay in a column instead of stepping in and out with the
    /// digits beside them.</para>
    ///
    /// <para><b>Why the rows are children of the money display and not of the panel.</b>
    /// Because that is what makes them disappear at the right times. The HUD does not
    /// hide itself as a panel: each element - the money, the clock, the held items -
    /// carries its own <c>CanvasGroup</c> and its own alpha tween, and the panel they
    /// share is left alone. A child of the panel would therefore stay on screen
    /// through every dialogue, cutscene and menu that fades the rest of the HUD out.
    /// A child of the money display inherits its group, so the counts fade exactly
    /// when the number beside them does.</para>
    ///
    /// <para><b>Why the icons are textures and not characters.</b> Because there are no
    /// characters to use. Every font asset the game ships is a static SDF atlas with
    /// no source font behind it, so nothing can be added at runtime, and the highest
    /// codepoint in any of them is U+FF70 - emoji start at U+1F300. The project's TMP
    /// sprite asset, which is what would otherwise stand in, is TextMesh Pro's own
    /// EmojiOne sample: fourteen smileys, no speech bubble and no globe. With
    /// <c>m_missingGlyphCharacter: 0</c> and warnings off in TMP Settings, an emoji
    /// character would therefore draw as nothing at all and say nothing about it. So
    /// the icons are flat white PNGs embedded in the plugin, drawn through
    /// <see cref="Image"/>s and tinted to the counts' own colour.</para>
    ///
    /// <para><b>Read-only, and cheap.</b> The two numbers are
    /// <see cref="UnifiedConversationState.EntryCount"/> and
    /// <see cref="UnifiedStateSession.CurrentSaveEntryCount"/>, both of which are a
    /// collection's own size rather than anything that has to be counted. Nothing
    /// polls: the counts can only change when a mark is recorded, a savegame is
    /// loaded, or a new game resets the current save, and all three of those hooks
    /// call <see cref="RefreshDisplayedCounts"/> on their way out. As with every other
    /// hook, a failure here costs the display and never the playthrough.</para>
    /// </remarks>
    internal static class MainHudDialogueCountPatch
    {
        /// <summary>
        /// How far left of the money/time panel's left edge the counts' right-hand
        /// edge sits, in canvas units. Negative is left. The measured gap between that
        /// edge and the thought cabinet button is about 117 units, so this leaves room
        /// for an icon and six or seven digits before anything collides.
        /// </summary>
        internal const float DefaultOffsetX = -10f;

        /// <summary>
        /// How far the pair of rows sits above (positive) or below (negative) the
        /// money display's own centre line, in canvas units. Zero straddles it, which
        /// keeps the block tied to the row of the HUD it lives in.
        /// </summary>
        internal const float DefaultOffsetY = 0f;

        /// <summary>
        /// The file the current-save row's speech-bubble icon is embedded under.
        /// Matched by suffix at runtime, so the manifest prefix MSBuild chooses does
        /// not matter.
        /// </summary>
        private const string CurrentSaveIconFileName = "current-save-count-icon.png";

        /// <summary>The file the all-saves row's globe icon is embedded under.</summary>
        private const string AllSavesIconFileName = "all-saves-count-icon.png";

        /// <summary>
        /// The names given to the objects the rows are drawn on. Long and explicit
        /// because they appear in the game's own hierarchy, where anything ambiguous
        /// would look like something the game shipped.
        /// </summary>
        private const string CurrentSaveRowName = "UnifiedConversationTracker Current Save Count";

        /// <summary>The all-saves row's object name.</summary>
        private const string AllSavesRowName = "UnifiedConversationTracker All Saves Count";

        /// <summary>Suffix given to a row's icon object, appended to the row's name.</summary>
        private const string IconNameSuffix = " Icon";

        /// <summary>
        /// Group separators, no decimals: the all-saves count runs into five figures,
        /// and the money display beside it is grouped the same way.
        /// </summary>
        private const string CountFormat = "N0";

        /// <summary>
        /// How wide a row's own rect is, in canvas units. The text is right aligned
        /// and does not wrap, so this is headroom to grow leftwards into and not a box
        /// anything is fitted to.
        /// </summary>
        private const float RectWidth = 240f;

        /// <summary>
        /// The gap between one row's baseline and the next, as a multiple of the font
        /// size. The pair is centred on the money's line, so each row sits half of this
        /// away from it.
        /// </summary>
        private const float LineSpacingInFontSizes = 1.15f;

        /// <summary>
        /// How tall an icon's box is as a multiple of the font size. The artwork fills
        /// about five-sixths of that box, which puts it a little taller than the digits
        /// beside it - the same proportion an emoji would have.
        /// </summary>
        /// <remarks>
        /// Sized off legibility rather than taste. The canvas scales by
        /// screenHeight/1080, so at 1080p one font size is 22 screen pixels and the
        /// globe inside it would draw about 18 across - not enough for its meridians
        /// to survive the downsample. At 1.1 it draws about 21 at 1080p and 23 at
        /// 1200p, which is where the grid stops turning to mush. It cannot go much
        /// past this: the box is centred on its row and the rows are only
        /// <see cref="LineSpacingInFontSizes"/> apart.
        /// </remarks>
        private const float IconHeightInFontSizes = 1.1f;

        /// <summary>The gap between the icon column and the widest number, in canvas units.</summary>
        private const float IconGap = 6f;

        /// <summary>
        /// The height used for a row when the money display's rect cannot supply one.
        /// Only affects where the text sits vertically inside its own rect, which is
        /// centred either way.
        /// </summary>
        private const float FallbackRowHeight = 24f;

        private static UnifiedStateSession? _session;
        private static HookFailureLimiter? _failures;
        private static IUnifiedStateLog? _log;
        private static float _offsetX;
        private static float _offsetY;

        private static CountRow? _currentSaveRow;
        private static CountRow? _allSavesRow;

        /// <summary>
        /// Applies the patch. Call once, from plugin load, after the session exists.
        /// </summary>
        /// <param name="harmony">The plugin's Harmony instance.</param>
        /// <param name="session">The session the displayed counts are read from.</param>
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
                "showing the dialogue counts on the main HUD", log);
            _offsetX = offsetX;
            _offsetY = offsetY;

            harmony.PatchAll(typeof(MoneyStartPatch));
        }

        /// <summary>
        /// Brings both displayed numbers up to date with the session, if they are out
        /// of date and if there is anything on screen to update.
        /// </summary>
        /// <remarks>
        /// <para><b>This never throws.</b> It is called from inside the tracking
        /// hooks, which have their own failure budgets to spend on tracking; a display
        /// that cannot draw itself must not be able to spend theirs, so this reports
        /// through its own limiter and returns.</para>
        ///
        /// <para><b>And it is called rather than polled.</b> The counts change exactly
        /// when a mark is recorded, a savegame is resynced, or a new game clears the
        /// current save, so those callers see every change there is. Between them the
        /// display costs nothing per frame.</para>
        /// </remarks>
        internal static void RefreshDisplayedCounts()
        {
            CountRow? currentSave = _currentSaveRow;
            CountRow? allSaves = _allSavesRow;
            UnifiedStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if (currentSave is null || allSaves is null || session == null || failures == null
                || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                bool changed = currentSave.Write(session.CurrentSaveEntryCount);
                changed |= allSaves.Write(session.EnsureInitialized().EntryCount);
                if (changed)
                {
                    AlignIcons(currentSave, allSaves);
                }
            }
            catch (Exception ex)
            {
                failures.Report(ex);
            }
        }

        /// <summary>
        /// Postfixes the money display's own startup, which is where the live HUD
        /// hands over both the panel to hang the rows off and the text to copy.
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
        /// Builds both rows beside the given money display, replacing any earlier
        /// ones.
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
                        + "there is nowhere to put the dialogue counts. The HUD is unchanged.");
                    return;
                }

                RectTransform panel = moneyRect.parent.GetComponent<RectTransform>();
                if (panel is null)
                {
                    log.Warning(
                        "The HUD money display's parent is not a rect, so the dialogue counts have no "
                        + "panel edge to sit beside. The HUD is unchanged.");
                    return;
                }

                // Taken from the money display's own flip clock, which is the number
                // these are meant to look like siblings of. Copying beats guessing:
                // the font asset is whichever one the current language loaded.
                TextMeshProUGUI donor = moneyRect.GetComponentInChildren<TextMeshProUGUI>(true);
                if (donor is null)
                {
                    log.Warning(
                        "The HUD money display has no text component to copy a font from, so the dialogue "
                        + "counts would be invisible. The HUD is unchanged.");
                    return;
                }

                // Start() can only run once per HUD, but a rebuilt HUD would run it
                // again; leaving the old objects behind would stack up copies.
                DestroyStale(moneyRect, CurrentSaveRowName);
                DestroyStale(moneyRect, AllSavesRowName);

                // Half a line above the money's own line and half a line below it, so
                // the pair straddles the row of the HUD it belongs to.
                float lineSpacing = donor.fontSize * LineSpacingInFontSizes;
                CountRow currentSave = Build(
                    panel, moneyRect, donor, CurrentSaveRowName, CurrentSaveIconFileName,
                    lineSpacing / 2f, log);
                CountRow allSaves = Build(
                    panel, moneyRect, donor, AllSavesRowName, AllSavesIconFileName,
                    -lineSpacing / 2f, log);

                _currentSaveRow = currentSave;
                _allSavesRow = allSaves;

                currentSave.Write(session.CurrentSaveEntryCount);
                allSaves.Write(session.EnsureInitialized().EntryCount);
                AlignIcons(currentSave, allSaves);

                log.Info(
                    $"Dialogue counts added to the main HUD, under '{panel.name}/{moneyRect.name}' at "
                    + $"{currentSave.Text.rectTransform.anchoredPosition.ToString()} and "
                    + $"{allSaves.Text.rectTransform.anchoredPosition.ToString()}, "
                    + $"{(-_offsetX).ToString(CultureInfo.InvariantCulture)} units left of a "
                    + $"{panel.rect.width.ToString(CultureInfo.InvariantCulture)} x "
                    + $"{panel.rect.height.ToString(CultureInfo.InvariantCulture)} panel, at font size "
                    + $"{donor.fontSize.ToString(CultureInfo.InvariantCulture)}. Nudge them with "
                    + "HudCountOffsetX / HudCountOffsetY in the plugin's config file.");
            }
            catch (Exception ex)
            {
                failures.Report(ex);
            }
        }

        /// <summary>Removes a row left over from an earlier attach, if there is one.</summary>
        private static void DestroyStale(RectTransform parent, string name)
        {
            Transform stale = parent.Find(name);
            if (stale is not null)
            {
                UnityEngine.Object.Destroy(stale.gameObject);
            }
        }

        /// <summary>
        /// Creates one row - a right-aligned number with its icon - and places it
        /// against the panel's left edge, <paramref name="verticalOffset"/> units from
        /// the money display's centre line.
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
        /// The rows hang off the money display so that they inherit its fading, but
        /// what they are positioned against is the panel's left edge, and the money's
        /// own rect is far wider than the number drawn in it - the digits are right
        /// aligned inside it, and its left edge is off past the other side of the
        /// screen. So the panel's left edge is converted into the money's coordinates
        /// and the rows are placed there, with a right-hand pivot so a negative x
        /// offset moves them into the gap and the numbers grow leftwards, away from
        /// the panel.</para>
        /// </remarks>
        private static CountRow Build(
            RectTransform panel,
            RectTransform moneyRect,
            TextMeshProUGUI donor,
            string name,
            string iconFileName,
            float verticalOffset,
            IUnifiedStateLog log)
        {
            var carrier = new GameObject(name);

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
            // which is the line the numbers have to share.
            float panelLeft = moneyRect.InverseTransformPoint(
                panel.TransformPoint(new Vector3(panelRect.x, panelRect.y, 0f))).x;
            float moneyCentreY = moneyLocal.y + (moneyLocal.height / 2f);

            RectTransform rect = display.rectTransform;

            // Anchored to the money rect's own bottom-left corner, so anchoredPosition
            // below is measured from (moneyLocal.x, moneyLocal.y).
            rect.anchorMin = new Vector2(0f, 0f);
            rect.anchorMax = new Vector2(0f, 0f);
            rect.pivot = new Vector2(1f, 0.5f);
            float rowHeight = donor.fontSize * LineSpacingInFontSizes;
            rect.sizeDelta = new Vector2(
                RectWidth,
                rowHeight > 0f ? rowHeight : FallbackRowHeight);
            rect.anchoredPosition = new Vector2(
                (panelLeft + _offsetX) - moneyLocal.x,
                (moneyCentreY + verticalOffset + _offsetY) - moneyLocal.y);

            // Drawn after the money display's own flip clock. The two do not overlap,
            // but a number that could end up behind another one would be a silent
            // failure rather than a visible one.
            rect.SetAsLastSibling();

            return new CountRow(display, BuildIcon(display, name + IconNameSuffix, iconFileName, log));
        }

        /// <summary>
        /// Creates one row's icon, or returns null and says why if the artwork cannot
        /// be had. The counts work without it.
        /// </summary>
        /// <remarks>
        /// It is a child of its row's own text object so that the two share a right
        /// edge: the digits are right aligned against it, so an offset measured
        /// leftwards from that edge lands predictably whatever the number is.
        /// <see cref="AlignIcons"/> is what then puts both rows' icons in one column.
        /// </remarks>
        private static RectTransform? BuildIcon(
            TextMeshProUGUI display, string name, string iconFileName, IUnifiedStateLog log)
        {
            Sprite? sprite = LoadIconSprite(iconFileName, log);
            if (sprite is null)
            {
                return null;
            }

            var carrier = new GameObject(name);
            carrier.layer = display.gameObject.layer;
            carrier.transform.SetParent(display.rectTransform, false);

            Image image = carrier.AddComponent<Image>();
            image.sprite = sprite;

            // The icons ship as flat white, so the tint is what makes them the HUD's
            // colour, and keeps them the same colour as the digits if that changes.
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
        /// Puts both icons in one column, left of whichever number is wider.
        /// </summary>
        /// <remarks>
        /// Measured off the wider row rather than each row separately, which is the
        /// whole point: aligned with each other is what makes them read as one block
        /// rather than two labels that happen to be stacked.
        /// </remarks>
        private static void AlignIcons(CountRow currentSave, CountRow allSaves)
        {
            // preferredWidth is the width of the digits themselves, not of the rect
            // they are right aligned in, which is deliberately much wider.
            float widest = Math.Max(currentSave.Text.preferredWidth, allSaves.Text.preferredWidth);
            var position = new Vector2(-(widest + IconGap), 0f);

            currentSave.PlaceIcon(position);
            allSaves.PlaceIcon(position);
        }

        /// <summary>
        /// Decodes one embedded icon, once per session per file. Returns null, having
        /// said so, if it cannot be read or decoded.
        /// </summary>
        private static Sprite? LoadIconSprite(string iconFileName, IUnifiedStateLog log)
        {
            Assembly assembly = typeof(MainHudDialogueCountPatch).Assembly;
            string? resource = null;
            foreach (string candidate in assembly.GetManifestResourceNames())
            {
                if (candidate.EndsWith(iconFileName, StringComparison.Ordinal))
                {
                    resource = candidate;
                    break;
                }
            }

            if (resource is null)
            {
                log.Warning(
                    $"The plugin has no embedded {iconFileName}, so that HUD dialogue count is shown "
                    + "without its icon.");
                return null;
            }

            byte[] png;
            using (Stream? stream = assembly.GetManifestResourceStream(resource))
            {
                if (stream is null)
                {
                    log.Warning(
                        $"The embedded {resource} could not be opened, so that HUD dialogue count is "
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
                name = iconFileName,
                hideFlags = HideFlags.HideAndDontSave,
                wrapMode = TextureWrapMode.Clamp,
                filterMode = FilterMode.Bilinear,
            };

            if (!ImageConversion.LoadImage(texture, new Il2CppStructArray<byte>(png)))
            {
                log.Warning(
                    $"The embedded {resource} is not a texture Unity could decode, so that HUD dialogue "
                    + "count is shown without its icon.");
                return null;
            }

            Sprite sprite = Sprite.Create(
                texture,
                new Rect(0f, 0f, texture.width, texture.height),
                new Vector2(0.5f, 0.5f));
            sprite.hideFlags = HideFlags.HideAndDontSave;
            return sprite;
        }

        /// <summary>
        /// One line of the display: a number, and the icon that says which number it
        /// is.
        /// </summary>
        private sealed class CountRow
        {
            /// <summary>
            /// What is currently drawn, so a refresh that changes nothing does nothing.
            /// Starts at -1 rather than 0, because 0 is a real count that must still be
            /// written the first time.
            /// </summary>
            private int _shown = -1;

            internal CountRow(TextMeshProUGUI text, RectTransform? icon)
            {
                Text = text;
                Icon = icon;
            }

            /// <summary>The number itself.</summary>
            internal TextMeshProUGUI Text { get; }

            /// <summary>The icon left of it, or null if the artwork could not be loaded.</summary>
            private RectTransform? Icon { get; }

            /// <summary>Writes the count if it is not already what is on screen.</summary>
            /// <returns>True if the text changed, so the icons need re-aligning.</returns>
            internal bool Write(int count)
            {
                if (count == _shown)
                {
                    return false;
                }

                Text.text = count.ToString(CountFormat, CultureInfo.InvariantCulture);
                _shown = count;
                return true;
            }

            /// <summary>Moves this row's icon, if it has one.</summary>
            internal void PlaceIcon(Vector2 position)
            {
                if (Icon is not null)
                {
                    Icon.anchoredPosition = position;
                }
            }
        }
    }
}
