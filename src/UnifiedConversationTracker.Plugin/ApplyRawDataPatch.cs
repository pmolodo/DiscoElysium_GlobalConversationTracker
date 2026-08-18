using System;
using HarmonyLib;
using PixelCrushers.DialogueSystem;
using UnifiedConversationTracker.Session;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// The load-time hook: after a savegame has rewritten the game's SimStatus
    /// tables behind <c>MarkDialogueEntry</c>'s back, the unified state is resynced
    /// from the game (de-0s5).
    /// </summary>
    [HarmonyPatch(
        typeof(PersistentDataManager),
        nameof(PersistentDataManager.ApplyRawData))]
    internal static class ApplyRawDataPatch
    {
        private static UnifiedStateSession? _session;
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
        internal static void Install(Harmony harmony, UnifiedStateSession session, IUnifiedStateLog log)
        {
            if (harmony == null)
            {
                throw new ArgumentNullException(nameof(harmony));
            }

            _session = session ?? throw new ArgumentNullException(nameof(session));
            _failures = new HookFailureLimiter(
                "resyncing the unified state after a savegame load", log);
            harmony.PatchAll(typeof(ApplyRawDataPatch));
        }

        /// <summary>
        /// Runs once the game has read the raw save file bytes.
        /// </summary>
        [HarmonyPrefix]
        private static void ApplyRawDataPrefix(byte[] bytes)
        {
            UnifiedStateSession? session = _session;
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
            }
        }
    }
}
