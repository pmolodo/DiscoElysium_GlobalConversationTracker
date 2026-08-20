using System;
using System.Diagnostics;
using System.IO;
using BepInEx;
using BepInEx.Unity.IL2CPP;
using HarmonyLib;
using UnifiedConversationTracker.Persistence;
using UnifiedConversationTracker.Session;
using UnityEngine;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// BepInEx entry point for the Unified Conversation Tracker mod.
    /// </summary>
    /// <remarks>
    /// <para><b>What Load does, and deliberately does not do.</b> It resolves the
    /// SaveGames directory, reads the optional articy id map, builds the
    /// <see cref="UnifiedStateSession"/> and installs the two hooks. It does not read
    /// the state file and it does not read the game. Both of those happen on first
    /// access, through <see cref="UnifiedStateSession.EnsureInitialized"/>, which
    /// either hook calls on its way in.
    /// </para>
    /// <para><b>The map is the one thing read here on purpose</b>, and it is the
    /// exception that proves the rule: it is a pure function of the dialogue database
    /// rather than of the game's live state, so there is nothing to wait for, and
    /// reading it later would put it on the load path it exists to shorten. See
    /// <see cref="TryCreateInterceptor"/>.
    /// </para>
    /// <para><b>Two hooks, because there are two SimStatus writers.</b>
    /// <see cref="MarkDialogueEntryPatch"/> is the write-through hook for everything
    /// the game does while playing (de-omm.8).
    /// <see cref="ExpandCompressedSimStatusDataPatch"/> covers the one writer that
    /// never goes through it: <c>PersistentDataManager</c> rebuilding the whole Lua
    /// SimStatus table when a savegame is loaded (de-0s5). Between them they see
    /// every write; nothing else in the game writes SimStatus.
    /// </para>
    /// <para>
    /// Deferring the disk and game reads to first access is not tidiness, it is
    /// correctness. At chainload there is no dialogue system, no database and no
    /// save, so reading the game there would copy an all-Untouched table and record
    /// nothing. Both triggers therefore belong to the hooks, each of which can only
    /// fire once a game is in play - and the bulk read specifically belongs to the
    /// load hook, which fires at the one moment the game's SimStatus values are
    /// known to be real (de-omm.23).
    /// </para>
    /// </remarks>
    [BepInPlugin(PluginGuid, PluginName, PluginVersion)]
    public class UnifiedConversationTrackerPlugin : BasePlugin
    {
        public const string PluginGuid = "com.molodowitch.unifiedconversationtracker";
        public const string PluginName = "UnifiedConversationTracker";
        public const string PluginVersion = "0.1.0";

        /// <summary>Unity's own "the player is quitting" event, as it reads in the log.</summary>
        private const string ApplicationQuittingTrigger = "Application.quitting";

        /// <summary>The BCL's graceful-runtime-shutdown event, as it reads in the log.</summary>
        private const string ProcessExitTrigger = "AppDomain.ProcessExit";

        /// <summary>BepInEx's plugin unload, as it reads in the log if a host ever calls it.</summary>
        private const string UnloadTrigger = "BasePlugin.Unload";

        /// <summary>
        /// The articy id map, looked for next to this DLL. Not shipped with the mod:
        /// its provenance has not been audited (de-0m0.21), so it is something a user
        /// puts there deliberately, and everything works without it.
        /// </summary>
        private const string ArticyIdMapFileName = "articy_ids_final_cut.json";

        /// <summary>
        /// What the log says the load path does when there is no map. One string
        /// because every way of failing to get one ends the same way, and because
        /// de-0m0.25 deletes all of them together.
        /// </summary>
        private const string WithoutTheMapNotice =
            "Savegame loads will resync by walking the whole master database instead, which the "
            + "only in-game measurement there is puts at 1892 ms (de-0m0.17).";

        /// <summary>
        /// The session for this run of the game, available from the moment
        /// <see cref="Load"/> returns. The hook calls
        /// <see cref="UnifiedStateSession.EnsureInitialized"/> on it before every
        /// merge; it is idempotent and cheap after the first call.
        /// </summary>
        /// <exception cref="InvalidOperationException">The plugin has not loaded.</exception>
        public static UnifiedStateSession Session =>
            _session ?? throw new InvalidOperationException(
                $"{PluginName} has not finished loading; there is no unified state session yet.");

        private static UnifiedStateSession? _session;

        private Harmony? _harmony;

        public override void Load()
        {
            Log.LogMessage($"{PluginName} v{PluginVersion} loaded.");

            string saveGameDirectory = SaveGameDirectory.Resolve(Log);
            var store = new UnifiedStateStore(saveGameDirectory);
            Log.LogMessage($"Unified state file: {store.LivePath}");

            var log = new BepInExUnifiedStateLog(Log);
            var session = new UnifiedStateSession(store, log);
            _session = session;

            var harmony = new Harmony(PluginGuid);
            _harmony = harmony;

            bool recording = TryInstall(
                "DialogueLua.MarkDialogueEntry",
                "dialogue statuses are being tracked",
                "Dialogue seen during play will not be recorded this session",
                () => MarkDialogueEntryPatch.Install(harmony, session, log));

            bool resyncing = TryInstall(
                "PersistentDataManager.ApplyRawData",
                "the unified state is resynced whenever a savegame is loaded (using raw file bytes)",
                "Statuses restored by loading a savegame will be missed this session",
                () => ApplyRawDataPatch.Install(harmony, session, log));

            bool UnzipBytesToMemoryTiming = TryInstall(
                "SunshinePersistenceFileManager.UnzipBytesToMemory",
                "timing of SunshinePersistenceFileManager.UnzipBytesToMemory is being tracked",
                "Timing of SunshinePersistenceFileManager.UnzipBytesToMemory is untracked",
                () => UnzipBytesToMemoryPatch.Install(harmony, session, log));

            bool InitializeMissingVariableValuesTiming = TryInstall(
                "SunshinePersistenceLuaDataCollector.InitializeMissingVariableValues",
                "timing of SunshinePersistenceLuaDataCollector.InitializeMissingVariableValues is being tracked",
                "Timing of SunshinePersistenceLuaDataCollector.InitializeMissingVariableValues is untracked",
                () => InitializeMissingVariableValuesPatch.Install(harmony, session, log));

            if (!recording && !resyncing && !UnzipBytesToMemoryTiming && !InitializeMissingVariableValuesTiming)
            {
                _harmony = null;
            }

            RegisterShutdownFlush(session);
        }

        /// <summary>
        /// Arranges for the unified state to be flushed when the game goes away.
        /// </summary>
        /// <remarks>
        /// <para><b>Why this is needed at all.</b> The unified state is written by a
        /// background thread now (de-omm.22), so at any moment the last few marks may
        /// be in memory and not yet on disk. Losing those to a hard crash is an
        /// accepted, recorded cost - every one of them is re-marked the next time the
        /// line is reached - but losing them when the player simply quits is not.</para>
        ///
        /// <para><b>Why not <see cref="Unload"/>.</b> BepInEx's IL2CPP chainloader
        /// never calls it: <c>IL2CPPChainloader</c> calls <c>Load()</c> on every
        /// plugin and has no unload path at all. It is overridden below anyway,
        /// because a host that does call it should get a clean shutdown, but nothing
        /// may depend on it.</para>
        ///
        /// <para><b>Two events, because neither is guaranteed on its own.</b>
        /// <c>Application.quitting</c> is Unity's own "the player is quitting" signal
        /// and fires on the main thread while the engine is still up, which is the
        /// right moment; it is reached through IL2CPP interop, so it is registered
        /// defensively. <c>AppDomain.ProcessExit</c> is plain BCL and touches nothing
        /// of Unity's, but it only fires if the hosted runtime gets a graceful
        /// shutdown, which a Unity player exiting through native code may not give it.
        /// Both funnel into the same idempotent call, so firing twice, once, or in
        /// either order all behave the same.</para>
        ///
        /// <para><b>Which one actually fires is an open question, so the log answers
        /// it</b> (de-6fi). Each handler passes its own name into
        /// <see cref="UnifiedStateSession.Shutdown"/>, which logs on arrival and again
        /// on completion; a second trigger reports that the first already did the work.
        /// This line - the one that says what was registered - is the other half: a log
        /// showing a registration and no trigger says the event never fired, which is
        /// the finding <c>de-0m0.4</c> is waiting on.</para>
        /// </remarks>
        private void RegisterShutdownFlush(UnifiedStateSession session)
        {
            bool quittingSubscribed = true;
            try
            {
                Application.quitting += (Action)(() => FlushOnShutdown(session, ApplicationQuittingTrigger));
            }
            catch (Exception ex)
            {
                quittingSubscribed = false;
                Log.LogWarning(
                    $"Could not subscribe to {ApplicationQuittingTrigger}: {ex}. The unified state will "
                    + "still be flushed at process exit if the runtime shuts down cleanly.");
            }

            AppDomain.CurrentDomain.ProcessExit += (_, _) => FlushOnShutdown(session, ProcessExitTrigger);

            string registered = quittingSubscribed
                ? $"{ApplicationQuittingTrigger} and {ProcessExitTrigger}"
                : ProcessExitTrigger;
            Log.LogMessage(
                $"Shutdown flush registered on {registered}. Whichever fires first flushes and names "
                + "itself in the log; any later one reports that it had nothing left to do. No such "
                + "line at the end of a session means neither event ever fired.");
        }

        /// <summary>
        /// Flushes and stops the session's writer, reporting failures rather than
        /// throwing out of a shutdown handler.
        /// </summary>
        /// <remarks>
        /// The report itself is guarded too. Since de-6fi the shutdown path logs on
        /// every outcome rather than only on failure, and one of the two triggers is
        /// <c>AppDomain.ProcessExit</c>, where BepInEx's own log sink may already be
        /// tearing itself down. Throwing out of a process-exit handler over a failed
        /// log line would be a strictly worse outcome than the missing line.
        /// </remarks>
        private void FlushOnShutdown(UnifiedStateSession session, string trigger)
        {
            try
            {
                session.Shutdown(trigger);
            }
            catch (Exception ex)
            {
                try
                {
                    Log.LogError($"Failed to flush the unified state on {trigger}: {ex}");
                }
                catch (Exception)
                {
                    // Nowhere left to report to. The process is going away regardless.
                }
            }
        }

        /// <summary>
        /// Installs one hook, reporting a failure rather than taking the plugin down.
        /// </summary>
        /// <remarks>
        /// The two hooks are installed independently on purpose. They cover different
        /// SimStatus writers and neither depends on the other, so losing one is a
        /// partial loss of tracking rather than a reason to abandon the other.
        /// Failing to patch at all leaves the game exactly as it was, which is not
        /// worth taking anything down over.
        /// </remarks>
        /// <param name="target">The game method being hooked, for the log.</param>
        /// <param name="whatItBuys">What works because of it, for the log.</param>
        /// <param name="whatIsLost">What stops working without it, for the log.</param>
        /// <param name="install">Applies the patch.</param>
        /// <returns>True if the hook is in place.</returns>
        private bool TryInstall(string target, string whatItBuys, string whatIsLost, Action install)
        {
            try
            {
                install();
                Log.LogMessage($"Hooked {target}; {whatItBuys}.");
                return true;
            }
            catch (Exception ex)
            {
                Log.LogError($"Failed to hook {target}: {ex}. {whatIsLost}; the game is unaffected.");
                return false;
            }
        }

        public override bool Unload()
        {
            _harmony?.UnpatchSelf();
            _harmony = null;

            // Unpatch first: with the hooks gone nothing new can be recorded, so the
            // flush that follows is the last word rather than a race with the game.
            _session?.Shutdown(UnloadTrigger);
            return true;
        }
    }
}
