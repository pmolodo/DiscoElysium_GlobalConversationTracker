// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics;
using System.IO;
using BepInEx;
using BepInEx.Unity.IL2CPP;
using HarmonyLib;
using GlobalConversationTracker.Persistence;
using GlobalConversationTracker.Session;
using UnityEngine;

namespace GlobalConversationTracker
{
    /// <summary>
    /// BepInEx entry point for the Global Conversation Tracker mod.
    /// </summary>
    /// <remarks>
    /// <para><see cref="Load"/> resolves the SaveGames directory, builds the
    /// <see cref="GlobalStateSession"/> and installs the hooks. It reads neither the
    /// state file nor the game: both happen on first access, through
    /// <see cref="GlobalStateSession.EnsureInitialized"/>, which every hook calls on its
    /// way in. That deferral is correctness, not tidiness - at chainload there is no
    /// dialogue system, no database and no save, so reading the game there would copy an
    /// all-Untouched table.</para>
    ///
    /// <para>Tracking hooks. <see cref="MarkDialogueEntryPatch"/> is write-through for
    /// everything the game does while playing; <see cref="ApplyRawDataPatch"/> covers
    /// the one writer that never goes through it, <c>PersistentDataManager</c>
    /// rebuilding the whole Lua SimStatus table on savegame load.
    /// <see cref="SenseOrbSetShownPatch"/> and <see cref="LoadedOrbsPatch"/> are the
    /// same pair for orbs. <see cref="NewGameResetPatch"/> covers the one event none of
    /// them see: a new game, which would otherwise leave the previous save's
    /// current-save count on screen. It resets that tally only.</para>
    ///
    /// <para>Display hooks. <see cref="MainHudDialogueCountPatch"/> puts the totals on
    /// the main HUD beside the money and clock; the tracking hooks tell it when a count
    /// has moved, which is the only coupling - nothing polls, and a display that never
    /// installed is a no-op to call. <see cref="NovelResponseColorPatch"/> reads the
    /// state one entry at a time, colouring a dialogue option no save has ever
    /// picked.</para>
    /// </remarks>
    [BepInPlugin(PluginGuid, PluginName, PluginVersion)]
    public class GlobalConversationTrackerPlugin : BasePlugin
    {
        /// <summary>
        /// The plugin's unique ID, as BepInEx logs it and as it keys the config file
        /// under BepInEx\config. Changing it orphans a player's existing settings.
        /// </summary>
        public const string PluginGuid = "com.molodowitch.globalconversationtracker";

        /// <summary>The plugin's display name, used in the log and in the config header.</summary>
        public const string PluginName = "GlobalConversationTracker";

        /// <summary>
        /// The plugin's version as BepInEx reports it. Updated by hand alongside the
        /// csproj's own Version, which is what the release zip is named after.
        /// </summary>
        public const string PluginVersion = "0.1.0";

        /// <summary>Unity's own "the player is quitting" event, as it reads in the log.</summary>
        private const string ApplicationQuittingTrigger = "Application.quitting";

        /// <summary>The BCL's graceful-runtime-shutdown event, as it reads in the log.</summary>
        private const string ProcessExitTrigger = "AppDomain.ProcessExit";

        /// <summary>BepInEx's plugin unload, as it reads in the log if a host ever calls it.</summary>
        private const string UnloadTrigger = "BasePlugin.Unload";

        /// <summary>
        /// The session for this run of the game, available from the moment
        /// <see cref="Load"/> returns. The hook calls
        /// <see cref="GlobalStateSession.EnsureInitialized"/> on it before every
        /// merge; it is idempotent and cheap after the first call.
        /// </summary>
        /// <exception cref="InvalidOperationException">The plugin has not loaded.</exception>
        public static GlobalStateSession Session =>
            _session ?? throw new InvalidOperationException(
                $"{PluginName} has not finished loading; there is no global state session yet.");

        private static GlobalStateSession? _session;

        private Harmony? _harmony;

        /// <summary>
        /// BepInEx's entry point, called once during chainload. Builds the session,
        /// installs each hook independently, and returns; nothing here reads the disk
        /// or the game, so a failure to hook costs tracking rather than the
        /// playthrough.
        /// </summary>
        public override void Load()
        {
            Log.LogMessage($"{PluginName} v{PluginVersion} loaded.");

            string saveGameDirectory = SaveGameDirectory.Resolve(Log);
            var store = new GlobalStateStore(saveGameDirectory);
            Log.LogMessage($"Global state file: {store.LivePath}");

            var log = new BepInExGlobalStateLog(Log);
            var session = new GlobalStateSession(store, log);
            _session = session;

            // One switch per thing the mod draws, all default on. Separate because they
            // answer different questions - how far along this run is, how much has ever
            // been seen, which options are new - and a player who wants one does not
            // necessarily want the others. Switching a display off never stops tracking.
            var showCurrentSaveCount = Config.Bind(
                "Display",
                "ShowCurrentSaveCount",
                true,
                "Show the this-save dialogue count on the main HUD.");
            var showAllSavesCount = Config.Bind(
                "Display",
                "ShowAllSavesCount",
                true,
                "Show the across-all-saves dialogue count on the main HUD.");
            var markNovelOptions = Config.Bind(
                "Display",
                "MarkNovelOptions",
                true,
                "Colour dialogue options that have never been picked in any save. Switch off to play "
                + "a run blind; the mod keeps tracking either way.");

            // A nudge, not a coordinate: placement is computed from the game's own rects,
            // so the display lands beside the money whatever the aspect ratio, and these
            // move it from there. Config rather than constants because how it looks is
            // the one thing the dumps cannot settle.
            var hudCountOffsetX = Config.Bind(
                "Display",
                "HudCountOffsetX",
                MainHudDialogueCountPatch.DefaultOffsetX,
                "How far left of the HUD's money/time panel the dialogue counts sit, in canvas units. "
                + "Negative is left, towards the thought cabinet button.");
            var hudCountOffsetY = Config.Bind(
                "Display",
                "HudCountOffsetY",
                MainHudDialogueCountPatch.DefaultOffsetY,
                "How far above the money display's own line the pair of dialogue counts sits, in canvas "
                + "units. Negative is down. Zero straddles that line, one count either side of it.");

            // Config for the same reason. Anything Unity's ColorUtility can read works.
            var novelOptionColor = Config.Bind(
                "Display",
                "NovelOptionColor",
                NovelResponseColorPatch.DefaultNovelColorHtml,
                "Colour for dialogue options that have never been picked in any save, as #RRGGBB, "
                + "#RRGGBBAA, or a colour name. Options picked in this save keep the game's "
                + "exhausted colour; options picked only in other saves keep the game's normal "
                + "option colour.");

            var harmony = new Harmony(PluginGuid);
            _harmony = harmony;

            bool recording = TryInstall(
                "DialogueLua.MarkDialogueEntry",
                "dialogue statuses are being tracked",
                "Dialogue seen during play will not be recorded this session",
                () => MarkDialogueEntryPatch.Install(harmony, session, log));

            bool recordingOrbs = TryInstall(
                "SenseOrb.SetShown",
                "opened orbs are being tracked",
                "Orbs opened during play will not be recorded this session",
                () => SenseOrbSetShownPatch.Install(harmony, session, log));

            bool resyncing = TryInstall(
                "PersistentDataManager.ApplyRawData",
                "the global state is resynced whenever a savegame is loaded (using raw file bytes)",
                "Statuses restored by loading a savegame will be missed this session",
                () => ApplyRawDataPatch.Install(harmony, session, log));

            bool resyncingOrbs = TryInstall(
                "SaveGameLoadedData.GetString",
                "orbs already in a loaded savegame are counted when it is loaded",
                "Orbs recorded in a loaded savegame will only be counted if they are opened again",
                () => LoadedOrbsPatch.Install(harmony, session, log));

            bool resettingCurrentSave = TryInstall(
                "World.ResetStates",
                "the current save's dialogue count is reset when a new game starts",
                "A new game will keep showing the previous save's dialogue count this session",
                () => NewGameResetPatch.Install(harmony, session, log));

            bool showingCount = showCurrentSaveCount.Value || showAllSavesCount.Value;
            if (showingCount)
            {
                showingCount = TryInstall(
                    "HudMoneyController.Start",
                    "the main HUD shows how many dialogue entries have been reached"
                        + DescribeCountRows(showCurrentSaveCount.Value, showAllSavesCount.Value),
                    "The main HUD will not show the dialogue counts this session",
                    () => MainHudDialogueCountPatch.Install(
                        harmony, session, log, hudCountOffsetX.Value, hudCountOffsetY.Value,
                        showCurrentSaveCount.Value, showAllSavesCount.Value));
            }
            else
            {
                Log.LogMessage(
                    "Both HUD dialogue counts are switched off; the main HUD is left alone. Tracking "
                    + "is unaffected. Turn either back on with ShowCurrentSaveCount or "
                    + "ShowAllSavesCount.");
            }

            bool colouringNovelOptions = markNovelOptions.Value;
            if (colouringNovelOptions)
            {
                colouringNovelOptions = TryInstall(
                    "SunshineResponseButton.GetData",
                    "dialogue options never picked in any save are drawn in their own colour",
                    "Every unpicked dialogue option will look the same this session, whether or not it "
                        + "was picked in another save",
                    () => NovelResponseColorPatch.Install(
                        harmony, session, log, novelOptionColor.Value));
            }
            else
            {
                Log.LogMessage(
                    "Novel-option colouring is switched off; dialogue options are drawn as the game "
                    + "draws them. Tracking is unaffected. Turn it back on with MarkNovelOptions.");
            }

            if (!recording && !recordingOrbs && !resyncing && !resyncingOrbs
                && !resettingCurrentSave && !showingCount && !colouringNovelOptions)
            {
                _harmony = null;
            }

            RegisterShutdownFlush(session);
        }

        /// <summary>
        /// Arranges for the global state to be flushed when the game goes away.
        /// </summary>
        /// <remarks>
        /// <para>The global state is written by a background thread, so the last few
        /// marks may be in memory and not yet on disk. Losing those to a hard crash is
        /// an accepted cost - each is re-marked the next time the line is reached - but
        /// losing them when the player simply quits is not.</para>
        ///
        /// <para>Not <see cref="Unload"/>: BepInEx's <c>IL2CPPChainloader</c> has no
        /// unload path at all. It is overridden below so a host that does call it gets a
        /// clean shutdown, but nothing may depend on it.</para>
        ///
        /// <para>Two events, because neither is guaranteed on its own.
        /// <c>Application.quitting</c> fires on the main thread while the engine is
        /// still up, which is the right moment, but is reached through IL2CPP interop,
        /// so it is registered defensively. <c>AppDomain.ProcessExit</c> is plain BCL
        /// and touches nothing of Unity's, but only fires if the hosted runtime gets a
        /// graceful shutdown, which a Unity player exiting through native code may not
        /// give it. Both funnel into the same idempotent call.</para>
        /// </remarks>
        private void RegisterShutdownFlush(GlobalStateSession session)
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
                    $"Could not subscribe to {ApplicationQuittingTrigger}: {ex}. The global state will "
                    + "still be flushed at process exit if the runtime shuts down cleanly.");
            }

            AppDomain.CurrentDomain.ProcessExit += (_, _) => FlushOnShutdown(session, ProcessExitTrigger);

            string registered = quittingSubscribed
                ? $"{ApplicationQuittingTrigger} and {ProcessExitTrigger}"
                : ProcessExitTrigger;
            Log.LogMessage($"Shutdown flush registered on {registered}.");
        }

        /// <summary>
        /// Flushes and stops the session's writer, reporting failures rather than
        /// throwing out of a shutdown handler.
        /// </summary>
        /// <remarks>
        /// The report itself is guarded: on <c>AppDomain.ProcessExit</c> BepInEx's own
        /// log sink may already be tearing itself down, and throwing out of a
        /// process-exit handler over a failed log line is worse than the missing line.
        /// </remarks>
        private void FlushOnShutdown(GlobalStateSession session, string trigger)
        {
            try
            {
                session.Shutdown(trigger);
            }
            catch (Exception ex)
            {
                try
                {
                    Log.LogError($"Failed to flush the global state on {trigger}: {ex}");
                }
                catch (Exception)
                {
                    // Nowhere left to report to. The process is going away regardless.
                }
            }
        }

        /// <summary>
        /// Names which of the two HUD count rows are switched on, for the install log.
        /// Never called with both off; that case does not install at all.
        /// </summary>
        private static string DescribeCountRows(bool currentSave, bool allSaves)
        {
            if (currentSave && allSaves)
            {
                return ", in this save and across all saves";
            }

            return currentSave ? ", in this save" : ", across all saves";
        }

        /// <summary>
        /// Installs one hook, reporting a failure rather than taking the plugin down.
        /// </summary>
        /// <remarks>
        /// Each hook is installed independently: none depends on another, so losing one
        /// is a partial loss of tracking rather than a reason to abandon the rest.
        /// Failing to patch leaves the game exactly as it was.
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

        /// <summary>
        /// BepInEx's unload, if a host ever calls it. Unpatches first so nothing new
        /// can be recorded, then flushes what is pending.
        /// </summary>
        /// <returns>True, since unloading is always allowed.</returns>
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
