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
    /// <para><b>What Load does, and deliberately does not do.</b> It resolves the
    /// SaveGames directory, builds the <see cref="GlobalStateSession"/>
    /// and installs the hooks. It does not read
    /// the state file and it does not read the game. Both of those happen on first
    /// access, through <see cref="GlobalStateSession.EnsureInitialized"/>, which
    /// every hook calls on its way in.
    /// </para>
    /// <para><b>Two tracking hooks, because there are two SimStatus writers.</b>
    /// <see cref="MarkDialogueEntryPatch"/> is the write-through hook for everything
    /// the game does while playing. <see cref="ApplyRawDataPatch"/> covers the one
    /// writer that never goes through it: <c>PersistentDataManager</c> rebuilding the
    /// whole Lua SimStatus table when a savegame is loaded. Between them they see
    /// every write; nothing else in the game writes SimStatus.
    /// </para>
    /// <para><b>A third hook, for the count that can go down.</b>
    /// <see cref="NewGameResetPatch"/> covers the one event neither writer above can
    /// see: a new game, which rebuilds the game's whole SimStatus table at once rather
    /// than marking entries, and so would otherwise leave the previous save's
    /// current-save count on screen. It resets that tally only; the across-all-saves
    /// state is what a new game exists to survive.
    /// </para>
    /// <para><b>Two display hooks.</b> <see cref="MainHudDialogueCountPatch"/> reads
    /// the tracked totals back out onto the main HUD, beside the money and the clock:
    /// this save on one line, every save on the next. It writes to the game's UI and
    /// to nothing else, so it is independent of the three above and is installed
    /// separately. Those three tell it when a count has moved, which is the only
    /// coupling between them: nothing polls, and a display that never installed is a
    /// no-op to call. <see cref="NovelResponseColorPatch"/> is the other, and reads
    /// the state one entry at a time rather than in total: it colours a dialogue
    /// option differently when no save has ever picked it, which is the one thing the
    /// game cannot work out for itself.
    /// </para>
    /// <para>
    /// Deferring the disk and game reads to first access is not tidiness, it is
    /// correctness. At chainload there is no dialogue system, no database and no
    /// save, so reading the game there would copy an all-Untouched table and record
    /// nothing. Both triggers therefore belong to the hooks, each of which can only
    /// fire once a game is in play - and the bulk read specifically belongs to the
    /// load hook, which fires at the one moment the game's SimStatus values are
    /// known to be real.
    /// </para>
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
        /// The plugin's version as BepInEx reports it. Kept in step with the csproj's
        /// own Version by hand: this one is what the log line says, that one is what
        /// the release zip is named after.
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

            // Three switches, one per thing the mod draws. All default on: the mod
            // exists to show these. They are separate because the three answer
            // different questions - how far along this run is, how much of the game
            // has ever been seen, and which options in front of me are new - and a
            // player who wants one of those does not necessarily want the others.
            // Switching a display off never stops tracking; the write path does not
            // pass through any of them.
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

            // The HUD count's placement is computed from the game's own rects, so
            // these are a nudge and not a coordinate: the display lands beside the
            // money whatever the screen's aspect ratio, and these move it from there.
            // They are config rather than constants because the one thing that cannot
            // be checked from the dumps is how it looks.
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

            // The one thing the dumps cannot settle is what a colour looks like next
            // to the game's own, so the novel-option colour is config rather than a
            // constant. Anything Unity's ColorUtility can read works here.
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
                    "Both HUD dialogue counts are switched off in the config, so the main HUD is left "
                    + "alone. Tracking is unaffected. Turn either back on with ShowCurrentSaveCount "
                    + "or ShowAllSavesCount.");
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
                    "Novel-option colouring is switched off in the config, so dialogue options are "
                    + "drawn exactly as the game draws them. Tracking is unaffected. Turn it back on "
                    + "with MarkNovelOptions.");
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
        /// <para><b>Why this is needed at all.</b> The global state is written by a
        /// background thread, so at any moment the last few marks may be in memory and
        /// not yet on disk. Losing those to a hard crash is an accepted, recorded cost
        /// - every one of them is re-marked the next time the line is reached - but
        /// losing them when the player simply quits is not.</para>
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
        /// it.</b> Each handler passes its own name into
        /// <see cref="GlobalStateSession.Shutdown"/>, which logs on arrival and again
        /// on completion; a second trigger reports that the first already did the work.
        /// This line - the one that says what was registered - is the other half: a log
        /// showing a registration and no trigger says the event never fired.</para>
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
        /// The report itself is guarded too. The shutdown path logs on every outcome
        /// rather than only on failure, and one of the two triggers is
        /// <c>AppDomain.ProcessExit</c>, where BepInEx's own log sink may already be
        /// tearing itself down. Throwing out of a process-exit handler over a failed
        /// log line would be a strictly worse outcome than the missing line.
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
        /// </summary>
        /// <remarks>
        /// The caller never asks with both off - that case does not install at all -
        /// so there are only three answers to give.
        /// </remarks>
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
