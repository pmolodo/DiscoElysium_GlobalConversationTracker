using System;
using BepInEx;
using BepInEx.Unity.IL2CPP;
using UnifiedConversationTracker.Persistence;
using UnifiedConversationTracker.Session;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// BepInEx entry point for the Unified Conversation Tracker mod.
    /// </summary>
    /// <remarks>
    /// <para><b>What Load does, and deliberately does not do.</b> It resolves the
    /// SaveGames directory and builds the <see cref="UnifiedStateSession"/>. It does
    /// not read the state file and it does not read the game. Both of those happen
    /// on first access, through <see cref="UnifiedStateSession.EnsureInitialized"/>.
    /// </para>
    /// <para>
    /// That split is not tidiness, it is correctness. Seeding an empty unified state
    /// from the running game only produces anything after a savegame has been
    /// loaded, because <c>PersistentDataManager</c> rewrites the whole Lua SimStatus
    /// table at load time (de-0s5). At chainload there is no dialogue system, no
    /// database and no save; seeding there would copy nothing, and the file it wrote
    /// would then suppress the seed forever. The trigger therefore belongs to the
    /// write-through hook on <c>DialogueLua.MarkDialogueEntry</c> (de-omm.8), which
    /// can only fire inside a running conversation, and so only after a game is in
    /// play.
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

        public override void Load()
        {
            Log.LogMessage($"{PluginName} v{PluginVersion} loaded.");

            string saveGameDirectory = SaveGameDirectory.Resolve(Log);
            var store = new UnifiedStateStore(saveGameDirectory);
            Log.LogMessage($"Unified state file: {store.LivePath}");

            _session = new UnifiedStateSession(
                store,
                new DialogueLuaSimStatusSource(),
                new BepInExUnifiedStateLog(Log));
        }
    }
}
