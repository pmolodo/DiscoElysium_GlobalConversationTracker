// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Reflection;
using HarmonyLib;
using Il2CppInterop.Runtime.InteropTypes.Arrays;
using TMPro;
using GlobalConversationTracker.Session;
using UnityEngine;
using UnityEngine.UI;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The display hook: the main HUD carries how many dialogue entries have been
    /// reached, in the gap between the thought cabinet button and the money/time
    /// panel at the bottom of the screen. Two rows - this save above, every save
    /// below - each prefixed by its own icon.
    /// </summary>
    /// <remarks>
    /// <para>The HUD rather than the character sheet, which is gated behind two pieces
    /// of early-game progression - picking up the ledger, and reading it under halogen
    /// lights - so a count shown there is invisible for the first stretch of a
    /// playthrough.</para>
    ///
    /// <para>The live HUD is the one built into the <c>Init</c> scene, under
    /// <c>Global UI Canvas/Global UI Fitter</c> - not the <c>HUD Page</c> prefab, whose
    /// money subtree has no controller on it and is inert. The object of interest is
    /// <c>HUD Money Time</c>, the panel pinned to the fitter's bottom-right corner
    /// holding the money display, clock, day counter and hand-equip slots. Every
    /// measurement here is taken off that panel at runtime: the fitter's size follows
    /// the screen's aspect ratio, so none of it is fixed in screen pixels.</para>
    ///
    /// <para><c>HudMoneyController.Start</c> is the trigger because it is a Unity
    /// message - IL2CPP cannot inline it away - and it hands over the money display as
    /// <c>__instance</c>, whose parent is the panel and whose children carry the exact
    /// font, size and colour the HUD uses for a number. <c>ClockToggler</c> keeps its
    /// instance in a private static, and <c>HudController</c> belongs to the HUD
    /// hierarchy that is not on screen.</para>
    ///
    /// <para>The panel's left edge is the right-hand wall of the gap the display sits
    /// in, so the rows are placed against it with a right-hand pivot and grow leftwards
    /// towards the thought cabinet button. The two numbers share a right edge; the icon
    /// column is measured off whichever number is wider, so the icons do not step in
    /// and out with the digits beside them.</para>
    ///
    /// <para>The rows are children of the money display, not of the panel, so they fade
    /// with it. The HUD does not hide itself as a panel: each element carries its own
    /// <c>CanvasGroup</c> and alpha tween while the shared panel is left alone, so a
    /// child of the panel would stay on screen through every dialogue and cutscene that
    /// fades the rest of the HUD out.</para>
    ///
    /// <para>The icons are textures because there are no characters to use. Every font
    /// asset the game ships is a static SDF atlas with no source font behind it, so
    /// nothing can be added at runtime, and the highest codepoint in any of them is
    /// U+FF70 - emoji start at U+1F300. The project's TMP sprite asset is TextMesh Pro's
    /// own EmojiOne sample: fourteen smileys, no speech bubble and no globe. With
    /// <c>m_missingGlyphCharacter: 0</c> and warnings off in TMP Settings, an emoji
    /// would draw as nothing and say nothing about it. So: flat white PNGs embedded in
    /// the plugin, drawn through <see cref="Image"/>s and tinted.</para>
    ///
    /// <para>Read-only and cheap. Both numbers are collection sizes weighted by
    /// <see cref="DialogueScore"/>. Nothing polls: the counts change only when a mark is
    /// recorded, a savegame is loaded, or a new game resets the current save, and all
    /// three hooks call <see cref="RefreshDisplayedCounts"/> on their way out. A failure
    /// here costs the display, never the playthrough.</para>
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
        private const string CurrentSaveRowName = "GlobalConversationTracker Current Save Count";

        /// <summary>The all-saves row's object name.</summary>
        private const string AllSavesRowName = "GlobalConversationTracker All Saves Count";

        /// <summary>Suffix given to a row's icon object, appended to the row's name.</summary>
        private const string IconNameSuffix = " Icon";

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
        /// Sized off legibility. The canvas scales by screenHeight/1080, so at 1.0 the
        /// globe draws about 18 pixels across at 1080p - not enough for its meridians to
        /// survive the downsample. 1.1 gives about 21 at 1080p and 23 at 1200p. It
        /// cannot go much higher: the box is centred on its row, and the rows are only
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

        private static GlobalStateSession? _session;
        private static HookFailureLimiter? _failures;
        private static IGlobalStateLog? _log;
        private static float _offsetX;
        private static float _offsetY;
        private static bool _showCurrentSave;
        private static bool _showAllSaves;

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
        /// <param name="showCurrentSave">Whether to draw the this-save row.</param>
        /// <param name="showAllSaves">Whether to draw the across-all-saves row.</param>
        /// <exception cref="ArgumentNullException">Any argument is null.</exception>
        /// <exception cref="ArgumentException">Both rows are switched off.</exception>
        /// <exception cref="Exception">
        /// Harmony could not patch the method - it was not found, or the detour
        /// failed. The caller decides what that means; the HUD is unchanged either
        /// way.
        /// </exception>
        internal static void Install(
            Harmony harmony,
            GlobalStateSession session,
            IGlobalStateLog log,
            float offsetX,
            float offsetY,
            bool showCurrentSave,
            bool showAllSaves)
        {
            if (harmony == null)
            {
                throw new ArgumentNullException(nameof(harmony));
            }

            if (!showCurrentSave && !showAllSaves)
            {
                throw new ArgumentException(
                    "Both HUD rows are switched off, so there is nothing to install. Callers should "
                    + "skip the install instead of asking for an empty display.");
            }

            _session = session ?? throw new ArgumentNullException(nameof(session));
            _log = log ?? throw new ArgumentNullException(nameof(log));
            _failures = new HookFailureLimiter(
                "showing the dialogue counts on the main HUD", log);
            _offsetX = offsetX;
            _offsetY = offsetY;
            _showCurrentSave = showCurrentSave;
            _showAllSaves = showAllSaves;

            harmony.PatchAll(typeof(MoneyStartPatch));
        }

        /// <summary>
        /// Brings whichever numbers are on screen up to date with the session, if they
        /// are out of date and if there is anything on screen to update.
        /// </summary>
        /// <remarks>
        /// Never throws. It is called from inside the tracking hooks, which have their
        /// own failure budgets to spend on tracking; a display that cannot draw itself
        /// must not spend theirs, so this reports through its own limiter and returns.
        /// </remarks>
        internal static void RefreshDisplayedCounts()
        {
            CountRow? currentSave = _currentSaveRow;
            CountRow? allSaves = _allSavesRow;
            GlobalStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if ((currentSave is null && allSaves is null) || session == null || failures == null
                || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                bool changed = currentSave is not null
                    && currentSave.Write(session.CurrentSaveScore);
                changed |= allSaves is not null
                    && allSaves.Write(session.EnsureInitialized().Score);
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
        /// Builds whichever rows are switched on beside the given money display,
        /// replacing any earlier ones.
        /// </summary>
        private static void Attach(HudMoneyController money)
        {
            GlobalStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            IGlobalStateLog? log = _log;
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
                // the pair straddles the row of the HUD it belongs to. With one row
                // switched off there is no pair to straddle with, so the survivor sits
                // on that line rather than hanging half a line off it.
                float lineSpacing = donor.fontSize * LineSpacingInFontSizes;
                float halfLine = _showCurrentSave && _showAllSaves ? lineSpacing / 2f : 0f;

                CountRow? currentSave = _showCurrentSave
                    ? Build(
                        panel, moneyRect, donor, CurrentSaveRowName, CurrentSaveIconFileName,
                        halfLine, log)
                    : null;
                CountRow? allSaves = _showAllSaves
                    ? Build(
                        panel, moneyRect, donor, AllSavesRowName, AllSavesIconFileName,
                        -halfLine, log)
                    : null;

                _currentSaveRow = currentSave;
                _allSavesRow = allSaves;

                currentSave?.Write(session.CurrentSaveScore);
                allSaves?.Write(session.EnsureInitialized().Score);
                AlignIcons(currentSave, allSaves);

                log.Info(
                    $"Dialogue counts added to the main HUD: this save {OnOff(currentSave)}, "
                    + $"all saves {OnOff(allSaves)}. Nudge them with HudCountOffsetX / "
                    + "HudCountOffsetY in the plugin's config file, or switch either off with "
                    + "ShowCurrentSaveCount / ShowAllSavesCount.");
            }
            catch (Exception ex)
            {
                failures.Report(ex);
            }
        }

        private static string OnOff(CountRow? row) => row is null ? "off" : "on";

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
        /// The placement is computed, not written down: the HUD lives inside
        /// <c>Global UI Fitter</c>, whose size follows the screen's aspect ratio, so
        /// reading both rects at attach time is the only way to land in the same place
        /// on an ultrawide monitor as on a 16:9 one.
        /// <para>Two coordinate systems, because the parent is not the reference. The
        /// rows hang off the money display to inherit its fading, but are positioned
        /// against the panel's left edge - and the money's own rect is far wider than
        /// the number drawn in it, with its left edge off past the other side of the
        /// screen. So the panel's left edge is converted into the money's coordinates,
        /// with a right-hand pivot so a negative x offset moves the rows into the gap.
        /// </para>
        /// </remarks>
        private static CountRow Build(
            RectTransform panel,
            RectTransform moneyRect,
            TextMeshProUGUI donor,
            string name,
            string iconFileName,
            float verticalOffset,
            IGlobalStateLog log)
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
            TextMeshProUGUI display, string name, string iconFileName, IGlobalStateLog log)
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
        /// Puts both icons in one column, left of whichever number is wider - aligned
        /// with each other, so the pair reads as one block rather than two stacked
        /// labels.
        /// </summary>
        private static void AlignIcons(CountRow? currentSave, CountRow? allSaves)
        {
            // preferredWidth is the width of the digits themselves, not of the rect
            // they are right aligned in, which is deliberately much wider. A row that
            // is switched off contributes no width: with one row there is no column to
            // keep, only that row's own icon to place.
            float widest = 0f;
            if (currentSave is not null)
            {
                widest = currentSave.Text.preferredWidth;
            }

            if (allSaves is not null)
            {
                widest = Math.Max(widest, allSaves.Text.preferredWidth);
            }

            var position = new Vector2(-(widest + IconGap), 0f);

            currentSave?.PlaceIcon(position);
            allSaves?.PlaceIcon(position);
        }

        /// <summary>
        /// Decodes one embedded icon, once per session per file. Returns null, having
        /// said so, if it cannot be read or decoded.
        /// </summary>
        private static Sprite? LoadIconSprite(string iconFileName, IGlobalStateLog log)
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
            /// Starts at -1 rather than 0, because 0 is a real score that must still be
            /// written the first time.
            /// </summary>
            private double _shown = -1d;

            internal CountRow(TextMeshProUGUI text, RectTransform? icon)
            {
                Text = text;
                Icon = icon;
            }

            /// <summary>The number itself.</summary>
            internal TextMeshProUGUI Text { get; }

            /// <summary>The icon left of it, or null if the artwork could not be loaded.</summary>
            private RectTransform? Icon { get; }

            /// <summary>Writes the score if it is not already what is on screen.</summary>
            /// <returns>True if the text changed, so the icons need re-aligning.</returns>
            internal bool Write(double score)
            {
                // Exact comparison, on purpose: a score is a whole number of halves,
                // which a double carries exactly, so two equal scores compare equal.
                if (score == _shown)
                {
                    return false;
                }

                Text.text = DialogueScore.Format(score);
                _shown = score;
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
