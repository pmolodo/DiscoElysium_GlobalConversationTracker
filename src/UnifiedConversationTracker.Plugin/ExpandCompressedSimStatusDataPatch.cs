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
    /// <remarks>
    /// <para><b>Why this method, out of the whole load path.</b>
    /// <c>PersistentDataManager.ExpandCompressedSimStatusData</c> is the method that
    /// does the rewriting: it clears each conversation's Lua <c>Dialog</c> table and
    /// rebuilds it from the save's compressed <c>SimX</c> strings. Nothing that runs
    /// after it in a load writes SimStatus - <c>InitializeNewSimStatusFromDatabase</c>
    /// is an empty method, and the game's own code (checked across the whole of
    /// Assembly-CSharp) never writes SimStatus at all - so a postfix here is late
    /// enough by construction, and a single patch covers both load paths.</para>
    ///
    /// <para><b>Why not the alternatives.</b>
    /// <c>ApplySimStatusFromRawData</c> looks like the natural hook for the byte
    /// save format the shipping game uses, but it is DEAD CODE in the Final Cut
    /// IL2CPP build: it still has a body of its own, and that body does call
    /// <c>ExpandCompressedSimStatusData</c>, but nothing anywhere in the ISIL dump
    /// ever calls it - <c>ApplyRawData</c> calls <c>ExpandCompressedSimStatusData</c>
    /// directly instead. Patching it would silently never fire on a real load.
    /// Patching <c>ApplySaveData</c> and <c>ApplyRawData</c> themselves would take
    /// two patches to say what one says here, and would still miss nothing extra:
    /// they are the only two live callers.</para>
    ///
    /// <para><b>It can fire more than once per load.</b> The game's own load
    /// coroutine, <c>SunshinePersistence.LoadCoR</c>, calls <c>ApplySaveData</c> and
    /// then <c>ApplyRawData</c>, each behind its own check on a different field of
    /// the loaded save. Whether a single load reaches both was not established -
    /// that needs the game running, which no one has done for this. If it does, the
    /// second resync walks the database again and raises nothing, because the first
    /// one already merged it. Correctness is unaffected either way, since the merge
    /// only ever raises and the write is skipped when nothing was raised, but the
    /// walk is not free (measured in-game at 1744 ms, de-p1h) and a doubled one is
    /// tracked as de-cvq.</para>
    ///
    /// <para><b>Postfix, and nothing escapes into game code.</b> The game's own
    /// rewrite happens first and completely unmodified; the unified state is a
    /// passive observer. Everything this postfix does is inside a catch-all, and it
    /// runs inside the load coroutine's own try block, so an exception escaping here
    /// would abort the player's load and show them an error dialog. That must never
    /// happen for a tracking failure.</para>
    /// </remarks>
    [HarmonyPatch(
        typeof(PersistentDataManager),
        nameof(PersistentDataManager.ExpandCompressedSimStatusData))]
    internal static class ExpandCompressedSimStatusDataPatch
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
            harmony.PatchAll(typeof(ExpandCompressedSimStatusDataPatch));
        }

        /// <summary>
        /// Runs after the game has finished rebuilding its SimStatus tables from a
        /// save.
        /// </summary>
        [HarmonyPostfix]
        private static void ExpandCompressedSimStatusDataPostfix()
        {
            UnifiedStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if (session == null || failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                session.ResyncFromGame();
            }
            catch (Exception ex)
            {
                failures.Report(ex);
            }
        }
    }
}
