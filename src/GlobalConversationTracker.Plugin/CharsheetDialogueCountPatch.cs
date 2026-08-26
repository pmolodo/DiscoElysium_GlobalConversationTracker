using System;
using HarmonyLib;
using TMPro;
using GlobalConversationTracker.Session;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The display hook: the character sheet gets one extra line saying how many
    /// dialogue entries have been reached across every save.
    /// </summary>
    /// <remarks>
    /// <para><b>Why two patched methods.</b> The character sheet's info panel writes
    /// its text from exactly one of two places, depending on whether anything is
    /// selected: <c>ShowCharsheetInfo</c> when nothing is, <c>ShowModifiable</c> when
    /// a skill or attribute is. Both assign <c>extraText.text</c> as they finish, so
    /// postfixing both is what makes the line present on every state of the sheet
    /// rather than only on the one the player happened to open into. They are two
    /// nested patch classes because one Harmony patch class describes one target.</para>
    ///
    /// <para><b>Why not <c>GetCharsheetInfo</c>.</b> The private string builder that
    /// looks like the natural target has been inlined into its caller by IL2CPP. It
    /// still exists as a patchable body in the metadata, so the patch would apply
    /// cleanly and then never run.</para>
    ///
    /// <para><b>Why <c>extraText</c> and not <c>textArea</c>.</b> It is the
    /// bonus/modifier block, which the game already builds as a plain multi-line
    /// string, so one more line is a string append and not a new layout object.
    /// <c>ShowCharsheetInfo</c> leaves it empty and <c>ShowModifiable</c> leaves the
    /// modifier rows in it; appending preserves whichever the player is looking at.
    /// No localisation is involved: the game writes this field from code-built
    /// strings, and the I2 Localize component on it carries no term - it is there to
    /// swap the font per language, not the text.</para>
    ///
    /// <para><b>Read-only, and cheap.</b> The number is
    /// <see cref="GlobalConversationState.Score"/>, which is the entries above
    /// Untouched across all saves - Untouched is never stored - weighted by
    /// <see cref="DialogueScore"/>, so there is nothing to count and nothing to
    /// enumerate. As with every other hook,
    /// a failure here costs the line and never the playthrough.</para>
    /// </remarks>
    internal static class CharsheetDialogueCountPatch
    {
        /// <summary>
        /// What the line says before the number. Also how an already-appended line is
        /// recognized, so it has to be distinctive enough not to occur in the game's
        /// own modifier rows.
        /// </summary>
        private const string LinePrefix = "Dialogue entries reached (all saves): ";

        private static GlobalStateSession? _session;
        private static HookFailureLimiter? _failures;

        /// <summary>
        /// Applies the patch. Call once, from plugin load, after the session exists.
        /// </summary>
        /// <param name="harmony">The plugin's Harmony instance.</param>
        /// <param name="session">The session the displayed count is read from.</param>
        /// <param name="log">Where hook failures are reported.</param>
        /// <exception cref="ArgumentNullException">Any argument is null.</exception>
        /// <exception cref="Exception">
        /// Harmony could not patch a method - it was not found, or the detour failed.
        /// The caller decides what that means; the sheet is unchanged either way.
        /// </exception>
        internal static void Install(Harmony harmony, GlobalStateSession session, IGlobalStateLog log)
        {
            if (harmony == null)
            {
                throw new ArgumentNullException(nameof(harmony));
            }

            _session = session ?? throw new ArgumentNullException(nameof(session));
            _failures = new HookFailureLimiter(
                "showing the across-all-saves dialogue count on the character sheet", log);

            // Both or neither: a line that appears only while nothing is selected
            // would look like a bug rather than a partial feature.
            harmony.PatchAll(typeof(ShowCharsheetInfoPatch));
            harmony.PatchAll(typeof(ShowModifiablePatch));
        }

        /// <summary>Postfixes the redraw that runs with nothing selected.</summary>
        [HarmonyPatch(
            typeof(CharacterSheetInfoPanel),
            nameof(CharacterSheetInfoPanel.ShowCharsheetInfo))]
        private static class ShowCharsheetInfoPatch
        {
            [HarmonyPostfix]
            private static void Postfix(CharacterSheetInfoPanel __instance)
            {
                AppendCountLine(__instance);
            }
        }

        /// <summary>Postfixes the redraw that runs with a skill or attribute selected.</summary>
        [HarmonyPatch(
            typeof(CharacterSheetInfoPanel),
            nameof(CharacterSheetInfoPanel.ShowModifiable))]
        private static class ShowModifiablePatch
        {
            [HarmonyPostfix]
            private static void Postfix(CharacterSheetInfoPanel __instance)
            {
                AppendCountLine(__instance);
            }
        }

        /// <summary>
        /// Adds the count line to the panel's bonus text, keeping whatever the game
        /// just put there.
        /// </summary>
        private static void AppendCountLine(CharacterSheetInfoPanel panel)
        {
            GlobalStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if (session == null || failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                if (panel is null)
                {
                    return;
                }

                TextMeshProUGUI extraText = panel.extraText;
                if (extraText is null)
                {
                    // A reference check, not a Unity liveness check: the method that
                    // just ran wrote through this field, so a destroyed object would
                    // have taken the game down before reaching us.
                    return;
                }

                string existing = extraText.text ?? string.Empty;
                if (existing.Contains(LinePrefix))
                {
                    // The game reassigns this field on every redraw, so this normally
                    // cannot happen; it is here so that a redraw path that ever
                    // appends instead of assigning cannot stack up copies of the line.
                    return;
                }

                string line = LinePrefix
                    + DialogueScore.Format(session.EnsureInitialized().Score);
                extraText.text = existing.Length == 0 ? line : existing + "\n" + line;
            }
            catch (Exception ex)
            {
                failures.Report(ex);
            }
        }
    }
}
