using System;
using System.Diagnostics;
using System.Globalization;
using System.Numerics;
using HarmonyLib;
using UnifiedConversationTracker.Session;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// Installs timing hooks for measuring load times.
    /// </summary>
    [HarmonyPatch(
        typeof(SunshinePersistenceLuaDataCollector),
        nameof(SunshinePersistenceLuaDataCollector.InitializeMissingVariableValues))]
    internal static class InitializeMissingVariableValuesPatch
    {
        private static UnifiedStateSession? _session;
        private static HookFailureLimiter? _failures;
        private static IUnifiedStateLog? _log;

        private static long _numStarts = 0;
        private static long _totalEnvelopeTicks = 0;

        private const string _methodName = "SunshinePersistenceLuaDataCollector.InitializeMissingVariableValues";

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
            _log = log;
            if (harmony == null)
            {
                throw new ArgumentNullException(nameof(harmony));
            }

            _session = session ?? throw new ArgumentNullException(nameof(session));
            _failures = new HookFailureLimiter(
                $"installing '{_methodName}' timing patches", log);
            harmony.PatchAll(typeof(InitializeMissingVariableValuesPatch));
        }

        /// <summary>
        /// Runs before each invocation of the InitializeMissingVariableValues method
        /// </summary>
        [HarmonyPrefix]
        private static void InitializeMissingVariableValuesPrefix()
        {
            UnifiedStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if (session == null || failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                _numStarts++;
                long now = Stopwatch.GetTimestamp();
                // Because these functions always seem to run in this order
                // (though with other unknown stuff inbetween):
                //
                // - UzipBytesToMemory
                // - ApplyRawData
                // - InitializeMissingVariableValues
                //
                // ...I use the time between:
                //
                // - UzipBytesToMemory (postfix)
                // - InitializeMissingVariableValues (prefix)
                //
                // as an upper-envelope to estimate the time ApplyRawData
                // takes to run, INCLUDING the effects of any modifications
                // to ApplyRawData, such as marshalling data for the prefix's
                // args (ie, the large byte-array)
                long envelopeTicks = now - UnzipBytesToMemoryPatch.lastPostfixTime;
                _totalEnvelopeTicks += envelopeTicks;
                _log?.Info($"Run {_numStarts} of '{_methodName}' prefix: {now}");
                _log?.Info($"  Estimated envelope for ApplyRawData: {Ms(envelopeTicks)}");
                _log?.Info($"  Average envelope for ApplyRawData  : {Ms(_totalEnvelopeTicks / _numStarts)} ({_numStarts} calls)");
            }
            catch (Exception ex)
            {
                failures.Report(ex);
            }
        }

        private static string Ms<T>(T ticks) where T : struct =>
            $" {(Convert.ToDouble(ticks) * 1000.0 / Stopwatch.Frequency).ToString("N2", CultureInfo.InvariantCulture)} ms";
    }
}
