using System;
using HarmonyLib;
using Il2CppInterop.Runtime.InteropTypes.Arrays;
using PixelCrushers.DialogueSystem;
using GlobalConversationTracker.Session;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The load-time hook: a savegame load rebuilds the game's SimStatus table behind
    /// <c>MarkDialogueEntry</c>'s back, so the global state is resynced from the raw
    /// save bytes the game is about to apply.
    /// </summary>
    [HarmonyPatch(
        typeof(PersistentDataManager),
        nameof(PersistentDataManager.ApplyRawData))]
    internal static class ApplyRawDataPatch
    {
        private static GlobalStateSession? _session;
        private static HookFailureLimiter? _failures;

        /// <summary>
        /// Applies the patch. Call once, from plugin load, after the session exists.
        /// </summary>
        /// <param name="harmony">The plugin's Harmony instance.</param>
        /// <param name="session">The session that is resynced after every load.</param>
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
            _failures = new HookFailureLimiter(
                "resyncing the global state after a savegame load", log);
            harmony.PatchAll(typeof(ApplyRawDataPatch));
        }

        /// <summary>
        /// Runs once the game has the raw save file bytes in hand, before it applies
        /// them.
        /// </summary>
        [HarmonyPrefix]
        private static void ApplyRawDataPrefix(Il2CppStructArray<byte> bytes)
        {
            GlobalStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if (session == null || failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                session.ResyncFromSaveRawBytes(bytes);
            }
            catch (Exception ex)
            {
                failures.Report(ex);
                return;
            }

            // A load is the other way the count moves, so the HUD is told here too.
            // Outside the catch on purpose: the display has its own failure budget,
            // and this call reports its own failures and never throws.
            MainHudDialogueCountPatch.RefreshDisplayedCounts();
        }
    }
}
