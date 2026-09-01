// SPDX-License-Identifier: MIT
using System;
using System.IO;
using BepInEx;
using BepInEx.Unity.IL2CPP;
using HarmonyLib;
using Il2CppInterop.Runtime.InteropTypes.Arrays;
using PixelCrushers.DialogueSystem;
using UnityEngine;

namespace GlobalConversationTracker.TestProbe
{
    /// <summary>
    /// A second plugin that watches the mod from outside and narrates what the game is
    /// doing, so an automated test can drive it and judge it without screenshots.
    /// </summary>
    /// <remarks>
    /// <para>It exists because the alternative is worse. What the look-ahead feature
    /// produces is an asterisk a few dozen pixels wide at the end of a line of prose,
    /// and proving it is there from a screen capture means a pixel diff against a
    /// reference for every option in every scenario - fragile, slow to author, and
    /// silent about <em>which</em> option was marked. The text the game is about to
    /// draw already contains the marker and the colour tag around it; logging that text
    /// is the same evidence, exactly, and it names the option.</para>
    ///
    /// <para>It is a separate assembly rather than a switch inside the mod, and it is
    /// deployed only for the length of an automated run. What ships must be the thing
    /// under test, with no test-only branch in it that could behave differently from
    /// what a player installs.</para>
    ///
    /// <para>Every hook is a postfix that reads arguments and writes a log line, and
    /// every one swallows its own exceptions: an instrument that breaks the run it is
    /// measuring is worse than no instrument.</para>
    ///
    /// <para>It also drives, through <see cref="ProbeCommands"/>. That is the one place
    /// it is not an observer, and it is deliberate: a run has to reach a particular save
    /// and a particular conversation, and doing that by clicking through the pause menu
    /// means encoding a layout that can only be learned from a screenshot and re-learned
    /// whenever the UI moves. The commands call the game's own loader and its own
    /// dialogue system, so the setup travels the same code the tests already hook - it
    /// replaces the navigation, not the behaviour under test.</para>
    /// </remarks>
    [BepInPlugin(PluginGuid, PluginName, PluginVersion)]
    public class TestProbePlugin : BasePlugin
    {
        /// <summary>The probe's own plugin id.</summary>
        internal const string PluginGuid =
            "com.molodowitch.globalconversationtracker.testprobe";

        /// <summary>The probe's display name, as BepInEx logs it.</summary>
        internal const string PluginName = "GlobalConversationTrackerTestProbe";

        /// <summary>The probe's version.</summary>
        internal const string PluginVersion = "0.1.0";

        /// <summary>
        /// The mod under test, so the probe's patches can be ordered after its.
        /// </summary>
        /// <remarks>
        /// Copied rather than referenced. Referencing the mod would put a second copy
        /// of its assembly in the plugins folder beside this one, and BepInEx would
        /// load it as a second plugin.
        /// </remarks>
        internal const string ModGuid = "com.molodowitch.globalconversationtracker";

        private static readonly ResponseMenuRecorder Menu = new ResponseMenuRecorder();

        /// <summary>Installs the hooks and the command pump.</summary>
        public override void Load()
        {
            ProbeLog.Attach(Log);

            var harmony = new Harmony(PluginGuid);
            harmony.PatchAll(typeof(ResponseTextProbe));
            harmony.PatchAll(typeof(ResponseMenuProbe));
            harmony.PatchAll(typeof(ConversationStartProbe));
            harmony.PatchAll(typeof(ConversationEndProbe));
            harmony.PatchAll(typeof(SaveLoadedProbe));
            harmony.PatchAll(typeof(WorldReadyProbe));
            harmony.PatchAll(typeof(MainMenuProbe));

            // Beside the global state, in the profile the harness stages. Resolved the
            // same way the mod resolves it, and with the same fallback, so the probe
            // and the mod cannot end up looking at two different folders.
            string saveGames = SaveGamesFolder();
            ProbeCommands.UseDirectory(saveGames);
            AddComponent<ProbeCommands>();

            ProbeLog.Write(
                "ready",
                "version", PluginVersion,
                "watching", ModGuid,
                "commands", ProbeCommands.CommandPath);
        }

        /// <summary>
        /// The SaveGames folder, asking the game first and falling back to Unity.
        /// </summary>
        /// <remarks>
        /// The game's own accessor is coupled to the interop shape, which is
        /// regenerated per build and would break on a rename; persistentDataPath is
        /// stable but is only the parent. Trying both, in that order, is what the mod
        /// does, and the two must agree or the harness would write commands into a
        /// folder nothing is watching.
        /// </remarks>
        private static string SaveGamesFolder()
        {
            try
            {
                string folder = SunshinePersistenceFileManager.GetSaveGameDirectoryPath();
                if (!string.IsNullOrWhiteSpace(folder))
                {
                    return folder;
                }
            }
            catch (Exception error)
            {
                ProbeLog.Failed("asking the game where its saves are", error);
            }

            return Path.Combine(
                Application.persistentDataPath,
                SunshinePersistenceFileManager.SAVE_GAME_DIRECTORY);
        }

        /// <summary>The money the game reports now, or null if Lua would not answer.</summary>
        /// <remarks>
        /// In centimes, as the database's ClickCost fields are: a 50.00 real purchase
        /// reads 5000. Read exactly as the mod's own world snapshot reads it, so the
        /// probe and the feature under test can never disagree about what the player
        /// can afford - which is the whole subject of the money scenarios.
        /// </remarks>
        internal static int? Money()
        {
            try
            {
                Lua.Result result = Lua.Run("return MoneyAmount()");
                if (!result.isNumber)
                {
                    return null;
                }

                float amount = result.asFloat;
                return amount <= 0 ? 0 : (int)amount;
            }
            catch (Exception)
            {
                return null;
            }
        }

        /// <summary>
        /// The conversation the game is currently in, or null between conversations.
        /// </summary>
        internal static int? ConversationId()
        {
            try
            {
                Conversation? current = Sunshine.ConversationLogger.lastConversation;
                return current == null ? (int?)null : current.id;
            }
            catch (Exception)
            {
                return null;
            }
        }

        /// <summary>
        /// Every option's text, after the mod has finished composing it.
        /// </summary>
        /// <remarks>
        /// Ordered last on purpose. The mod appends its look-ahead asterisk in a
        /// postfix on this same method, and a probe that ran first would log the text
        /// without the marker - indistinguishable from the marker not being produced,
        /// which is the exact failure the tests look for. <c>HarmonyAfter</c> puts this
        /// behind the mod's patch by id, and the priority puts it behind anything else
        /// that did not ask for an order.
        /// </remarks>
        [HarmonyPatch(
            typeof(Sunshine.ConversationLogger),
            nameof(Sunshine.ConversationLogger.ChooseResponseText))]
        [HarmonyAfter(ModGuid)]
        [HarmonyPriority(Priority.Last)]
        private static class ResponseTextProbe
        {
            /// <summary>
            /// The parameter name is matched against the patched method by Harmony, so
            /// it has to stay <c>response</c>.
            /// </summary>
            [HarmonyPostfix]
            private static void Postfix(Response response, FinalResponseText __result)
            {
                try
                {
                    DialogueEntry? entry = response == null ? null : response.destinationEntry;
                    Menu.AddOption(new RecordedOption(
                        entry == null ? (int?)null : entry.conversationID,
                        entry == null ? (int?)null : entry.id,
                        __result == null ? null : __result.responseText));
                }
                catch (Exception error)
                {
                    ProbeLog.Failed("an option's text", error);
                }
            }
        }

        /// <summary>
        /// The menu itself, which supplies the option count and the balance the
        /// look-ahead crawled from.
        /// </summary>
        [HarmonyPatch(
            typeof(Sunshine.ConversationLogger),
            nameof(Sunshine.ConversationLogger.OnConversationResponseMenu))]
        private static class ResponseMenuProbe
        {
            /// <summary>The parameter name has to stay <c>responses</c>.</summary>
            [HarmonyPostfix]
            private static void Postfix(Il2CppReferenceArray<Response> responses)
            {
                try
                {
                    Menu.MenuShown(
                        responses == null ? 0 : responses.Length, ConversationId(), Money());
                }
                catch (Exception error)
                {
                    ProbeLog.Failed("a response menu", error);
                }
            }
        }

        /// <summary>
        /// The start of a conversation, which separates "it never began" from "it began
        /// and offered nothing".
        /// </summary>
        [HarmonyPatch(
            typeof(Sunshine.ConversationLogger),
            nameof(Sunshine.ConversationLogger.OnConversationStart))]
        private static class ConversationStartProbe
        {
            /// <summary>The parameter name has to stay <c>actor</c>.</summary>
            [HarmonyPostfix]
            private static void Postfix(Transform actor)
            {
                try
                {
                    ProbeLog.Write(
                        "conversation-start",
                        "conversation", ConversationId(),
                        "money", Money());
                }
                catch (Exception error)
                {
                    ProbeLog.Failed("the start of a conversation", error);
                }
            }
        }

        /// <summary>
        /// The end of a conversation, which is the last chance to report a menu whose
        /// options never all arrived.
        /// </summary>
        [HarmonyPatch(
            typeof(Sunshine.ConversationLogger),
            nameof(Sunshine.ConversationLogger.OnConversationEnd))]
        private static class ConversationEndProbe
        {
            /// <summary>The parameter name has to stay <c>actor</c>.</summary>
            [HarmonyPostfix]
            private static void Postfix(Transform actor)
            {
                try
                {
                    Menu.Flush("conversation-ended");
                    ProbeLog.Write("conversation-end", "money", Money());
                }
                catch (Exception error)
                {
                    ProbeLog.Failed("the end of a conversation", error);
                }
            }
        }

        /// <summary>
        /// A savegame being applied, which is how a test knows which scenario is live
        /// after it drives the load menu.
        /// </summary>
        /// <remarks>
        /// The same hook the mod resyncs on, for the same reason: it is the one place
        /// every load goes through, whether from the main menu or the pause menu.
        /// </remarks>
        [HarmonyPatch(
            typeof(PersistentDataManager),
            nameof(PersistentDataManager.ApplyRawData))]
        private static class SaveLoadedProbe
        {
            /// <summary>The parameter name has to stay <c>bytes</c>.</summary>
            [HarmonyPostfix]
            private static void Postfix(Il2CppStructArray<byte> bytes)
            {
                try
                {
                    ProbeLog.Write(
                        "save-applied",
                        "bytes", bytes == null ? 0 : bytes.Length,
                        "money", Money());
                }
                catch (Exception error)
                {
                    ProbeLog.Failed("a savegame load", error);
                }
            }
        }

        /// <summary>
        /// The main menu being built, which is when a keypress can start a game.
        /// </summary>
        /// <remarks>
        /// Needed because <c>world-ready</c> is not it. The HUD is built about ten
        /// seconds in, while the legal notice and the logo are still to come, and a run
        /// that took that for the menu pressed Continue into a splash screen. The menu
        /// list is the thing that has to exist for Continue to mean anything.
        /// </remarks>
        [HarmonyPatch(typeof(MainMenuList), "Start")]
        private static class MainMenuProbe
        {
            [HarmonyPostfix]
            private static void Postfix()
            {
                try
                {
                    ProbeLog.Write("main-menu");
                }
                catch (Exception error)
                {
                    ProbeLog.Failed("the main menu appearing", error);
                }
            }
        }

        /// <summary>
        /// The main HUD coming up, which is the earliest point at which the world is
        /// really there and a click will hit something.
        /// </summary>
        /// <remarks>
        /// A screenshot cannot say this: the loading screen animates, so there is no
        /// settling to wait for, and to any threshold loose enough to be stable the
        /// first frame of the world looks like the last frame of the loading screen.
        /// </remarks>
        [HarmonyPatch(typeof(HudMoneyController), "Start")]
        private static class WorldReadyProbe
        {
            [HarmonyPostfix]
            private static void Postfix()
            {
                try
                {
                    ProbeLog.Write("world-ready", "money", Money());
                }
                catch (Exception error)
                {
                    ProbeLog.Failed("the HUD appearing", error);
                }
            }
        }
    }
}
