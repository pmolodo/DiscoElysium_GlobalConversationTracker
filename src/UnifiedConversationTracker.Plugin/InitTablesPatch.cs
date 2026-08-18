using System;
using System.Diagnostics;
using System.Globalization;
using System.Numerics;
using HarmonyLib;
using Sunshine.Dialogue;
using UnifiedConversationTracker.Session;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// Installs timing hooks for measuring load times.
    /// </summary>
    [HarmonyPatch(
        typeof(GenericLuaFunctions),
        nameof(GenericLuaFunctions.InitTables))]
    internal static class InitTablesPatch
    {
        private static UnifiedStateSession? _session;
        private static HookFailureLimiter? _failures;
        private static IUnifiedStateLog? _log;

        private static long _numStarts = 0;
        private static long _lastPrefixTime = 0;
        private static long _lastPostfixTime = 0;
        private static long _totalInCallTicks = 0;

        private const string _methodName = "GenericLuaFunctions.InitTables";

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
            harmony.PatchAll(typeof(InitTablesPatch));
        }

        /// <summary>
        /// Runs before each invocation of the InitTables method
        /// </summary>
        [HarmonyPrefix]
        private static void InitTablesPrefix()
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
                _log?.Info($"Run {_numStarts} of '{_methodName}' prefix: {now}");
                if (_numStarts != 1)
                {
                    _log?.Info($"  Time since last '{_methodName}' finished: {Ms(now - _lastPostfixTime)}");
                    _log?.Info($"  Total time since last '{_methodName}' started: {Ms(now - _lastPrefixTime)}");
                }
                _lastPrefixTime = now;
            }
            catch (Exception ex)
            {
                failures.Report(ex);
            }
        }

        /// <summary>
        /// Runs after each invocation of the InitTables method.
        /// </summary>
        [HarmonyPostfix]
        private static void InitTablesPostfix()
        {
            UnifiedStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if (session == null || failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                long now = Stopwatch.GetTimestamp();
                long inCallTicks = now - _lastPrefixTime;
                _totalInCallTicks += inCallTicks;
                _log?.Info($"  '{_methodName}' postfix: {now}");
                _log?.Info($"  '{_methodName}' In-call: {Ms(inCallTicks)}");
                _log?.Info($"  '{_methodName}' Average In-call: {Ms(_totalInCallTicks / _numStarts)} ({_numStarts} calls)");
                _lastPostfixTime = now;
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
