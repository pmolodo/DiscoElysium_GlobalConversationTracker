// SPDX-License-Identifier: MIT
using System;
using HarmonyLib;
using PixelCrushers.DialogueSystem;
using GlobalConversationTracker.Session;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The write-through hook: every SimStatus change the game makes while playing
    /// goes into the global state on its way past.
    /// </summary>
    /// <remarks>
    /// <para><b>Why this method.</b>
    /// <c>DialogueLua.MarkDialogueEntry(DialogueEntry, string)</c> is the single
    /// funnel through which the running game writes SimStatus:
    /// <c>MarkDialogueEntryDisplayed</c>, <c>MarkDialogueEntryOffered</c> and
    /// <c>MarkDialogueEntryUntouched</c> all call it, it survives Final Cut with an
    /// unchanged signature, and it is not inlined (verified against the shipping
    /// IL2CPP binary). The one writer that does not come through here is
    /// <c>PersistentDataManager</c> rebuilding the table on savegame load; that one is
    /// covered by <see cref="ApplyRawDataPatch"/>, which hands the raw save bytes to
    /// <see cref="GlobalStateSession.ResyncFromSaveRawBytes"/>.</para>
    ///
    /// <para><b>Postfix, not prefix.</b> The stock per-save behavior runs first and
    /// completely unmodified; the global state is a passive observer of what the
    /// game already did. Nothing here changes what the game sees, which is the whole
    /// point of the design: the global state is write-only.</para>
    ///
    /// <para><b>Nothing escapes into game code.</b> Everything the postfix does is
    /// inside a catch-all - the recording under this hook's own, and the HUD refresh
    /// that follows under the display hook's. A mod that corrupts a playthrough is
    /// worse than a mod that stops tracking, so a failure here costs tracking and
    /// nothing else. Repeated failures stop being logged, and then stop being
    /// attempted, rather than producing one log line per line of dialogue for the rest
    /// of the session.</para>
    /// </remarks>
    [HarmonyPatch(typeof(DialogueLua), nameof(DialogueLua.MarkDialogueEntry))]
    internal static class MarkDialogueEntryPatch
    {
        private static GlobalStateSession? _session;
        private static HookFailureLimiter? _failures;

        /// <summary>
        /// Applies the patch. Call once, from plugin load, after the session exists.
        /// </summary>
        /// <param name="harmony">The plugin's Harmony instance.</param>
        /// <param name="session">The session every mark is recorded into.</param>
        /// <param name="log">Where hook failures are reported.</param>
        /// <exception cref="ArgumentNullException">Any argument is null.</exception>
        /// <exception cref="Exception">
        /// Harmony could not patch the method - it was not found, or the detour
        /// failed. The caller decides what that means; nothing is hooked either way.
        /// </exception>
        internal static void Install(Harmony harmony, GlobalStateSession session, IGlobalStateLog log)
        {
            if (harmony == null)
            {
                throw new ArgumentNullException(nameof(harmony));
            }

            _session = session ?? throw new ArgumentNullException(nameof(session));
            _failures = new HookFailureLimiter("recording dialogue statuses into the global state", log);
            harmony.PatchAll(typeof(MarkDialogueEntryPatch));
        }

        /// <summary>
        /// Runs after the game has written the status to its own per-save Lua table.
        /// </summary>
        /// <remarks>
        /// The parameter names are matched against the patched method by Harmony, so
        /// they have to stay <c>dialogueEntry</c> and <c>status</c>.
        /// </remarks>
        [HarmonyPostfix]
        private static void MarkDialogueEntryPostfix(DialogueEntry dialogueEntry, string status)
        {
            GlobalStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if (session == null || failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                if (dialogueEntry == null)
                {
                    // The game marking a null entry is its own business; there is
                    // nothing to record and nothing has gone wrong on our side.
                    return;
                }

                session.Record(dialogueEntry.conversationID, dialogueEntry.id, status);
            }
            catch (Exception ex)
            {
                failures.Report(ex);
                return;
            }

            // Outside the catch on purpose: the display has its own failure budget,
            // and a HUD that cannot draw itself must not be able to spend the one
            // that keeps tracking alive. This call reports its own failures and
            // never throws.
            MainHudDialogueCountPatch.RefreshDisplayedCounts();
        }
    }
}
