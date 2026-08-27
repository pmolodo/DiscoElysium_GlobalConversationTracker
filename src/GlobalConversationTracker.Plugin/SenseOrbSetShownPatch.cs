// SPDX-License-Identifier: MIT
using System;
using HarmonyLib;
using GlobalConversationTracker.Session;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The orb write-through hook: every orb the player opens while playing goes into
    /// the global state on its way past.
    /// </summary>
    /// <remarks>
    /// <para><b>Why this method.</b> <c>SenseOrb.SetShown()</c> is the single funnel
    /// through which the running game records an opened orb - it is what writes
    /// <c>ShownOrbs[conversation] = {OrbSeen=1}</c>. Every click path reaches it:
    /// <c>OrbUiElement.Open</c> calls it directly for a plain orb and for a thought
    /// orb, and reaches it through <c>OrbUiElement.SetShown</c> and
    /// <c>SenseOrb.StartConversation</c> for the rest. It is public and non-virtual,
    /// and neither <c>ConditionalSenseOrb</c> nor <c>VisCalOrb</c> shadows it, so one
    /// patch covers the whole orb family.</para>
    ///
    /// <para><b>Postfix, not prefix</b>, for the same reason as
    /// <see cref="MarkDialogueEntryPatch"/>: the stock behavior runs first and
    /// unmodified, and this only watches.</para>
    ///
    /// <para><b>An orb with no conversation is skipped, and that is not a failure.</b>
    /// <c>SetShown</c> itself only writes to Lua when
    /// <c>conversation != null &amp;&amp; conversation.Length != 0</c>, so an orb with
    /// no conversation never reaches <c>ShownOrbs</c> and there is nothing to count.
    /// That is not a rare edge: a thought orb is instantiated from the orb template by
    /// <c>GlobalOrbManager.AddThought</c>, which sets its type and its name but never
    /// its conversation, so every thought orb the player clicks arrives here with an
    /// empty title. Skipping silently rather than reporting keeps the hook's failure
    /// budget for actual faults.</para>
    ///
    /// <para><b>Fires on every click, not only the first.</b> Only the Lua write inside
    /// <c>SetShown</c> is guarded by the current value, so re-opening an orb calls this
    /// again. Recording is idempotent - both the global state and the current-save
    /// tally hold orbs in sets - so the repeat costs a set probe and changes
    /// nothing.</para>
    ///
    /// <para><b>Nothing escapes into game code</b>, exactly as for the dialogue hook:
    /// the recording runs under this hook's own catch-all and failure budget, and the
    /// HUD refresh that follows under the display hook's.</para>
    /// </remarks>
    [HarmonyPatch(typeof(SenseOrb), nameof(SenseOrb.SetShown))]
    internal static class SenseOrbSetShownPatch
    {
        private static GlobalStateSession? _session;
        private static HookFailureLimiter? _failures;

        /// <summary>
        /// Applies the patch. Call once, from plugin load, after the session exists.
        /// </summary>
        /// <param name="harmony">The plugin's Harmony instance.</param>
        /// <param name="session">The session every opened orb is recorded into.</param>
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
            _failures = new HookFailureLimiter("recording opened orbs into the global state", log);
            harmony.PatchAll(typeof(SenseOrbSetShownPatch));
        }

        /// <summary>
        /// Runs after the game has recorded the orb in its own per-save Lua table.
        /// </summary>
        [HarmonyPostfix]
        private static void SetShownPostfix(SenseOrb __instance)
        {
            GlobalStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if (session == null || failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                if (__instance == null)
                {
                    return;
                }

                string? title = __instance.conversation;
                if (string.IsNullOrEmpty(title))
                {
                    // No conversation, so the game wrote no ShownOrbs row either. See
                    // the type's remarks: thought orbs are always this case.
                    return;
                }

                session.RecordOrb(title);
            }
            catch (Exception ex)
            {
                failures.Report(ex);
                return;
            }

            // Outside the catch on purpose, matching MarkDialogueEntryPatch: the display
            // has its own failure budget and must not spend the one that keeps tracking
            // alive. This call reports its own failures and never throws.
            MainHudDialogueCountPatch.RefreshDisplayedCounts();
        }
    }
}
