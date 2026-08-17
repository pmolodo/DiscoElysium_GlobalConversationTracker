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
    /// walk is not free - 649 ms for the walk this one replaced and 1744 ms for this
    /// one, the only two in-game measurements there are, over the same rows and the
    /// same method (de-p1h.1) - and a doubled one is tracked as de-cvq.</para>
    ///
    /// <para><b>Two hooks on the one method, because the data exists twice.</b> The
    /// PREFIX is the fast path: the savegame's SimStatus is still in the compressed
    /// <c>Variable["Conversation_SimX_*"]</c> strings it arrived in, and reading
    /// ~1,500 of those is what <c>ExpandCompressedSimStatusData</c> is about to spend
    /// its time expanding. A postfix cannot see any of it - the same method nils every
    /// one of those variables through <c>Lua.Run</c> before it returns (de-0m0.19,
    /// confirmed against the Final Cut ISIL), and leaves no intermediate bulk form
    /// behind. The POSTFIX is the slow path that was here first: once expansion has
    /// finished, the same information is readable from the game's own tables, one
    /// interop crossing per row, 112,940 rows, measured at 1,892 ms (de-0m0.17).</para>
    ///
    /// <para><b>Nothing escapes into game code.</b> The game's own rewrite happens
    /// completely unmodified either way; the unified state is a passive observer that
    /// reads Lua values and calls nothing. Both hooks are wrapped in a catch-all, and
    /// they run inside the load coroutine's own try block, so an exception escaping
    /// here would abort the player's load and show them an error dialog. That must
    /// never happen for a tracking failure.</para>
    /// </remarks>
    [HarmonyPatch(
        typeof(PersistentDataManager),
        nameof(PersistentDataManager.ExpandCompressedSimStatusData))]
    internal static class ExpandCompressedSimStatusDataPatch
    {
        private static UnifiedStateSession? _session;
        private static HookFailureLimiter? _failures;

        /// <summary>
        /// Whether the prefix of the call currently in flight already merged this
        /// load's SimStatus, so the postfix must not walk as well.
        /// </summary>
        /// <remarks>
        /// A plain static is enough. <c>ExpandCompressedSimStatusData</c> is called
        /// from the game's load coroutine on the Unity main thread and is not
        /// re-entrant, so prefix and postfix always pair up on one thread; and the
        /// prefix clears it before doing anything, so a prefix that never ran and a
        /// prefix that failed both leave it false.
        /// </remarks>
        private static bool _interceptionMerged;

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
        /// Runs while the savegame's compressed SimStatus blobs are still there, which
        /// is only until this method's own <c>Lua.Run</c> nils them.
        /// </summary>
        [HarmonyPrefix]
        private static void ExpandCompressedSimStatusDataPrefix()
        {
            // Cleared first, so every way out of here below leaves the postfix to do
            // the work.
            _interceptionMerged = false;

            UnifiedStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if (session == null || failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                _interceptionMerged = session.TryResyncFromInterception();
            }
            catch (Exception ex)
            {
                failures.Report(ex);
            }
        }

        /// <summary>
        /// Runs after the game has finished rebuilding its SimStatus tables from a
        /// save.
        /// </summary>
        [HarmonyPostfix]
        private static void ExpandCompressedSimStatusDataPostfix()
        {
            // ------------------------------------------------------------------
            // PROVISIONAL FALLBACK SEAM (de-0m0.25).
            //
            // Everything below this comment is the previous route: walking the
            // master database after expansion, which is the only path with an
            // in-game measurement behind it (1,892 ms over 112,940 rows, de-0m0.17)
            // and the only reason a failed interception is not yet fatal.
            // Interception's own cost has never been observed in a running game.
            //
            // It is kept until interception has one clean in-game measurement, and
            // de-0m0.25 then deletes it: this method, this flag, and the branch
            // below. Nothing in the prefix path depends on it, so removing it is a
            // deletion rather than an unpicking. The prefix already reports its own
            // failures as errors, so nothing diagnostic is lost with it.
            // ------------------------------------------------------------------
            if (_interceptionMerged)
            {
                _interceptionMerged = false;
                return;
            }

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
