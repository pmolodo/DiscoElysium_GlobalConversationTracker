using System;
using BepInEx;
using BepInEx.Unity.IL2CPP;
using HarmonyLib;
using UnifiedConversationTracker.Persistence;
using UnifiedConversationTracker.Session;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// BepInEx entry point for the Unified Conversation Tracker mod.
    /// </summary>
    /// <remarks>
    /// <para><b>What Load does, and deliberately does not do.</b> It resolves the
    /// SaveGames directory, builds the <see cref="UnifiedStateSession"/> and installs
    /// the two hooks. It does not read the state file and it does not read the game.
    /// Both of those happen on first access, through
    /// <see cref="UnifiedStateSession.EnsureInitialized"/>, which either hook calls on
    /// its way in.
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
    /// correctness. Seeding an empty unified state from the running game only
    /// produces anything after a savegame has been loaded, for exactly the de-0s5
    /// reason above. At chainload there is no dialogue system, no database and no
    /// save; seeding there would copy nothing, and the file it wrote would then
    /// suppress the seed forever. Both triggers therefore belong to the hooks, each
    /// of which can only fire once a game is in play.
    /// </para>
    /// </remarks>
    [BepInPlugin(PluginGuid, PluginName, PluginVersion)]
    public class UnifiedConversationTrackerPlugin : BasePlugin
    {
        public const string PluginGuid = "com.molodowitch.unifiedconversationtracker";
        public const string PluginName = "UnifiedConversationTracker";
        public const string PluginVersion = "0.1.0";

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
            var session = new UnifiedStateSession(store, new DialogueLuaSimStatusSource(), log);
            _session = session;

            var harmony = new Harmony(PluginGuid);
            _harmony = harmony;

            bool recording = TryInstall(
                "DialogueLua.MarkDialogueEntry",
                "dialogue statuses are being tracked",
                "Dialogue seen during play will not be recorded this session",
                () => MarkDialogueEntryPatch.Install(harmony, session, log));

            bool resyncing = TryInstall(
                "PersistentDataManager.ExpandCompressedSimStatusData",
                "the unified state is resynced whenever a savegame is loaded",
                "Statuses restored by loading a savegame will be missed this session",
                () => ExpandCompressedSimStatusDataPatch.Install(harmony, session, log));

            if (!recording && !resyncing)
            {
                _harmony = null;
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
            return true;
        }
    }
}
