// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using HarmonyLib;
using GlobalConversationTracker.Persistence;
using GlobalConversationTracker.Session;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The orb load hook: when a savegame is loaded, the orbs it already holds are
    /// counted, rather than staying invisible until they are opened again.
    /// </summary>
    /// <remarks>
    /// <para><b>Why this is a separate hook from <see cref="ApplyRawDataPatch"/>.</b>
    /// The two halves of a save are two files. Dialogue SimStatus is in
    /// <c>{save}.ntwtf.lua</c>, which the game hands to
    /// <c>PersistentDataManager.ApplyRawData</c> as bytes; orbs are in
    /// <c>{save}.states.lua</c>, which the game does not parse at all - it executes it
    /// as a Lua script. The raw dialogue bytes contain no orbs, so no amount of reading
    /// them harder would find one.</para>
    ///
    /// <para><b>Why this method.</b>
    /// <c>SunshinePersistence.SaveGameLoadedData.GetString(string filename)</c>
    /// is how the Final Cut load path gets the text of one file out of the save it has
    /// unzipped into memory - it replaces the pre-Final-Cut
    /// <c>SunshinePersistenceFileManager.ReadCustomLuaFromFile</c>, which no longer
    /// exists. A postfix on it sees the states file's contents on the way past, as a
    /// plain string. That is the safest thing that can cross the IL2CPP boundary, and
    /// it lets the parsing be ordinary C# with tests against real save files rather
    /// than an interop walk of a live Lua table that could only be tested by running
    /// the game.</para>
    ///
    /// <para><b>Reading the text rather than the table also removes an ordering
    /// hazard.</b> The text is the whole answer whether or not the game has executed it
    /// yet, so there is no question of having looked at <c>ShownOrbs</c> too early and
    /// mistaken a not-yet-populated table for a save with no orbs.</para>
    ///
    /// <para><b>Any other file is left alone.</b> The suffix is checked before anything
    /// is parsed, so a call for one of the save's JSON parts does not reach the session
    /// and cannot be mistaken for a states file with no orbs in it.</para>
    /// </remarks>
    [HarmonyPatch(
        typeof(SunshinePersistence.SaveGameLoadedData),
        nameof(SunshinePersistence.SaveGameLoadedData.GetString))]
    internal static class LoadedOrbsPatch
    {
        private static GlobalStateSession? _session;
        private static HookFailureLimiter? _failures;
        private static IGlobalStateLog? _log;

        /// <summary>
        /// Applies the patch. Call once, from plugin load, after the session exists.
        /// </summary>
        /// <param name="harmony">The plugin's Harmony instance.</param>
        /// <param name="session">The session the loaded save's orbs are resynced into.</param>
        /// <param name="log">Where hook failures are reported.</param>
        /// <exception cref="ArgumentNullException">Any argument is null.</exception>
        /// <exception cref="Exception">
        /// Harmony could not patch the method - it was not found, or the detour failed.
        /// </exception>
        internal static void Install(Harmony harmony, GlobalStateSession session, IGlobalStateLog log)
        {
            if (harmony == null)
            {
                throw new ArgumentNullException(nameof(harmony));
            }

            _session = session ?? throw new ArgumentNullException(nameof(session));
            _log = log ?? throw new ArgumentNullException(nameof(log));
            _failures = new HookFailureLimiter(
                "resyncing orbs from a loaded savegame", log);
            harmony.PatchAll(typeof(LoadedOrbsPatch));
        }

        /// <summary>
        /// Runs after the game has pulled one file's text out of the loaded save.
        /// </summary>
        /// <remarks>
        /// Harmony matches injected parameters against the patched method BY NAME, so
        /// <paramref name="filename"/> has to keep exactly that name - it is the name the
        /// interop assembly gives that parameter, and a wrong one fails the patch at
        /// install time rather than at the call site.
        /// </remarks>
        [HarmonyPostfix]
        private static void GetStringPostfix(string filename, string __result)
        {
            GlobalStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if (session == null || failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                if (!ShownOrbsParser.IsStatesFile(filename))
                {
                    return;
                }

                if (__result == null)
                {
                    _log?.Warning(
                        $"The loaded save's '{ShownOrbsParser.StatesFileSuffix}' came back null, so "
                        + "its orbs were not counted. The orbs it holds will be counted if they are "
                        + "opened again.");
                    return;
                }

                List<string> titles = ShownOrbsParser.GetSeenOrbTitles(__result);
                session.ResyncOrbs(titles);
            }
            catch (Exception ex)
            {
                failures.Report(ex);
                return;
            }

            // Outside the catch on purpose, as in the other hooks: the display has its
            // own failure budget and reports its own failures, and never throws.
            MainHudDialogueCountPatch.RefreshDisplayedCounts();
        }
    }
}
