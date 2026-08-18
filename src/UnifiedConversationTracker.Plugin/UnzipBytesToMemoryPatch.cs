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
        typeof(SunshinePersistenceFileManager),
        nameof(SunshinePersistenceFileManager.UnzipBytesToMemory))]
    internal static class UnzipBytesToMemoryPatch
    {
        private static UnifiedStateSession? _session;
        private static HookFailureLimiter? _failures;

        /// <summary>
        /// Time of the last postfix call
        /// <summary/>
        public static long lastPostfixTime = 0;

        private const string _methodName = "SunshinePersistenceFileManager.UnzipBytesToMemory";

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
                $"installing '{_methodName}' timing patches", log);
            harmony.PatchAll(typeof(UnzipBytesToMemoryPatch));
        }

        /// <summary>
        /// Runs after each invocation of the UnzipBytesToMemory method.
        /// </summary>
        [HarmonyPostfix]
        private static void UnzipBytesToMemoryPostfix()
        {
            UnifiedStateSession? session = _session;
            HookFailureLimiter? failures = _failures;
            if (session == null || failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                lastPostfixTime = Stopwatch.GetTimestamp();
            }
            catch (Exception ex)
            {
                failures.Report(ex);
            }
        }
    }
}
