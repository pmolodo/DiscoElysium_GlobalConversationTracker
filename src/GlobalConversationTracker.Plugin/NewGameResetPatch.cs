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
    /// <para><b>Why this is needed at all.</b> The current-save tally is established
    /// from a savegame's own rows when one is loaded, and kept live off
    /// <c>MarkDialogueEntry</c> during play. A new game is neither: it rebuilds the
    /// game's whole Lua Conversation table at once rather than marking entries one at
    /// a time, so no mark arrives and no save is loaded. Without this hook the
    /// previous save's figure would simply sit on the HUD while the player starts a
    /// fresh playthrough.</para>
    ///
    /// <para><b>Why <c>World.ResetStates</c>.</b> It is the game's own idea of the
    /// world state starting over - <c>World.NewGameCoR</c> and
    /// <c>World.OnResetWorldState</c> sit beside it - and it is 326 instructions in
    /// the shipping binary, far too large for IL2CPP to have inlined into its callers.
    /// That size is the point: a patch that applies cleanly and never fires is the
    /// trap this project has already been caught by once. <c>NewGameCoR</c> was
    /// rejected because it is a coroutine, so the body worth patching is a
    /// compiler-generated <c>MoveNext</c>; the Dialogue System's own table rebuilds
    /// were rejected because they also run at boot.</para>
    ///
    /// <para><b>Only the current save is reset.</b> The across-all-saves state is
    /// untouched, and a new game is precisely the event it exists to survive.</para>
    ///
    /// <para><b>It does not fire on a savegame load, which was the one thing worth
    /// checking.</b> Had the game also reset world state during a load, and done it
    /// after that load's resync rather than before, this hook would have emptied a
    /// tally that had just been filled correctly. Settled from a log rather than
    /// guessed: a session that did nothing but load a save produced the resync line
    /// and no reset line at all. <see cref="GlobalStateSession.ResetCurrentSave"/>
    /// still reports how long it has been since the last resync, which is what makes
    /// that check repeatable - a reset reading "N s since the last resync" instead of
    /// "no savegame has been resynced this session" is the failure this survived.
    /// Only the main-menu load path has been exercised; loading from inside a running
    /// game has not.</para>
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
