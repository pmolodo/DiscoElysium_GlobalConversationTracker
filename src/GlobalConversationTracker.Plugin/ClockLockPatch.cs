// SPDX-License-Identifier: MIT
using System;
using GlobalConversationTracker.Session;
using HarmonyLib;

namespace GlobalConversationTracker
{
    /// <summary>
    /// Watches whether the game's clock is locked, so a crawl can be told.
    /// </summary>
    /// <remarks>
    /// <para>THE LOCK IS THE MUST-SLEEP MECHANIC, not a normal state.
    /// <c>SunshineClockTime.LockTimeIfNeed</c> sets it when the player has crossed midnight
    /// without sleeping and the hour reaches two, and the game then freezes time until they
    /// do. During ordinary play the clock runs. A crawl told otherwise answers every hour
    /// question at a frozen clock and reports a branch closed that the player would walk -
    /// which is the one direction the look-ahead is not allowed to be wrong in.</para>
    ///
    /// <para>WHY A HOOK RATHER THAN A READ. <c>DaytimeLuaFunctions</c> registers nineteen
    /// functions and not one of them exposes the lock, so the Lua path <see cref="GameFacts"/>
    /// reads the hour through cannot answer this. The object itself is reached through
    /// <c>Voidforge.SingletonClass&lt;T&gt;.Singleton</c>, whose getter is
    /// <c>if (singleton == null) singleton = new T();</c> - so a reader that goes there and
    /// gets null through the interop layer does not fail, it CONSTRUCTS A FRESH CLOCK reading
    /// midnight on day one. That is de-3jec, and it is why nothing here goes near the
    /// singleton.</para>
    ///
    /// <para>A postfix on a method the game calls with the live clock in hand sidesteps both.
    /// <c>SunshineClock.Tick</c> calls <c>NormalTimeForward</c> every game-minute and
    /// <c>Clang</c> calls it for every <c>PassTime</c>, and it is called whether or not the
    /// clock is locked - the check is inside it - so a locked clock keeps reporting itself.
    /// <c>SetTimeAfterLoading</c> is hooked as well so a freshly loaded save is known before
    /// the first tick rather than after it.</para>
    ///
    /// <para>ON A METHOD WITH A BODY, deliberately. The obvious target is the
    /// <c>IsTimeLocked</c> getter, and a property that small is exactly what an IL2CPP build
    /// inlines - which would leave nothing to patch and no way to tell that from a clock that
    /// never ticks.</para>
    ///
    /// <para>UNTIL IT HAS FIRED, <see cref="Locked"/> is null and the callers send locked.
    /// That is the conservative answer and the one the mod has always given.</para>
    /// </remarks>
    [HarmonyPatch]
    internal static class ClockLockPatch
    {
        private static HookFailureLimiter? _failures;

        /// <summary>Whether the lock has been seen at all yet.</summary>
        private static bool _seen;

        /// <summary>What it said when it was last seen.</summary>
        private static bool _locked;

        /// <summary>
        /// Whether the game's clock is locked, or null where it has not been seen yet.
        /// </summary>
        internal static bool? Locked => _seen ? _locked : (bool?)null;

        /// <summary>
        /// Applies the patch. Call once, from plugin load.
        /// </summary>
        /// <param name="harmony">The plugin's Harmony instance.</param>
        /// <param name="log">Where hook failures are reported.</param>
        /// <exception cref="ArgumentNullException">Any argument is null.</exception>
        /// <exception cref="Exception">
        /// Harmony could not patch the method - it was not found, or the detour failed.
        /// </exception>
        internal static void Install(Harmony harmony, IGlobalStateLog log)
        {
            if (harmony == null)
            {
                throw new ArgumentNullException(nameof(harmony));
            }

            _failures = new HookFailureLimiter("reading whether the game's clock is locked", log);
            harmony.PatchAll(typeof(ClockLockPatch));
        }

        /// <summary>Runs after every clock tick, and after every <c>PassTime</c>.</summary>
        /// <param name="__instance">The game's clock.</param>
        [HarmonyPatch(typeof(SunshineClock), nameof(SunshineClock.NormalTimeForward))]
        [HarmonyPostfix]
        private static void NormalTimeForwardPostfix(SunshineClock __instance)
        {
            Observe(__instance);
        }

        /// <summary>Runs once a savegame's clock has been put in place.</summary>
        /// <param name="__instance">The game's clock.</param>
        [HarmonyPatch(typeof(SunshineClock), nameof(SunshineClock.SetTimeAfterLoading))]
        [HarmonyPostfix]
        private static void SetTimeAfterLoadingPostfix(SunshineClock __instance)
        {
            Observe(__instance);
        }

        /// <summary>Records what the clock says about itself.</summary>
        /// <param name="clock">The game's clock, as the hook was handed it.</param>
        private static void Observe(SunshineClock clock)
        {
            HookFailureLimiter? failures = _failures;
            if (failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                SunshineClockTime? time = clock?.Time;
                if (time == null)
                {
                    return;
                }

                _locked = time.IsTimeLocked;
                _seen = true;
            }
            catch (Exception ex)
            {
                failures.Report(ex);
            }
        }
    }
}
