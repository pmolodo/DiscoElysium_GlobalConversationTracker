// SPDX-License-Identifier: MIT
using System;
using HarmonyLib;
using GlobalConversationTracker.Session;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The new-game hook: when the game throws its world state away and starts over,
    /// the count for "this save" starts over with it.
    /// </summary>
    /// <remarks>
    /// <para>The current-save tally is established from a savegame's own rows on load,
    /// and kept live off <c>MarkDialogueEntry</c> during play. A new game is neither: it
    /// rebuilds the whole Lua Conversation table at once, so no mark arrives and no save
    /// is loaded. Without this hook the previous save's figure would sit on the HUD
    /// through a fresh playthrough.</para>
    ///
    /// <para><c>World.ResetStates</c> is the game's own idea of the world state starting
    /// over, and at 326 instructions in the shipping binary it is far too large for
    /// IL2CPP to have inlined into its callers - which matters, because a patch that
    /// applies cleanly and never fires looks identical to one that works.
    /// <c>NewGameCoR</c> is a coroutine, so the body worth patching is a
    /// compiler-generated <c>MoveNext</c>; the Dialogue System's own table rebuilds also
    /// run at boot.</para>
    ///
    /// <para>Only the current save is reset. The across-all-saves state is untouched - a
    /// new game is precisely the event it exists to survive.</para>
    ///
    /// <para>It does not fire on a savegame load, which is what makes it safe: a reset
    /// running after a load's resync would empty a tally that had just been filled.
    /// <see cref="GlobalStateSession.ResetCurrentSave"/> logs how long it has been since
    /// the last resync, so that stays checkable. Only the main-menu load path has been
    /// exercised; loading from inside a running game has not.</para>
    /// </remarks>
    internal static class NewGameResetPatch
    {
        /// <summary>How the reset names itself in the log.</summary>
        private const string Trigger = "World.ResetStates (new game)";

        private static GlobalStateSession? _session;
        private static HookFailureLimiter? _failures;

        /// <summary>
        /// Applies the patch. Call once, from plugin load, after the session exists.
        /// </summary>
        /// <param name="harmony">The plugin's Harmony instance.</param>
        /// <param name="session">The session whose current-save tally is reset.</param>
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
                "resetting the current-save dialogue count when a new game starts", log);
            harmony.PatchAll(typeof(NewGameResetPatch));
        }

        /// <summary>
        /// Runs after the game has reset its world state, which is after it has
        /// discarded whatever the previous save had reached.
        /// </summary>
        [HarmonyPatch(typeof(World), nameof(World.ResetStates))]
        [HarmonyPostfix]
        private static void ResetStatesPostfix()
        {
            GlobalStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if (session == null || failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                session.ResetCurrentSave(Trigger);
            }
            catch (Exception ex)
            {
                failures.Report(ex);
                return;
            }

            // Outside the catch on purpose: the display has its own failure budget,
            // and this call reports its own failures and never throws.
            MainHudDialogueCountPatch.RefreshDisplayedCounts();
        }
    }
}
