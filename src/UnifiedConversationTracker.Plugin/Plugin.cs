using BepInEx;
using BepInEx.Unity.IL2CPP;

namespace UnifiedConversationTracker
{
    /// <summary>
    /// BepInEx entry point for the Unified Conversation Tracker mod.
    /// Currently a load-only skeleton: it logs a startup line and nothing else.
    /// </summary>
    [BepInPlugin(PluginGuid, PluginName, PluginVersion)]
    public class UnifiedConversationTrackerPlugin : BasePlugin
    {
        public const string PluginGuid = "com.molodowitch.unifiedconversationtracker";
        public const string PluginName = "UnifiedConversationTracker";
        public const string PluginVersion = "0.1.0";

        public override void Load()
        {
            Log.LogMessage($"{PluginName} v{PluginVersion} loaded.");
        }
    }
}
