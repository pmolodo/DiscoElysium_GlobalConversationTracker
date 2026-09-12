// SPDX-License-Identifier: MIT
using System;
using System.IO;
using System.Text;
using BepInEx;
using BepInEx.Unity.IL2CPP;
using DiscoPages.Elements.Dialogue;
using HarmonyLib;
using Il2CppInterop.Runtime.InteropTypes.Arrays;
using PixelCrushers.DialogueSystem;
using Sunshine.Metric;
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

        /// <summary>
        /// The mod's LOOK-AHEAD Harmony instance, which is NOT <see cref="ModGuid"/>.
        /// </summary>
        /// <remarks>
        /// <para>THE ONE THAT APPENDS THE MARKER. The mod installs its two look-ahead hooks
        /// through an instance of its own - the plugin guid with a suffix - so that it can
        /// take them off again when the engine dies without touching the tracking hooks. So
        /// ordering this probe after the plugin guid orders it after NOTHING on
        /// ChooseResponseText, which is the method the marker is appended in.</para>
        ///
        /// <para>It went unnoticed because the mod loads before the probe and therefore
        /// patched first, which put the order right by accident. de-pszk made the mod
        /// re-install those hooks mid-run, the accident stopped holding, and an entire suite
        /// logged its options as plain while the game was drawing them marked.</para>
        ///
        /// <para>Copied rather than referenced, for the reason given on
        /// <see cref="ModGuid"/>.</para>
        /// </remarks>
        internal const string LookAheadGuid = ModGuid + ".lookahead";

        /// <summary>The field that makes an entry a white check.</summary>
        internal const string WhiteCheckField = "DifficultyWhite";

        /// <summary>The field that makes it a red one.</summary>
        internal const string RedCheckField = "DifficultyRed";

        private static readonly ResponseMenuRecorder Menu = new ResponseMenuRecorder();

        /// <summary>
        /// The mouse UI's dialogue logger, once it has put a line up.
        /// </summary>
        /// <remarks>
        /// WHAT ADVANCES A LINE. The game's own continue - the thing its continue button
        /// and its hotkey call - is an instance method on the logger, so telling the
        /// dialogue to go on means having the instance. It arrives with the first line
        /// and stays put afterwards.
        ///
        /// Calling it beats sending Enter at the window: a keypress goes wherever the
        /// focus is, cannot be aimed at a line rather than a menu, and needs the game in
        /// front. This needs none of that.
        /// </remarks>
        internal static Sunshine.ConversationLogger? MouseLogger { get; private set; }

        /// <summary>The page-system UI's logger, which is the one this game runs.</summary>
        /// <remarks>
        /// TWO UIS, TWO LOGGERS, and only one of them is live. The mod already carries the
        /// same split for its option colouring - <c>SunshineResponseButton</c> against
        /// <c>SunshineResponseButtonPageSystem</c> - and hooking only the mouse one here
        /// cost a whole in-game run: the patch applied, the method was never called, and
        /// the harness sat waiting for a line that nothing was going to announce. Both are
        /// hooked, and whichever speaks is the one that gets told to continue.
        /// </remarks>
        internal static ConversationLoggerPageSystem? PageLogger { get; private set; }

        /// <summary>How many lines this conversation has put up.</summary>
        /// <remarks>
        /// What makes advancing exact: a line is answered at most once, however many polls
        /// it spends asking. Without it, a line that keeps offering its continue would be
        /// answered again on every poll.
        /// </remarks>
        internal static int LinesShown { get; private set; }

        /// <summary>
        /// How many response menus this conversation has finished composing.
        /// </summary>
        /// <remarks>
        /// The probe's own answer to "is a menu up", and the reason it is trustworthy: it
        /// is reset when a conversation starts, so it cannot report the previous
        /// scenario's menu the way the interface's own toggle does.
        /// </remarks>
        internal static int MenusShown { get; private set; }

        /// <summary>Counts a menu the game finished composing.</summary>
        internal static void NoteMenu()
        {
            MenusShown++;
        }

        /// <summary>Forgets what this conversation has drawn, as one starts.</summary>
        internal static void ForgetLines()
        {
            LinesShown = 0;
            MenusShown = 0;
        }

        /// <summary>Tells whichever dialogue UI is live to go on to the next line.</summary>
        /// <returns>False when no line has been shown yet, so there is nothing to tell.</returns>
        internal static bool Advance()
        {
            if (PageLogger != null)
            {
                PageLogger.OnContinue();
                return true;
            }

            if (MouseLogger != null)
            {
                MouseLogger.OnContinue();
                return true;
            }

            return false;
        }

        /// <summary>Installs the hooks and the command pump.</summary>
        public override void Load()
        {
            ProbeLog.Attach(Log);

            var harmony = new Harmony(PluginGuid);
            harmony.PatchAll(typeof(ResponseTextProbe));
            harmony.PatchAll(typeof(ResponseMenuProbe));
            harmony.PatchAll(typeof(ConversationLineProbe));
            harmony.PatchAll(typeof(PageConversationLineProbe));
            DialogueWaitProbe.Install(harmony);
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

        /// <summary>
        /// Every ability and skill as the RUNNING GAME values them, or null before the
        /// world exists.
        /// </summary>
        /// <remarks>
        /// <para>THE SAVE'S OWN SHEET IS NOT OBVIOUSLY THE ANSWER, which is why this is
        /// worth logging. A committed save records a value per skill, and an offline
        /// fixture decides passive checks from it - but one save records a level 9
        /// character with every ability at 1 and no points in any skill, and the game
        /// holding that save cleared checks that arithmetic says it cannot. See
        /// de-2p8j.5.</para>
        ///
        /// <para>Read exactly the way the mod reads it - World.Singleton.you, then the
        /// value the sheet holds - so the number here is the number the check rule used,
        /// and a difference from the file is a difference in the game rather than in how
        /// two readers spell the question.</para>
        ///
        /// <para>All twenty-eight rather than a handful, because a fixture has to
        /// reproduce every one of them: any skill can be a passive check's speaker.</para>
        /// </remarks>
        internal static string? Skills()
        {
            try
            {
                CharacterSheet? sheet = PlayerSheet();
                if (sheet == null)
                {
                    return null;
                }

                var said = new StringBuilder();
                foreach (Ability ability in sheet.abilities)
                {
                    Say(said, ability.abilityType.ToString(), ability.value);
                }

                foreach (Skill skill in sheet.skills)
                {
                    Say(said, skill.skillType.ToString(), skill.value);
                }

                return said.ToString();
            }
            catch (Exception)
            {
                return null;
            }
        }

        /// <summary>
        /// What each of those values is made of: every modifier on every ability and
        /// skill, with the amount the game counts and what caused it.
        /// </summary>
        /// <remarks>
        /// <para>A VALUE ON ITS OWN CANNOT BE REPRODUCED OFFLINE. The save stores the
        /// modifier list stripped - <c>ClearModifiersForPersistence</c> runs before
        /// serialising - and what the game rebuilds on load is not simply what was
        /// stripped: an ITEM or THC modifier's amount is re-read from the asset, and one
        /// whose thought is no longer cooking or internalised is dropped. So the parts
        /// are the measurement, and the total is only the check on it.</para>
        ///
        /// <para><c>Amount</c> rather than the persisted amount, because that is what
        /// <c>Recalc</c> sums: a CALCULATED_ABILITY modifier reports the ability's live
        /// value and stores zero.</para>
        /// </remarks>
        internal static string? SkillModifiers()
        {
            try
            {
                CharacterSheet? sheet = PlayerSheet();
                if (sheet == null)
                {
                    return null;
                }

                var said = new StringBuilder();
                foreach (Ability ability in sheet.abilities)
                {
                    Say(said, ability.abilityType.ToString(), ability);
                }

                foreach (Skill skill in sheet.skills)
                {
                    Say(said, skill.skillType.ToString(), skill);
                }

                return said.ToString();
            }
            catch (Exception)
            {
                return null;
            }
        }

        /// <summary>The player's character sheet, or null before the world exists.</summary>
        private static CharacterSheet? PlayerSheet()
        {
            World? world = World.Singleton;
            return world == null ? null : world.you;
        }

        /// <summary>One `NAME=value` entry, separated from whatever came before it.</summary>
        private static void Say(StringBuilder said, string name, int value)
        {
            if (said.Length > 0)
            {
                said.Append(' ');
            }

            said.Append(name).Append('=').Append(value);
        }

        /// <summary>
        /// One `NAME=value[modifier,modifier]` entry, where a modifier is its type, its
        /// signed amount, and the thought, item or ability behind it where there is one.
        /// </summary>
        private static void Say(StringBuilder said, string name, Modifiable modifiable)
        {
            Say(said, name, modifiable.value);
            said.Append('[');

            bool first = true;
            foreach (Modifier modifier in modifiable.GetModifierList())
            {
                if (!first)
                {
                    said.Append(',');
                }

                first = false;
                said.Append(modifier.type.ToString()).Append(modifier.Amount.ToString("+#;-#;0"));

                ModifierCauseHolder? cause =
                    ModifierCauseHolder.GetModifierCauseHolder(modifier.modifierCause);
                if (cause != null)
                {
                    said.Append('(').Append(cause.ModifierKey).Append(')');
                }
            }

            said.Append(']');
        }

        /// <summary>
        /// What the game answers the three scene queries with, beside the two Lua variables
        /// an offline world answers them from.
        /// </summary>
        /// <remarks>
        /// <para>THE OFFLINE ANSWER FOR THE WEATHER IS AN INFERENCE. `FELDLuaFunctions` is
        /// IL2CPP, so the export's `IsRaining` is a stub returning false and says nothing
        /// about what the real one reads; what named `auto.is_raining` and `auto.is_snowing`
        /// is `ArcticSwimmerEasterEgg`, which watches one of them for the condition the
        /// dialogue guards ask about. Plausible is not measured, and this is what measures
        /// it: the variable and the query side by side, on a save that sets the variable.
        /// They agree or they do not.</para>
        ///
        /// <para>Run as Lua because that is how the mod's own world snapshot answers a
        /// query key - the key IS a call with literal arguments - so the probe and the
        /// feature under test cannot disagree about what was asked.</para>
        ///
        /// <para>READ IT FROM THE MENU EVENT RATHER THAN FROM SAVE-APPLIED where the two
        /// have to agree with each other. The Lua tables are in by the time a save is
        /// applied but the area is not, so a mid-load reading pairs the save being loaded
        /// with the scene of the one before it - which is a useful thing to see and a
        /// misleading thing to quote.</para>
        /// </remarks>
        internal static string? Scene()
        {
            // Each query as the guards spell it, and each variable as the save's table does.
            string[] asked =
            {
                "IsExterior()",
                "IsRaining()",
                "IsSnowing()",
                "Variable[\"auto.is_raining\"]",
                "Variable[\"auto.is_snowing\"]",
            };

            try
            {
                var said = new StringBuilder();
                foreach (string question in asked)
                {
                    if (said.Length > 0)
                    {
                        said.Append(' ');
                    }

                    said.Append(question).Append('=').Append(Answer(question));
                }

                return said.ToString();
            }
            catch (Exception)
            {
                return null;
            }
        }

        /// <summary>
        /// What Lua makes of one expression, or "?" where it will not answer with a
        /// boolean - which is a reading in its own right and not something to default.
        /// </summary>
        private static string Answer(string question)
        {
            Lua.Result result = Lua.Run($"return {question}");
            return result.isBool ? (result.asBool ? "true" : "false") : "?";
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
        /// Whether a conversation is running right now, or null if the question could
        /// not be asked.
        /// </summary>
        /// <remarks>
        /// Not answerable from <see cref="ConversationId"/>, which reads
        /// <c>lastConversation</c> - the conversation most recently entered, which keeps
        /// its value after that conversation ends. Telling "still talking" from "started
        /// and fell straight back out" needs the dialogue system's own live flag, and
        /// that difference is the whole of what a caller retrying a failed open wants to
        /// know.
        /// </remarks>
        internal static bool? IsConversationActive()
        {
            try
            {
                return DialogueManager.isConversationActive;
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
        ///
        /// BOTH IDS, and <see cref="LookAheadGuid"/> is the one that matters here: the
        /// marker is appended by the look-ahead's own Harmony instance, not by the
        /// plugin's. See that constant for what naming only the plugin's cost.
        /// </remarks>
        [HarmonyPatch(
            typeof(Sunshine.ConversationLogger),
            nameof(Sunshine.ConversationLogger.ChooseResponseText))]
        [HarmonyAfter(ModGuid, LookAheadGuid)]
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
                        __result == null ? null : __result.responseText,
                        CheckKind(entry)));
                }
                catch (Exception error)
                {
                    ProbeLog.Failed("an option's text", error);
                }
            }

            /// <summary>
            /// Which kind of roll an option is, from the game's own fields.
            /// </summary>
            /// <remarks>
            /// The two difficulty fields are what make an entry a rolled check - the same
            /// fields the shipped index carries them by, and the same ones the mod's engine
            /// reads to decide an option gets two outcomes rather than one. Read here from
            /// the live entry, so a suite can hold the mod to the game rather than to
            /// itself.
            /// </remarks>
            private static string? CheckKind(DialogueEntry? entry)
            {
                if (entry == null)
                {
                    return null;
                }

                if (Field.FieldExists(entry.fields, WhiteCheckField))
                {
                    return "white";
                }

                return Field.FieldExists(entry.fields, RedCheckField) ? "red" : null;
            }
        }

        /// <summary>
        /// A line of dialogue going up on screen, which is a line waiting to be advanced.
        /// </summary>
        /// <remarks>
        /// WHY A RUN NEEDS TO BE TOLD THIS. A conversation opens on narration and waits to
        /// be told to go on, exactly as it would for a player, and the harness has no other
        /// way to know a line is up. Without it the only strategy is to press Enter and see
        /// what happens - which is what it used to do, and an Enter that arrives while a
        /// RESPONSE menu is open picks an option instead of advancing a line. That is how
        /// the same save opened four different menus over three runs.
        ///
        /// So: one event per line, and the run presses once per line it is told about and
        /// never otherwise.
        /// </remarks>
        [HarmonyPatch(
            typeof(Sunshine.ConversationLogger),
            nameof(Sunshine.ConversationLogger.OnConversationLine))]
        private static class ConversationLineProbe
        {
            /// <summary>
            /// The parameter names have to stay <c>subtitle</c> and <c>__instance</c>.
            /// </summary>
            [HarmonyPostfix]
            private static void Postfix(
                Sunshine.ConversationLogger __instance, Subtitle subtitle)
            {
                // KEPT, because it is the thing that can advance the line this event
                // announces. The logger is a scene object with no accessor the probe can
                // call for it, and the line hook is where one arrives.
                MouseLogger = __instance;
                ReportLine(subtitle);
            }
        }

        /// <summary>The same line, on the UI this game actually runs.</summary>
        /// <remarks>
        /// See <see cref="PageLogger"/>: the two dialogue UIs have the same shape and only
        /// one of them speaks. Hooking both is cheaper than being sure which.
        /// </remarks>
        [HarmonyPatch(
            typeof(ConversationLoggerPageSystem),
            nameof(ConversationLoggerPageSystem.OnConversationLine))]
        private static class PageConversationLineProbe
        {
            /// <summary>
            /// The parameter names have to stay <c>subtitle</c> and <c>__instance</c>.
            /// </summary>
            [HarmonyPostfix]
            private static void Postfix(
                ConversationLoggerPageSystem __instance, Subtitle subtitle)
            {
                PageLogger = __instance;
                ReportLine(subtitle);
            }
        }

        /// <summary>Reports one line of dialogue going up, whichever UI put it there.</summary>
        private static void ReportLine(Subtitle? subtitle)
        {
            try
            {
                LinesShown++;
                DialogueEntry? entry = subtitle == null ? null : subtitle.dialogueEntry;
                ProbeLog.Write(
                    "line",
                    "conversation", entry == null ? (int?)null : entry.conversationID,
                    "entry", entry == null ? (int?)null : entry.id,
                    // What the interface says at the moment a line goes up. Paired with
                    // the same two readings on the menu event, one run says whether either
                    // can tell "a line is waiting" from "a menu is up" - which is what
                    // would replace the harness's fixed wait (de-6vyj).
                    "continueButton", DialogueWaitProbe.ContinueButton(),
                    "toggle", DialogueWaitProbe.Toggle());
            }
            catch (Exception error)
            {
                ProbeLog.Failed("a line of dialogue", error);
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
                // Before the logging, and outside the try, because this is the latch the
                // load-finished report hangs on: losing it to a formatting failure would
                // cost a whole scenario, and it cannot itself throw.
                ProbeCommands.NoteSaveApplied();

                try
                {
                    ProbeLog.Write(
                        "save-applied",
                        "bytes", bytes == null ? 0 : bytes.Length,
                        "money", Money(),
                        "skills", Skills(),
                        "modifiers", SkillModifiers(),
                        "scene", Scene());
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
